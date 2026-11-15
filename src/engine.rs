//! The engine turns what sources report into a frame of key colours, and
//! key presses into actions. It has no I/O, so it is tested directly.

use crate::board::Board;
use crate::config::{Config, IdleMode, SlotKind};
use crate::model::{AgentState, AgentUpdate, Rgb};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Session {
    pub source: String,
    pub id: String,
    pub state: AgentState,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub directory: Option<String>,
    pub changed: Instant,
}

/// Something the caller should run because a slot was pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub command: String,
    pub env: Vec<(String, String)>,
}

pub struct Engine {
    cfg: Config,
    board: Board,
    sessions: Vec<Session>,
    /// For each configured slot, the (source, session) it currently shows.
    assigned: Vec<Option<(String, String)>>,
    /// The slot key that was just pressed, so it can flash in reply.
    pressed: Option<(u8, Instant)>,
    started: Instant,
}

/// How long a pressed slot flashes to confirm the press was received.
const PRESS_FLASH: Duration = Duration::from_millis(350);

impl Engine {
    pub fn new(cfg: Config, board: Board) -> Engine {
        let assigned = vec![None; cfg.slots.len()];
        Engine { cfg, board, sessions: Vec::new(), assigned, pressed: None, started: Instant::now() }
    }

    pub fn apply(&mut self, update: AgentUpdate, now: Instant) {
        match self.sessions.iter_mut().find(|s| s.source == update.source && s.id == update.session) {
            Some(s) => {
                if s.state != update.state {
                    s.state = update.state;
                    s.changed = now;
                }
                s.title = update.title.or(s.title.take());
                s.directory = update.directory.or(s.directory.take());
                s.detail = update.detail;
            }
            None => self.sessions.push(Session {
                source: update.source,
                id: update.session,
                state: update.state,
                title: update.title,
                detail: update.detail,
                directory: update.directory,
                changed: now,
            }),
        }
        self.reassign();
    }

    /// Whether any session is in this state right now.
    pub fn any(&self, state: AgentState) -> bool {
        self.sessions.iter().any(|s| s.state == state)
    }

    pub fn gone(&mut self, source: &str, session: &str) {
        self.sessions.retain(|s| !(s.source == source && s.id == session));
        self.reassign();
    }

    /// Finished and failed sessions stop being shown after their hold time.
    pub fn expire(&mut self, now: Instant) {
        let done = Duration::from_secs(self.cfg.lighting.done_hold_secs);
        let failed = Duration::from_secs(self.cfg.lighting.failed_hold_secs);
        let mut changed = false;
        for s in &mut self.sessions {
            let age = now.saturating_duration_since(s.changed);
            if (s.state == AgentState::Done && age >= done) || (s.state == AgentState::Failed && age >= failed) {
                s.state = AgentState::Idle;
                s.changed = now;
                changed = true;
            }
        }
        if changed {
            self.reassign();
        }
    }

    fn session(&self, key: &(String, String)) -> Option<&Session> {
        self.sessions.iter().find(|s| s.source == key.0 && s.id == key.1)
    }

    /// Sessions keep their slot while they are visible. When slots run out, a
    /// waiting session takes the slot of a strictly less urgent one.
    fn reassign(&mut self) {
        let visible = |s: &Session| s.state != AgentState::Idle;
        for i in 0..self.assigned.len() {
            let keep = self.assigned[i].as_ref().and_then(|k| self.session(k)).is_some_and(visible);
            if !keep {
                self.assigned[i] = None;
            }
        }
        let mut waiting: Vec<&Session> = self
            .sessions
            .iter()
            .filter(|s| visible(s))
            .filter(|s| !self.assigned.iter().flatten().any(|k| k.0 == s.source && k.1 == s.id))
            .collect();
        waiting.sort_by(|a, b| b.state.priority().cmp(&a.state.priority()).then(a.changed.cmp(&b.changed)));
        let waiting: Vec<(String, String, u8)> =
            waiting.into_iter().map(|s| (s.source.clone(), s.id.clone(), s.state.priority())).collect();

        for (source, id, priority) in waiting {
            let agent_slots = || (0..self.cfg.slots.len()).filter(|i| self.cfg.slots[*i].kind == SlotKind::Agent);
            let free = agent_slots().find(|i| self.assigned[*i].is_none());
            let target = free.or_else(|| {
                agent_slots()
                    .filter_map(|i| {
                        let p = self.assigned[i].as_ref().and_then(|k| self.session(k)).map(|s| s.state.priority())?;
                        (p < priority).then_some((p, i))
                    })
                    .min()
                    .map(|(_, i)| i)
            });
            if let Some(i) = target {
                self.assigned[i] = Some((source, id));
            }
        }

        // A session that wants a response belongs on a key that can give one:
        // trade places with a less urgent session sitting on a pressable slot.
        let urgency = |this: &Engine, i: usize| {
            this.assigned[i].as_ref().and_then(|k| this.session(k)).map_or(0, |s| s.state.priority())
        };
        let wants_response = AgentState::Failed.priority();
        for i in 0..self.cfg.slots.len() {
            let slot = &self.cfg.slots[i];
            if slot.kind != SlotKind::Agent || slot.pressable || urgency(self, i) < wants_response {
                continue;
            }
            let swap = (0..self.cfg.slots.len())
                .filter(|j| self.cfg.slots[*j].kind == SlotKind::Agent && self.cfg.slots[*j].pressable)
                .map(|j| (urgency(self, j), j))
                .filter(|(u, _)| *u < wants_response)
                .min();
            if let Some((_, j)) = swap {
                self.assigned.swap(i, j);
            }
        }
    }

