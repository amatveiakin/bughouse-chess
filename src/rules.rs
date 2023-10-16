use std::time::Duration;

use chain_cmp::chmp;
use serde::{Deserialize, Serialize};

use crate::clock::TimeControl;
use crate::coord::{BoardShape, SubjectiveRow};


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
    // pawn goes to their reserve. Limitations on king exposure:
    //   - Normally a king cannot be exposed to new attacks y stealing. Meaning that no pieces that
    //     we not attacking the king before should be able to attack the king afterwards. Even if
    //     it's already checked. Even if the total number of attacking pieces has not increased.
    //     This limitation does not depend on the drop aggression setting.
    //   - In regicide mode there are no limitation on king exposure.
    // The very same rules apply to you teammate's king.
    Steal,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MatchRules {
    pub rated: bool,
}

// Some thoughts on relationship options between `ChessRules` and `BughouseRules`. The goal is to
// keep the door open to adding non-bughouse chess in the future.
//
// # `ChessRules` and `BughouseRules` are two independent structures.
//
// This was the original approach. It neatly maps to type system: `Board` has `ChessRules` and
// `Option<BughouseRules>`, `ChessGame` has `ChessRules`, `BughouseGame` has `ChessRules` and
// `BughouseRules`. The resulting code has few `unwrap`s. The problem is, rules eventually start to
// overlap, e.g. `regicide` depends both on chess settings (like duck chess) and bughouse settings
// (like koedem). Which means, many things need to be resolved externally.
//
// # `BughouseRules` contains `ChessRules`.
//
// This is sort of classic OOP approach: bughouse is built on top of chess, so it reuses it. This
// approach also results in code with few `unwrap`s. But it solidifies the idea that bughouse and
// single-board chess exist completely in parallel, which is questionable. It means most code
// duplication, because `BughouseRules` needs to reexport all `ChessRules` concepts. As a
// consequence, we'll have `BughouseVariants` enum, which is a superset of `ChessVariants` enum, for
// example. Writing generic code that works with either chess or bughouse is going to be harder.
//
// # `ChessRules` contains `Option<BughouseRules>`.
//
// This is the current approach. Here `ChessRules` encompasses all game aspects. It means less
// strong typing than the first two options and requires a bit more `unwrap`s. But it also means
// less code duplication. `ChessRules` is a now a one-stop shop for all game settings, which can be
// relied upon by game engine and UI alike. It is also in line with how we treat other settings:
// there are no separate `FogOfWarChessRules`, for example. Maybe in the future we could just have a
// single game class, which contains a vector of boards, and differences between chess and bughouses
// are resolved similarly to how other rules differences are resolved now.
//
// # There is just `ChessRules`. It contains all setting directly, some of them optional.
//
// This is similar to the previous approach in that `ChessRules` is self-contained. It also means
// bughouse settings are easier to access and to override and feel less like a second-class citizen.
// And it allows to support variants that share only some settings with bughouse (like crazy-house).
// But it also means tonns of `unwrap`s. It allow completely non-sensical combinations
// (`min_pawn_drop_rank` is `Some` while `max_pawn_drop_rank` is `None`) and it makes settings
// harder to reason about: should we support somewhat sensical combinations that don't correspond to
// any know variant but could? Whereas grouping all bughouse options together, like we do now,
// results in a much simpler mental model.
//
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
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

    pub bughouse_rules: Option<BughouseRules>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BughouseRules {
    pub koedem: bool,
    pub promotion: Promotion,
    pub min_pawn_drop_rank: SubjectiveRow,
    pub max_pawn_drop_rank: SubjectiveRow, // TODO: Update it when board shape changes
    pub drop_aggression: DropAggression,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Rules {
    pub match_rules: MatchRules,
    pub chess_rules: ChessRules,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ChessVariant {
    Accolade,
    FischerRandom,
    DuckChess,
    FogOfWar,
    Koedem,
}

impl MatchRules {
    pub fn unrated() -> Self { Self { rated: false } }
}

// Improvement potential. Precompute `variants` and `regicide_reason`. Note that this would mean
// all `ChessRules` fields need to become private.
impl ChessRules {
    pub fn chess_blitz() -> Self {
        Self {
            fairy_pieces: FairyPieces::NoFairy,
            starting_position: StartingPosition::Classic,
            duck_chess: false,
            fog_of_war: false,
            time_control: TimeControl { starting_time: Duration::from_secs(300) },
            bughouse_rules: None,
        }
    }

    pub fn bughouse_chess_com() -> Self {
        let mut rules = Self::chess_blitz();
        let bughouse_rules = BughouseRules::chess_com(rules.board_shape());
        rules.bughouse_rules = Some(bughouse_rules);
        rules
    }

    pub fn board_shape(&self) -> BoardShape { BoardShape { num_rows: 8, num_cols: 8 } }

    // TODO: Use to improve UI tooltips.
    pub fn regicide_reason(&self) -> Vec<ChessVariant> {
        use ChessVariant::*;
        self.variants()
            .into_iter()
            .filter(|v| match v {
                FogOfWar | DuckChess | Koedem => true,
                Accolade | FischerRandom => false,
            })
            .collect()
    }

    // If false, use normal chess rules: players are not allowed to leave the king undefended,
    // the king cannot pass through a square attacked by an enemy piece when castling, the game
    // end with a mate.
    // If true, there are no checks and mates. The game ends when the king is captured.
    pub fn regicide(&self) -> bool { !self.regicide_reason().is_empty() }

    // Conceptually we always allow a single preturn, but this may technically require several
    // preturns in game modes where each turn has multiple stages.
    pub fn max_preturns_per_board(&self) -> usize {
        if self.duck_chess {
            2
        } else {
            1
        }
    }

    pub fn variants(&self) -> Vec<ChessVariant> {
        let mut v = vec![];
        match self.fairy_pieces {
            FairyPieces::NoFairy => {}
            FairyPieces::Accolade => {
                v.push(ChessVariant::Accolade);
            }
        }
        match self.starting_position {
            StartingPosition::Classic => {}
            StartingPosition::FischerRandom => {
                v.push(ChessVariant::FischerRandom);
            }
        }
        if self.duck_chess {
            v.push(ChessVariant::DuckChess);
        }
        if self.fog_of_war {
            v.push(ChessVariant::FogOfWar);
        }
        if let Some(bughouse_rules) = &self.bughouse_rules {
            if bughouse_rules.koedem {
                v.push(ChessVariant::Koedem);
            }
        }
        v
    }

    pub fn verify(&self) -> Result<(), String> {
        if let Some(bughouse_rules) = &self.bughouse_rules {
            let num_ranks = self.board_shape().num_rows as i8;
            let min_pawn_drop_rank = bughouse_rules.min_pawn_drop_rank.to_one_based();
            let max_pawn_drop_rank = bughouse_rules.max_pawn_drop_rank.to_one_based();
            if !chmp!(1 <= min_pawn_drop_rank <= max_pawn_drop_rank < num_ranks) {
                return Err(format!(
                    "Invalid pawn drop ranks: {min_pawn_drop_rank}-{max_pawn_drop_rank}"
                ));
            }
            if self.regicide() && bughouse_rules.drop_aggression != DropAggression::MateAllowed {
                return Err("The game is played until a king is captured. \
                    Drop aggression must be set to \"mate allowed\""
                    .to_owned());
            }
        }
        Ok(())
    }
}

impl BughouseRules {
    pub fn chess_com(board_shape: BoardShape) -> Self {
        Self {
            koedem: false,
            promotion: Promotion::Upgrade,
            min_pawn_drop_rank: SubjectiveRow::from_one_based(2),
            max_pawn_drop_rank: SubjectiveRow::from_one_based(board_shape.num_rows as i8 - 1),
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
    pub fn bughouse_rules(&self) -> Option<&BughouseRules> {
        self.chess_rules.bughouse_rules.as_ref()
    }
    pub fn bughouse_rules_mut(&mut self) -> Option<&mut BughouseRules> {
        self.chess_rules.bughouse_rules.as_mut()
    }

    pub fn verify(&self) -> Result<(), String> { self.chess_rules.verify() }
}

impl ChessVariant {
    pub fn to_pgn(self) -> &'static str {
        match self {
            ChessVariant::Accolade => "Accolade",
            ChessVariant::FischerRandom => "Chess960",
            ChessVariant::DuckChess => "DuckChess",
            // TODO: Should it be "DarkChess" of "FogOfWar"? Similarity with "DuckChess" is
            // confusing. If renaming, don't forget to update existing PGNs!
            ChessVariant::FogOfWar => "DarkChess",
            ChessVariant::Koedem => "Koedem",
        }
    }

    pub fn to_human_readable(self) -> &'static str {
        match self {
            ChessVariant::Accolade => "Accolade",
            ChessVariant::FischerRandom => "Fischer random",
            ChessVariant::DuckChess => "Duck chess",
            ChessVariant::FogOfWar => "Fog of war",
            ChessVariant::Koedem => "Koedem",
        }
    }
}
