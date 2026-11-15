mod board;
mod catalog;
mod config;
mod device;
mod discover;
mod engine;
mod gate;
mod mcp;
mod model;
mod source;
mod telemetry;
mod update;

use anyhow::{anyhow, bail, Result};
use board::Board;
use catalog::Catalog;
use clap::{Parser, Subcommand};
use config::{Backend, Config};
use device::{dry::DryDevice, oryx::OryxDevice, Device, DeviceEvent};
use engine::{Action, Engine};
use gate::{Gate, Target};
use model::{AgentState, AgentUpdate, BusEvent, Request, Rgb};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use telemetry::Telemetry;

/// The adapter this build can drive a keyboard through.
const KEYBOARD_ADAPTER: &str = "oryx-hid";

/// What this build can actually reach on the keyboard it drives. Everything
/// else a card lists is described to an agent, but cannot be acted on yet.
const REACHABLE: [&str; 5] = ["light.per_key", "light.global", "identity.firmware", "keys.layer", "keys.position_events"];

#[derive(Parser)]
#[command(name = "hardpoint", version, about = "Cards that tell your agent what each of your devices can do, what it may change, and what needs your yes first")]
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
    /// Serve the gatekeeper to an agent over the Model Context Protocol, on
    /// standard input and output. Needs `hardpoint run` to be running.
    Mcp,
    /// List every change an agent has proposed, and what became of it.
    Changes,
    /// Put back the value from before a change.
    Undo { change: u64 },
    /// Keep a guarded change, so it does not go back. For a person to type.
    Keep { change: u64 },
    /// Approve a gated change, so it is made. For a person to type.
    Approve { change: u64 },
    /// Say no to a change that is waiting for an answer.
    Decline { change: u64 },
    /// Bring an installed copy up to date. The catalog is updated; the
    /// program is only rebuilt with --program.
    Update {
        /// Only say what an update would do.
        #[arg(long)]
        check: bool,
        /// Also rebuild the program itself from the newest source.
        #[arg(long)]
        program: bool,
        /// Accept a catalog update that makes some label less strict. For a person to type.
        #[arg(long)]
        allow_looser: bool,
    },
    /// Start Hardpoint at login: install, uninstall, restart or status.
    Service { action: String },
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
        Command::Mcp => mcp::serve(&|request| {
            source::socket::request(&config::socket_path(), &request).map_err(|e| format!("{e:#}"))
        }),
        Command::Changes => send(Request::Changes),
        Command::Undo { change } => send(Request::Undo { change }),
        Command::Decline { change } => send(Request::Decline { change }),
        Command::Update { check, program, allow_looser } => update_now(&catalog()?, check, program, allow_looser),
        Command::Service { action } => {
            println!("{}", update::service(&action)?);
            Ok(())
        }
        Command::Keep { change } => by_a_person(Request::Keep { change }),
        Command::Approve { change } => by_a_person(Request::Approve { change }),
    }
}

/// Keeping and approving are a person's answer. An agent's shell has no
/// terminal attached, so these refuse to run without one. That is a speed
/// bump and not a wall: anything running as you could reach the daemon's
/// socket directly. The key on the keyboard is the answer that cannot be faked.
fn by_a_person(request: Request) -> Result<()> {
    at_a_terminal()?;
    send(request)
}

fn at_a_terminal() -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        bail!("this is a person's answer, so it has to be typed at a terminal");
    }
    Ok(())
}

fn update_now(catalog: &Catalog, check: bool, program: bool, allow_looser: bool) -> Result<()> {
    if allow_looser {
        at_a_terminal()?;
    }
    let outcome = update::catalog(&catalog.root, allow_looser, !check)?;
    println!("{}", outcome.said);
    if outcome.changed {
        // A running daemon picks the new catalog up without a restart.
        let _ = source::socket::request(&config::socket_path(), &Request::Reload);
    }
    if program && !check {
        println!("{}", update::program()?);
    } else if update::program_is_behind()? {
        println!("the program itself has a newer version. It never updates by itself: run `hardpoint update --program` when you want it.");
    } else {
        println!("the program is up to date");
    }
    Ok(())
}

fn send(request: Request) -> Result<()> {
    let reply = source::socket::request(&config::socket_path(), &request)?;
    if reply["ok"].as_bool() != Some(true) {
        bail!("daemon refused: {}", reply["error"].as_str().unwrap_or("unknown error"));
    }
    if let Some(shown) = reply.get("status").or(reply.get("result")) {
        println!("{}", serde_json::to_string_pretty(shown)?);
    }
    Ok(())
}

/// What an agent has asked to be lit. Kept apart from the slots, so it can be
/// read back, undone, and never mistaken for a session's light.
#[derive(Default)]
struct Lights {
    keys: BTreeMap<u8, Rgb>,
    all: Option<Rgb>,
}

