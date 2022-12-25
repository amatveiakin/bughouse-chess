use std::time::Duration;

use serde::{Serialize, Deserialize};

use crate::coord::SubjectiveRow;
use crate::clock::TimeControl;
use crate::player::{Team, Faction};


const FIXED_TEAMS_FACTIONS: [Faction; 4] = [
    Faction::Random,
    Faction::Fixed(Team::Red),
    Faction::Fixed(Team::Blue),
    Faction::Observer,
];
const INDIVIDUAL_MODE_FACTIONS: [Faction; 2] = [
    Faction::Random,
    Faction::Observer,
];


#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum StartingPosition {
    Classic,
    FischerRandom,  // a.k.a. Chess960
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DropAggression {
    NoCheck,
    NoChessMate,
    NoBughouseMate,
    MateAllowed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Teaming {
    FixedTeams,
    IndividualMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChessRules {
    pub starting_position: StartingPosition,
    pub time_control: TimeControl,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BughouseRules {
    // Improvement potential. Should `teaming` reside in `BughouseRules` or be moved to
    //   a separate struct (e.g. `ContestRules`)?
    pub teaming: Teaming,
    pub min_pawn_drop_row: SubjectiveRow,
    pub max_pawn_drop_row: SubjectiveRow,
    pub drop_aggression: DropAggression,
}

impl Teaming {
    pub fn allowed_factions(self) -> &'static [Faction] {
        match self {
            Teaming::FixedTeams => &FIXED_TEAMS_FACTIONS,
            Teaming::IndividualMode => &INDIVIDUAL_MODE_FACTIONS,
        }
    }
}

impl ChessRules {
    pub fn classic_blitz() -> Self {
        Self{
            starting_position: StartingPosition::Classic,
            time_control: TimeControl{ starting_time: Duration::from_secs(300) }
        }
    }
}

impl BughouseRules {
    pub fn chess_com() -> Self {
        Self{
            teaming: Teaming::FixedTeams,
            min_pawn_drop_row: SubjectiveRow::from_one_based(2),
            max_pawn_drop_row: SubjectiveRow::from_one_based(7),
            drop_aggression: DropAggression::MateAllowed,
        }
    }
}
