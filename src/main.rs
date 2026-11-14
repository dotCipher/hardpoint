mod board;
mod catalog;
mod config;
mod device;
mod discover;
mod engine;
mod model;
mod source;
mod telemetry;

use anyhow::{anyhow, bail, Result};
use board::Board;
use catalog::Catalog;
use clap::{Parser, Subcommand};
use config::{Backend, Config};
use device::{dry::DryDevice, oryx::OryxDevice, Device, DeviceEvent};
use engine::{Action, Engine};
use model::{AgentState, AgentUpdate, BusEvent, Request, Rgb};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use telemetry::Telemetry;

/// The adapter this build can drive a keyboard through.
const KEYBOARD_ADAPTER: &str = "oryx-hid";

#[derive(Parser)]
#[command(name = "hardpoint", version, about = "A safety label for everything your AI can touch")]
struct Cli {
    /// The catalog directory. Defaults to HARDPOINT_CATALOG, then the user
    /// data directory, then a `catalog` directory beside the program.
    #[arg(long, global = true)]
    catalog: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what is attached to this computer and which cards describe it.
    Inventory {
        #[arg(long)]
        json: bool,
    },
    /// Run the daemon: follow agents, light slots, act on slot presses.
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
        /// Override the configured device backend.
        #[arg(long, value_enum)]
        device: Option<Backend>,
    },
    /// Connect to the keyboard, report what it says about itself, and count key events.
    Probe {
        #[arg(long, default_value_t = 5)]
        secs: u64,
        /// Print the key number of every press. Off by default: it shows what is typed.
        #[arg(long)]
        show_keys: bool,
    },
    /// Light keys so you can check the card's key numbers against the board.
    Identify {
        /// Key numbers to light, or `slots` (from your config) or `sweep`.
        pattern: Vec<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, default_value_t = 6)]
        secs: u64,
        #[arg(long, default_value = "#00ff40")]
        color: String,
    },
    /// Report an agent's state to the running daemon.
    Emit {
        #[arg(long)]
        source: String,
        #[arg(long)]
        session: String,
        #[arg(long, value_enum)]
        state: AgentState,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long)]
        directory: Option<String>,
    },
    /// Tell the daemon a session no longer exists.
    Gone {
        #[arg(long)]
        source: String,
        #[arg(long)]
        session: String,
    },
    /// Press a key by number, as if it had been pressed on the board.
    Press { key: u8 },
    /// Show what the daemon is tracking.
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let catalog = || -> Result<Catalog> { Catalog::load(&Catalog::locate(cli.catalog.as_deref())?) };
    match cli.command {
        Command::Inventory { json } => inventory(&catalog()?, json),
        Command::Run { config, device } => run(&catalog()?, config, device),
        Command::Probe { secs, show_keys } => probe(&catalog()?, secs, show_keys),
        Command::Identify { pattern, config, secs, color } => identify(&catalog()?, pattern, config, secs, &color),
        Command::Emit { source, session, state, title, detail, directory } => {
            send(Request::Agent(AgentUpdate { source, session, state, title, detail, directory }))
        }
        Command::Gone { source, session } => send(Request::Gone { source, session }),
        Command::Press { key } => send(Request::Press { key }),
        Command::Status => send(Request::Status),
    }
}

fn send(request: Request) -> Result<()> {
    let reply = source::socket::request(&config::socket_path(), &request)?;
    if reply["ok"].as_bool() != Some(true) {
        bail!("daemon refused: {}", reply["error"].as_str().unwrap_or("unknown error"));
    }
    if let Some(status) = reply.get("status") {
        println!("{}", serde_json::to_string_pretty(status)?);
    }
    Ok(())
}