/// The one device this build can reach: the keyboard it is driving.
struct Reach<'a> {
    board: &'a Board,
    lights: &'a mut Lights,
    firmware: Option<String>,
    layer: u8,
    presses: u64,
}

fn colour(value: &Value) -> Result<Option<Rgb>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Rgb::parse(text).map(Some).map_err(|e| e.to_string()),
        _ => Err("a colour is written \"#rrggbb\", or null for none".to_string()),
    }
}

impl Target for Reach<'_> {
    fn read(&mut self, device: &str, capability: &str) -> Result<Value, String> {
        if device != self.board.card {
            return Err(format!("this build cannot reach {device} yet"));
        }
        match capability {
            "light.per_key" => Ok(json!(self.lights.keys.iter().map(|(key, c)| (key.to_string(), c.hex())).collect::<BTreeMap<_, _>>())),
            "light.global" => Ok(json!(self.lights.all.map(Rgb::hex))),
            "identity.firmware" => self.firmware.clone().map(Value::from).ok_or_else(|| "the keyboard is not connected".to_string()),
            "keys.layer" => Ok(json!(self.layer)),
            // Counts only: which keys, in what order, never leaves the daemon.
            "keys.position_events" => Ok(json!({ "presses_seen": self.presses })),
            other => Err(format!("this build cannot reach {other} on {device} yet")),
        }
    }

    fn write(&mut self, device: &str, capability: &str, value: &Value) -> Result<(), String> {
        if device != self.board.card {
            return Err(format!("this build cannot reach {device} yet"));
        }
        let key = |text: &str| -> Result<u8, String> {
            text.parse::<u8>().ok().filter(|k| self.board.by_index(*k).is_some()).ok_or_else(|| format!("{device} has no key {text}"))
        };
        match capability {
            "light.global" => self.lights.all = colour(value)?,
            "light.per_key" => {
                let form = "give {\"key\": 12, \"color\": \"#rrggbb\"}, with null for the colour to clear it";
                let fields = value.as_object().ok_or(form)?;
                if let Some(index) = fields.get("key") {
                    let index = key(&index.as_u64().ok_or(form)?.to_string())?;
                    match colour(fields.get("color").unwrap_or(&Value::Null))? {
                        Some(c) => self.lights.keys.insert(index, c),
                        None => self.lights.keys.remove(&index),
                    };
                } else {
                    // The form `read` returns: every lit key at once. This is how an undo puts things back.
                    let mut lit = BTreeMap::new();
                    for (name, c) in fields {
                        if let Some(c) = colour(c)? {
                            lit.insert(key(name)?, c);
                        }
                    }
                    self.lights.keys = lit;
                }
            }
            other => return Err(format!("this build cannot change {other} on {device} yet")),
        }
        Ok(())
    }
}

/// Lay what an agent asked for over the slots' frame. Slots always show through.
fn compose(frame: Option<Vec<Rgb>>, lights: &Lights, board: &Board, slot_leds: &[u8], answer: Option<(u8, Rgb)>) -> Option<Vec<Rgb>> {
    if lights.keys.is_empty() && lights.all.is_none() && answer.is_none() {
        return frame;
    }
    let mut out = match (&frame, lights.all) {
        (_, Some(all)) => vec![all; board.led_count()],
        (Some(frame), None) => frame.clone(),
        (None, None) => vec![Rgb::OFF; board.led_count()],
    };
    for (index, c) in &lights.keys {
        if let Some(key) = board.by_index(*index) {
            out[key.led as usize] = *c;
        }
    }
    if let Some(frame) = &frame {
        for led in slot_leds {
            out[*led as usize] = frame[*led as usize];
        }
    }
    if let Some((led, c)) = answer {
        out[led as usize] = c;
    }
    Some(out)
}

/// What to tell whoever proposed a change: what happened, and what a person has to do next.
fn explain(change: gate::Change) -> Value {
    use gate::State;
    let left = change.seconds_left.unwrap_or(0);
    let next = match change.state {
        State::Applied => "Done. It can be put back with `undo`.".to_string(),
        State::Holding => format!(
            "Done, but it goes back by itself in {left} seconds unless the person keeps it. You cannot keep it for them. \
             Ask them to press the lit key, or to type `hardpoint keep {}` in a terminal.",
            change.id
        ),
        State::Awaiting => format!(
            "Nothing has been done. This waits {left} seconds for the person to approve it. You cannot approve it for them. \
             Ask them to press the lit key, or to type `hardpoint approve {}` in a terminal. If nobody answers, nothing happens.",
            change.id
        ),
        State::Kept => "Kept. It stays.".to_string(),
        State::Reverted => "Nobody kept it, so it was put back.".to_string(),
        State::Undone => "Put back.".to_string(),
        State::Declined => "Declined. Nothing was done.".to_string(),
        State::Expired => "Nobody approved it in time. Nothing was done.".to_string(),
    };
    let mut out = json!(change);
    out["next"] = json!(next);
    out
}

