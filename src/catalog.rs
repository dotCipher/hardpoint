//! The device catalog: one card per device model, read from disk.
//!
//! The daemon takes everything it knows about a device from its card. Nothing
//! about a particular keyboard or mouse is built into the code. Cards are data
//! only: nothing in a card is ever executed.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
pub struct Card {
    pub device: DeviceInfo,
    #[serde(rename = "match")]
    pub matches: Vec<Match>,
    #[serde(default)]
    pub keys: Option<Keys>,
    #[serde(default, rename = "capability")]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub requires: Vec<Requirement>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub vendor: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Match {
    pub bus: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub usage_page: Option<u16>,
    pub model: Option<String>,
    pub name: Option<String>,
}

/// For keyboards: how key numbers, switch positions and lights line up.
#[derive(Clone, Debug, Deserialize)]
pub struct Keys {
    pub matrix: Vec<[u8; 2]>,
    pub leds: Vec<u8>,
    pub grid: Vec<[u8; 2]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Capability {
    pub id: String,
    pub adapter: String,
    pub risk: Risk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Risk {
    Read,
    Reversible,
    Guarded,
    Gated,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Requirement {
    pub platform: String,
    pub what: String,
}

/// Something found attached to this computer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Observation {
    pub bus: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub usage_pages: Vec<u16>,
    pub name: String,
    pub model: Option<String>,
}

impl Match {
    pub fn fits(&self, seen: &Observation) -> bool {
        let same = |want: Option<u16>, have: Option<u16>| want.is_none() || want == have;
        self.bus == seen.bus
            && same(self.vendor_id, seen.vendor_id)
            && same(self.product_id, seen.product_id)
            && self.usage_page.is_none_or(|page| seen.usage_pages.contains(&page))
            && self.model.as_ref().is_none_or(|m| seen.model.as_ref() == Some(m))
            && self.name.as_ref().is_none_or(|n| seen.name.to_lowercase().contains(&n.to_lowercase()))
    }
}

impl Card {
    pub fn has_adapter(&self, adapter: &str) -> bool {
        self.capabilities.iter().any(|c| c.adapter == adapter)
    }

    pub fn fits(&self, seen: &Observation) -> bool {
        self.matches.iter().any(|m| m.fits(seen))
    }

    /// What must be true on this platform before the card's capabilities work.
    pub fn needs(&self, platform: &str) -> Vec<&str> {
        self.requires
            .iter()
            .filter(|r| r.platform == "any" || r.platform == platform)
            .map(|r| r.what.as_str())
            .collect()
    }
}

pub struct Catalog {
    pub root: PathBuf,
    pub cards: Vec<Card>,
}

impl Catalog {
    pub fn load(root: &Path) -> Result<Catalog> {
        let devices = root.join("devices");
        let mut paths = Vec::new();
        for vendor in std::fs::read_dir(&devices).with_context(|| format!("reading {}", devices.display()))? {
            let vendor = vendor?.path();
            if !vendor.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(&vendor)? {
                let file = file?.path();
                if file.extension().is_some_and(|e| e == "toml") {
                    paths.push(file);
                }
            }
        }
        paths.sort();
        let mut cards = Vec::new();
        for path in paths {
            let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let card: Card = toml::from_str(&text).with_context(|| format!("in {}", path.display()))?;
            if let Some(keys) = &card.keys {
                if keys.matrix.len() != keys.leds.len() || keys.matrix.len() != keys.grid.len() {
                    bail!("{}: the key lists have different lengths", path.display());
                }
            }
            cards.push(card);
        }
        Ok(Catalog { root: root.to_path_buf(), cards })
    }

    /// Where the catalog is: an explicit path, then `HARDPOINT_CATALOG`, then
    /// the user's data directory, then a `catalog` directory beside or above
    /// the program (which is where it is in a source checkout).
    pub fn locate(explicit: Option<&Path>) -> Result<PathBuf> {
        let holds_cards = |dir: &Path| dir.join("devices").is_dir();
        if let Some(path) = explicit {
            if holds_cards(path) {
                return Ok(path.to_path_buf());
            }
            bail!("{} has no devices directory", path.display());
        }
        let mut candidates = Vec::new();
        if let Some(path) = std::env::var_os("HARDPOINT_CATALOG").filter(|v| !v.is_empty()) {
            candidates.push(PathBuf::from(path));
        }
        candidates.push(crate::config::data_dir().join("catalog"));
        if let Ok(exe) = std::env::current_exe() {
            candidates.extend(exe.ancestors().skip(1).map(|dir| dir.join("catalog")));
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("catalog"));
        }
        candidates
            .into_iter()
            .find(|dir| holds_cards(dir))
            .ok_or_else(|| anyhow!("no catalog found; pass --catalog <dir> or set HARDPOINT_CATALOG"))
    }

    pub fn card(&self, id: &str) -> Option<&Card> {
        self.cards.iter().find(|c| c.device.id == id)
    }

    pub fn recognise(&self, seen: &Observation) -> Option<&Card> {
        self.cards.iter().find(|c| c.fits(seen))
    }
}

/// The catalog that ships in this repository, for tests.
#[cfg(test)]
pub fn repo_catalog() -> Catalog {
    Catalog::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog")).expect("the repository's catalog loads")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(bus: &str, vendor: u16, product: u16) -> Observation {
        Observation {
            bus: bus.into(),
            vendor_id: Some(vendor),
            product_id: Some(product),
            usage_pages: vec![],
            name: "Something".into(),
            model: None,
        }
    }

    #[test]
    fn every_card_in_the_repository_loads() {
        let catalog = repo_catalog();
        assert!(!catalog.cards.is_empty());
        for card in &catalog.cards {
            assert!(!card.matches.is_empty(), "{} has no match", card.device.id);
            assert!(!card.capabilities.is_empty(), "{} has no capabilities", card.device.id);
        }
    }

    #[test]
    fn a_device_is_recognised_by_its_identifiers_only() {
        let catalog = repo_catalog();
        let card = catalog.cards.iter().find(|c| c.keys.is_some()).expect("a keyboard card");
        let m = &card.matches[0];
        let hit = seen(&m.bus, m.vendor_id.unwrap(), m.product_id.unwrap());
        assert_eq!(catalog.recognise(&hit).map(|c| c.device.id.as_str()), Some(card.device.id.as_str()));
        assert!(catalog.recognise(&seen(&m.bus, m.vendor_id.unwrap(), 0xFFFF)).is_none());
        assert!(catalog.recognise(&seen("audio", m.vendor_id.unwrap(), m.product_id.unwrap())).is_none());
    }

    #[test]
    fn a_step_only_a_person_can_take_is_always_gated() {
        for card in &repo_catalog().cards {
            for cap in &card.capabilities {
                if cap.adapter == "manual" {
                    assert_eq!(cap.risk, Risk::Gated, "{} {}", card.device.id, cap.id);
                }
            }
        }
    }
}
