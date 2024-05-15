use std::cmp::Ordering;
use std::time::Duration;
use std::{fmt, iter, ops};

use enum_map::{enum_map, EnumMap};
use instant::Instant;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

use crate::force::Force;
use crate::nanable::Nanable;


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

const MILLIS_PER_SEC: u64 = 1000;
const MILLIS_PER_DECI: u64 = MILLIS_PER_SEC / 10;

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


// In all valid cases `Exact` measurement is equivalent to `Approximate`, but `Exact` checks
// additional invariants that should hold on server and in standalone apps.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TimeMeasurement {
    Exact,
    Approximate,
}

// Class similar to `std::time::Duration`, but with milliseconds precision. This is the precision we
// use in BPGN files. By rounding all game time to milliseconds we ensure that BPGN replays are 100%
// accurate, without any weird roundnig effects.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct MillisDuration {
    ms: u64,
}

// Duration class with milliseconds precision (see `MillisDuration` for the reasoning), which also
// allows unknown values. Unknown values are used when loading BPGN files without timestamps.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct GameDuration {
    ms: Nanable<u64>,
}

// Time since game start.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct GameInstant {
    elapsed_since_start: GameDuration,
}

impl MillisDuration {
    pub const ZERO: Self = MillisDuration { ms: 0 };
    pub const MIN_POSITIVE: Self = MillisDuration { ms: 1 };

    pub fn from_millis(ms: u64) -> Self { MillisDuration { ms } }
    pub fn from_secs(s: u64) -> Self { MillisDuration::from_millis(s * 1000) }

    pub fn is_zero(self) -> bool { self.ms == 0 }
    pub fn as_millis(self) -> u64 { self.ms }
    pub fn subsec_millis(self) -> u64 { self.ms % MILLIS_PER_SEC }
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        Some(MillisDuration { ms: self.ms.checked_sub(other.ms)? })
    }
    pub fn saturating_sub(self, other: Self) -> Self {
        MillisDuration { ms: self.ms.saturating_sub(other.ms) }
    }
}

impl GameDuration {
    pub const ZERO: Self = GameDuration { ms: Nanable::Regular(0) };
    pub const MIN_POSITIVE: Self = GameDuration { ms: Nanable::Regular(1) };
    pub const UNKNOWN: Self = GameDuration { ms: Nanable::NaN };

    pub fn from_millis(ms: u64) -> Self { GameDuration { ms: Nanable::Regular(ms) } }
    pub fn from_secs(s: u64) -> Self { GameDuration::from_millis(s * 1000) }

    pub fn is_zero(self) -> bool { self.ms == Nanable::Regular(0) }
    pub fn is_unknown(self) -> bool { self.ms.is_nan() }
    pub fn as_millis(self) -> Nanable<u64> { self.ms }
    pub fn subsec_millis(self) -> Nanable<u64> { self.ms % MILLIS_PER_SEC.into() }
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        Some(GameDuration {
            ms: self.ms.combine(other.ms, |a, b| a.checked_sub(b)).transpose()?,
        })
    }
    pub fn saturating_sub(self, other: Self) -> Self {
        GameDuration {
            ms: self.ms.combine(other.ms, |a, b| a.saturating_sub(b)),
        }
    }
}

impl ops::Add for MillisDuration {
    type Output = Self;
    fn add(self, other: Self) -> Self { MillisDuration { ms: self.ms + other.ms } }
}
impl ops::Sub for MillisDuration {
    type Output = Self;
    fn sub(self, other: Self) -> Self { MillisDuration { ms: self.ms - other.ms } }
}
impl iter::Sum for MillisDuration {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(MillisDuration::ZERO, ops::Add::add)
    }
}

impl ops::Add for GameDuration {
    type Output = Self;
    fn add(self, other: Self) -> Self { GameDuration { ms: self.ms + other.ms } }
}
impl ops::Sub for GameDuration {
    type Output = Self;
    fn sub(self, other: Self) -> Self { GameDuration { ms: self.ms - other.ms } }
}
impl iter::Sum for GameDuration {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(GameDuration::ZERO, ops::Add::add)
    }
}