/// Every attached device with a card, described for an agent.
fn devices(catalog: &Catalog, cfg: &Config, driven: &str) -> Result<Value, String> {
    let seen = discover::observe().map_err(|e| format!("{e:#}"))?;
    let mut cards: Vec<&catalog::Card> = catalog.card(driven).into_iter().collect();
    for card in seen.iter().filter_map(|s| catalog.recognise(s)) {
        if !cards.iter().any(|c| c.device.id == card.device.id) {
            cards.push(card);
        }
    }
    let described: Vec<Value> = cards
        .iter()
        .map(|card| {
            let abilities: Vec<Value> = card
                .capabilities
                .iter()
                .map(|cap| {
                    let word = catalog.vocabulary.get(&cap.id);
                    json!({
                        "id": cap.id,
                        "what": word.map(|w| w.summary.as_str()),
                        "kind": word.map(|w| w.kind.as_str()),
                        "label": catalog.effective_risk(cap, stricter(cfg, &card.device.id, &cap.id)),
                        "if_done_carelessly": word.and_then(|w| w.harm.as_deref()),
                        "checked": cap.checked(),
                        "note": cap.note,
                        "reachable": card.device.id == driven && cap.adapter == KEYBOARD_ADAPTER && REACHABLE.contains(&cap.id.as_str()),
                    })
                })
                .collect();
            json!({ "id": card.device.id, "name": format!("{} {}", card.device.vendor, card.device.name), "kind": card.device.kind, "abilities": abilities })
        })
        .collect();
    Ok(json!({
        "devices": described,
        "values": {
            "light.global": "\"#rrggbb\", or null to clear",
            "light.per_key": "{\"key\": 12, \"color\": \"#rrggbb\"}, with null for the colour to clear that key",
        },
        "note": "Only abilities marked reachable can be read or changed by this build. The rest are described so you know what they are and how risky they would be.",
    }))
}

/// The owner's stricter label for one ability of one device, if they set one.
fn stricter(cfg: &Config, device: &str, capability: &str) -> Option<catalog::Risk> {
    cfg.stricter.iter().filter(|s| s.device == device && s.capability == capability).map(|s| s.risk).max()
}