    fn state_color(&self, state: AgentState) -> Rgb {
        match state {
            AgentState::Running => self.cfg.colors.running,
            AgentState::NeedsYou => self.cfg.colors.needs_you,
            AgentState::Done => self.cfg.colors.done,
            AgentState::Failed => self.cfg.colors.failed,
            AgentState::Idle => self.cfg.lighting.empty_slot,
        }
    }

    /// The colour of every LED right now, or `None` when the keyboard's own
    /// lighting should show instead.
    pub fn frame(&self, now: Instant, hour: u32) -> Option<Vec<Rgb>> {
        let any_session = self.assigned.iter().any(Option::is_some);
        let any_fixed = self.cfg.slots.iter().any(|s| s.kind == SlotKind::Fixed);
        if !any_session && !any_fixed && self.cfg.lighting.idle == IdleMode::Release {
            return None;
        }
        let level = self.cfg.brightness.at_hour(hour);
        let mut frame = vec![self.cfg.lighting.base.scale(level); self.board.led_count()];
        let t = now.saturating_duration_since(self.started).as_secs_f32();
        for (i, slot) in self.cfg.slots.iter().enumerate() {
            let Some(key) = self.board.by_index(slot.key) else { continue };
            let color = match slot.kind {
                SlotKind::Fixed => slot.color.unwrap_or(Rgb::OFF),
                SlotKind::Agent => match self.assigned[i].as_ref().and_then(|k| self.session(k)) {
                    None => self.cfg.lighting.empty_slot,
                    Some(s) => {
                        let c = self.state_color(s.state);
                        match s.state {
                            // Needing you pulses; everything else holds still.
                            AgentState::NeedsYou => c.scale(0.25 + 0.75 * (0.5 + 0.5 * (t * 5.2).sin())),
                            AgentState::Done => {
                                let hold = self.cfg.lighting.done_hold_secs.max(1) as f32;
                                let age = now.saturating_duration_since(s.changed).as_secs_f32();
                                c.scale(1.0 - 0.8 * (age / hold).min(1.0))
                            }
                            _ => c,
                        }
                    }
                },
            };
            let flashing = self.pressed.is_some_and(|(k, at)| k == slot.key && now.saturating_duration_since(at) < PRESS_FLASH);
            frame[key.led as usize] = if flashing { Rgb::new(255, 255, 255).scale(level) } else { color.scale(level) };
        }
        Some(frame)
    }

    /// A key went down, identified by its matrix position.
    pub fn press_matrix(&mut self, row: u8, col: u8, now: Instant) -> Option<Action> {
        let index = self.board.by_matrix(row, col)?.index;
        self.press(index, now)
    }

