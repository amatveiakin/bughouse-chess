use std::time::{Instant, Duration};

use enum_map::{EnumMap, enum_map};

use crate::force::Force;


// TODO: Disable time control in tests.
#[derive(Clone, Debug)]
pub struct TimeControl {
    pub starting_time: Duration,
    // TODO: Support increment, delay, etc.
}


#[derive(Clone, Debug)]
pub struct Clock {
    turn_state: Option<(Force, Instant)>,  // force, start time
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

    pub fn time_left(&self, force: Force, now: Instant) -> Duration {
        let mut ret = self.remaining_time[force];
        if let Some((current_force, current_start)) = self.turn_state {
            if force == current_force {
                ret = ret.saturating_sub(now.duration_since(current_start));
            }
        }
        ret
    }

    pub fn new_turn(&mut self, new_force: Force, now: Instant) {
        if let Some((prev_force, _)) = self.turn_state {
            assert_ne!(prev_force, new_force);
            let remaining = self.time_left(prev_force, now);
            assert!(remaining > Duration::ZERO);
            self.remaining_time[prev_force] = remaining;
        }
        self.turn_state = Some((new_force, now));
    }

    pub fn stop(&mut self, now: Instant) {
        if let Some((prev_force, _)) = self.turn_state {
            let remaining = self.time_left(prev_force, now);
            self.remaining_time[prev_force] = remaining;
        }
        self.turn_state = None;
    }
}
