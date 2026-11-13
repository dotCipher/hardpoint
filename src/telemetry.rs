//! Press counts per key, per layer, per day. This deliberately records
//! nothing else: no order, no timing and no text can be recovered from it.
//! The `layers` array is one list of counts per layer, indexed by key number.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Day {
    pub date: String,
    pub board: String,
    pub layout: String,
    pub layers: Vec<Vec<u64>>,
}

pub struct Telemetry {
    dir: PathBuf,
    keys: usize,
    day: Day,
    dirty: bool,
}

impl Telemetry {
    pub fn open(dir: PathBuf, board: &str, layout: &str, keys: usize, date: &str) -> Result<Telemetry> {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let mut t = Telemetry { dir, keys, day: Day::default(), dirty: false };
        t.start_day(board, layout, date);
        Ok(t)
    }

    /// One file per day and layout: a new layout renumbers what keys mean, so
    /// its counts are never mixed with, or written over, the old one's.
    fn path(&self, date: &str, layout: &str) -> PathBuf {
        let layout: String = layout.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        self.dir.join(format!("{date}_{layout}.json"))
    }

    fn start_day(&mut self, board: &str, layout: &str, date: &str) {
        let existing = std::fs::read_to_string(self.path(date, layout)).ok().and_then(|t| serde_json::from_str::<Day>(&t).ok());
        self.day = existing.unwrap_or_else(|| Day {
            date: date.to_string(),
            board: board.to_string(),
            layout: layout.to_string(),
            layers: Vec::new(),
        });
    }

    pub fn record(&mut self, layer: u8, key: u8, date: &str) -> Result<()> {
        if date != self.day.date {
            self.flush()?;
            let (board, layout) = (self.day.board.clone(), self.day.layout.clone());
            self.start_day(&board, &layout, date);
        }
        let layer = layer as usize;
        if self.day.layers.len() <= layer {
            self.day.layers.resize(layer + 1, vec![0; self.keys]);
        }
        if let Some(count) = self.day.layers[layer].get_mut(key as usize) {
            *count += 1;
            self.dirty = true;
        }
        Ok(())
    }

    pub fn total(&self) -> u64 {
        self.day.layers.iter().flatten().sum()
    }

    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let path = self.path(&self.day.date, &self.day.layout);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&self.day)?).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_survive_a_restart_and_roll_over_at_midnight() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = Telemetry::open(dir.path().into(), "vendor/board", "abc/1", 72, "2026-01-01").unwrap();
        t.record(0, 34, "2026-01-01").unwrap();
        t.record(0, 34, "2026-01-01").unwrap();
        t.record(3, 15, "2026-01-01").unwrap();
        t.flush().unwrap();

        let mut t = Telemetry::open(dir.path().into(), "vendor/board", "abc/1", 72, "2026-01-01").unwrap();
        assert_eq!(t.total(), 3);
        t.record(0, 34, "2026-01-02").unwrap();
        assert_eq!(t.total(), 1);
        t.flush().unwrap();

        let first: Day =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("2026-01-01_abc-1.json")).unwrap()).unwrap();
        assert_eq!(first.layers[0][34], 2);
        assert_eq!(first.layers[3][15], 1);
        assert_eq!(first.layers.len(), 4);
    }

    #[test]
    fn a_new_layout_gets_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut old = Telemetry::open(dir.path().into(), "vendor/board", "abc/1", 72, "2026-01-01").unwrap();
        old.record(0, 1, "2026-01-01").unwrap();
        old.flush().unwrap();
        let mut new = Telemetry::open(dir.path().into(), "vendor/board", "abc/2", 72, "2026-01-01").unwrap();
        assert_eq!(new.total(), 0);
        new.record(0, 2, "2026-01-01").unwrap();
        new.flush().unwrap();
        let old = Telemetry::open(dir.path().into(), "vendor/board", "abc/1", 72, "2026-01-01").unwrap();
        assert_eq!(old.total(), 1);
    }
}