    pub fn press(&mut self, key: u8, now: Instant) -> Option<Action> {
        let i = self.cfg.slots.iter().position(|s| s.key == key && s.pressable)?;
        let slot = self.cfg.slots[i].clone();
        match slot.kind {
            SlotKind::Fixed => {
                self.pressed = Some((key, now));
                slot.command.filter(|c| !c.is_empty()).map(|command| Action {
                    command,
                    env: vec![("HARDPOINT_KEY".into(), key.to_string())],
                })
            }
            SlotKind::Agent => {
                let target = self.assigned[i].clone()?;
                let s = self.session(&target)?.clone();
                self.pressed = Some((key, now));
                let state = serde_json::to_value(s.state).ok().and_then(|v| v.as_str().map(str::to_string));
                // Pressing a finished or failed session acknowledges it.
                if matches!(s.state, AgentState::Done | AgentState::Failed) {
                    if let Some(live) = self.sessions.iter_mut().find(|x| x.source == s.source && x.id == s.id) {
                        live.state = AgentState::Idle;
                        live.changed = now;
                    }
                    self.reassign();
                }
                let command = self.cfg.actions.agent_press.clone();
                (!command.is_empty()).then(|| Action {
                    command,
                    env: vec![
                        ("HARDPOINT_KEY".into(), key.to_string()),
                        ("HARDPOINT_SOURCE".into(), s.source.clone()),
                        ("HARDPOINT_SESSION".into(), s.id.clone()),
                        ("HARDPOINT_TITLE".into(), s.title.clone().unwrap_or_default()),
                        ("HARDPOINT_DIRECTORY".into(), s.directory.clone().unwrap_or_default()),
                        ("HARDPOINT_STATE".into(), state.unwrap_or_default()),
                    ],
                })
            }
        }
    }

