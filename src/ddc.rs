//! The `ddc-ci` adapter: a display's own controls, over the video cable.
//!
//! Hardpoint does not speak DDC itself. It asks a tool that already does:
//! `m1ddc` on Apple Silicon, `ddcutil` on Linux. The setup worker resolves
//! and installs supported helpers without blocking the device loop.
//!
//! Displays answer unreliably through docks and adapters, so every read is
//! checked and retried.

use serde_json::{json, Value};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long to leave a display alone between two exchanges.
const PAUSE: Duration = Duration::from_millis(350);

/// The controls this module knows, by their DDC number and the name `m1ddc` gives them.
const CONTROLS: [(i64, &str, i64); 4] = [(0x10, "luminance", 100), (0x12, "contrast", 100), (0x60, "input", 255), (0x62, "volume", 100)];

pub fn supports(vcp: i64) -> bool {
    crate::tools::helper("ddc-ci").is_some() && CONTROLS.iter().any(|(code, _, _)| *code == vcp)
}

/// One DDC exchange at a time, with a pause after each: a display that is
/// asked again too soon answers the previous question, or does not answer.
fn run(args: &[String]) -> Result<String, String> {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let tool = crate::tools::helper("ddc-ci").ok_or("the display helper is missing or this OS/architecture has no backend; run `hardpoint doctor` for details or `hardpoint setup` to retry setup")?;
    let mut last = LAST.lock().map_err(|_| "the display channel is unavailable".to_string())?;
    if let Some(wait) = last.and_then(|at| PAUSE.checked_sub(at.elapsed())) {
        std::thread::sleep(wait);
    }
    let out = Command::new(tool).args(args).output().map_err(|e| e.to_string());
    *last = Some(Instant::now());
    let out = out?;
    if !out.status.success() {
        return Err(format!("the display helper failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn control(vcp: i64) -> Result<(&'static str, i64), String> {
    CONTROLS.iter().find(|(code, _, _)| *code == vcp).map(|(_, name, most)| (*name, *most)).ok_or_else(|| format!("this build does not know display control {vcp:#04x}"))
}

/// One reading, if the display gave one that is possible at all.
fn once(vcp: i64, name: &str, most: i64) -> Result<Option<i64>, String> {
    let args: Vec<String> = if cfg!(target_os = "macos") { vec!["get".into(), name.into()] } else { vec!["getvcp".into(), format!("{vcp:x}"), "--terse".into()] };
    let said = run(&args)?;
    // `ddcutil --terse` answers "VCP 10 C 50 100"; `m1ddc` answers with the number alone.
    let number = if cfg!(target_os = "macos") { said.parse::<i64>().ok() } else { said.split_whitespace().nth(3).and_then(|n| n.parse().ok()) };
    Ok(number.filter(|n| (0..=most).contains(n)))
}

/// A reading is only believed when the display gives the same possible value
/// twice running. A single answer can be nonsense, or the answer to an
/// earlier question.
pub fn read(vcp: i64) -> Result<Value, String> {
    let (name, most) = control(vcp)?;
    let mut before = None;
    for _ in 0..8 {
        let now = once(vcp, name, most)?;
        if now.is_some() && now == before {
            return Ok(json!(now));
        }
        before = now;
    }
    Err("the display did not give the same answer twice. Displays behind a dock often answer unreliably".to_string())
}

/// A change is only reported as made once the display reads back the new
/// value. A display can drop a command without saying so.
pub fn write(vcp: i64, value: &Value) -> Result<(), String> {
    let (name, most) = control(vcp)?;
    let wanted = value.as_i64().filter(|n| (0..=most).contains(n)).ok_or_else(|| format!("{name} is a whole number from 0 to {most}"))?;
    let args: Vec<String> = if cfg!(target_os = "macos") { vec!["set".into(), name.into(), wanted.to_string()] } else { vec!["setvcp".into(), format!("{vcp:x}"), wanted.to_string()] };
    for _ in 0..3 {
        run(&args)?;
        if read(vcp).ok().and_then(|v| v.as_i64()) == Some(wanted) {
            return Ok(());
        }
    }
    Err(format!("the display did not take the change: {name} could not be confirmed at {wanted}"))
}
