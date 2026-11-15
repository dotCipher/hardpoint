//! The gatekeeper. Every change an agent proposes passes through here, and
//! what happens to it is decided by its label alone:
//!
//! - `reversible`: the old value is recorded, the change is made, and it can
//!   be undone.
//! - `guarded`: the same, but the change goes back by itself unless a person
//!   keeps it in time.
//! - `gated`: nothing is sent to the device until a person approves this
//!   exact change. If nobody answers, the answer is no.
//!
//! There is no I/O here. Whatever reaches the hardware implements `Target`,
//! so the rules are tested without any.

use crate::catalog::Risk;
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Whatever can read and change a device. `read` returns the value in a form
/// `write` accepts back, which is what makes undo possible.
pub trait Target {
    fn read(&mut self, device: &str, capability: &str) -> Result<Value, String>;
    fn write(&mut self, device: &str, capability: &str, value: &Value) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Made, and can be undone.
    Applied,
    /// Made, and going back unless a person keeps it.
    Holding,
    /// Not made. Waiting for a person to approve it.
    Awaiting,
    Kept,
    /// Nobody kept it in time, so it was put back.
    Reverted,
    Undone,
    Declined,
    /// Nobody approved it in time. Nothing was done.
    Expired,
}

#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub id: u64,
    pub device: String,
    pub capability: String,
    pub risk: Risk,
    /// The value before the change, once one has been made.
    pub before: Option<Value>,
    pub after: Value,
    pub state: State,
    /// Seconds a person has left to answer, while one is being waited for.
    pub seconds_left: Option<u64>,
    #[serde(skip)]
    deadline: Option<Instant>,
}

pub struct Gate {
    journal: Vec<Change>,
    next: u64,
    hold: Duration,
    approval: Duration,
}

impl Gate {
    pub fn new(hold: Duration, approval: Duration) -> Gate {
        Gate { journal: Vec::new(), next: 1, hold, approval }
    }

    /// `risk` must already be the strictest of the card's label, the
    /// vocabulary's floor and anything the owner has added.
    pub fn propose(
        &mut self,
        device: &str,
        capability: &str,
        risk: Risk,
        value: Value,
        target: &mut dyn Target,
        now: Instant,
    ) -> Result<Change, String> {
        let mut change = Change {
            id: self.next,
            device: device.to_string(),
            capability: capability.to_string(),
            risk,
            before: None,
            after: value,
            state: State::Awaiting,
            seconds_left: None,
            deadline: None,
        };
        match risk {
            Risk::Read => return Err("this can only be read, not changed".to_string()),
            Risk::Gated => change.deadline = Some(now + self.approval),
            Risk::Reversible | Risk::Guarded => {
                // Record the old value first: a change that cannot be read back cannot be undone.
                change.before = Some(target.read(device, capability)?);
                target.write(device, capability, &change.after)?;
                change.state = if risk == Risk::Guarded { State::Holding } else { State::Applied };
                change.deadline = (risk == Risk::Guarded).then(|| now + self.hold);
            }
        }
        self.next += 1;
        self.journal.push(change);
        Ok(self.view(self.journal.len() - 1, now))
    }

    /// A person keeps a guarded change.
    pub fn keep(&mut self, id: u64, now: Instant) -> Result<Change, String> {
        let i = self.find(id)?;
        if self.journal[i].state != State::Holding {
            return Err(format!("change {id} is not waiting to be kept"));
        }
        self.journal[i].state = State::Kept;
        self.journal[i].deadline = None;
        Ok(self.view(i, now))
    }

    /// A person approves a gated change, and only now is it made.
    pub fn approve(&mut self, id: u64, target: &mut dyn Target, now: Instant) -> Result<Change, String> {
        let i = self.find(id)?;
        let change = &self.journal[i];
        if change.state != State::Awaiting {
            return Err(format!("change {id} is not waiting for approval"));
        }
        if change.deadline.is_some_and(|d| now >= d) {
            self.journal[i].state = State::Expired;
            return Err(format!("change {id} was not approved in time, so nothing was done"));
        }
        let (device, capability, after) = (change.device.clone(), change.capability.clone(), change.after.clone());
        let before = target.read(&device, &capability).ok();
        target.write(&device, &capability, &after)?;
        let change = &mut self.journal[i];
        change.before = before;
        change.state = State::Applied;
        change.deadline = None;
        Ok(self.view(i, now))
    }

