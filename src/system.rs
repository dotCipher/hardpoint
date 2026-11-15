//! The `macos-system` adapter: what the operating system already knows.
//!
//! A card names a `source` and a `field`, and this module finds that field
//! in what macOS reports about the device. Nothing here is specific to any
//! one device, so a new card for something macOS already reports works with
//! no new code: battery levels, firmware versions, health verdicts.
//!
//! It never returns anything that identifies a particular unit or person:
//! serial numbers and addresses are refused whatever a card asks for.

use crate::catalog::Observation;
use serde_json::{json, Map, Value};
use std::process::Command;

/// The reports macOS can be asked for, by the name a card uses for them.
const REPORTS: [(&str, &str); 5] = [
    ("power", "SPPowerDataType"),
    ("bluetooth", "SPBluetoothDataType"),
    ("thunderbolt", "SPThunderboltDataType"),
    ("usb", "SPUSBHostDataType"),
    ("displays", "SPDisplaysDataType"),
];

/// Fields that would identify one unit or one person. Never passed on.
const PRIVATE: [&str; 5] = ["serial", "address", "uuid", "lot", "location"];

fn output(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("could not run {program}: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn report(kinds: &[&str]) -> Result<Value, String> {
    let mut args = vec!["-json"];
    args.extend(kinds);
    serde_json::from_str(&output("/usr/sbin/system_profiler", &args)?).map_err(|e| format!("macOS gave a report that could not be read: {e}"))
}

fn plain(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

fn hex(value: &Value) -> Option<u16> {
    let text = value.as_str()?.trim();
    u16::from_str_radix(text.trim_start_matches("0x").split_whitespace().next()?, 16).ok()
}

fn walk<'a>(node: &'a Value, found: &mut Vec<&'a Map<String, Value>>) {
    match node {
        Value::Object(map) => {
            found.push(map);
            map.values().for_each(|v| walk(v, found));
        }
        Value::Array(items) => items.iter().for_each(|v| walk(v, found)),
        _ => {}
    }
}

/// "TRUE" is a yes and "92%" is 92. Anything else stays as written: a version such as "44.1" is not a number.
fn tidy(value: &Value) -> Value {
    let Some(text) = value.as_str() else { return value.clone() };
    match text {
        "TRUE" | "Yes" | "attrib_Yes" => return json!(true),
        "FALSE" | "No" | "attrib_No" => return json!(false),
        _ => {}
    }
    match text.strip_suffix('%').and_then(|n| n.parse::<f64>().ok()) {
        Some(number) => json!(number),
        None => json!(text),
    }
}

/// Every device macOS reports that the HID layer does not: Bluetooth devices
/// that are paired, anything on USB, and external displays.
pub fn seen() -> Vec<Observation> {
    let mut found = Vec::new();
    if !cfg!(target_os = "macos") {
        return found;
    }
    let Ok(all) = report(&["SPBluetoothDataType", "SPUSBHostDataType", "SPDisplaysDataType"]) else { return found };
    let mut add = |bus: &str, name: &str, vendor: Option<u16>, product: Option<u16>| {
        if let (Some(vendor), Some(product)) = (vendor, product) {
            found.push(Observation { bus: bus.into(), vendor_id: Some(vendor), product_id: Some(product), usage_pages: vec![], name: name.into(), model: None });
        }
    };
    for group in ["device_connected", "device_not_connected"] {
        for entry in all["SPBluetoothDataType"][0][group].as_array().into_iter().flatten() {
            for (name, facts) in entry.as_object().into_iter().flatten() {
                add("bluetooth", name, hex(&facts["device_vendorID"]), hex(&facts["device_productID"]));
            }
        }
    }
    let mut nodes = Vec::new();
    walk(&all["SPUSBHostDataType"], &mut nodes);
    for node in &nodes {
        let name = node.get("_name").and_then(Value::as_str).unwrap_or("");
        add("usb", name, node.get("USBDeviceKeyVendorID").and_then(hex), node.get("USBDeviceKeyProductID").and_then(hex));
    }
    let mut nodes = Vec::new();
    walk(&all["SPDisplaysDataType"], &mut nodes);
    for node in nodes.iter().filter(|n| n.get("spdisplays_connection_type").and_then(Value::as_str) != Some("spdisplays_internal")) {
        let name = node.get("_name").and_then(Value::as_str).unwrap_or("");
        add("display", name, node.get("_spdisplays_display-vendor-id").and_then(hex), node.get("_spdisplays_display-product-id").and_then(hex));
    }
    found
}

/// Whether this build, on this system, has a way to reach `source`.
pub fn supports(source: &str) -> bool {
    cfg!(target_os = "macos") && (REPORTS.iter().any(|(name, _)| *name == source) || ["storage", "thermal", "audio"].contains(&source))
}

/// Find `field` in what a report says about one device.
fn field_of(kind: &str, source: &str, field: &str, seen: &Observation, names: &[&str]) -> Result<Value, String> {
    let all = report(&[kind])?;
    let mut nodes = Vec::new();
    walk(&all[kind], &mut nodes);
    // Narrow the report to the part about this device.
    let about: Vec<&Map<String, Value>> = match source {
        "power" => nodes,
        "bluetooth" => nodes
            .into_iter()
            .filter(|n| n.get("device_vendorID").and_then(hex) == seen.vendor_id && n.get("device_productID").and_then(hex) == seen.product_id && seen.vendor_id.is_some())
            .collect(),
        _ => {
            let wanted: Vec<String> = names.iter().map(|name| plain(name)).filter(|name| !name.is_empty()).collect();
            let named = |n: &&Map<String, Value>| n.values().any(|v| v.as_str().is_some_and(|text| wanted.contains(&plain(text))));
            let mut under = Vec::new();
            for node in nodes.into_iter().filter(named) {
                walk_map(node, &mut under);
            }
            under
        }
    };
    if about.is_empty() {
        return Err("macOS does not report this device right now".to_string());
    }
    let want = plain(field);
    let mut hits: Vec<(String, Value)> = Vec::new();
    for node in about {
        for (key, value) in node {
            let key = plain(key);
            let private = PRIVATE.iter().any(|word| key.contains(word));
            if key.contains(&want) && !private && !value.is_object() && !value.is_array() && !hits.iter().any(|(k, _)| *k == key) {
                hits.push((key, tidy(value)));
            }
        }
    }
    match hits.len() {
        0 => Err(format!("macOS does not report {field} for this device right now. Some things, such as a battery level, only appear while it is connected")),
        1 => Ok(hits.remove(0).1),
        // Several parts report the same thing, such as each earbud and the case: name each by what sets it apart.
        _ => Ok(Value::Object(
            hits.into_iter()
                .map(|(key, value)| {
                    let part = key.split(&want).last().unwrap_or("").trim_end_matches("key").to_string();
                    (if part.is_empty() { "value".to_string() } else { part }, value)
                })
                .collect(),
        )),
    }
}

fn walk_map<'a>(node: &'a Map<String, Value>, found: &mut Vec<&'a Map<String, Value>>) {
    found.push(node);
    node.values().for_each(|v| walk(v, found));
}

