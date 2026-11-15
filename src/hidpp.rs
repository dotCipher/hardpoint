//! The `hidpp` adapter: Logitech devices over HID++ 2.0.
//!
//! A card names the HID++ feature an ability uses, and this module looks
//! that feature up on the device and calls it. Only devices connected
//! directly over Bluetooth are reached so far, not ones behind a USB
//! receiver. The device is never opened exclusively, so it keeps working as
//! a mouse while it is being asked.

use crate::catalog::Observation;
use hidapi::{HidApi, HidDevice};
use serde_json::{json, Value};

/// The vendor-defined collection HID++ long messages travel on.
const USAGE_PAGE: u16 = 0xFF43;

fn open(seen: &Observation) -> Result<HidDevice, String> {
    let api = HidApi::new().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    api.set_open_exclusive(false);
    let info = api
        .device_list()
        .find(|d| Some(d.vendor_id()) == seen.vendor_id && Some(d.product_id()) == seen.product_id && d.usage_page() == USAGE_PAGE)
        .ok_or("it is not connected over Bluetooth right now")?;
    info.open_device(&api).map_err(|e| {
        format!("it could not be opened ({e}). On macOS the program needs Input Monitoring permission: System Settings, Privacy & Security, Input Monitoring")
    })
}

/// One HID++ call: a feature by its index on this device, a function, its parameters.
fn call(dev: &HidDevice, index: u8, function: u8, params: &[u8]) -> Result<Vec<u8>, String> {
    let mut message = [0u8; 20];
    message[..4].copy_from_slice(&[0x11, 0xFF, index, (function << 4) | 0x0A]);
    message[4..4 + params.len()].copy_from_slice(params);
    dev.write(&message).map_err(|e| e.to_string())?;
    let mut reply = [0u8; 64];
    for _ in 0..20 {
        match dev.read_timeout(&mut reply, 100) {
            Ok(n) if n >= 7 && reply[0] == 0x11 && reply[2] == 0xFF && reply[3] == index => return Err(format!("the device refused (HID++ error {})", reply[5])),
            Ok(n) if n >= 4 && reply[0] == 0x11 && reply[2] == index && reply[3] == message[3] => return Ok(reply[4..n].to_vec()),
            _ => {}
        }
    }
    Err("the device did not answer".to_string())
}

/// Where a feature lives on this device. Every device numbers them differently.
fn feature(dev: &HidDevice, id: u16) -> Result<u8, String> {
    let found = call(dev, 0x00, 0, &[(id >> 8) as u8, id as u8])?;
    found.first().copied().filter(|index| *index != 0).ok_or_else(|| format!("the device does not have HID++ feature {id:#06x}"))
}

/// The abilities this module knows how to work, whatever the device.
pub fn supports(capability: &str) -> bool {
    ["battery.level", "pointer.dpi", "scroll.mode", "pointer.host"].contains(&capability)
}

pub fn read(seen: &Observation, capability: &str, id: u16) -> Result<Value, String> {
    let dev = open(seen)?;
    let index = feature(&dev, id)?;
    match capability {
        "battery.level" => Ok(json!(call(&dev, index, 0, &[])?[0])),
        "pointer.dpi" => {
            let r = call(&dev, index, 2, &[0])?;
            Ok(json!(u16::from_be_bytes([r[1], r[2]])))
        }
        "scroll.mode" => Ok(json!(if call(&dev, index, 0, &[])?[0] == 1 { "free-spin" } else { "ratchet" })),
        "pointer.host" => {
            let r = call(&dev, index, 0, &[])?;
            Ok(json!({ "current": r[1] + 1, "of": r[0] }))
        }
        other => Err(format!("this build does not know how to read {other} over HID++")),
    }
}

pub fn write(seen: &Observation, capability: &str, id: u16, value: &Value) -> Result<(), String> {
    let dev = open(seen)?;
    let index = feature(&dev, id)?;
    match capability {
        "pointer.dpi" => {
            let dpi = value.as_u64().filter(|n| (200..=8000).contains(n)).ok_or("the speed is a number of dots per inch, from 200 to 8000")? as u16;
            call(&dev, index, 3, &[0, (dpi >> 8) as u8, dpi as u8]).map(|_| ())
        }
        "scroll.mode" => {
            let mode = match value.as_str() {
                Some("free-spin") => 1,
                Some("ratchet") => 2,
                _ => return Err("the wheel is \"ratchet\" or \"free-spin\"".to_string()),
            };
            call(&dev, index, 1, &[mode, 0, 0]).map(|_| ())
        }
        other => Err(format!("this build has no way to change {other} over HID++ yet")),
    }
}