    /// A person says no: a waiting change is dropped, a held one is put back.
    pub fn decline(&mut self, id: u64, target: &mut dyn Target, now: Instant) -> Result<Change, String> {
        let i = self.find(id)?;
        match self.journal[i].state {
            State::Awaiting => {
                self.journal[i].state = State::Declined;
                self.journal[i].deadline = None;
                Ok(self.view(i, now))
            }
            State::Holding => self.undo(id, target, now),
            _ => Err(format!("change {id} is not waiting for an answer")),
        }
    }

    /// Put the old value back.
    pub fn undo(&mut self, id: u64, target: &mut dyn Target, now: Instant) -> Result<Change, String> {
        let i = self.find(id)?;
        let change = &self.journal[i];
        if !matches!(change.state, State::Applied | State::Holding | State::Kept) {
            return Err(format!("change {id} is not in effect, so there is nothing to undo"));
        }
        let Some(before) = change.before.clone() else {
            return Err(format!("change {id} has no recorded earlier value to go back to"));
        };
        // Undoing out of order would put back a value a later change has already replaced.
        let later = self.journal[i + 1..].iter().any(|c| {
            c.device == change.device && c.capability == change.capability && matches!(c.state, State::Applied | State::Holding | State::Kept)
        });
        if later {
            return Err(format!("a later change to the same thing is still in effect; undo that one first"));
        }
        target.write(&change.device.clone(), &change.capability.clone(), &before)?;
        self.journal[i].state = State::Undone;
        self.journal[i].deadline = None;
        Ok(self.view(i, now))
    }

    /// Called regularly. Puts back guarded changes nobody kept, and drops
    /// gated ones nobody approved. Returns what changed.
    pub fn tick(&mut self, target: &mut dyn Target, now: Instant) -> Vec<Change> {
        let mut settled = Vec::new();
        for i in 0..self.journal.len() {
            let change = &self.journal[i];
            if !change.deadline.is_some_and(|d| now >= d) {
                continue;
            }
            match change.state {
                State::Awaiting => self.journal[i].state = State::Expired,
                State::Holding => {
                    let (device, capability) = (change.device.clone(), change.capability.clone());
                    if let Some(before) = change.before.clone() {
                        // If the device cannot be reached, keep trying on later ticks.
                        if target.write(&device, &capability, &before).is_err() {
                            continue;
                        }
                    }
                    self.journal[i].state = State::Reverted;
                }
                _ => {}
            }
            self.journal[i].deadline = None;
            settled.push(self.view(i, now));
        }
        settled
    }

    /// The oldest change a person is being waited on for.
    pub fn waiting(&self, now: Instant) -> Option<Change> {
        let i = self.journal.iter().position(|c| matches!(c.state, State::Holding | State::Awaiting))?;
        Some(self.view(i, now))
    }

    pub fn changes(&self, now: Instant) -> Vec<Change> {
        (0..self.journal.len()).map(|i| self.view(i, now)).collect()
    }

    fn find(&self, id: u64) -> Result<usize, String> {
        self.journal.iter().position(|c| c.id == id).ok_or_else(|| format!("there is no change {id}"))
    }

    fn view(&self, i: usize, now: Instant) -> Change {
        let mut change = self.journal[i].clone();
        change.seconds_left = change.deadline.map(|d| d.saturating_duration_since(now).as_secs());
        change
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    /// A stand-in device: a few named values, and a switch to make it unreachable.
    #[derive(Default)]
    struct Fake {
        values: BTreeMap<String, Value>,
        writes: u32,
        down: bool,
    }

    impl Target for Fake {
        fn read(&mut self, _device: &str, capability: &str) -> Result<Value, String> {
            if self.down {
                return Err("unreachable".into());
            }
            Ok(self.values.get(capability).cloned().unwrap_or(Value::Null))
        }
        fn write(&mut self, _device: &str, capability: &str, value: &Value) -> Result<(), String> {
            if self.down {
                return Err("unreachable".into());
            }
            self.writes += 1;
            self.values.insert(capability.to_string(), value.clone());
            Ok(())
        }
    }

    fn gate() -> Gate {
        Gate::new(Duration::from_secs(10), Duration::from_secs(60))
    }

    #[test]
    fn a_reversible_change_is_made_at_once_and_can_be_undone() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        dev.values.insert("light".into(), json!("off"));
        let change = g.propose("d", "light", Risk::Reversible, json!("blue"), &mut dev, now).unwrap();
        assert_eq!((change.state, change.before.clone()), (State::Applied, Some(json!("off"))));
        assert_eq!(dev.values["light"], json!("blue"));
        assert_eq!(g.undo(change.id, &mut dev, now).unwrap().state, State::Undone);
        assert_eq!(dev.values["light"], json!("off"));
        assert!(g.undo(change.id, &mut dev, now).is_err(), "it cannot be undone twice");
    }

