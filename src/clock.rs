use std::cmp::Ordering;
use std::fmt;
use std::time::Duration;

use enum_map::{enum_map, EnumMap};
use instant::Instant;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::force::Force;


#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct TimeControl {
    // Must be a whole number of seconds.
    // Improvement potential. A Duration type that statically guarantees this.
    pub starting_time: Duration,
    // Improvement potential. Support increment, delay, etc.
    //   Note that `Clock::total_time_elapsed` should be adjusted in this case.
}

impl fmt::Display for TimeControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        format_duration_to_mss(self.starting_time, f)
    }
}

pub fn duration_to_mss(d: Duration) -> String {
    let mut ret = String::new();
    format_duration_to_mss(d, &mut ret).unwrap();
    ret
}

fn format_duration_to_mss(d: Duration, f: &mut impl fmt::Write) -> fmt::Result {
    assert!(d.subsec_nanos() == 0, "{d:?}");
    let s = d.as_secs();
    let minutes = s / 60;
    let seconds = s % 60;
    write!(f, "{minutes}:{seconds:02}")
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

impl PartialOrd for GameInstant {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use TimeMeasurement::*;
        match (self.measurement, other.measurement) {
            (Exact, Exact) => Some(self.elapsed_since_start.cmp(&other.elapsed_since_start)),
            (Approximate, _) | (_, Approximate) => None,
        }
    }
}

