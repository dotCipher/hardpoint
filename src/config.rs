use crate::model::Rgb;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub device: DeviceCfg,
    pub brightness: Brightness,
    pub lighting: Lighting,
    pub colors: Colors,
    #[serde(default, rename = "slot")]
    pub slots: Vec<SlotCfg>,
    pub sources: Sources,
    pub actions: Actions,
    pub telemetry: Telemetry,
    #[serde(default)]
    pub gate: GateCfg,
    /// Labels the owner has made stricter for their own devices.
    #[serde(default)]
    pub stricter: Vec<Stricter>,
    #[serde(default)]
    pub update: UpdateCfg,
    /// Install trusted adapter helpers for attached devices, without elevation.
    #[serde(default)]
    pub tools: ToolsCfg,
    /// Readings to keep an eye on. When one crosses its line, it shows up on
    /// a spare key the same way an agent that needs you does.
    #[serde(default)]
    pub watch: Vec<Watch>,
    /// "When this, set that." The owner's standing instructions.
    #[serde(default)]
    pub rule: Vec<Rule>,
}

/// One standing instruction: set an ability to a value at a time of day, or
/// for as long as an agent is in some state. A rule goes through the
/// gatekeeper like any other change, and can never make a gated one.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// A time of day, as "20:00". The change is made once, when the clock gets there.
    #[serde(default)]
    pub at: Option<String>,
    /// An agent state. The change is made when any session enters it, and
    /// put back when none is in it any more.
    #[serde(default, rename = "while")]
    pub during: Option<crate::model::AgentState>,
    pub device: String,
    pub capability: String,
    pub value: toml::Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    pub device: String,
    pub capability: String,
    /// Raise it when the reading is below this number.
    #[serde(default)]
    pub below: Option<f64>,
    /// Or when the reading is exactly this.
    #[serde(default)]
    pub equals: Option<toml::Value>,
    /// What to call it on the key. Defaults to the device and the reading.
    #[serde(default)]
    pub say: Option<String>,
    #[serde(default = "five_minutes")]
    pub every_secs: u64,
}

fn five_minutes() -> u64 {
    300
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ToolsCfg {
    pub auto_install: bool,
}

impl Default for ToolsCfg {
    fn default() -> Self {
        Self { auto_install: true }
    }
}

/// Whether the catalog keeps itself current. The program never updates itself.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UpdateCfg {
    /// Fetch new and corrected cards from the project while the daemon runs.
    pub catalog: bool,
    pub every_hours: u64,
}

impl Default for UpdateCfg {
    fn default() -> UpdateCfg {
        UpdateCfg { catalog: false, every_hours: 24 }
    }
}

/// How changes proposed by an agent are held and approved.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GateCfg {
    /// How long a guarded change waits to be kept before it goes back.
    pub hold_secs: u64,
    /// How long a gated change waits for approval before it is dropped.
    pub approve_secs: u64,
    /// A key that lights up when an answer is wanted. Pressing it means yes.
    pub key: Option<u8>,
}

impl Default for GateCfg {
    fn default() -> GateCfg {
        GateCfg { hold_secs: 10, approve_secs: 60, key: None }
    }
}

