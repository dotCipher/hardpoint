//! OpenCode, read from its background service's event stream. Nothing is
//! installed into OpenCode: this is a plain HTTP client of the local API.

use crate::config::{opencode_service_file, OpencodeCfg};
use crate::model::{AgentState, AgentUpdate, BusEvent};
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::time::Duration;

const SOURCE: &str = "opencode";

struct Service {
    url: String,
    auth: String,
}

fn service() -> Result<Service> {
    let path = opencode_service_file();
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let json: Value = serde_json::from_str(&text)?;
    let url = json["url"].as_str().ok_or_else(|| anyhow!("no url in {}", path.display()))?;
    if !is_local(url) {
        // The password in this file is only ever sent to this computer.
        return Err(anyhow!("{} points somewhere other than this computer", path.display()));
    }
    let password = json["password"].as_str().unwrap_or_default();
    let token = base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"));
    Ok(Service { url: url.trim_end_matches('/').to_string(), auth: format!("Basic {token}") })
}

fn is_local(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or("");
    let host = rest.split(['/', ':']).next().unwrap_or("");
    matches!(host, "127.0.0.1" | "localhost" | "[")
        && (host != "[" || rest.starts_with("[::1]"))
}

/// What one stream event means for one session, if anything.
#[derive(Debug, PartialEq, Eq)]
pub enum Meaning {
    State(AgentState, Option<String>),
    /// A question or permission prompt was answered or withdrawn.
    Answered,
    Gone,
}

pub fn interpret(event: &Value) -> Option<(String, Meaning)> {
    let data = &event["data"];
    let session = |v: &Value| v["sessionID"].as_str().map(str::to_string);
    let kind = event["type"].as_str()?;
    let meaning = match kind {
        "session.execution.started" => Meaning::State(AgentState::Running, None),
        "session.execution.succeeded" => Meaning::State(AgentState::Done, None),
        "session.execution.failed" => Meaning::State(AgentState::Failed, None),
        "session.execution.interrupted" => Meaning::State(AgentState::Idle, None),
        "permission.asked" => {
            let detail = data["message"].as_str().or(data["action"].as_str()).map(str::to_string);
            Meaning::State(AgentState::NeedsYou, detail)
        }
        "form.created" => {
            let form = &data["form"];
            return Some((session(form)?, Meaning::State(AgentState::NeedsYou, form["title"].as_str().map(str::to_string))));
        }
        "permission.replied" | "form.replied" | "form.cancelled" => {
            let id = session(data).or_else(|| session(&data["form"]))?;
            return Some((id, Meaning::Answered));
        }
        "session.deleted" => Meaning::Gone,
        _ => return None,
    };
    Some((session(data)?, meaning))
}

struct Client {
    agent: ureq::Agent,
    service: Service,
    cfg: OpencodeCfg,
    /// Session id -> (title, directory, is a top-level session).
    info: HashMap<String, (Option<String>, Option<String>, bool)>,
    /// Sessions with an unanswered prompt.
    asking: HashSet<String>,
}

impl Client {
    fn get(&self, path: &str) -> Result<Value> {
        let response = self
            .agent
            .get(&format!("{}{}", self.service.url, path))
            .set("Authorization", &self.service.auth)
            .timeout(Duration::from_secs(5))
            .call()
            .map_err(|e| anyhow!("GET {path}: {e}"))?;
        Ok(serde_json::from_reader(response.into_reader())?)
    }

    fn info(&mut self, id: &str) -> (Option<String>, Option<String>, bool) {
        if let Some(known) = self.info.get(id) {
            return known.clone();
        }
        let fetched = match self.get(&format!("/api/session/{id}")) {
            Ok(v) => {
                let d = &v["data"];
                (
                    d["title"].as_str().map(str::to_string),
                    d["location"]["directory"].as_str().map(str::to_string),
                    d["parentID"].as_str().is_none(),
                )
            }
            // Unknown sessions are shown rather than hidden.
            Err(_) => (None, None, true),
        };
        self.info.insert(id.to_string(), fetched.clone());
        fetched
    }

    fn report(&mut self, bus: &Sender<BusEvent>, id: &str, state: AgentState, detail: Option<String>) -> Result<()> {
        let (title, directory, root) = self.info(id);
        if self.cfg.roots_only && !root {
            return Ok(());
        }
        let update = AgentUpdate { source: SOURCE.into(), session: id.into(), state, title, detail, directory };
        bus.send(BusEvent::Agent(update)).map_err(|_| anyhow!("bus closed"))
    }

