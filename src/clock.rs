use std::time::{Instant, Duration};

use enum_map::{EnumMap, enum_map};
use serde::{Serialize, Deserialize};

use crate::force::Force;


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeControl {
    pub starting_time: Duration,
    // Improvement potential. Support increment, delay, etc.
}


#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum TimeMeasurement {
    Exact,
    Approximate,
}

// Time since game start.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct GameInstant {
    elapsed_since_start: Duration,
    measurement: TimeMeasurement,
}

impl GameInstant {
    pub fn from_active_game(game_start: Instant, now: Instant) -> Self {
        GameInstant::new(now - game_start)
    }
    pub fn from_maybe_active_game(game_start: Option<Instant>, now: Instant) -> GameInstant {
        match game_start {
            Some(t) => GameInstant::from_active_game(t, now),
            None => GameInstant::game_start(),
        }
    }
    pub fn game_start() -> Self {
        GameInstant::new(Duration::ZERO)
    }
    pub fn duration_since(self, earlier: GameInstant) -> Duration {
        use TimeMeasurement::*;
        match (self.measurement, earlier.measurement) {
            (Exact, Exact) =>
                self.elapsed_since_start.checked_sub(earlier.elapsed_since_start).unwrap(),
            (Approximate, _) | (_, Approximate) =>
                self.elapsed_since_start.saturating_sub(earlier.elapsed_since_start),
        }
    }
    // Mark as approximate, so that attemps to go back in time wouldn't panic. Could be
    // used in online clients where local time and server time can be sligtly desynced.
    // Should not be used on the server side or in offline clients - if you get a crash
    // without it this is likely a bug.
    pub fn approximate(mut self) -> Self {
        self.measurement = TimeMeasurement::Approximate;
        self
    }

    fn new(elapsed_since_start: Duration) -> Self {
        GameInstant{ elapsed_since_start, measurement: TimeMeasurement::Exact }
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

    pub fn time_left(&self, force: Force, now: GameInstant) -> Duration {
        let mut ret = self.remaining_time[force];
        if let Some((current_force, current_start)) = self.turn_state {
            if force == current_force {
                ret = ret.saturating_sub(now.duration_since(current_start));
            }
        }
        ret
    }

    pub fn new_turn(&mut self, new_force: Force, now: GameInstant) {
        if let Some((prev_force, _)) = self.turn_state {
            assert_ne!(prev_force, new_force);
            let remaining = self.time_left(prev_force, now);
            assert!(remaining > Duration::ZERO);
            self.remaining_time[prev_force] = remaining;
        }
        self.turn_state = Some((new_force, now));
    }

    pub fn stop(&mut self, now: GameInstant) {
        if let Some((prev_force, _)) = self.turn_state {
            let remaining = self.time_left(prev_force, now);
            self.remaining_time[prev_force] = remaining;
        }
        self.turn_state = None;
    }
}
