//! Device adapters. An adapter knows how to light LEDs and report key events
//! on one kind of hardware; it knows nothing about agents or slots.

pub mod dry;
pub mod oryx;

use crate::model::Rgb;
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceEvent {
    KeyDown { row: u8, col: u8 },
    KeyUp { row: u8, col: u8 },
    Layer(u8),
}

pub trait Device {
    fn describe(&self) -> String;
    /// Light one LED, addressed by its position in the LED chain.
    fn set_led(&mut self, led: u8, color: Rgb) -> Result<()>;
    fn fill(&mut self, color: Rgb) -> Result<()>;
    /// Hand lighting back to the keyboard's own firmware.
    fn release(&mut self) -> Result<()>;
    /// Switch one separate status indicator LED (0..6), independent of key RGB.
    fn set_status_led(&mut self, _index: u8, _on: bool) -> Result<()> {
        Ok(())
    }
    /// Hand status indicator LEDs back to the keyboard's own firmware.
    fn release_status_leds(&mut self) -> Result<()> {
        Ok(())
    }
    /// Called once after a batch of lighting changes.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
    /// Wait up to `timeout_ms` for input and return whatever arrived.
    fn poll(&mut self, timeout_ms: i32) -> Result<Vec<DeviceEvent>>;
}
