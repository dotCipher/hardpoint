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
    pub fn parse(text: &str) -> Result<Config> {
        let cfg: Config = toml::from_str(text)?;
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