fn gatekeeper(request: &Request, catalog: &Catalog, cfg: &Config, gate: &mut Gate, reach: &mut Reach, now: Instant) -> Result<Value, String> {
    let find = |device: &str, capability: &str| {
        let card = catalog.card(device).ok_or_else(|| format!("there is no card for {device}"))?;
        let cap = card.capabilities.iter().find(|c| c.id == capability).ok_or_else(|| format!("the card for {device} lists no {capability}"))?;
        Ok::<_, String>(cap)
    };
    match request {
        Request::Devices => devices(catalog, cfg, &reach.board.card),
        Request::Changes => Ok(json!(gate.changes(now))),
        Request::Read { device, capability } => {
            let cap = find(device, capability)?;
            Ok(json!({ "value": reach.read(device, capability)?, "checked": cap.checked() }))
        }
        Request::Propose { device, capability, value } => {
            let cap = find(device, capability)?;
            if cap.adapter == "manual" {
                return Err(format!("Only a person can do this; software has no way to. {}", cap.note.as_deref().unwrap_or("")));
            }
            let risk = catalog.effective_risk(cap, stricter(cfg, device, capability));
            gate.propose(device, capability, risk, value.clone(), reach, now).map(explain)
        }
        Request::Undo { change } => gate.undo(*change, reach, now).map(explain),
        Request::Keep { change } => gate.keep(*change, now).map(explain),
        Request::Approve { change } => gate.approve(*change, reach, now).map(explain),
        Request::Decline { change } => gate.decline(*change, reach, now).map(explain),
        _ => Err("that is not a request for the gatekeeper".to_string()),
    }
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
    println!("\nDisplays, audio devices and things on the home network are not looked for yet.");
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

    // Cards can keep themselves current. The program never does.
    let update_note = Arc::new(Mutex::new(String::new()));
    if cfg.update.catalog {
        let (root, every, tx, note) = (catalog.root.clone(), cfg.update.every_hours.max(1), tx.clone(), update_note.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(20));
            let said = match update::catalog(&root, false, true) {
                Ok(outcome) => {
                    if outcome.changed {
                        let _ = tx.send(BusEvent::CatalogChanged);
                    }
                    let behind = update::program_is_behind().unwrap_or(false);
                    format!("{}{}", outcome.said, if behind { "; a newer program is available (`hardpoint update --program`)" } else { "" })
                }
                Err(e) => format!("could not check for updates: {e:#}"),
            };
            eprintln!("update: {said}");
            if let Ok(mut shared) = note.lock() {
                *shared = said;
            }
            std::thread::sleep(Duration::from_secs(every * 3600));
        });
    }
    let mut reloaded: Option<Catalog> = None;
    let mut reload = false;
    // Which commits the installed catalog and program came from, for `hardpoint status`.
    let mut installed = update::stamp();

    let mut engine = Engine::new(cfg.clone(), board.clone());
    let mut gate = Gate::new(Duration::from_secs(cfg.gate.hold_secs), Duration::from_secs(cfg.gate.approve_secs));
    let mut lights = Lights::default();
    let mut firmware: Option<String> = None;
    let slot_leds: Vec<u8> = cfg.slots.iter().filter_map(|s| board.by_index(s.key)).map(|k| k.led).collect();
    let answer_key = cfg.gate.key.and_then(|k| board.by_index(k)).copied();
    if cfg.gate.key.is_some() && answer_key.is_none() {
        bail!("the gate's answer key {} does not exist on {}", cfg.gate.key.unwrap_or(0), board.card);
    }
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
    let started = Instant::now();

    while !stop.load(Ordering::SeqCst) {
        if std::mem::take(&mut reload) {
            match Catalog::load(&catalog.root) {
                Ok(fresh) => {
                    eprintln!("catalog reloaded: {} cards", fresh.cards.len());
                    reloaded = Some(fresh);
                    installed = update::stamp();
                }
                Err(e) => eprintln!("the updated catalog could not be read, so the old one stays in use: {e:#}"),
            }
        }
        let catalog: &Catalog = reloaded.as_ref().unwrap_or(catalog);
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
                    firmware = Some(layout);
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
                                // A real press on the answer key is a person saying yes. Only a press
                                // that came from the keyboard counts: one sent over the socket does not.
                                if let (Some(key), Some(waiting)) = (answer_key.filter(|k| k.row == row && k.col == col), gate.waiting(now)) {
                                    let _ = key;
                                    let mut reach = Reach { board: &board, lights: &mut lights, firmware: firmware.clone(), layer, presses };
                                    let answered = match waiting.state {
                                        gate::State::Holding => gate.keep(waiting.id, now),
                                        _ => gate.approve(waiting.id, &mut reach, now),
                                    };
                                    match answered {
                                        Ok(change) => eprintln!("change {} ({} on {}): a person said yes", change.id, change.capability, change.device),
                                        Err(error) => eprintln!("change {}: {error}", waiting.id),
                                    }
                                    continue;
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
                BusEvent::CatalogChanged => reload = true,
                BusEvent::Gate { request: Request::Reload, reply } => {
                    reload = true;
                    let _ = reply.send(json!({ "ok": true }));
                }
                BusEvent::Gate { request, reply } => {
                    let mut reach = Reach { board: &board, lights: &mut lights, firmware: firmware.clone(), layer, presses };
                    let answer = match gatekeeper(&request, catalog, &cfg, &mut gate, &mut reach, now) {
                        Ok(result) => json!({ "ok": true, "result": result }),
                        Err(error) => json!({ "ok": false, "error": error }),
                    };
                    let _ = reply.send(answer);
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
            {
                let mut reach = Reach { board: &board, lights: &mut lights, firmware: firmware.clone(), layer, presses };
                for change in gate.tick(&mut reach, now) {
                    eprintln!("change {} ({} on {}): {}", change.id, change.capability, change.device, explain(change.clone())["next"].as_str().unwrap_or(""));
                }
            }
            // While an answer is wanted, the answer key pulses: amber to keep a change, red to approve one.
            let waiting = gate.waiting(now);
            let pulse = 0.3 + 0.7 * (0.5 + 0.5 * (now.duration_since(started).as_secs_f32() * 5.2).sin());
            let answer = answer_key.zip(waiting.as_ref()).map(|(key, change)| {
                let c = if change.state == gate::State::Holding { cfg.colors.needs_you } else { cfg.colors.failed };
                (key.led, c.scale(pulse))
            });
            let frame = compose(engine.frame(now, hour()), &lights, &board, &slot_leds, answer);
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
            status["waiting_for_an_answer"] = json!(waiting.map(explain));
            status["catalog"] = json!({ "cards": catalog.cards.len(), "from": catalog.root });
            status["installed"] = json!(installed);
            status["update"] = json!(update_note.lock().map(|note| note.clone()).unwrap_or_default());
            if let Ok(mut shared) = snapshot.lock() {
                *shared = status;
            }
        }

        if let Some(e) = lost {
            // Unplugged, or a KVM moved the keyboard to another computer.
            eprintln!("device lost: {e:#}");
            dev = None;
            firmware = None;
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
