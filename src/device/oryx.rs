//! ZSA keyboards over the Oryx raw-HID protocol (usage page 0xFF60).
//!
//! The protocol is published by ZSA in `zsa/qmk_modules` (`oryx/oryx.h`).
//! Packets are 32 bytes: a command or event code followed by parameters.
//! Only one program can hold this channel, so the vendor's own app must be closed.

use super::{Device, DeviceEvent};
use crate::board::Board;
use crate::model::Rgb;
use anyhow::{anyhow, bail, Context, Result};
use hidapi::{HidApi, HidDevice};
use std::time::{Duration, Instant};

const USAGE_PAGE: u16 = 0xFF60;
const PACKET: usize = 32;

const CMD_GET_FW_VERSION: u8 = 0x00;
const CMD_PAIRING_INIT: u8 = 0x01;
const CMD_RGB_CONTROL: u8 = 0x05;
const CMD_SET_RGB_LED: u8 = 0x06;
const CMD_SET_RGB_LED_ALL: u8 = 0x09;
const CMD_GET_PROTOCOL_VERSION: u8 = 0xFE;

const EVT_FW_VERSION: u8 = 0x00;
const EVT_PAIRING_SUCCESS: u8 = 0x04;
const EVT_LAYER: u8 = 0x05;
const EVT_KEYDOWN: u8 = 0x06;
const EVT_KEYUP: u8 = 0x07;
const EVT_RGB_CONTROL: u8 = 0x08;
const EVT_PROTOCOL_VERSION: u8 = 0xFE;
const EVT_ERROR: u8 = 0xFF;

const STOP: u8 = 0xFE;

pub struct OryxDevice {
    dev: HidDevice,
    product: String,
    pub layout: String,
    pub protocol: u8,
    pending: Vec<DeviceEvent>,
}

impl OryxDevice {
    pub fn open(board: &Board) -> Result<OryxDevice> {
        let api = HidApi::new()?;
        let info = api
            .device_list()
            .find(|d| {
                d.vendor_id() == board.vendor_id
                    && board.product_ids.contains(&d.product_id())
                    && d.usage_page() == USAGE_PAGE
            })
            .ok_or_else(|| anyhow!("{} is not attached", board.name))?;
        let product = info.product_string().unwrap_or(&board.name).to_string();
        let dev = info.open_device(&api).map_err(|e| {
            let text = e.to_string();
            if text.contains("exclusive") {
                anyhow!("{product} is held by another program (the vendor's own app, or another hardpoint)")
            } else {
                anyhow!("opening {product}: {text}")
            }
        })?;
        let mut this = OryxDevice { dev, product, layout: String::new(), protocol: 0, pending: Vec::new() };

        this.send(&[CMD_GET_PROTOCOL_VERSION])?;
        let reply = this.wait_for(EVT_PROTOCOL_VERSION, 800).context("keyboard did not report a protocol version")?;
        this.protocol = reply[1];

        this.send(&[CMD_GET_FW_VERSION])?;
        let reply = this.wait_for(EVT_FW_VERSION, 800).context("keyboard did not report its layout")?;
        this.layout = reply[1..].iter().take_while(|b| **b != STOP && **b != 0).map(|b| *b as char).collect();

        this.send(&[CMD_PAIRING_INIT])?;
        this.wait_for(EVT_PAIRING_SUCCESS, 800).context("keyboard did not accept pairing")?;
        Ok(this)
    }

    /// Ask the firmware whether it is now showing host-controlled lighting.
    /// This is the keyboard's own answer, not an assumption from having sent
    /// the command.
    pub fn host_lighting(&mut self, on: bool) -> Result<bool> {
        self.send(&[CMD_RGB_CONTROL, on as u8])?;
        Ok(self.wait_for(EVT_RGB_CONTROL, 800).context("keyboard did not confirm its lighting mode")?[1] != 0)
    }

    fn send(&self, bytes: &[u8]) -> Result<()> {
        // hidapi wants the report id first; this interface has none, so 0.
        let mut packet = [0u8; PACKET + 1];
        packet[1..1 + bytes.len()].copy_from_slice(bytes);
        self.dev.write(&packet).context("writing to keyboard")?;
        Ok(())
    }

    fn read(&self, timeout_ms: i32) -> Result<Option<[u8; PACKET]>> {
        let mut buf = [0u8; PACKET];
        let n = self.dev.read_timeout(&mut buf, timeout_ms).context("reading from keyboard")?;
        Ok(if n == 0 { None } else { Some(buf) })
    }

    /// Read until an event with `code` arrives, keeping anything else that
    /// shows up meanwhile (the user may be typing).
    fn wait_for(&mut self, code: u8, timeout_ms: u64) -> Result<[u8; PACKET]> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while Instant::now() < deadline {
            if let Some(packet) = self.read(50)? {
                if packet[0] == code {
                    return Ok(packet);
                }
                if packet[0] == EVT_ERROR {
                    bail!("keyboard returned error {}", packet[1]);
                }
                if let Some(event) = decode(&packet) {
                    self.pending.push(event);
                }
            }
        }
        bail!("timed out")
    }
}

fn decode(packet: &[u8; PACKET]) -> Option<DeviceEvent> {
    match packet[0] {
        EVT_KEYDOWN => Some(DeviceEvent::KeyDown { col: packet[1], row: packet[2] }),
        EVT_KEYUP => Some(DeviceEvent::KeyUp { col: packet[1], row: packet[2] }),
        EVT_LAYER => Some(DeviceEvent::Layer(packet[1])),
        _ => None,
    }
}

impl Device for OryxDevice {
    fn describe(&self) -> String {
        format!("{} (layout {}, Oryx protocol v{})", self.product, self.layout, self.protocol)
    }

    fn set_led(&mut self, led: u8, c: Rgb) -> Result<()> {
        self.send(&[CMD_SET_RGB_LED, led, c.r, c.g, c.b])
    }

    fn fill(&mut self, c: Rgb) -> Result<()> {
        self.send(&[CMD_SET_RGB_LED_ALL, c.r, c.g, c.b])
    }

    fn release(&mut self) -> Result<()> {
        self.send(&[CMD_RGB_CONTROL, 0])
    }

    fn poll(&mut self, timeout_ms: i32) -> Result<Vec<DeviceEvent>> {
        let mut events = std::mem::take(&mut self.pending);
        let mut wait = timeout_ms;
        while let Some(packet) = self.read(wait)? {
            if let Some(event) = decode(&packet) {
                events.push(event);
            }
            wait = 0;
            if events.len() >= 64 {
                break;
            }
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_key_and_layer_events() {
        let mut p = [0u8; PACKET];
        p[..4].copy_from_slice(&[EVT_KEYDOWN, 3, 1, STOP]);
        assert_eq!(decode(&p), Some(DeviceEvent::KeyDown { col: 3, row: 1 }));
        p[..4].copy_from_slice(&[EVT_KEYUP, 0, 11, STOP]);
        assert_eq!(decode(&p), Some(DeviceEvent::KeyUp { col: 0, row: 11 }));
        p[..3].copy_from_slice(&[EVT_LAYER, 2, STOP]);
        assert_eq!(decode(&p), Some(DeviceEvent::Layer(2)));
        p[0] = 0x08;
        assert_eq!(decode(&p), None);
    }
}
