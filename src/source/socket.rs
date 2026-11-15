//! A Unix socket that accepts one JSON request per line. This is how hooks,
//! scripts and `hardpoint emit` report state, and how `hardpoint status` reads it.

use crate::model::{BusEvent, Request};
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub type Snapshot = Arc<Mutex<serde_json::Value>>;

pub fn serve(path: PathBuf, bus: Sender<BusEvent>, snapshot: Snapshot) -> Result<()> {
    if let Some(dir) = path.parent() {
        let fresh = !dir.exists();
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        if fresh {
            restrict(dir, 0o700)?;
        }
    }
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            bail!("another hardpoint is already listening on {}", path.display());
        }
        std::fs::remove_file(&path).with_context(|| format!("removing stale {}", path.display()))?;
    }
    let listener = UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
    // Anything that can write to this socket can light keys and trigger your
    // press action, so only the owner may.
    restrict(&path, 0o600)?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let bus = bus.clone();
            let snapshot = snapshot.clone();
            std::thread::spawn(move || handle(stream, bus, snapshot));
        }
    });
    Ok(())
}

fn handle(stream: UnixStream, bus: Sender<BusEvent>, snapshot: Snapshot) {
    let Ok(mut writer) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }),
            Ok(Request::Status) => {
                let state = snapshot.lock().map(|s| s.clone()).unwrap_or_default();
                serde_json::json!({ "ok": true, "status": state })
            }
            Ok(request) => {
                let event = match request {
                    Request::Agent(update) => BusEvent::Agent(update),
                    Request::Gone { source, session } => BusEvent::Gone { source, session },
                    Request::Press { key } => BusEvent::Press(key),
                    Request::Status => unreachable!(),
                    request => {
                        // The gatekeeper answers from the main loop, which owns the device.
                        let (reply, answer) = std::sync::mpsc::channel();
                        let sent = bus.send(BusEvent::Gate { request, reply }).is_ok();
                        // Some readings are slow: a sleeping drive has to spin up before it answers.
                        let answer = answer.recv_timeout(std::time::Duration::from_secs(45));
                        let reply = match (sent, answer) {
                            (true, Ok(answer)) => answer,
                            _ => serde_json::json!({ "ok": false, "error": "the daemon did not answer" }),
                        };
                        if writeln!(writer, "{reply}").is_err() {
                            return;
                        }
                        continue;
                    }
                };
                serde_json::json!({ "ok": bus.send(event).is_ok() })
            }
        };
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
    }
}

fn restrict(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("setting permissions on {}", path.display()))
}

/// Send one request to a running daemon and return its reply.
pub fn request(path: &Path, request: &Request) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("no hardpoint daemon on {} (start one with `hardpoint run`)", path.display()))?;
    writeln!(stream, "{}", serde_json::to_string(request)?)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}