impl From<MillisDuration> for GameDuration {
    fn from(d: MillisDuration) -> Self { GameDuration { ms: Nanable::Regular(d.ms) } }
}
impl TryFrom<GameDuration> for MillisDuration {
    type Error = ();
    fn try_from(d: GameDuration) -> Result<Self, ()> {
        match d.ms {
            Nanable::Regular(ms) => Ok(MillisDuration { ms }),
            Nanable::NaN => Err(()),
        }
    }
}

impl From<Duration> for MillisDuration {
    fn from(d: Duration) -> Self { MillisDuration::from_millis(d.as_millis() as u64) }
}
impl From<Duration> for GameDuration {
    fn from(d: Duration) -> Self { GameDuration::from_millis(d.as_millis() as u64) }
}

impl From<MillisDuration> for Duration {
    fn from(d: MillisDuration) -> Self { Duration::from_millis(d.as_millis() as u64) }
}
impl TryFrom<GameDuration> for Duration {
    type Error = ();
    fn try_from(d: GameDuration) -> Result<Self, ()> {
        MillisDuration::try_from(d).map(|d| d.into())
    }
}

impl GameInstant {
    pub const UNKNOWN: Self = GameInstant {
        elapsed_since_start: GameDuration::UNKNOWN,
    };
    pub fn from_game_duration(elapsed_since_start: GameDuration) -> Self {
        GameInstant { elapsed_since_start }
    }
    pub fn from_millis_duration(elapsed_since_start: MillisDuration) -> Self {
        GameInstant::from_game_duration(elapsed_since_start.into())
    }
    pub fn from_duration(elapsed_since_start: Duration) -> Self {
        GameInstant::from_game_duration(elapsed_since_start.into())
    }
    pub fn from_now_game_active(game_start: Instant, now: Instant) -> Self {
        GameInstant::from_duration(now - game_start)
    }
    pub fn from_now_game_maybe_active(game_start: Option<Instant>, now: Instant) -> GameInstant {
        match game_start {
            Some(t) => GameInstant::from_now_game_active(t, now),
            None => GameInstant::game_start(),
        }
    }
    pub fn from_pair_game_active(pair: WallGameTimePair, now: Instant) -> GameInstant {
        GameInstant {
            elapsed_since_start: pair.game_t.elapsed_since_start + (now - pair.world_t).into(),
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

    pub const fn game_start() -> Self { GameInstant { elapsed_since_start: GameDuration::ZERO } }
    pub fn elapsed_since_start(self) -> GameDuration { self.elapsed_since_start }
    pub fn duration_since(
        self, earlier: GameInstant, measurement: TimeMeasurement,
    ) -> GameDuration {
        match measurement {
            TimeMeasurement::Exact => {
                self.elapsed_since_start.checked_sub(earlier.elapsed_since_start).unwrap()
            }
            TimeMeasurement::Approximate => {
                self.elapsed_since_start.saturating_sub(earlier.elapsed_since_start)
            }
        }
    }

    pub fn checked_sub(self, d: GameDuration) -> Option<Self> {
        self.elapsed_since_start
            .checked_sub(d)
            .map(|elapsed_since_start| GameInstant { elapsed_since_start })
    }

    pub fn to_pgn_timestamp(self) -> Option<String> {
        let millis = self.elapsed_since_start.as_millis().into_inner()?;
        let secs = millis / MILLIS_PER_SEC;
        let subsecs = millis % MILLIS_PER_SEC;
        Some(format!("{secs}.{subsecs:03}"))
    }
    pub fn from_pgn_timestamp(s: &str) -> Result<Self, ()> {
        let (secs, subsecs) = s.split_once('.').ok_or(())?;
        let elapsed_since_start = GameDuration::from_millis(
            secs.parse::<u64>().map_err(|_| ())? * MILLIS_PER_SEC
                + subsecs.parse::<u64>().map_err(|_| ())?,
        );
        Ok(GameInstant { elapsed_since_start })
    }
}


// We want to do something like
//   game_start = Instant::now() - time.elapsed_since_start()
// when reconnecting to existing game, but this could panic because Rust doesn't
// allow for negative durations. So this class can be used to sync game clock and
// real-world clock instead.
// TODO: Should we allow negative `GameDuration`s instead?
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
    Unknown,
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
    Unknown,
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
                format!("{:02}{}{} ", seconds, separator("."), deciseconds)
            }
            TimeBreakdown::Unknown => "-:--".to_string(),
        }
    }
}

