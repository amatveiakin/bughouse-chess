use std::time::Duration;

use chain_cmp::chmp;
use indoc::formatdoc;
use serde::{Deserialize, Serialize};

use crate::clock::TimeControl;
use crate::coord::SubjectiveRow;
use crate::BoardShape;


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


#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum StartingPosition {
    Classic,
    FischerRandom, // a.k.a. Chess960
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FairyPieces {
    NoFairy,

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
pub enum Promotion {
    // Classic rules. When captured, promoted pieces go back as pawns.
    Upgrade,

    // You simply lose the pawn. It goes directly to diagonal opponent's reserve.
    Discard,

    // Take diagonal opponent's piece. Can only steal a piece from the board, not from reserve. The
    // pawn goes to their reserve. Cannot check player by stealing their piece.
    //   Q. Is "no introducing checks" a good limitation? Or should it be "cannot checkmate"?
    //      Alternatively, should it be "no new checks", e.g. even if the king is checked, cannot
    //      intoduce a check by another piece?
    //   Q. What about king-capture chess? Should you be able to expose the king?
    Steal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchRules {
    pub rated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChessRules {
    pub fairy_pieces: FairyPieces,

    pub starting_position: StartingPosition,

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
    pub duck_chess: bool,

    // Can only see squares that are legal move destinations for your pieces.
    pub fog_of_war: bool,

    pub time_control: TimeControl,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BughouseRules {
    pub promotion: Promotion,
    pub min_pawn_drop_rank: SubjectiveRow,
    pub max_pawn_drop_rank: SubjectiveRow, // TODO: Update it when board shape changes
    pub drop_aggression: DropAggression,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rules {
    pub match_rules: MatchRules,
    pub chess_rules: ChessRules,
    pub bughouse_rules: BughouseRules,
}

impl MatchRules {
    pub fn unrated() -> Self { Self { rated: false } }
}

impl ChessRules {
    pub fn classic_blitz() -> Self {
        Self {
            fairy_pieces: FairyPieces::NoFairy,
            starting_position: StartingPosition::Classic,
            duck_chess: false,
            fog_of_war: false,
            time_control: TimeControl { starting_time: Duration::from_secs(300) },
        }
    }

    pub fn board_shape(&self) -> BoardShape { BoardShape { num_rows: 8, num_cols: 8 } }

    // If true, use normal chess rules: players are not allowed to leave the king undefended,
    // the king cannot pass through a square attacked by an enemy piece when castling, the game
    // end with a mate.
    // If false, there are no checks and mates. The game ends when the king is captured.
    pub fn enable_check_and_mate(&self) -> bool { !self.duck_chess && !self.fog_of_war }

    // Conceptually we always allow a single preturn, but this may technically require several
    // preturns in game modes where each turn has multiple stages.
    pub fn max_preturns_per_board(&self) -> usize {
        if self.duck_chess {
            2
        } else {
            1
        }
    }

    pub fn variants(&self) -> Vec<&'static str> {
        let mut v = vec![];
        match self.starting_position {
            StartingPosition::Classic => {}
            StartingPosition::FischerRandom => {
                v.push("Chess960");
            }
        }
        match self.fairy_pieces {
            FairyPieces::NoFairy => {}
            FairyPieces::Accolade => {
                v.push("Accolade");
            }
        }
        if self.fog_of_war {
            // TODO: Should it be "DarkChess" of "FogOfWar"? Similarity with "DuckChess" is
            // confusing. If renaming, don't forget to update existing PGNs!
            v.push("DarkChess");
        }
        if self.duck_chess {
            v.push("DuckChess");
        }
        v
    }
}

impl BughouseRules {
    pub fn chess_com() -> Self {
        Self {
            promotion: Promotion::Upgrade,
            min_pawn_drop_rank: SubjectiveRow::from_one_based(2),
            max_pawn_drop_rank: SubjectiveRow::from_one_based(7),
            drop_aggression: DropAggression::MateAllowed,
        }
    }
}

impl BughouseRules {
    pub fn promotion_string(&self) -> &'static str {
        match self.promotion {
            Promotion::Upgrade => "Upgrade",
            Promotion::Discard => "Discard",
            Promotion::Steal => "Steal",
        }
    }

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
        if self.chess_rules.fog_of_war
            && self.bughouse_rules.drop_aggression != DropAggression::MateAllowed
        {
            return Err("Fog-of-war chess is played until a king is captured. \
                Drop aggression must be set to \"mate allowed\""
                .to_owned());
        }
        if self.chess_rules.duck_chess
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

    // Try to keep in sync with "New match" dialog. Not including "rated" because it is shown
    // separately in web UI.
    // TODO: Just list the variations. "Chess variant: Standard" is not useful.
    pub fn to_human_readable(&self) -> String {
        let variants = self.chess_rules.variants().join(", ");
        let time_control = self.chess_rules.time_control.to_string();
        let promotion = self.bughouse_rules.promotion_string();
        let drop_aggression = self.bughouse_rules.drop_aggression_string();
        let pawn_drop_ranks = self.bughouse_rules.pawn_drop_ranks_string();
        formatdoc!(
            "
            Variants: {variants}
            Time control: {time_control}
            Promotion: {promotion}
            Drop aggression: {drop_aggression}
            Pawn drop ranks: {pawn_drop_ranks}
        "
        )
    }
}
