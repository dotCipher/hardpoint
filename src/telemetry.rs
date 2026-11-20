//! Press counts per key, per layer, per day. This deliberately records
//! nothing else: no order, no timing and no text can be recovered from it.
//! The `layers` array is one list of counts per layer, indexed by key number.

use crate::board::Board;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct KeyCount {
    pub key: u8,
    pub count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HandBalance {
    pub left: u64,
    pub right: u64,
    pub left_pct: f64,
    pub right_pct: f64,
}

/// Aggregate ergonomic summary computed solely from per-key press counts.
/// Never records or infers text, keystroke order, or timing.
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub board: String,
    pub layout: String,
    pub days: usize,
    pub total_presses: u64,
    pub per_layer: BTreeMap<usize, u64>,
    pub unused_keys: Vec<u8>,
    pub rare_keys: Vec<KeyCount>,
    pub top_keys: Vec<KeyCount>,
    pub hand_balance: Option<HandBalance>,
    pub counts: Vec<u64>,
}

/// Read aggregate daily count files for a board/layout and compute unused keys,
/// hotspots, layer usage and (for split boards) left/right hand balance.
pub fn summarize(dir: &Path, board: &Board, layout: Option<&str>, days: Option<usize>) -> Result<Summary> {
    let mut entries: Vec<Day> = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(day) = serde_json::from_str::<Day>(&text) {
                        let same_keys = day.layers.first().is_none_or(|l| l.len() == board.keys.len());
                        let board_matches = day.board == board.card || (same_keys && !day.board.is_empty() && board.card.contains(&day.board));
                        if board_matches && layout.is_none_or(|want| day.layout == want) {
                            entries.push(day);
                        }
                    }
                }
            }
        }
    }
    if entries.is_empty() {
        bail!("no telemetry files found in {} for {}", dir.display(), board.card);
    }
    entries.sort_by(|a, b| a.date.cmp(&b.date).then(a.layout.cmp(&b.layout)));
    let chosen_layout = layout.map(str::to_string).unwrap_or_else(|| entries.last().unwrap().layout.clone());
    entries.retain(|d| d.layout == chosen_layout);
    if let Some(limit) = days.filter(|&n| n > 0) {
        if entries.len() > limit {
            entries = entries.split_off(entries.len() - limit);
        }
    }
    let key_count = board.keys.len();
    let mut counts = vec![0u64; key_count];
    let mut per_layer: BTreeMap<usize, u64> = BTreeMap::new();
    for day in &entries {
        for (layer, arr) in day.layers.iter().enumerate() {
            let sum: u64 = arr.iter().take(key_count).sum();
            if sum > 0 {
                *per_layer.entry(layer).or_insert(0) += sum;
            }
            for (i, &n) in arr.iter().take(key_count).enumerate() {
                counts[i] += n;
            }
        }
    }
    let total_presses: u64 = counts.iter().sum();
    let unused_keys: Vec<u8> = counts.iter().enumerate().filter(|(_, &n)| n == 0).map(|(i, _)| i as u8).collect();
    let mut rare_keys: Vec<KeyCount> = counts
        .iter()
        .enumerate()
        .filter(|(_, &n)| n > 0 && n <= 15)
        .map(|(i, &count)| KeyCount { key: i as u8, count })
        .collect();
    rare_keys.sort_by_key(|k| (k.count, k.key));
    let mut top_keys: Vec<KeyCount> = counts
        .iter()
        .enumerate()
        .filter(|(_, &n)| n > 0)
        .map(|(i, &count)| KeyCount { key: i as u8, count })
        .collect();
    top_keys.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    top_keys.truncate(12);

    // Detect a split keyboard by a gap in grid x-coordinates (same geometry rule as tools/build.py).
    let mut xs: Vec<u8> = board.keys.iter().map(|k| k.x).collect();
    xs.sort();
    xs.dedup();
    let split_x = xs.windows(2).find(|w| w[1] > w[0] + 1).map(|w| w[1]);
    let hand_balance = split_x.map(|mid| {
        let mut left = 0u64;
        let mut right = 0u64;
        for key in &board.keys {
            let n = counts[key.index as usize];
            if key.x < mid {
                left += n;
            } else {
                right += n;
            }
        }
        let denom = (left + right).max(1) as f64;
        HandBalance {
            left,
            right,
            left_pct: ((left as f64 / denom) * 1000.0).round() / 10.0,
            right_pct: ((right as f64 / denom) * 1000.0).round() / 10.0,
        }
    });

    Ok(Summary {
        board: board.card.clone(),
        layout: chosen_layout,
        days: entries.len(),
        total_presses,
        per_layer,
        unused_keys,
        rare_keys,
        top_keys,
        hand_balance,
        counts,
    })
}

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

    #[test]
    fn summarizes_unused_keys_and_split_hand_balance() {
        let dir = tempfile::tempdir().unwrap();
        let board = crate::board::test_board();
        let mut t = Telemetry::open(dir.path().into(), &board.card, "lay/1", board.keys.len(), "2026-01-01").unwrap();
        // Press one left-half key (index 0) three times and one right-half key (index 36) once.
        for _ in 0..3 {
            t.record(0, 0, "2026-01-01").unwrap();
        }
        t.record(1, 36, "2026-01-01").unwrap();
        t.flush().unwrap();

        let s = summarize(dir.path(), &board, None, None).unwrap();
        assert_eq!(s.total_presses, 4);
        assert_eq!(s.per_layer.get(&0), Some(&3));
        assert_eq!(s.per_layer.get(&1), Some(&1));
        assert!(!s.unused_keys.contains(&0));
        assert!(!s.unused_keys.contains(&36));
        assert!(s.unused_keys.contains(&1));
        let balance = s.hand_balance.expect("split board reports hand balance");
        assert_eq!((balance.left, balance.right), (3, 1));
        assert_eq!((balance.left_pct, balance.right_pct), (75.0, 25.0));
    }
}
