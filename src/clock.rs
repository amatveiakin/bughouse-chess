use std::time::Duration;

use enum_map::{EnumMap, enum_map};
use instant::Instant;
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
    pub fn from_now_game_active(game_start: Instant, now: Instant) -> Self {
        GameInstant {
            elapsed_since_start: now - game_start,
            measurement: TimeMeasurement::Exact
        }
    }
    pub fn from_now_game_maybe_active(game_start: Option<Instant>, now: Instant) -> GameInstant {
        match game_start {
            Some(t) => GameInstant::from_now_game_active(t, now),
            None => GameInstant::game_start(),
        }
    }
    pub fn from_pair_game_active(pair: WallGameTimePair, now: Instant) -> GameInstant {
        GameInstant {
            elapsed_since_start: (now - pair.world_t) + pair.game_t.elapsed_since_start,
            measurement: pair.game_t.measurement,
        }
    }
    pub fn from_pair_game_maybe_active(pair: Option<WallGameTimePair>, now: Instant) -> GameInstant {
        match pair {
            Some(pair) => GameInstant::from_pair_game_active(pair, now),
            None => GameInstant::game_start(),
        }
    }

    pub fn game_start() -> Self {
        GameInstant {
            elapsed_since_start: Duration::ZERO,
            measurement: TimeMeasurement::Exact
        }
    }
    pub fn elapsed_since_start(self) -> Duration {
        self.elapsed_since_start
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
}


// We want to do something like
//   game_start = Instant::now() - time.elapsed_since_start()
// when reconnecting to existing game, but this could panic because Rust doesn't
// allow for negative durations. So this class can be used to sync game clock and
// real-world clock instead.
#[derive(Clone, Copy, Debug)]
pub struct WallGameTimePair {
    world_t: Instant,
    game_t: GameInstant,
}

impl WallGameTimePair {
    pub fn new(world_t: Instant, game_t: GameInstant) -> Self {
        WallGameTimePair{ world_t, game_t }
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
            // TODO: Fix assertion failure in web client when reconnecting to game over.
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
