//! Discovery: what is attached to this computer, and which cards describe it.

use crate::catalog::{Catalog, Observation, Risk};
use anyhow::Result;
use hidapi::{BusType, HidApi};
use serde::Serialize;
use std::collections::BTreeMap;

pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// The computer's own model, as its maker identifies it.
fn computer_model() -> Option<String> {
    let text = if cfg!(target_os = "macos") {
        let out = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "hw.model"]).output().ok()?;
        String::from_utf8(out.stdout).ok()?
    } else {
        std::fs::read_to_string("/sys/class/dmi/id/product_name").ok()?
    };
    Some(text.trim().to_string()).filter(|m| !m.is_empty())
}

/// Everything this version knows how to look for: USB and Bluetooth input
/// devices, and the computer itself. Displays and audio are not enumerated yet.
pub fn observe() -> Result<Vec<Observation>> {
    let api = HidApi::new()?;
    let mut found: BTreeMap<(String, u16, u16, String), Vec<u16>> = BTreeMap::new();
    for device in api.device_list() {
        let bus = match device.bus_type() {
            BusType::Usb => "usb",
            BusType::Bluetooth => "bluetooth",
            _ => continue,
        };
        let name = device.product_string().unwrap_or("").trim().to_string();
        let pages = found.entry((bus.to_string(), device.vendor_id(), device.product_id(), name)).or_default();
        if !pages.contains(&device.usage_page()) {
            pages.push(device.usage_page());
        }
    }
    let mut seen: Vec<Observation> = found
        .into_iter()
        .map(|((bus, vendor, product, name), mut usage_pages)| {
            usage_pages.sort();
            Observation { bus, vendor_id: Some(vendor), product_id: Some(product), usage_pages, name, model: None }
        })
        .collect();
    if let Some(model) = computer_model() {
        seen.push(Observation {
            bus: "builtin".into(),
            vendor_id: None,
            product_id: None,
            usage_pages: vec![],
            name: model.clone(),
            model: Some(model),
        });
    }
    Ok(seen)
}

#[derive(Debug, Serialize)]
pub struct Found {
    pub seen: Observation,
    /// The card that describes it, if the catalog has one.
    pub card: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    /// How many capabilities the card lists at each risk level.
    pub capabilities: BTreeMap<String, usize>,
    /// Each capability the card lists, with its risk and the adapter that reaches it.
    pub can: Vec<Can>,
    pub needs: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Can {
    pub id: String,
    pub risk: Risk,
    pub adapter: String,
}

pub fn inventory(catalog: &Catalog, seen: Vec<Observation>) -> Vec<Found> {
    seen.into_iter()
        .map(|seen| match catalog.recognise(&seen) {
            None => Found {
                seen,
                card: None,
                name: None,
                kind: None,
                capabilities: BTreeMap::new(),
                can: vec![],
                needs: vec![],
            },
            Some(card) => {
                let mut capabilities = BTreeMap::new();
                for cap in &card.capabilities {
                    let label = match cap.risk {
                        Risk::Read => "read",
                        Risk::Reversible => "reversible",
                        Risk::Guarded => "guarded",
                        Risk::Gated => "gated",
                    };
                    *capabilities.entry(label.to_string()).or_insert(0) += 1;
                }
                Found {
                    card: Some(card.device.id.clone()),
                    name: Some(format!("{} {}", card.device.vendor, card.device.name)),
                    kind: Some(card.device.kind.clone()),
                    capabilities,
                    can: card
                        .capabilities
                        .iter()
                        .map(|c| Can { id: c.id.clone(), risk: c.risk, adapter: c.adapter.clone() })
                        .collect(),
                    needs: card.needs(platform()).into_iter().map(str::to_string).collect(),
                    seen,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::repo_catalog;

    #[test]
    fn inventory_separates_known_devices_from_unknown_ones() {
        let catalog = repo_catalog();
        let card = catalog.cards.iter().find(|c| c.keys.is_some()).unwrap();
        let m = &card.matches[0];
        let known = Observation {
            bus: m.bus.clone(),
            vendor_id: m.vendor_id,
            product_id: m.product_id,
            usage_pages: vec![],
            name: "x".into(),
            model: None,
        };
        let unknown = Observation { vendor_id: Some(0xFFFE), ..known.clone() };
        let found = inventory(&catalog, vec![known, unknown]);
        assert_eq!(found[0].card.as_deref(), Some(card.device.id.as_str()));
        assert_eq!(found[0].capabilities.values().sum::<usize>(), card.capabilities.len());
        assert!(found[1].card.is_none() && found[1].capabilities.is_empty());
    }
}
