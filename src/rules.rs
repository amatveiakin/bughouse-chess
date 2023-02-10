use std::time::Duration;

use indoc::formatdoc;
use serde::{Serialize, Deserialize};

use crate::coord::SubjectiveRow;
use crate::clock::TimeControl;
use crate::player::{Team, Faction};


// Time spent in the lobby before starting the first game after all players signal readiness.
//
// Why have the countdown? First, it allows everybody to absorb the finial setting and prepare
// for the game. Many games feature similar mechanics.
//
// Second, countdown solves the following problem. Imagine there are five participants, four
// of them ready. The fifth participant toggles faction from `Random` to `Observer` (maybe
// we've supported fixed teams mode with 5+ players, or maybe their are just experiment with
// the UI). Since the server doesn't wait for observer readiness, the game starts right away,
// not allowing them to switch back their faction.
//
// Another way of solving this problem would be to wait for observer readiness, but this
// approach would be misleading. It would make lobby UX inconsistent with in-game UX where
// we don't wait for observer readiness (and the latter is definitely not changing).
pub const FIRST_GAME_COUNTDOWN_DURATION: Duration = Duration::from_secs(3);

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
pub struct ContestRules {
    pub rated: bool,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rules {
    pub contest_rules: ContestRules,
    pub chess_rules: ChessRules,
    pub bughouse_rules: BughouseRules,
}

impl Teaming {
    pub fn allowed_factions(self) -> &'static [Faction] {
        match self {
            Teaming::FixedTeams => &FIXED_TEAMS_FACTIONS,
            Teaming::IndividualMode => &INDIVIDUAL_MODE_FACTIONS,
        }
    }
}

impl ContestRules {
    pub fn rated() -> Self {
        Self {
            rated: true,
        }
    }
}

impl ChessRules {
    pub fn classic_blitz() -> Self {
        Self {
            starting_position: StartingPosition::Classic,
            time_control: TimeControl{ starting_time: Duration::from_secs(300) }
        }
    }
}

impl BughouseRules {
    pub fn chess_com() -> Self {
        Self {
            teaming: Teaming::FixedTeams,
            min_pawn_drop_row: SubjectiveRow::from_one_based(2).unwrap(),
            max_pawn_drop_row: SubjectiveRow::from_one_based(7).unwrap(),
            drop_aggression: DropAggression::MateAllowed,
        }
    }
}

impl Rules {
    // Try to keep in sync with "New contest" dialog.
    pub fn to_human_readable(&self) -> String {
        let teaming = match self.bughouse_rules.teaming {
            Teaming::FixedTeams => "Fixed Teams",
            Teaming::IndividualMode => "Individual mode",
        };
        let starting_position = match self.chess_rules.starting_position {
            StartingPosition::Classic => "Classic",
            StartingPosition::FischerRandom => "Fischer random",
        };
        let time_control = self.chess_rules.time_control.to_string();
        let drop_aggression = match self.bughouse_rules.drop_aggression {
            DropAggression::NoCheck => "No check",
            DropAggression::NoChessMate => "No chess mate",
            DropAggression::NoBughouseMate => "No bughouse mate",
            DropAggression::MateAllowed => "Mate allowed",
        };
        let pawn_drop_rows = format!(
            "{}-{}",
            self.bughouse_rules.min_pawn_drop_row.to_one_based(),
            self.bughouse_rules.max_pawn_drop_row.to_one_based()
        );
        let rating = match self.contest_rules.rated {
            true => "Rated",
            false => "Unrated",
        };
        formatdoc!("
            Teaming: {teaming}
            Starting position: {starting_position}
            Time control: {time_control}
            Drop aggression: {drop_aggression}
            Pawn drop rows: {pawn_drop_rows}
            Rating: {rating}
        ")
    }
}