    pub fn snapshot(&self) -> serde_json::Value {
        let slots: Vec<_> = self
            .cfg
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let s = self.assigned[i].as_ref().and_then(|k| self.session(k));
                json!({
                    "key": slot.key,
                    "pressable": slot.pressable,
                    "session": s.map(|s| s.id.clone()),
                    "source": s.map(|s| s.source.clone()),
                    "state": s.map(|s| s.state),
                    "title": s.and_then(|s| s.title.clone()),
                })
            })
            .collect();
        let active = self.sessions.iter().filter(|s| s.state != AgentState::Idle).count();
        json!({ "sessions": self.sessions.len(), "active": active, "slots": slots })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::test_board;

    /// The example setup: six agent slots, the first two pressable.
    const EXAMPLE: &str = include_str!("../examples/moonlander.toml");

    fn engine() -> Engine {
        Engine::new(Config::parse(EXAMPLE).unwrap(), test_board())
    }

    fn update(id: &str, state: AgentState) -> AgentUpdate {
        AgentUpdate { source: "test".into(), session: id.into(), state, title: Some(format!("t-{id}")), detail: None, directory: None }
    }

    fn led(e: &Engine, key: u8) -> u8 {
        e.board.by_index(key).unwrap().led
    }

    #[test]
    fn idle_board_is_released_to_the_firmware() {
        let mut e = engine();
        let now = Instant::now();
        assert!(e.frame(now, 12).is_none());
        e.apply(update("a", AgentState::Idle), now);
        assert!(e.frame(now, 12).is_none());
    }

    #[test]
    fn a_running_session_lights_the_first_slot_and_nothing_else_brightly() {
        let mut e = engine();
        let now = Instant::now();
        e.apply(update("a", AgentState::Running), now);
        let frame = e.frame(now, 12).unwrap();
        let level = 0.8;
        assert_eq!(frame[led(&e, 36) as usize], e.cfg.colors.running.scale(level));
        assert_eq!(frame[led(&e, 50) as usize], Rgb::OFF);
        assert_eq!(frame[led(&e, 15) as usize], e.cfg.lighting.base.scale(level));
    }

    #[test]
    fn night_is_dimmer_than_day() {
        let mut e = engine();
        let now = Instant::now();
        e.apply(update("a", AgentState::Running), now);
        let day = e.frame(now, 12).unwrap()[led(&e, 36) as usize];
        let night = e.frame(now, 23).unwrap()[led(&e, 36) as usize];
        assert!(night.b < day.b);
    }

    #[test]
    fn sessions_keep_their_slot_as_others_come_and_go() {
        let mut e = engine();
        let now = Instant::now();
        e.apply(update("a", AgentState::Running), now);
        e.apply(update("b", AgentState::Running), now);
        assert_eq!(e.assigned[0].as_ref().unwrap().1, "a");
        assert_eq!(e.assigned[1].as_ref().unwrap().1, "b");
        e.apply(update("a", AgentState::Idle), now);
        assert!(e.assigned[0].is_none());
        assert_eq!(e.assigned[1].as_ref().unwrap().1, "b");
        e.apply(update("c", AgentState::Running), now);
        assert_eq!(e.assigned[0].as_ref().unwrap().1, "c");
    }

    #[test]
    fn a_session_that_needs_you_takes_a_slot_from_a_running_one_when_full() {
        let mut e = engine();
        let now = Instant::now();
        for id in ["a", "b", "c", "d", "e", "f"] {
            e.apply(update(id, AgentState::Running), now);
        }
        e.apply(update("g", AgentState::Running), now);
        assert!(!e.assigned.iter().flatten().any(|k| k.1 == "g"));
        e.apply(update("g", AgentState::NeedsYou), now);
        assert!(e.assigned.iter().flatten().any(|k| k.1 == "g"));
        assert_eq!(e.assigned.iter().flatten().count(), 6);
    }

    #[test]
    fn a_session_that_needs_you_moves_to_a_pressable_key() {
        let mut e = engine();
        let now = Instant::now();
        e.apply(update("a", AgentState::Running), now);
        e.apply(update("b", AgentState::Running), now);
        e.apply(update("c", AgentState::Running), now);
        assert_eq!(e.assigned[2].as_ref().unwrap().1, "c", "third session starts on a display-only key");
        e.apply(update("c", AgentState::NeedsYou), now);
        assert_eq!(e.assigned[0].as_ref().unwrap().1, "c", "and moves to pressable key 36 when it needs you");
        assert_eq!(e.assigned[2].as_ref().unwrap().1, "a");
        // Two urgent sessions fill both pressable keys; a third stays where it is.
        e.apply(update("b", AgentState::Failed), now);
        e.apply(update("a", AgentState::NeedsYou), now);
        assert_eq!(e.assigned[0].as_ref().unwrap().1, "c");
        assert_eq!(e.assigned[1].as_ref().unwrap().1, "b");
        assert_eq!(e.assigned[2].as_ref().unwrap().1, "a");
    }

    #[test]
    fn needing_you_pulses_and_done_fades_then_expires() {
        let mut e = engine();
        let t0 = Instant::now();
        e.apply(update("a", AgentState::NeedsYou), t0);
        let i = led(&e, 36) as usize;
        let samples: std::collections::HashSet<u8> =
            (0..12).map(|n| e.frame(t0 + Duration::from_millis(n * 100), 12).unwrap()[i].r).collect();
        assert!(samples.len() > 3, "expected a pulse, got {samples:?}");

        e.apply(update("a", AgentState::Done), t0);
        let early = e.frame(t0, 12).unwrap()[i].g;
        let late = e.frame(t0 + Duration::from_secs(15), 12).unwrap()[i].g;
        assert!(late < early);
        e.expire(t0 + Duration::from_secs(21));
        assert!(e.frame(t0 + Duration::from_secs(21), 12).is_none());
    }

    #[test]
    fn only_pressable_slots_with_a_session_act() {
        let mut cfg = Config::parse(EXAMPLE).unwrap();
        cfg.actions.agent_press = "true".into();
        let mut e = Engine::new(cfg, test_board());
        let now = Instant::now();
        assert!(e.press(36, now).is_none(), "empty slot");
        e.apply(update("a", AgentState::NeedsYou), now);
        assert!(e.press(15, now).is_none(), "an ordinary typing key is never a slot");
        assert!(e.press(6, now).is_none(), "display-only slot");
        let key = *e.board.by_index(36).unwrap();
        let action = e.press_matrix(key.row, key.col, now).expect("a press arrives by switch position");
        assert!(action.env.contains(&("HARDPOINT_SESSION".to_string(), "a".to_string())));
        assert!(action.env.contains(&("HARDPOINT_STATE".to_string(), "needs-you".to_string())));
    }

    #[test]
    fn a_pressed_slot_flashes_white_briefly() {
        let mut e = engine();
        let now = Instant::now();
        e.apply(update("a", AgentState::Running), now);
        let i = led(&e, 36) as usize;
        e.press(36, now);
        let white = Rgb::new(255, 255, 255).scale(0.8);
        assert_eq!(e.frame(now + Duration::from_millis(100), 12).unwrap()[i], white);
        assert_ne!(e.frame(now + Duration::from_millis(500), 12).unwrap()[i], white);
        // Display-only slots and ordinary keys never flash.
        e.apply(update("b", AgentState::Running), now);
        e.apply(update("c", AgentState::Running), now);
        e.press(6, now);
        assert_ne!(e.frame(now + Duration::from_millis(100), 12).unwrap()[led(&e, 6) as usize], white);
    }

    #[test]
    fn pressing_a_finished_session_acknowledges_it() {
        let mut e = engine();
        let now = Instant::now();
        e.apply(update("a", AgentState::Failed), now);
        assert!(e.frame(now, 12).is_some());
        e.press(36, now);
        assert!(e.frame(now, 12).is_none());
    }
}
