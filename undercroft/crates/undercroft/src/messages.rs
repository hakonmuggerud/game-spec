//! The event bus. `main.js:makeEvents` fanned `events.emit(name, payload)` out to synchronous
//! listeners; here every state change the sim returns is written as a Bevy [`Message`] and mirrored
//! into an [`EventLog`] ring buffer that tests and the Phase 3 parity checks read.

use bevy::prelude::*;
use std::collections::VecDeque;
use undercroft_sim::SimEvent;

use crate::tick::{SimSet, TickCount};

/// One `events.emit(...)` from the sim. Lanes read `MessageReader<SimMessage>` and match on the
/// payload; there are deliberately no per-variant observers in the skeleton.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct SimMessage(pub SimEvent);

/// Write everything a sim call returned (`const evs = sim.doThing(); evs.forEach(events.emit)`).
pub fn emit(w: &mut MessageWriter<SimMessage>, evs: Vec<SimEvent>) {
    for e in evs {
        w.write(SimMessage(e));
    }
}

/// How many `(tick, event)` pairs [`EventLog`] keeps.
pub const EVENT_LOG_CAP: usize = 4096;

/// A ring buffer of every [`SimEvent`] the app has emitted, newest last, capped at
/// [`EVENT_LOG_CAP`]. Present in native and wasm builds too — it is cheap, and the pause menu can
/// show it later.
#[derive(Resource, Debug, Default)]
pub struct EventLog {
    entries: VecDeque<(u64, SimEvent)>,
}

impl EventLog {
    /// Append one event, dropping the oldest when full.
    pub fn push(&mut self, tick: u64, e: SimEvent) {
        if self.entries.len() == EVENT_LOG_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back((tick, e));
    }

    /// Every `(tick, event)` in order.
    pub fn entries(&self) -> &VecDeque<(u64, SimEvent)> {
        &self.entries
    }

    /// The JS event names in order (`begin`, `hubEnter`, …) — what parity traces compare.
    pub fn names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|(_, e)| e.name()).collect()
    }

    /// How many events with this JS name have been logged.
    pub fn count(&self, name: &str) -> usize {
        self.entries
            .iter()
            .filter(|(_, e)| e.name() == name)
            .count()
    }

    /// The last event with this JS name.
    pub fn last(&self, name: &str) -> Option<&SimEvent> {
        self.entries
            .iter()
            .rev()
            .find(|(_, e)| e.name() == name)
            .map(|(_, e)| e)
    }

    /// Every event with this JS name, oldest first.
    pub fn all(&self, name: &str) -> Vec<&SimEvent> {
        self.entries
            .iter()
            .filter(|(_, e)| e.name() == name)
            .map(|(_, e)| e)
            .collect()
    }

    /// Number of events logged.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No events logged.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Forget everything (a new game).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Mirror this tick's messages into the log. Runs last in `FixedUpdate` ([`SimSet::Fanout`]).
fn log_messages(mut r: MessageReader<SimMessage>, tick: Res<TickCount>, mut log: ResMut<EventLog>) {
    for m in r.read() {
        log.push(tick.0, m.0.clone());
    }
}

/// Messages, the log and the fan-out system.
pub fn plugin(app: &mut App) {
    app.add_message::<SimMessage>()
        .init_resource::<EventLog>()
        .add_systems(FixedUpdate, log_messages.in_set(SimSet::Fanout));
}