impl ClockDifference {
    pub fn ui_string(&self) -> Option<String> {
        let sign = match self.comparison {
            Ordering::Less => '−', // U+2212 Minus Sign
            Ordering::Equal => '=',
            Ordering::Greater => '+',
        };
        match self.time_breakdown {
            TimeDifferenceBreakdown::Minutes { minutes, seconds } => {
                Some(format!("{}{}:{:02}", sign, minutes, seconds))
            }
            TimeDifferenceBreakdown::Seconds { seconds } => Some(format!("{}{:02}", sign, seconds)),
            TimeDifferenceBreakdown::Subseconds { seconds, deciseconds } => {
                Some(format!("{}{}.{}", sign, seconds, deciseconds))
            }
            TimeDifferenceBreakdown::Unknown => None,
        }
    }
}

impl From<GameDuration> for TimeBreakdown {
    fn from(time: GameDuration) -> Self {
        // Always round the time up, so that we never show "0.00" for a player who has not lost by
        // flag. Also in the beginning of the game rounding up ensures that the first tick happens
        // one second after the game starts rather than immediately, which seems nice (although not
        // as important).
        let millis = match time.as_millis() {
            Nanable::Regular(ms) => ms,
            Nanable::NaN => return TimeBreakdown::Unknown,
        };
        let ds_ceil = millis.div_ceil(MILLIS_PER_DECI);
        if ds_ceil < 200 {
            let seconds = (ds_ceil / 10).try_into().unwrap();
            let deciseconds = (ds_ceil % 10).try_into().unwrap();
            TimeBreakdown::LowTime { seconds, deciseconds }
        } else {
            let s_ceil = millis.div_ceil(MILLIS_PER_SEC);
            let minutes = (s_ceil / 60).try_into().unwrap();
            let seconds = (s_ceil % 60).try_into().unwrap();
            TimeBreakdown::NormalTime { minutes, seconds }
        }
    }
}

