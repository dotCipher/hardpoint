use anyhow::{bail, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    pub fn parse(s: &str) -> Result<Rgb> {
        let hex = s.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.is_ascii() {
            bail!("colour must be #rrggbb, got {s:?}");
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16);
        match (byte(0), byte(2), byte(4)) {
            (Ok(r), Ok(g), Ok(b)) => Ok(Rgb { r, g, b }),
            _ => bail!("colour must be #rrggbb, got {s:?}"),
        }
    }

    pub fn scale(self, factor: f32) -> Rgb {
        let f = factor.clamp(0.0, 1.0);
        let ch = |v: u8| (v as f32 * f).round() as u8;
        Rgb::new(ch(self.r), ch(self.g), ch(self.b))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Rgb, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum AgentState {
    Idle,
    Running,
    NeedsYou,
    Done,
    Failed,
}

impl AgentState {
    /// Which session wins a slot when there are more sessions than slots.
    pub fn priority(self) -> u8 {
        match self {
            AgentState::NeedsYou => 4,
            AgentState::Failed => 3,
            AgentState::Running => 2,
            AgentState::Done => 1,
            AgentState::Idle => 0,
        }
    }
}

/// What a source reports about one agent session. Sources know nothing about
/// keyboards; the engine decides where and how this is shown.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentUpdate {
    pub source: String,
    pub session: String,
    pub state: AgentState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

#[derive(Clone, Debug)]
pub enum BusEvent {
    Agent(AgentUpdate),
    Gone { source: String, session: String },
    /// A key press injected over the socket, by vendor key index.
    Press(u8),
    Source { name: String, ok: bool, message: String },
    /// The catalog on disk was replaced by an update.
    CatalogChanged,
    /// Something for the gatekeeper, and where to send its answer.
    Gate { request: Request, reply: std::sync::mpsc::Sender<serde_json::Value> },
}

/// One line of the socket protocol.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    Agent(AgentUpdate),
    Gone { source: String, session: String },
    Press { key: u8 },
    Status,
    // The gatekeeper. An agent may list, read, propose, undo and look at the
    // journal. Keeping, approving and declining are a person's to do.
    Devices,
    Read { device: String, capability: String },
    Propose { device: String, capability: String, value: serde_json::Value },
    Undo { change: u64 },
    Changes,
    Keep { change: u64 },
    Approve { change: u64 },
    Decline { change: u64 },
    /// The catalog on disk has changed; read it again.
    Reload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_scales_colours() {
        assert_eq!(Rgb::parse("#ff8000").unwrap(), Rgb::new(255, 128, 0));
        assert_eq!(Rgb::parse("0a0b0c").unwrap(), Rgb::new(10, 11, 12));
        assert!(Rgb::parse("#fff").is_err());
        assert!(Rgb::parse("#gggggg").is_err());
        assert_eq!(Rgb::new(200, 100, 0).scale(0.5), Rgb::new(100, 50, 0));
        assert_eq!(Rgb::new(200, 100, 0).scale(7.0), Rgb::new(200, 100, 0));
    }

    #[test]
    fn socket_requests_round_trip() {
        let line = r#"{"cmd":"agent","source":"claude","session":"s1","state":"needs-you"}"#;
        match serde_json::from_str::<Request>(line).unwrap() {
            Request::Agent(u) => assert_eq!(u.state, AgentState::NeedsYou),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            serde_json::from_str::<Request>(r#"{"cmd":"press","key":36}"#).unwrap(),
            Request::Press { key: 36 }
        ));
    }
}
