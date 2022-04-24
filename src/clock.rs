use std::time::{Instant, Duration};

use enum_map::{EnumMap, enum_map};
use serde::{Serialize, Deserialize};

use crate::force::Force;


// TODO: Disable time control in tests.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeControl {
    pub starting_time: Duration,
    // TODO: Support increment, delay, etc.
}


#[derive(Clone, Copy)]
pub enum TimeMeasurement {
    Exact,  // for updating game state
    Approximate,  // for network interfaces where times can be sligtly desynced
}


// Time since game start.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct GameInstant {
    elapsed_since_start: Duration,
}

impl GameInstant {
    pub fn new(game_start: Instant, now: Instant) -> GameInstant {
        GameInstant{ elapsed_since_start: now - game_start }
    }
    pub fn game_start() -> GameInstant {
        GameInstant{ elapsed_since_start: Duration::ZERO }
    }
    pub fn duration_since(self, earlier: GameInstant, measurement: TimeMeasurement) -> Duration {
        match measurement {
            TimeMeasurement::Exact =>
                self.elapsed_since_start.checked_sub(earlier.elapsed_since_start).unwrap(),
            TimeMeasurement::Approximate =>
                self.elapsed_since_start.saturating_sub(earlier.elapsed_since_start),
        }
    }
}


#[derive(Clone, Debug)]
pub struct Clock {
    turn_state: Option<(Force, GameInstant)>,  // force, start time
    remaining_time: EnumMap<Force, Duration>,
    #[allow(dead_code)] control: TimeControl,
}

impl Clock {
    pub fn new(control: TimeControl) -> Self {
        Self {
            turn_state: None,
            remaining_time: enum_map!{ _ => control.starting_time },
            control,
        }
    }

    pub fn is_active(&self) -> bool { self.turn_state.is_some() }
    pub fn active_force(&self) -> Option<Force> { self.turn_state.map(|st| st.0) }
    pub fn turn_start(&self) -> Option<GameInstant> { self.turn_state.map(|st| st.1) }

    pub fn time_left(&self, force: Force, now: GameInstant, measurement: TimeMeasurement) -> Duration {
        let mut ret = self.remaining_time[force];
        if let Some((current_force, current_start)) = self.turn_state {
            if force == current_force {
                ret = ret.saturating_sub(now.duration_since(current_start, measurement));
            }
        }
        ret
    }

    pub fn new_turn(&mut self, new_force: Force, now: GameInstant) {
        if let Some((prev_force, _)) = self.turn_state {
            assert_ne!(prev_force, new_force);
            let remaining = self.time_left(prev_force, now, TimeMeasurement::Exact);
            assert!(remaining > Duration::ZERO);
            self.remaining_time[prev_force] = remaining;
        }
        self.turn_state = Some((new_force, now));
    }

    pub fn stop(&mut self, now: GameInstant) {
        if let Some((prev_force, _)) = self.turn_state {
            let remaining = self.time_left(prev_force, now, TimeMeasurement::Exact);
            self.remaining_time[prev_force] = remaining;
        }
        self.turn_state = None;
    }
}