    #[test]
    fn a_guarded_change_goes_back_unless_a_person_keeps_it() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        dev.values.insert("input".into(), json!("this computer"));
        let left = g.propose("d", "input", Risk::Guarded, json!("other"), &mut dev, now).unwrap();
        let kept = g.propose("d", "speed", Risk::Guarded, json!("fast"), &mut dev, now).unwrap();
        assert_eq!((left.state, left.seconds_left), (State::Holding, Some(10)));
        g.keep(kept.id, now + Duration::from_secs(3)).unwrap();
        assert!(g.tick(&mut dev, now + Duration::from_secs(9)).is_empty(), "nothing happens before the time is up");
        let settled = g.tick(&mut dev, now + Duration::from_secs(10));
        assert_eq!(settled.iter().map(|c| (c.id, c.state)).collect::<Vec<_>>(), vec![(left.id, State::Reverted)]);
        assert_eq!(dev.values["input"], json!("this computer"));
        assert_eq!(dev.values["speed"], json!("fast"), "the kept one stays");
    }

    #[test]
    fn a_gated_change_touches_nothing_until_a_person_approves_it() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        let change = g.propose("d", "plug", Risk::Gated, json!("off"), &mut dev, now).unwrap();
        assert_eq!((change.state, dev.writes), (State::Awaiting, 0));
        let done = g.approve(change.id, &mut dev, now + Duration::from_secs(5)).unwrap();
        assert_eq!((done.state, dev.writes), (State::Applied, 1));
        assert!(g.approve(change.id, &mut dev, now).is_err(), "an approval is used once");
    }

    #[test]
    fn silence_means_no() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        let ignored = g.propose("d", "plug", Risk::Gated, json!("off"), &mut dev, now).unwrap();
        let late = g.propose("d", "door", Risk::Gated, json!("unlocked"), &mut dev, now).unwrap();
        let declined = g.propose("d", "firmware", Risk::Gated, json!("new"), &mut dev, now).unwrap();
        assert_eq!(g.decline(declined.id, &mut dev, now).unwrap().state, State::Declined);
        // One is never answered; one is answered after the time ran out.
        assert!(g.approve(late.id, &mut dev, now + Duration::from_secs(61)).is_err());
        g.tick(&mut dev, now + Duration::from_secs(61));
        let states: Vec<State> = g.changes(now).iter().map(|c| c.state).collect();
        assert_eq!(states, vec![State::Expired, State::Expired, State::Declined]);
        assert_eq!(dev.writes, 0, "nothing ever reached the device");
        assert!(g.approve(ignored.id, &mut dev, now).is_err());
    }

    #[test]
    fn something_that_can_only_be_read_is_never_changed() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        assert!(g.propose("d", "battery", Risk::Read, json!(100), &mut dev, now).is_err());
        assert_eq!(dev.writes, 0);
        assert!(g.changes(now).is_empty());
    }

    #[test]
    fn a_change_that_cannot_be_read_back_is_not_made() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        dev.down = true;
        assert!(g.propose("d", "light", Risk::Reversible, json!("blue"), &mut dev, now).is_err());
        assert!(g.changes(now).is_empty(), "a failed change leaves no record of having happened");
    }

    #[test]
    fn undo_runs_newest_first() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        dev.values.insert("light".into(), json!("off"));
        let first = g.propose("d", "light", Risk::Reversible, json!("blue"), &mut dev, now).unwrap();
        let second = g.propose("d", "light", Risk::Reversible, json!("red"), &mut dev, now).unwrap();
        assert!(g.undo(first.id, &mut dev, now).is_err(), "the older one must wait");
        g.undo(second.id, &mut dev, now).unwrap();
        g.undo(first.id, &mut dev, now).unwrap();
        assert_eq!(dev.values["light"], json!("off"));
    }

    #[test]
    fn a_held_change_is_still_put_back_after_the_device_returns() {
        let (mut g, mut dev, now) = (gate(), Fake::default(), Instant::now());
        dev.values.insert("input".into(), json!("here"));
        let change = g.propose("d", "input", Risk::Guarded, json!("there"), &mut dev, now).unwrap();
        dev.down = true;
        assert!(g.tick(&mut dev, now + Duration::from_secs(11)).is_empty());
        assert_eq!(g.waiting(now).map(|c| c.id), Some(change.id), "it is still owed a revert");
        dev.down = false;
        assert_eq!(g.tick(&mut dev, now + Duration::from_secs(12))[0].state, State::Reverted);
        assert_eq!(dev.values["input"], json!("here"));
    }
}
