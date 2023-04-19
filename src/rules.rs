use std::time::Duration;

use chain_cmp::chmp;
use indoc::formatdoc;
use serde::{Deserialize, Serialize};

use crate::clock::TimeControl;
use crate::coord::SubjectiveRow;
use crate::player::{Faction, Team};


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
const INDIVIDUAL_MODE_FACTIONS: [Faction; 2] = [Faction::Random, Faction::Observer];


#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum StartingPosition {
    Classic,
    FischerRandom, // a.k.a. Chess960
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ChessVariant {
    Standard,

    // Can only see squares that are legal move destinations for your pieces.
    FogOfWar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FairyPieces {
    NoFairy,

    // A duck occupies one square on the board and cannot be captured. Each turn consists of
    // two parts. First, a regular bughouse move. Second, moving the duck to any free square
    // on the board.
    //
    // We currently record duck relocation as a separate turn. An alternative would be to
    // pack both parts in a single turns. There are different trade-offs associated with each
    // option, so this decision could be revised.
    // Pros of a single combined turn:
    //   - Can undo subturns until the superturn if finished;
    //   - Can read/parse tense algebraic notation where duck relocation is appended to the turn;
    //   - Easier to make sure that a single full local turn/preturn is allowed.
    // Pros of multiple consecutive turn:
    //   - The inability to undo subturns can be viewed as a feature;
    //   - Simple Turn/TurnInput structure;
    //   - Can see opponent's subturns immediately;
    //   - Less risk of history horizontal overflow.
    DuckChess,

    // Can "glue" a Knight to a Bishop, a Rook or a Queen by moving one piece onto another
    // or dropping one piece onto another. Could move a Knight onto a piece, could move a
    // piece onto a Knight - both are fine. When captured the piece falls back apart. Other
    // than that, there is no way to unglue the piece.
    // Improvement potential: Add a special sign in the notation when pieces are glued,
    //   similarly to "x" for capture.
    // TODO: Add tests.
    Accolade,
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
pub struct MatchRules {
    pub rated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChessRules {
    pub starting_position: StartingPosition,
    pub chess_variant: ChessVariant,
    pub fairy_pieces: FairyPieces,
    pub time_control: TimeControl,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BughouseRules {
    // Improvement potential. Should `teaming` reside in `BughouseRules` or be moved to
    //   a separate struct (e.g. `MatchRules`)?
    pub teaming: Teaming,
    pub min_pawn_drop_rank: SubjectiveRow,
    pub max_pawn_drop_rank: SubjectiveRow,
    pub drop_aggression: DropAggression,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rules {
    pub match_rules: MatchRules,
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

impl MatchRules {
    pub fn unrated() -> Self { Self { rated: false } }
}

impl ChessRules {
    pub fn classic_blitz() -> Self {
        Self {
            starting_position: StartingPosition::Classic,
            chess_variant: ChessVariant::Standard,
            fairy_pieces: FairyPieces::NoFairy,
            time_control: TimeControl { starting_time: Duration::from_secs(300) },
        }
    }

    // If true, use normal chess rules: players are not allowed to leave the king undefended,
    // the king cannot pass through a square attacked by an enemy piece when castling, the game
    // end with a mate.
    // If false, there are no checks and mates. The game ends when the king is captured.
    pub fn enable_check_and_mate(&self) -> bool {
        match (self.chess_variant, self.fairy_pieces) {
            (ChessVariant::Standard, FairyPieces::NoFairy | FairyPieces::Accolade) => true,
            (ChessVariant::FogOfWar, _) | (_, FairyPieces::DuckChess) => false,
        }
    }

    // Conceptually we always allow a single preturn, but this may technically require several
    // preturns in game modes where each turn has multiple stages.
    pub fn max_preturns_per_board(&self) -> usize {
        match self.fairy_pieces {
            FairyPieces::DuckChess => 2,
            _ => 1,
        }
    }
}

impl BughouseRules {
    pub fn chess_com() -> Self {
        Self {
            teaming: Teaming::FixedTeams,
            min_pawn_drop_rank: SubjectiveRow::from_one_based(2).unwrap(),
            max_pawn_drop_rank: SubjectiveRow::from_one_based(7).unwrap(),
            drop_aggression: DropAggression::MateAllowed,
        }
    }
}

impl BughouseRules {
    pub fn drop_aggression_string(&self) -> &'static str {
        match self.drop_aggression {
            DropAggression::NoCheck => "No check",
            DropAggression::NoChessMate => "No chess mate",
            DropAggression::NoBughouseMate => "No bughouse mate",
            DropAggression::MateAllowed => "Mate allowed",
        }
    }

    pub fn pawn_drop_ranks_string(&self) -> String {
        format!(
            "{}-{}",
            self.min_pawn_drop_rank.to_one_based(),
            self.max_pawn_drop_rank.to_one_based()
        )
    }
}

impl Rules {
    pub fn verify(&self) -> Result<(), String> {
        let min_pawn_drop_rank = self.bughouse_rules.min_pawn_drop_rank.to_one_based();
        let max_pawn_drop_rank = self.bughouse_rules.max_pawn_drop_rank.to_one_based();
        if !chmp!(1 <= min_pawn_drop_rank <= max_pawn_drop_rank <= 7) {
            return Err(format!(
                "Invalid pawn drop ranks: {min_pawn_drop_rank}-{max_pawn_drop_rank}"
            ));
        }
        if self.chess_rules.chess_variant == ChessVariant::FogOfWar
            && self.bughouse_rules.drop_aggression != DropAggression::MateAllowed
        {
            return Err("Fog-of-war chess is played until a king is captured. \
                Drop aggression must be set to \"mate allowed\""
                .to_owned());
        }
        if self.chess_rules.fairy_pieces == FairyPieces::DuckChess
            && self.bughouse_rules.drop_aggression != DropAggression::MateAllowed
        {
            return Err("Duck chess is played until a king is captured. \
                Drop aggression must be set to \"mate allowed\""
                .to_owned());
        }
        assert!(
            self.chess_rules.enable_check_and_mate()
                || self.bughouse_rules.drop_aggression == DropAggression::MateAllowed
        );
        Ok(())
    }

    // Try to keep in sync with "New match" dialog.
    pub fn to_human_readable(&self) -> String {
        let teaming = match self.bughouse_rules.teaming {
            Teaming::FixedTeams => "Fixed Teams",
            Teaming::IndividualMode => "Individual mode",
        };
        let starting_position = match self.chess_rules.starting_position {
            StartingPosition::Classic => "Classic",
            StartingPosition::FischerRandom => "Fischer random",
        };
        let chess_variant = match self.chess_rules.chess_variant {
            ChessVariant::Standard => "Standard",
            ChessVariant::FogOfWar => "Fog of war",
        };
        let fairy_pieces = match self.chess_rules.fairy_pieces {
            FairyPieces::NoFairy => "None",
            FairyPieces::DuckChess => "Duck chess",
            FairyPieces::Accolade => "Accolade",
        };
        let time_control = self.chess_rules.time_control.to_string();
        let drop_aggression = self.bughouse_rules.drop_aggression_string();
        let pawn_drop_ranks = self.bughouse_rules.pawn_drop_ranks_string();
        let rating = match self.match_rules.rated {
            true => "Rated",
            false => "Unrated",
        };
        formatdoc!(
            "
            Teaming: {teaming}
            Starting position: {starting_position}
            Variant: {chess_variant}
            Fairy pieces: {fairy_pieces}
            Time control: {time_control}
            Drop aggression: {drop_aggression}
            Pawn drop ranks: {pawn_drop_ranks}
            Rating: {rating}
        "
        )
    }
}