impl From<MillisDuration> for TimeDifferenceBreakdown {
    fn from(time: MillisDuration) -> Self {
        let millis = time.as_millis();
        // Ceil for consistency with `TimeBreakdown`.
        let ds_ceil = millis.div_ceil(MILLIS_PER_DECI);
        let s_ceil = millis.div_ceil(MILLIS_PER_SEC);
        if ds_ceil < 100 {
            let seconds = (ds_ceil / 10).try_into().unwrap();
            let deciseconds = (ds_ceil % 10).try_into().unwrap();
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


#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Clock {
    #[allow(dead_code)]
    control: TimeControl,
    measurement: TimeMeasurement,
    turn_state: Option<(Force, GameInstant)>, // force, start time
    remaining_time: EnumMap<Force, GameDuration>,
}

impl Clock {
    pub fn new(control: TimeControl, measurement: TimeMeasurement) -> Self {
        let remaining_time = enum_map! { _ => control.starting_time.into() };
        Self {
            control,
            measurement,
            turn_state: None,
            remaining_time,
        }
    }

    pub fn is_active(&self) -> bool { self.turn_state.is_some() }
    pub fn active_force(&self) -> Option<Force> { self.turn_state.map(|st| st.0) }
    pub fn turn_start(&self) -> Option<GameInstant> { self.turn_state.map(|st| st.1) }

    pub fn time_left(&self, force: Force, now: GameInstant) -> GameDuration {
        let mut ret = self.remaining_time[force];
        if let Some((current_force, current_start)) = self.turn_state {
            if force == current_force {
                ret = ret.saturating_sub(now.duration_since(current_start, self.measurement));
            }
        }
        ret
    }
    // Effectively `time_left` with the opposite sign. `Some` only when `time_left` is zero.
    pub fn time_excess(&self, force: Force, now: GameInstant) -> Option<GameDuration> {
        if let Some((current_force, current_start)) = self.turn_state {
            if force == current_force {
                return now
                    .duration_since(current_start, self.measurement)
                    .checked_sub(self.remaining_time[force]);
            }
        } else if self.remaining_time[force].is_zero() {
            return Some(GameDuration::ZERO);
        } else if self.remaining_time[force].is_unknown() {
            return Some(GameDuration::UNKNOWN);
        }
        None
    }

    pub fn showing_for(&self, force: Force, now: GameInstant) -> ClockShowing {
        let is_active = self.active_force() == Some(force);
        let mut time = self.time_left(force, now);

        // Note. Never consider an active player to be out of time. On the server or in an
        // offline client this never happens, because all clocks stop when the game is over.
        // In an online client an active player can have zero time, but we shouldn't tell the
        // user they've run out of time until the server confirms game result, because the
        // game may have ended earlier on the other board.
        let out_of_time = !is_active && time.is_zero();
        if !out_of_time && time.is_zero() {
            time = GameDuration::MIN_POSITIVE;
        }

        let time_breakdown = time.into();

        let show_separator = match (is_active, time_breakdown) {
            (false, _) => true,
            (true, TimeBreakdown::NormalTime { .. }) => time.subsec_millis().unwrap_or(0) >= 500,
            (true, TimeBreakdown::LowTime { .. }) => true,
            (true, TimeBreakdown::Unknown) => true,
        };

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
        let (Ok(my_time), Ok(other_time)) = (
            MillisDuration::try_from(self.time_left(force, now)),
            MillisDuration::try_from(other_clock.time_left(force, now)),
        ) else {
            return ClockDifference {
                comparison: Ordering::Equal,
                time_breakdown: TimeDifferenceBreakdown::Unknown,
            };
        };
        // let Ok(my_time) = GameDuration::from(self.time_left(force, now)) else
        // let Ok(other_time) = GameDuration::from(other_clock.time_left(force, now)) else
        let comparison = my_time.cmp(&other_time);
        let time_breakdown = match comparison {
            Ordering::Less => (other_time - my_time).into(),
            Ordering::Greater => (my_time - other_time).into(),
            Ordering::Equal => TimeDifferenceBreakdown::Subseconds { seconds: 0, deciseconds: 0 },
        };
        ClockDifference { comparison, time_breakdown }
    }

    pub fn total_time_elapsed(&self) -> GameDuration {
        // Note. This assumes no time increments, delays, etc.
        Force::iter()
            .map(|force| {
                GameDuration::from(self.control.starting_time) - self.remaining_time[force]
            })
            .sum()
    }

    pub fn new_turn(&mut self, new_force: Force, now: GameInstant) {
        if let Some((prev_force, _)) = self.turn_state {
            let remaining = self.time_left(prev_force, now);
            self.remaining_time[prev_force] = remaining;
            match self.measurement {
                TimeMeasurement::Exact => {
                    // On the server or in offline game this should always hold true:
                    // otherwise game should've already finished by flag.
                    assert!(!remaining.is_zero());
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


#[cfg(test)]
#[macro_export]
macro_rules! game_d {
    (?) => {
        $crate::clock::GameDuration::UNKNOWN
    };
    (0) => {
        $crate::clock::GameDuration::ZERO
    };
    ($ms:literal ms) => {
        $crate::clock::GameDuration::from_millis($ms)
    };
    ($s:literal s) => {
        $crate::clock::GameDuration::from_secs($s)
    };
    ($m:literal m) => {
        $crate::clock::GameDuration::from_secs($m * 60)
    };
}

#[cfg(test)]
#[macro_export]
macro_rules! game_t {
    ($($arg:tt)*) => {
        $crate::clock::GameInstant::from_game_duration($crate::game_d!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_breakdown() {
        use TimeBreakdown::*;
        let inf = GameDuration::from_millis(1_000_000);
        let cases = [
            (0, LowTime { seconds: 0, deciseconds: 0 }),
            (1, LowTime { seconds: 0, deciseconds: 1 }),
            (100, LowTime { seconds: 0, deciseconds: 1 }),
            (101, LowTime { seconds: 0, deciseconds: 2 }),
            (900, LowTime { seconds: 0, deciseconds: 9 }),
            (901, LowTime { seconds: 1, deciseconds: 0 }),
            (1_000, LowTime { seconds: 1, deciseconds: 0 }),
            (1_001, LowTime { seconds: 1, deciseconds: 1 }),
            (19_900, LowTime { seconds: 19, deciseconds: 9 }),
            (19_901, NormalTime { minutes: 0, seconds: 20 }),
            (20_000, NormalTime { minutes: 0, seconds: 20 }),
            (20_001, NormalTime { minutes: 0, seconds: 21 }),
            (59_000, NormalTime { minutes: 0, seconds: 59 }),
            (59_001, NormalTime { minutes: 1, seconds: 0 }),
            (60_000, NormalTime { minutes: 1, seconds: 0 }),
            (60_001, NormalTime { minutes: 1, seconds: 1 }),
            (119_000, NormalTime { minutes: 1, seconds: 59 }),
            (119_001, NormalTime { minutes: 2, seconds: 0 }),
        ];
        for (millis, breakdown) in cases {
            let time_left = GameDuration::from_millis(millis);
            assert_eq!(TimeBreakdown::from(time_left), breakdown);

            // Make sure clock showings are not altered by precision loss during PGN export.
            let timestamp = inf - time_left;
            let pgn_timestamp = GameInstant::from_pgn_timestamp(
                &GameInstant::from_game_duration(timestamp).to_pgn_timestamp().unwrap(),
            )
            .unwrap()
            .elapsed_since_start();
            let pgn_time_left = inf - pgn_timestamp;
            assert_eq!(
                TimeBreakdown::from(pgn_time_left),
                breakdown,
                "{:?} ~> {:?}",
                time_left,
                pgn_time_left
            );
        }
    }

    #[test]
    fn time_difference_breakdown() {
        use TimeDifferenceBreakdown::*;
        let cases = [
            (0, Subseconds { seconds: 0, deciseconds: 0 }),
            (1, Subseconds { seconds: 0, deciseconds: 1 }),
            (100, Subseconds { seconds: 0, deciseconds: 1 }),
            (101, Subseconds { seconds: 0, deciseconds: 2 }),
            (900, Subseconds { seconds: 0, deciseconds: 9 }),
            (901, Subseconds { seconds: 1, deciseconds: 0 }),
            (1_000, Subseconds { seconds: 1, deciseconds: 0 }),
            (1_001, Subseconds { seconds: 1, deciseconds: 1 }),
            (2_000, Subseconds { seconds: 2, deciseconds: 0 }),
            (2_001, Subseconds { seconds: 2, deciseconds: 1 }),
            (9_900, Subseconds { seconds: 9, deciseconds: 9 }),
            (9_901, Seconds { seconds: 10 }),
            (59_000, Seconds { seconds: 59 }),
            (59_001, Minutes { minutes: 1, seconds: 0 }),
            (60_000, Minutes { minutes: 1, seconds: 0 }),
            (60_001, Minutes { minutes: 1, seconds: 1 }),
            (119_000, Minutes { minutes: 1, seconds: 59 }),
            (119_001, Minutes { minutes: 2, seconds: 0 }),
        ];
        for (millis, breakdown) in cases {
            let diff = MillisDuration::from_millis(millis);
            assert_eq!(TimeDifferenceBreakdown::from(diff), breakdown);
        }
    }

    #[test]
    fn pgn_timestamp_format() {
        let cases = [
            (0, "0.000"),
            (1, "0.001"),
            (123, "0.123"),
            (12_345, "12.345"),
            // Found experimentally: a value for which parsing as f64 yields different result.
            (64_954, "64.954"),
        ];
        for (millis, pgn) in cases {
            assert_eq!(
                GameInstant::from_millis_duration(MillisDuration::from_millis(millis))
                    .to_pgn_timestamp()
                    .unwrap(),
                pgn
            );
            assert_eq!(
                GameInstant::from_pgn_timestamp(pgn)
                    .unwrap()
                    .elapsed_since_start
                    .as_millis()
                    .unwrap(),
                millis
            );
        }
    }
}