/// The disk a device is, as `diskutil` names it: the startup disk for the
/// computer itself, or the external disk whose name matches.
fn disk(seen: &Observation) -> Result<String, String> {
    if seen.bus == "builtin" {
        return Ok("/".to_string());
    }
    for line in output("/usr/sbin/diskutil", &["list", "external", "physical"])?.lines() {
        let Some(id) = line.split_whitespace().next().filter(|word| word.starts_with("/dev/disk")) else { continue };
        let about = output("/usr/sbin/diskutil", &["info", id])?;
        let media = about.lines().find_map(|l| l.trim().strip_prefix("Device / Media Name:")).unwrap_or("").trim().to_string();
        if !media.is_empty() && plain(&media) == plain(&seen.name) {
            return Ok(id.to_string());
        }
    }
    Err("this drive is not attached right now".to_string())
}

pub fn read(source: &str, field: &str, seen: &Observation, names: &[&str]) -> Result<Value, String> {
    if !supports(source) {
        return Err(format!("this build has no way to read {source} on this system yet"));
    }
    if PRIVATE.iter().any(|word| plain(field).contains(word)) {
        return Err(format!("{field} identifies one particular unit, so it is never passed on"));
    }
    match (source, field) {
        ("audio", "output_volume") => {
            let text = output("/usr/bin/osascript", &["-e", "output volume of (get volume settings)"])?;
            text.trim().parse::<f64>().map(|n| json!(n)).map_err(|_| "the current sound output has no volume the system can set".to_string())
        }
        ("thermal", _) => {
            let text = output("/usr/bin/pmset", &["-g", "therm"])?;
            let level = text.lines().find_map(|l| l.to_lowercase().split("thermal warning level").nth(1).map(|rest| rest.trim_matches(|c: char| !c.is_alphanumeric()).to_string()));
            Ok(json!(level.filter(|l| !l.is_empty() && !l.contains("recorded")).unwrap_or_else(|| "none".to_string())))
        }
        ("storage", "smart_status") => {
            let about = output("/usr/sbin/diskutil", &["info", &disk(seen)?])?;
            let status = about.lines().find_map(|l| l.trim().strip_prefix("SMART Status:")).map(|s| s.trim().to_string());
            status.map(|s| json!(s)).ok_or_else(|| "this drive gives no health verdict".to_string())
        }
        ("audio", _) | ("storage", _) => Err(format!("this build does not know the {source} field {field}")),
        _ => {
            let kind = REPORTS.iter().find(|(name, _)| *name == source).map(|(_, kind)| *kind).unwrap_or("");
            field_of(kind, source, field, seen, names)
        }
    }
}

pub fn write(source: &str, field: &str, value: &Value) -> Result<(), String> {
    match (source, field) {
        ("audio", "output_volume") if cfg!(target_os = "macos") => {
            let level = value.as_f64().filter(|n| (0.0..=100.0).contains(n)).ok_or("the volume is a number from 0 to 100")?;
            output("/usr/bin/osascript", &["-e", &format!("set volume output volume {}", level.round())]).map(|_| ())
        }
        _ => Err(format!("this build has no way to change {source} {field} yet")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_tidied_and_private_fields_refused() {
        assert_eq!(tidy(&json!("TRUE")), json!(true));
        assert_eq!(tidy(&json!("92%")), json!(92.0));
        assert_eq!(tidy(&json!("44.1")), json!("44.1"));
        assert_eq!(tidy(&json!("0x0b00")), json!("0x0b00"));
        assert_eq!(tidy(&json!("Good")), json!("Good"));
        let anything = Observation { bus: "builtin".into(), vendor_id: None, product_id: None, usage_pages: vec![], name: "x".into(), model: None };
        for field in ["serial_number", "device_address", "serialNumberLeft"] {
            let refused = read("power", field, &anything, &[]);
            assert!(refused.is_err(), "{field} must never be readable");
        }
    }
}