fn inventory(catalog: &Catalog, json: bool) -> Result<()> {
    let found = discover::inventory(catalog, discover::observe()?);
    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
        return Ok(());
    }
    let ids = |f: &discover::Found| match (f.seen.vendor_id, f.seen.product_id) {
        (Some(v), Some(p)) => format!("{} {v:04x}:{p:04x}", f.seen.bus),
        _ => f.seen.bus.clone(),
    };
    println!("catalog: {} ({} cards)\n", catalog.root.display(), catalog.cards.len());
    println!("Recognised");
    for f in found.iter().filter(|f| f.card.is_some()) {
        println!("  {}  [{}]  {}", f.name.as_deref().unwrap_or(""), f.card.as_deref().unwrap_or(""), ids(f));
        let caps: Vec<String> = f.capabilities.iter().map(|(risk, n)| format!("{n} {risk}")).collect();
        println!("    {} · {}", f.kind.as_deref().unwrap_or(""), caps.join(", "));
        for need in &f.needs {
            println!("    needs: {need}");
        }
    }
    println!("\nNot in the catalog");
    for f in found.iter().filter(|f| f.card.is_none()) {
        println!("  {}  {}", ids(f), f.seen.name);
    }
    println!("\nDisplays and audio devices are not looked for yet.");
    Ok(())
}

/// The keyboard to drive: the pinned card, or the first attached device whose
/// card has a key layout and uses the keyboard adapter.
fn pick_board(catalog: &Catalog, pinned: &str, attached_only: bool) -> Result<Board> {
    let usable = |card: &catalog::Card| card.keys.is_some() && card.has_adapter(KEYBOARD_ADAPTER);
    if !pinned.is_empty() {
        let card = catalog.card(pinned).ok_or_else(|| anyhow!("no card {pinned:?} in the catalog"))?;
        if !usable(card) {
            bail!("{pinned} has no key layout this build can drive");
        }
        return Board::from_card(card);
    }
    let seen = discover::observe()?;
    if let Some(card) = seen.iter().filter_map(|s| catalog.recognise(s)).find(|c| usable(c)) {
        return Board::from_card(card);
    }
    if attached_only {
        bail!("no attached keyboard is described by the catalog (try `hardpoint inventory`)");
    }
    let card = catalog.cards.iter().find(|c| usable(c)).ok_or_else(|| anyhow!("the catalog describes no keyboard"))?;
    Board::from_card(card)
}

fn probe(catalog: &Catalog, secs: u64, show_keys: bool) -> Result<()> {
    let board = pick_board(catalog, "", true)?;
    let mut dev = OryxDevice::open(&board)?;
    println!("card: {}", board.card);
    println!("connected: {}", dev.describe());
    println!("listening for {secs}s...");
    let (mut downs, mut ups, mut layers, mut unknown) = (0u32, 0u32, Vec::new(), 0u32);
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        for event in dev.poll(50)? {
            match event {
                DeviceEvent::KeyDown { row, col } => {
                    downs += 1;
                    match board.by_matrix(row, col) {
                        Some(key) if show_keys => println!("down key {} (switch {row},{col}, light {})", key.index, key.led),
                        Some(_) => {}
                        None => unknown += 1,
                    }
                }
                DeviceEvent::KeyUp { .. } => ups += 1,
                DeviceEvent::Layer(layer) => layers.push(layer),
            }
        }
    }
    println!("{downs} key-down and {ups} key-up events, {unknown} at positions the card does not describe");
    println!("layer reports seen: {layers:?}");
    Ok(())
}

fn identify(catalog: &Catalog, pattern: Vec<String>, config: Option<PathBuf>, secs: u64, color: &str) -> Result<()> {
    let color = Rgb::parse(color)?;
    let (cfg, _) = Config::load(config.as_deref())?;
    let board = pick_board(catalog, &cfg.device.card, true)?;
    let mut keys: Vec<u8> = Vec::new();
    let mut sweep = false;
    for word in &pattern {
        match word.as_str() {
            "slots" => keys.extend(cfg.slots.iter().map(|s| s.key)),
            "sweep" => sweep = true,
            n => keys.push(n.parse().map_err(|_| anyhow!("unknown pattern {n:?}"))?),
        }
    }
    if keys.is_empty() && !sweep {
        bail!("give key numbers, `slots` (needs slots in your config) or `sweep`");
    }
    let mut dev = OryxDevice::open(&board)?;
    println!("connected: {}", dev.describe());
    println!("keyboard reports host lighting: {}", dev.host_lighting(true)?);
    dev.fill(Rgb::OFF)?;
    if sweep {
        // Walk the keys in number order so the numbering can be followed by eye.
        let step = Duration::from_millis((secs * 1000 / board.keys.len() as u64).max(20));
        for key in &board.keys {
            dev.set_led(key.led, color)?;
            std::thread::sleep(step);
            dev.set_led(key.led, Rgb::OFF)?;
        }
    } else {
        for index in &keys {
            let key = board.by_index(*index).ok_or_else(|| anyhow!("{} has no key {index}", board.card))?;
            dev.set_led(key.led, color)?;
        }
        println!("lit keys {keys:?} for {secs}s");
        std::thread::sleep(Duration::from_secs(secs));
    }
    println!("keyboard reports host lighting after release: {}", dev.host_lighting(false)?);
    Ok(())
}

