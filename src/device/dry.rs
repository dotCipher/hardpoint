//! A device with no hardware: it draws the board in the terminal. Used to
//! develop and test without a keyboard, and on machines that have none.

use super::{Device, DeviceEvent};
use crate::board::Board;
use crate::model::Rgb;
use anyhow::Result;
use std::io::Write;
use std::time::{Duration, Instant};

pub struct DryDevice {
    board: Board,
    frame: Vec<Option<Rgb>>,
    dirty: bool,
    last_draw: Option<Instant>,
    released: bool,
}

impl DryDevice {
    pub fn new(board: Board) -> DryDevice {
        let frame = vec![None; board.led_count()];
        DryDevice { board, frame, dirty: false, last_draw: None, released: true }
    }

    pub fn render(&self) -> String {
        let width = self.board.keys.iter().map(|k| k.x).max().unwrap_or(0) as usize + 1;
        let height = self.board.keys.iter().map(|k| k.y).max().unwrap_or(0) as usize + 1;
        let mut grid = vec![vec!["  ".to_string(); width]; height];
        for key in &self.board.keys {
            grid[key.y as usize][key.x as usize] = match self.frame[key.led as usize] {
                Some(c) if c != Rgb::OFF => format!("\x1b[48;2;{};{};{}m  \x1b[0m", c.r, c.g, c.b),
                _ => "\x1b[2m··\x1b[0m".to_string(),
            };
        }
        grid.iter().map(|row| row.concat()).collect::<Vec<_>>().join("\n")
    }

    /// Lit keys as `index:#rrggbb`, for logs and tests.
    pub fn summary(&self) -> String {
        if self.released {
            return "released".to_string();
        }
        let mut lit: Vec<(u8, Rgb)> = self
            .board
            .keys
            .iter()
            .filter_map(|k| self.frame[k.led as usize].filter(|c| *c != Rgb::OFF).map(|c| (k.index, c)))
            .collect();
        lit.sort_by_key(|(i, _)| *i);
        let base = lit.iter().map(|(_, c)| *c).fold(std::collections::HashMap::new(), |mut m, c| {
            *m.entry(c.hex()).or_insert(0usize) += 1;
            m
        });
        let common = base.into_iter().max_by_key(|(_, n)| *n).filter(|(_, n)| *n > 8).map(|(c, _)| c);
        let parts: Vec<String> = lit
            .iter()
            .filter(|(_, c)| Some(c.hex()) != common)
            .map(|(i, c)| format!("{i}:{}", c.hex()))
            .collect();
        match common {
            Some(c) => format!("base {c} | {}", parts.join(" ")),
            None => parts.join(" "),
        }
    }
}

impl Device for DryDevice {
    fn describe(&self) -> String {
        format!("dry run ({})", self.board.card)
    }

    fn set_led(&mut self, led: u8, color: Rgb) -> Result<()> {
        if let Some(cell) = self.frame.get_mut(led as usize) {
            *cell = Some(color);
            self.dirty = true;
            self.released = false;
        }
        Ok(())
    }

    fn fill(&mut self, color: Rgb) -> Result<()> {
        self.frame.iter_mut().for_each(|c| *c = Some(color));
        self.dirty = true;
        self.released = false;
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        self.frame.iter_mut().for_each(|c| *c = None);
        self.dirty = true;
        self.released = true;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        // Pulsing keys change every frame; redraw at most twice a second.
        let due = self.last_draw.is_none_or(|t| t.elapsed() >= Duration::from_millis(500));
        if self.dirty && due {
            let mut err = std::io::stderr().lock();
            if std::env::var_os("HARDPOINT_DRY_GRID").is_some() {
                writeln!(err, "{}", self.render())?;
            }
            writeln!(err, "[dry] {}", self.summary())?;
            self.dirty = false;
            self.last_draw = Some(Instant::now());
        }
        Ok(())
    }

    fn poll(&mut self, timeout_ms: i32) -> Result<Vec<DeviceEvent>> {
        std::thread::sleep(Duration::from_millis(timeout_ms.max(0) as u64));
        Ok(Vec::new())
    }
}