/// "Treat this ability of this device as at least this risky." It can only
/// raise a label: the gatekeeper takes the strictest of everything it is told.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stricter {
    pub device: String,
    pub capability: String,
    pub risk: crate::catalog::Risk,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceCfg {
    pub backend: Backend,
    /// A device card to use, as `vendor/model`. Empty means the first
    /// attached keyboard the catalog describes.
    pub card: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    Auto,
    Dry,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brightness {
    pub day: f32,
    pub night: f32,
    pub night_start: u32,
    pub night_end: u32,
}

impl Brightness {
    pub fn at_hour(&self, hour: u32) -> f32 {
        let night = if self.night_start <= self.night_end {
            hour >= self.night_start && hour < self.night_end
        } else {
            hour >= self.night_start || hour < self.night_end
        };
        if night {
            self.night
        } else {
            self.day
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdleMode {
    Release,
    Hold,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lighting {
    pub idle: IdleMode,
    pub base: Rgb,
    pub empty_slot: Rgb,
    pub done_hold_secs: u64,
    pub failed_hold_secs: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Colors {
    pub running: Rgb,
    pub needs_you: Rgb,
    pub done: Rgb,
    pub failed: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotKind {
    /// Shows one agent session; the engine decides which.
    Agent,
    /// Always shows `color` and runs `command` when pressed.
    Fixed,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotCfg {
    pub key: u8,
    pub kind: SlotKind,
    #[serde(default)]
    pub pressable: bool,
    #[serde(default)]
    pub color: Option<Rgb>,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    pub opencode: OpencodeCfg,
    pub socket: SocketCfg,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpencodeCfg {
    pub enabled: bool,
    pub roots_only: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocketCfg {
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actions {
    pub agent_press: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telemetry {
    pub enabled: bool,
}

impl Config {
    /// A config file only has to say what differs from the built-in
    /// settings: it is laid over them, table by table.
    pub fn parse(text: &str) -> Result<Config> {
        fn lay(under: &mut toml::Value, over: toml::Value) {
            match (under, over) {
                (toml::Value::Table(under), toml::Value::Table(over)) => {
                    for (key, value) in over {
                        match under.get_mut(&key) {
                            Some(slot) => lay(slot, value),
                            None => {
                                under.insert(key, value);
                            }
                        }
                    }
                }
                (slot, value) => *slot = value,
            }
        }
        let mut merged: toml::Value = toml::from_str(DEFAULT_CONFIG)?;
        lay(&mut merged, toml::from_str(text)?);
        let cfg: Config = merged.try_into()?;
        for rule in &cfg.rule {
            let timed = rule.at.as_deref().map(|at| at.len() == 5 && at.as_bytes()[2] == b':' && at.split(':').all(|n| n.parse::<u8>().is_ok()));
            match (timed, rule.during) {
                (Some(true), None) | (None, Some(_)) => {}
                (Some(false), _) => bail!("a rule's time is written like \"20:00\""),
                _ => bail!("a rule needs either `at` or `while`, and not both"),
            }
        }
        let mut seen = std::collections::HashSet::new();
        for slot in &cfg.slots {
            if !seen.insert(slot.key) {
                bail!("key {} is used by more than one slot", slot.key);
            }
            if slot.kind == SlotKind::Fixed && slot.color.is_none() {
                bail!("fixed slot on key {} needs a color", slot.key);
            }
        }
        Ok(cfg)
    }

    /// An explicit path must exist; otherwise the user file is used if present
    /// and the built-in defaults if not.
    pub fn load(explicit: Option<&Path>) -> Result<(Config, String)> {
        if let Some(path) = explicit {
            let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            return Ok((Config::parse(&text).with_context(|| format!("in {}", path.display()))?, path.display().to_string()));
        }
        let user = config_dir().join("config.toml");
        if user.exists() {
            let text = std::fs::read_to_string(&user).with_context(|| format!("reading {}", user.display()))?;
            return Ok((Config::parse(&text).with_context(|| format!("in {}", user.display()))?, user.display().to_string()));
        }
        Ok((Config::parse(DEFAULT_CONFIG)?, "built-in defaults".to_string()))
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("hardpoint")
}

pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("hardpoint")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("hardpoint")
}

pub fn socket_path() -> PathBuf {
    match std::env::var_os("HARDPOINT_SOCKET") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => state_dir().join("hardpoint.sock"),
    }
}

/// Where OpenCode's background service publishes its URL and password.
pub fn opencode_service_file() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("opencode").join("service.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../examples/moonlander.toml");

    #[test]
    fn a_rule_has_a_time_or_a_state() {
        let rule = |when: &str| format!("[[rule]]\n{when}\ndevice = \"a/b\"\ncapability = \"light.global\"\nvalue = \"#402000\"\n");
        assert!(Config::parse(&rule("at = \"20:00\"")).is_ok());
        assert_eq!(Config::parse(&rule("while = \"needs-you\"")).unwrap().rule[0].during, Some(crate::model::AgentState::NeedsYou));
        assert!(Config::parse(&rule("at = \"8pm\"")).is_err());
        assert!(Config::parse(&rule("")).is_err(), "a rule with no trigger never fires, so it is an error");
        assert!(Config::parse(&rule("at = \"20:00\"\nwhile = \"running\"")).is_err());
    }

    #[test]
    fn a_config_file_only_has_to_say_what_differs() {
        let cfg = Config::parse("[update]\ncatalog = true\n").unwrap();
        assert!(cfg.update.catalog);
        assert_eq!(cfg.update.every_hours, 24, "what it does not mention keeps the built-in setting");
        assert_eq!(cfg.gate.hold_secs, 10);
        assert!(cfg.tools.auto_install);
        assert!(!Config::parse("[tools]\nauto_install = false\n").unwrap().tools.auto_install);
        assert!(Config::parse("[tools]\ncommand = 'sh installer'\n").is_err());
        assert!(Config::parse("[update]\nnonsense = 1\n").is_err(), "an unknown setting is still an error");
    }

    #[test]
    fn built_in_defaults_name_no_device_and_no_keys() {
        let cfg = Config::parse(DEFAULT_CONFIG).unwrap();
        assert_eq!(cfg.device.backend, Backend::Auto);
        assert!(cfg.device.card.is_empty());
        assert!(cfg.slots.is_empty());
        assert!(cfg.actions.agent_press.is_empty());
        assert!(!cfg.telemetry.enabled);
    }

    #[test]
    fn the_example_config_parses() {
        let cfg = Config::parse(EXAMPLE).unwrap();
        assert_eq!(cfg.slots.len(), 6);
        assert_eq!(cfg.slots.iter().filter(|s| s.pressable).count(), 2);
    }

    #[test]
    fn night_window_can_wrap_midnight() {
        let b = Brightness { day: 0.8, night: 0.3, night_start: 21, night_end: 7 };
        assert_eq!(b.at_hour(22), 0.3);
        assert_eq!(b.at_hour(3), 0.3);
        assert_eq!(b.at_hour(7), 0.8);
        assert_eq!(b.at_hour(14), 0.8);
    }

    #[test]
    fn rejects_duplicate_slots_and_unknown_fields() {
        let dup = EXAMPLE.replace("key = 50", "key = 36");
        assert!(Config::parse(&dup).is_err());
        let unknown = DEFAULT_CONFIG.replace("[telemetry]", "[telemetry]\nbogus = 1");
        assert!(Config::parse(&unknown).is_err());
    }
}