impl GameInstant {
    pub fn from_duration(elapsed_since_start: Duration) -> Self {
        GameInstant {
            elapsed_since_start,
            measurement: TimeMeasurement::Exact,
        }
    }
    pub fn from_now_game_active(game_start: Instant, now: Instant) -> Self {
        GameInstant {
            elapsed_since_start: now - game_start,
            measurement: TimeMeasurement::Exact,
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
    // TODO: Fix: due to the `None` branch, the users have to call `.approximate()` even in places
    // where you'd expect it to be implied (from the `pair`).
    pub fn from_pair_game_maybe_active(
        pair: Option<WallGameTimePair>, now: Instant,
    ) -> GameInstant {
        match pair {
            Some(pair) => GameInstant::from_pair_game_active(pair, now),
            None => GameInstant::game_start(),
        }
    }

    pub const fn game_start() -> Self {
        GameInstant {
            elapsed_since_start: Duration::ZERO,
            measurement: TimeMeasurement::Exact,
        }
    }
    pub fn elapsed_since_start(self) -> Duration { self.elapsed_since_start }
    pub fn duration_since(self, earlier: GameInstant) -> Duration {
        use TimeMeasurement::*;
        match (self.measurement, earlier.measurement) {
            (Exact, Exact) => {
                self.elapsed_since_start.checked_sub(earlier.elapsed_since_start).unwrap()
            }
            (Approximate, _) | (_, Approximate) => {
                self.elapsed_since_start.saturating_sub(earlier.elapsed_since_start)
            }
        }
    }

    pub fn checked_sub(self, d: Duration) -> Option<Self> {
        self.elapsed_since_start.checked_sub(d).map(|elapsed_since_start| GameInstant {
            elapsed_since_start,
            measurement: self.measurement,
        })
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
        WallGameTimePair { world_t, game_t }
    }
}


#[derive(Clone, Debug)]
pub struct ClockShowing {
    pub is_active: bool,
    pub show_separator: bool,
    pub out_of_time: bool,
    pub time_breakdown: TimeBreakdown,
}

// Improvement potential: Support longer time controls (with hours).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeBreakdown {
    NormalTime { minutes: u32, seconds: u32 },
    LowTime { seconds: u32, deciseconds: u32 },
}

// Difference between player's and their diagonal opponent's clocks.
#[derive(Clone, Debug)]
pub struct ClockDifference {
    pub comparison: Ordering,
    pub time_breakdown: TimeDifferenceBreakdown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeDifferenceBreakdown {
    Minutes { minutes: u32, seconds: u32 },
    Seconds { seconds: u32 },
    Subseconds { seconds: u32, deciseconds: u32 },
}

impl ClockShowing {
    // Includes padding for TUI. HTML will ignore trailing spaces.
    pub fn ui_string(&self) -> String {
        let separator = |s| if self.show_separator { s } else { " " };
        match self.time_breakdown {
            TimeBreakdown::NormalTime { minutes, seconds } => {
                format!("{:02}{}{:02}", minutes, separator(":"), seconds)
            }
            TimeBreakdown::LowTime { seconds, deciseconds } => {
                format!(" {:02}{}{}", seconds, separator("."), deciseconds)
            }
        }
    }
}

impl ClockDifference {
    pub fn ui_string(&self) -> String {
        let sign = match self.comparison {
            Ordering::Less => '−', // U+2212 Minus Sign
            Ordering::Equal => '=',
            Ordering::Greater => '+',
        };
        match self.time_breakdown {
            TimeDifferenceBreakdown::Minutes { minutes, seconds } => {
                format!("{}{}:{:02}", sign, minutes, seconds)
            }
            TimeDifferenceBreakdown::Seconds { seconds } => {
                format!("{}{:02}", sign, seconds)
            }
            TimeDifferenceBreakdown::Subseconds { seconds, deciseconds } => {
                format!("{}{}.{}", sign, seconds, deciseconds)
            }
        }
    }
}

impl From<Duration> for TimeBreakdown {
    fn from(time: Duration) -> Self {
        // Get duration in the highest possible precision. It's important to round the time up, so
        // that we never show "0.00" for a player who has not lost by flag. Also in the beginning of
        // the game rounding up ensures that first tick happens one second after the game starts
        // rather than immediately.
        const NANOS_PER_SEC: u128 = 1_000_000_000;
        const NANOS_PER_DECI: u128 = NANOS_PER_SEC / 10;
        let nanos = time.as_nanos();
        let s_floor = nanos / NANOS_PER_SEC;
        let low_time = s_floor < 20;
        if low_time {
            let seconds = s_floor.try_into().unwrap();
            let deciseconds = (nanos.div_ceil(NANOS_PER_DECI) % 10).try_into().unwrap();
            TimeBreakdown::LowTime { seconds, deciseconds }
        } else {
            let s_ceil = nanos.div_ceil(NANOS_PER_SEC);
            let minutes = (s_ceil / 60).try_into().unwrap();
            let seconds = (s_ceil % 60).try_into().unwrap();
            TimeBreakdown::NormalTime { minutes, seconds }
        }
    }
}

impl From<Duration> for TimeDifferenceBreakdown {
    fn from(time: Duration) -> Self {
        const NANOS_PER_SEC: u128 = 1_000_000_000;
        const NANOS_PER_DECI: u128 = NANOS_PER_SEC / 10;
        let nanos = time.as_nanos();
        let s_floor = nanos / NANOS_PER_SEC;
        // Ceil whole seconds for consistency with `TimeBreakdown`.
        let s_ceil = nanos.div_ceil(NANOS_PER_SEC);
        if s_floor < 10 {
            let seconds = s_floor.try_into().unwrap();
            let deciseconds = (nanos.div_ceil(NANOS_PER_DECI) % 10).try_into().unwrap();
            TimeDifferenceBreakdown::Subseconds { seconds, deciseconds }
        } else if s_ceil < 60 {
            let seconds = s_ceil.try_into().unwrap();
            TimeDifferenceBreakdown::Seconds { seconds }
        } else {
            let minutes = (s_ceil / 60).try_into().unwrap();
            let seconds = (s_ceil % 60).try_into().unwrap();
            TimeDifferenceBreakdown::Minutes { minutes, seconds }
        }
    }
}


#[derive(Clone, Debug)]
pub struct Clock {
    turn_state: Option<(Force, GameInstant)>, // force, start time
    remaining_time: EnumMap<Force, Duration>,
    #[allow(dead_code)]
    control: TimeControl,
}

impl Clock {
    pub fn new(control: TimeControl) -> Self {
        Self {
            turn_state: None,
            remaining_time: enum_map! { _ => control.starting_time },
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
    // Effectively `time_left` with the opposite sign. `Some` only when `time_left` is zero.
    pub fn time_excess(&self, force: Force, now: GameInstant) -> Option<Duration> {
        let ret = self.remaining_time[force];
        if let Some((current_force, current_start)) = self.turn_state {
            if force == current_force {
                return now.duration_since(current_start).checked_sub(ret);
            }
        }
        Some(ret)
    }

    pub fn showing_for(&self, force: Force, now: GameInstant) -> ClockShowing {
        let is_active = self.active_force() == Some(force);
        let time = self.time_left(force, now);
        let show_separator = !is_active || time.subsec_millis() >= 500;

        // Note. Never consider an active player to be out of time. On the server or in an
        // offline client this never happens, because all clocks stop when the game is over.
        // In an online client an active player can have zero time, but we shouldn't tell the
        // user they've run out of time until the server confirms game result, because the
        // game may have ended earlier on the other board.
        let out_of_time = !is_active && time.is_zero();

        let time_breakdown = time.into();
        ClockShowing {
            is_active,
            show_separator,
            out_of_time,
            time_breakdown,
        }
    }

    // In practice `force` affects only the sign and `now` isn't actually required at all, but it's
    // easier to implement it this way.
    pub fn difference_for(
        &self, force: Force, other_clock: &Clock, now: GameInstant,
    ) -> ClockDifference {
        let my_time = self.time_left(force, now);
        let other_time = other_clock.time_left(force, now);
        let comparison = my_time.cmp(&other_time);
        let time_breakdown = match comparison {
            Ordering::Less => (other_time - my_time).into(),
            Ordering::Greater => (my_time - other_time).into(),
            Ordering::Equal => TimeDifferenceBreakdown::Subseconds { seconds: 0, deciseconds: 0 },
        };
        ClockDifference { comparison, time_breakdown }
    }

    pub fn total_time_elapsed(&self) -> Duration {
        // Note. This assumes no time increments, delays, etc.
        Force::iter()
            .map(|force| self.control.starting_time - self.remaining_time[force])
            .sum()
    }

    pub fn new_turn(&mut self, new_force: Force, now: GameInstant) {
        if let Some((prev_force, _)) = self.turn_state {
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
