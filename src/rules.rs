use std::time::Duration;

use crate::coord::SubjectiveRow;
use crate::clock::TimeControl;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartingPosition {
    Classic,
    FischerRandom,  // a.k.a. Chess960
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropAggression {
    NoCheck,
    NoChessMate,
    NoBughouseMate,
    MateAllowed,
}

#[derive(Clone, Debug)]
pub struct ChessRules {
    pub starting_position: StartingPosition,
    pub time_control: TimeControl,
}

#[derive(Clone, Debug)]
pub struct BughouseRules {
    pub min_pawn_drop_row: SubjectiveRow,
    pub max_pawn_drop_row: SubjectiveRow,
    pub drop_aggression: DropAggression,
}

impl ChessRules {
    pub fn classic_blitz() -> Self {
        Self{
            starting_position: StartingPosition::Classic,
            time_control: TimeControl{ starting_time: Duration::from_secs(300) }
        }
    }
}
