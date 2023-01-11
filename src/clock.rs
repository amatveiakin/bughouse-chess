use std::fmt;
use std::time::Duration;

use enum_map::{EnumMap, enum_map};
use instant::Instant;
use serde::{Serialize, Deserialize};
use strum::IntoEnumIterator;

use crate::force::Force;
use crate::util::div_ceil_u128;


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeControl {
    pub starting_time: Duration,
    // Improvement potential. Support increment, delay, etc.
    //   Note that `Clock::total_time_elapsed` should be adjusted in this case.
}

impl fmt::Display for TimeControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.starting_time.as_secs();
        let minutes = s / 60;
        let seconds = s % 60;
        write!(f, "{minutes}:{seconds:02}")
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TimeMeasurement {
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
    pub fn from_duration(elapsed_since_start: Duration) -> Self {
        GameInstant {
            elapsed_since_start,
            measurement: TimeMeasurement::Exact
        }
    }
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

    pub const fn game_start() -> Self {
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

    pub fn measurement(&self) -> TimeMeasurement { self.measurement }
    pub fn set_measurement(mut self, m: TimeMeasurement) -> Self {
        self.measurement = m;
        self
    }
    // Mark as approximate, so that attemps to go back in time wouldn't panic. Could be
    // used in online clients where local time and server time can be sligtly desynced.
    // Should not be used on the server side or in offline clients - if you get a crash
    // without it this is likely a bug.
    pub fn approximate(self) -> Self { self.set_measurement(TimeMeasurement::Approximate) }
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


pub struct ClockShowing {
    pub is_active: bool,
    pub show_separator: bool,
    pub out_of_time: bool,
    pub time_breakdown: TimeBreakdown,
}

// Improvement potential: Support longer time controls (with hours).
pub enum TimeBreakdown {
    NormalTime {
        minutes: u32,
        seconds: u32,
    },
    LowTime {
        seconds: u32,
        deciseconds: u32,
    },
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

    pub fn showing_for(&self, force: Force, now: GameInstant) -> ClockShowing {
        // Get duration in the highest possible precision. It's important to round the time up,
        // so that we never show "0.00" for a player who has not lost by flag.
        const NANOS_PER_SEC: u128 = 1_000_000_000;
        const NANOS_PER_DECI: u128 = NANOS_PER_SEC / 10;
        let is_active = self.active_force() == Some(force);
        let nanos = self.time_left(force, now).as_nanos();
        let s = nanos / NANOS_PER_SEC;
        let show_separator = !is_active || nanos % NANOS_PER_SEC >= NANOS_PER_SEC / 2;

        // Note. Never consider an active player to be out of time. On the server or in an
        // offline client this never happens, because all clocks stop when the game is over.
        // In an online client an active player can have zero time, but we shouldn't tell the
        // user they've run out of time until the server confirms game result, because the
        // game may have ended earlier on the other board.
        let out_of_time = !is_active && nanos == 0;

        let low_time = s < 20;
        let time_breakdown = if low_time {
            let seconds = s.try_into().unwrap();
            let deciseconds = (div_ceil_u128(nanos, NANOS_PER_DECI) % 10).try_into().unwrap();
            TimeBreakdown::LowTime{ seconds, deciseconds }
        } else {
            let minutes = (s / 60).try_into().unwrap();
            let seconds = (s % 60).try_into().unwrap();
            TimeBreakdown::NormalTime{ minutes, seconds }
        };
        ClockShowing{ is_active, show_separator, out_of_time, time_breakdown }
    }

    pub fn total_time_elapsed(&self) -> Duration {
        // Note. This assumes no time increments, delays, etc.
        Force::iter().map(|force| self.control.starting_time - self.remaining_time[force]).sum()
    }

    pub fn new_turn(&mut self, new_force: Force, now: GameInstant) {
        if let Some((prev_force, _)) = self.turn_state {
            assert_ne!(prev_force, new_force);
            let remaining = self.time_left(prev_force, now);
            self.remaining_time[prev_force] = remaining;
            match now.measurement {
                TimeMeasurement::Exact => {
                    // On the server or in offline game this should always hold true:
                    // otherwise game should've already finished by flag.
                    assert!(remaining > Duration::ZERO);
                }
                TimeMeasurement::Approximate => {}
            }
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