    fn run(&mut self, bus: &Sender<BusEvent>) -> Result<()> {
        // Sessions already running when we connect.
        let active = self.get("/api/session/active")?;
        let ids: Vec<String> = active["data"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        for id in ids {
            self.report(bus, &id, AgentState::Running, None)?;
        }
        let stream = self
            .agent
            .get(&format!("{}/api/event", self.service.url))
            .set("Authorization", &self.service.auth)
            .call()
            .map_err(|e| anyhow!("GET /api/event: {e}"))?;
        let _ = bus.send(BusEvent::Source { name: SOURCE.into(), ok: true, message: format!("connected to {}", self.service.url) });
        for line in BufReader::new(stream.into_reader()).lines() {
            let line = line?;
            let Some(payload) = line.strip_prefix("data:") else { continue };
            let Ok(event) = serde_json::from_str::<Value>(payload) else { continue };
            let Some((id, meaning)) = interpret(&event) else { continue };
            match meaning {
                Meaning::State(state, detail) => {
                    if state == AgentState::NeedsYou {
                        self.asking.insert(id.clone());
                    } else {
                        self.asking.remove(&id);
                    }
                    self.report(bus, &id, state, detail)?;
                }
                Meaning::Answered => {
                    if self.asking.remove(&id) {
                        self.report(bus, &id, AgentState::Running, None)?;
                    }
                }
                Meaning::Gone => {
                    self.info.remove(&id);
                    self.asking.remove(&id);
                    bus.send(BusEvent::Gone { source: SOURCE.into(), session: id }).map_err(|_| anyhow!("bus closed"))?;
                }
            }
        }
        Err(anyhow!("event stream ended"))
    }
}

/// Follow OpenCode forever, reconnecting when its service restarts.
pub fn spawn(cfg: OpencodeCfg, bus: Sender<BusEvent>) {
    std::thread::spawn(move || loop {
        let outcome = service().and_then(|service| {
            let agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(3))
                .timeout_read(Duration::from_secs(120))
                .build();
            Client { agent, service, cfg: cfg.clone(), info: HashMap::new(), asking: HashSet::new() }.run(&bus)
        });
        let message = outcome.err().map(|e| format!("{e:#}")).unwrap_or_default();
        if bus.send(BusEvent::Source { name: SOURCE.into(), ok: false, message }).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_secs(5));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_this_computer_counts_as_local() {
        assert!(is_local("http://127.0.0.1:4096"));
        assert!(is_local("http://localhost:4096/"));
        assert!(is_local("http://[::1]:4096"));
        assert!(!is_local("http://example.com:4096"));
        assert!(!is_local("https://127.0.0.1:4096"));
        assert!(!is_local("http://127.0.0.1.example.com"));
        assert!(!is_local("http://[2001:db8::1]:80"));
    }

    #[test]
    fn interprets_execution_events() {
        let e = json!({"type":"session.execution.started","data":{"sessionID":"ses_1"}});
        assert_eq!(interpret(&e), Some(("ses_1".into(), Meaning::State(AgentState::Running, None))));
        let e = json!({"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}});
        assert_eq!(interpret(&e), Some(("ses_1".into(), Meaning::State(AgentState::Done, None))));
        let e = json!({"type":"session.execution.failed","data":{"sessionID":"ses_1"}});
        assert_eq!(interpret(&e), Some(("ses_1".into(), Meaning::State(AgentState::Failed, None))));
    }

    #[test]
    fn interprets_prompts_and_ignores_noise() {
        let e = json!({"type":"permission.asked","data":{"id":"per_1","sessionID":"ses_2","action":"shell","resources":["rm"],"message":"Run rm?"}});
        assert_eq!(interpret(&e), Some(("ses_2".into(), Meaning::State(AgentState::NeedsYou, Some("Run rm?".into())))));
        let e = json!({"type":"form.created","data":{"form":{"id":"frm_1","sessionID":"ses_3","title":"Pick one","fields":[]}}});
        assert_eq!(interpret(&e), Some(("ses_3".into(), Meaning::State(AgentState::NeedsYou, Some("Pick one".into())))));
        let e = json!({"type":"permission.replied","data":{"sessionID":"ses_2"}});
        assert_eq!(interpret(&e), Some(("ses_2".into(), Meaning::Answered)));
        assert_eq!(interpret(&json!({"type":"session.text.delta","data":{"sessionID":"ses_2","delta":"hi"}})), None);
        assert_eq!(interpret(&json!({"type":"server.connected","data":{}})), None);
    }
}