fn open_device(backend: Backend, board: &Board) -> Result<(Box<dyn Device>, String)> {
    match backend {
        Backend::Dry => Ok((Box::new(DryDevice::new(board.clone())), "dry".to_string())),
        Backend::Auto => {
            let dev = OryxDevice::open(board)?;
            let layout = dev.layout.clone();
            Ok((Box::new(dev), layout))
        }
    }
}

fn run_action(action: Action) {
    let field = |name: &str| action.env.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or("");
    eprintln!("slot key {} pressed ({}): running action", field("HARDPOINT_KEY"), field("HARDPOINT_TITLE"));
    std::thread::spawn(move || {
        let status = std::process::Command::new("sh").arg("-c").arg(&action.command).envs(action.env).status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("action exited with {s}"),
            Err(e) => eprintln!("action could not start: {e}"),
        }
    });
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn hour() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().hour()
}

fn run(catalog: &Catalog, config_path: Option<PathBuf>, backend: Option<Backend>) -> Result<()> {
    let (cfg, origin) = Config::load(config_path.as_deref())?;
    let backend = backend.unwrap_or(cfg.device.backend);
    let board = pick_board(catalog, &cfg.device.card, backend == Backend::Auto)?;
    for slot in &cfg.slots {
        if board.by_index(slot.key).is_none() {
            bail!("slot key {} does not exist on {}", slot.key, board.card);
        }
    }
    eprintln!("hardpoint {} | config: {origin} | card: {}", env!("CARGO_PKG_VERSION"), board.card);
    if cfg.slots.is_empty() {
        eprintln!("no slots are configured, so no keys will be lit (see examples/)");
    }

    let (tx, rx) = mpsc::channel::<BusEvent>();
    let snapshot = Arc::new(Mutex::new(serde_json::Value::Null));
    if cfg.sources.socket.enabled {
        let path = config::socket_path();
        source::socket::serve(path.clone(), tx.clone(), snapshot.clone())?;
        eprintln!("socket: {}", path.display());
    }
    if cfg.sources.opencode.enabled {
        source::opencode::spawn(cfg.sources.opencode.clone(), tx.clone());
    }

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))?;
    }

    let mut engine = Engine::new(cfg.clone(), board.clone());
    let mut dev: Option<Box<dyn Device>> = None;
    let mut telemetry: Option<Telemetry> = None;
    let mut sent: Option<Vec<Rgb>> = None;
    let mut layer = 0u8;
    let mut presses = 0u64;
    let mut source_ok: std::collections::BTreeMap<String, bool> = Default::default();
    let mut last_open: Option<Instant> = None;
    let mut last_open_error = String::new();
    let mut last_frame = Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now);
    let mut last_flush = Instant::now();

    while !stop.load(Ordering::SeqCst) {
        if dev.is_none() && last_open.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            last_open = Some(Instant::now());
            match open_device(backend, &board) {
                Ok((d, layout)) => {
                    eprintln!("device: {}", d.describe());
                    if cfg.telemetry.enabled && backend == Backend::Auto {
                        telemetry = Some(Telemetry::open(
                            config::state_dir().join("telemetry"),
                            &board.card,
                            &layout,
                            board.keys.len(),
                            &today(),
                        )?);
                    }
                    dev = Some(d);
                    sent = None;
                    layer = 0;
                    last_open_error.clear();
                }
                Err(e) => {
                    let text = format!("{e:#}");
                    if text != last_open_error {
                        eprintln!("device unavailable: {text} (retrying)");
                        last_open_error = text;
                    }
                }
            }
        }

        let now = Instant::now();
        let mut lost = None;
        match dev.as_mut() {
            None => std::thread::sleep(Duration::from_millis(20)),
            Some(d) => match d.poll(15) {
                Err(e) => lost = Some(e),
                Ok(events) => {
                    for event in events {
                        match event {
                            DeviceEvent::Layer(l) => layer = l,
                            DeviceEvent::KeyUp { .. } => {}
                            DeviceEvent::KeyDown { row, col } => {
                                presses += 1;
                                if let (Some(t), Some(key)) = (telemetry.as_mut(), board.by_matrix(row, col)) {
                                    t.record(layer, key.index, &today())?;
                                }
                                if let Some(action) = engine.press_matrix(row, col, now) {
                                    run_action(action);
                                }
                            }
                        }
                    }
                }
            },
        }

        while let Ok(event) = rx.try_recv() {
            match event {
                BusEvent::Agent(update) => engine.apply(update, now),
                BusEvent::Gone { source, session } => engine.gone(&source, &session),
                BusEvent::Press(key) => {
                    if let Some(action) = engine.press(key, now) {
                        run_action(action);
                    }
                }
                BusEvent::Source { name, ok, message } => {
                    if source_ok.insert(name.clone(), ok) != Some(ok) {
                        eprintln!("source {name}: {}", if ok { message } else { format!("disconnected ({message})") });
                    }
                }
            }
        }

        if lost.is_none() && last_frame.elapsed() >= Duration::from_millis(80) {
            last_frame = Instant::now();
            engine.expire(now);
            let frame = engine.frame(now, hour());
            if let Some(d) = dev.as_mut() {
                if let Err(e) = render(d.as_mut(), &frame, &mut sent) {
                    lost = Some(e);
                }
            }
            let mut status = engine.snapshot();
            status["card"] = serde_json::json!(board.card);
            status["device"] = serde_json::json!(dev.as_ref().map(|d| d.describe()));
            status["lighting"] = serde_json::json!(if frame.is_some() { "hardpoint" } else { "firmware" });
            status["layer"] = serde_json::json!(layer);
            status["presses_seen"] = serde_json::json!(presses);
            status["sources"] = serde_json::json!(source_ok);
            status["telemetry_today"] = serde_json::json!(telemetry.as_ref().map(|t| t.total()));
            if let Ok(mut shared) = snapshot.lock() {
                *shared = status;
            }
        }

        if let Some(e) = lost {
            // Unplugged, or a KVM moved the keyboard to another computer.
            eprintln!("device lost: {e:#}");
            dev = None;
            sent = None;
        }

        if last_flush.elapsed() >= Duration::from_secs(30) {
            last_flush = Instant::now();
            if let Some(t) = telemetry.as_mut() {
                t.flush()?;
            }
        }
    }

    if let Some(t) = telemetry.as_mut() {
        t.flush()?;
    }
    if let Some(d) = dev.as_mut() {
        if sent.is_some() {
            d.release()?;
            d.flush()?;
        }
    }
    if cfg.sources.socket.enabled {
        let _ = std::fs::remove_file(config::socket_path());
    }
    eprintln!("hardpoint stopped; lighting returned to the keyboard");
    Ok(())
}

/// Send only what changed since the last frame.
fn render(dev: &mut dyn Device, frame: &Option<Vec<Rgb>>, sent: &mut Option<Vec<Rgb>>) -> Result<()> {
    match (frame, sent.as_ref()) {
        (None, None) => return Ok(()),
        (None, Some(_)) => dev.release()?,
        (Some(next), None) => {
            // Taking over: one fill for the most common colour, then the rest.
            let mut counts = std::collections::HashMap::new();
            for c in next {
                *counts.entry((c.r, c.g, c.b)).or_insert(0usize) += 1;
            }
            let (r, g, b) = counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c).unwrap_or((0, 0, 0));
            let common = Rgb::new(r, g, b);
            dev.fill(common)?;
            for (led, c) in next.iter().enumerate() {
                if *c != common {
                    dev.set_led(led as u8, *c)?;
                }
            }
        }
        (Some(next), Some(prev)) => {
            for (led, c) in next.iter().enumerate() {
                if prev.get(led) != Some(c) {
                    dev.set_led(led as u8, *c)?;
                }
            }
        }
    }
    *sent = frame.clone();
    dev.flush()
}
