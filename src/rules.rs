use std::time::Duration;

use chain_cmp::chmp;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, IntoEnumIterator};

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

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter, AsRefStr, Serialize, Deserialize)]
pub enum RulesPreset {
    International3,
    International5,
    Modern,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum StartingPosition {
    Classic,
    FischerRandom, // a.k.a. Chess960
}

// TODO: Rename to take into account that it also defined the board shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FairyPieces {
    NoFairy,

    // A game on 10x8 board with two additional pieces per player: a Cardinal and an Empress.
    Capablanca,

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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PawnDropRanks {
    pub min: SubjectiveRow,
    pub max: SubjectiveRow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DropAggression {
    NoCheck,
    NoChessMate,
    NoBughouseMate,
    MateAllowed,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct MatchRules {
    pub rated: bool,
    // Improvement potential. Tri-state:
    //   - private,
    //   - public lobby (allow joining before the match has started),
    //   - public game (allow joining after the match has started).
    pub public: bool,
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

    // Every time the piece is captured, it explodes, destroying all pieces in a 3x3 square except
    // pawns.
    // TODO: Fix promotions rules with atomic chess. Steal promotion is weird and upgrade promotion
    // just doesn't make sense.
    pub atomic_chess: bool,

    // Can only see squares that are legal move destinations for your pieces.
    pub fog_of_war: bool,

    pub time_control: TimeControl,

    pub bughouse_rules: Option<BughouseRules>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BughouseRules {
    pub koedem: bool,
    pub duplicate: bool,
    pub promotion: Promotion,
    pub pawn_drop_ranks: PawnDropRanks, // TODO: Update when board shape changes
    pub drop_aggression: DropAggression,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Rules {
    pub match_rules: MatchRules,
    pub chess_rules: ChessRules,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, EnumIter, Serialize, Deserialize)]
pub enum ChessVariant {
    Capablanca,
    Accolade,
    FischerRandom,
    DuckChess,
    AtomicChess,
    FogOfWar,
    Koedem,
    Duplicate,
}

impl MatchRules {
    pub fn unrated_public() -> Self { Self { rated: false, public: true } }
}

// Improvement potential. Precompute `variants` and `regicide_reason`. Note that this would mean
// all `ChessRules` fields need to become private.
impl ChessRules {
    pub fn from_preset(preset: RulesPreset) -> Self {
        let international_bughouse = BughouseRules {
            koedem: false,
            duplicate: false,
            promotion: Promotion::Upgrade,
            pawn_drop_ranks: PawnDropRanks::from_one_based(2, 7),
            drop_aggression: DropAggression::MateAllowed,
        };
        match preset {
            RulesPreset::International3 => Self {
                bughouse_rules: Some(international_bughouse),
                ..Self::chess_blitz_3()
            },
            RulesPreset::International5 => Self {
                bughouse_rules: Some(international_bughouse),
                ..Self::chess_blitz_5()
            },
            RulesPreset::Modern => Self {
                starting_position: StartingPosition::FischerRandom,
                bughouse_rules: Some(BughouseRules {
                    koedem: false,
                    duplicate: false,
                    promotion: Promotion::Steal,
                    pawn_drop_ranks: PawnDropRanks::from_one_based(2, 6),
                    drop_aggression: DropAggression::NoChessMate,
                }),
                ..Self::chess_blitz_5()
            },
        }
    }

    pub fn get_preset(&self) -> Option<RulesPreset> {
        RulesPreset::iter().find(|&preset| *self == Self::from_preset(preset))
    }

    pub fn chess_blitz_3() -> Self {
        Self {
            fairy_pieces: FairyPieces::NoFairy,
            starting_position: StartingPosition::Classic,
            duck_chess: false,
            atomic_chess: false,
            fog_of_war: false,
            time_control: TimeControl { starting_time: Duration::from_secs(180) },
            bughouse_rules: None,
        }
    }

    pub fn chess_blitz_5() -> Self {
        Self {
            time_control: TimeControl { starting_time: Duration::from_secs(300) },
            ..Self::chess_blitz_3()
        }
    }

    pub fn bughouse_international3() -> Self { Self::from_preset(RulesPreset::International3) }
    pub fn bughouse_international5() -> Self { Self::from_preset(RulesPreset::International5) }
    pub fn bughouse_modern() -> Self { Self::from_preset(RulesPreset::Modern) }

    pub fn board_shape(&self) -> BoardShape {
        use FairyPieces::*;
        match self.fairy_pieces {
            NoFairy | Accolade => BoardShape::standard(),
            Capablanca => BoardShape { num_rows: 8, num_cols: 10 },
        }
    }

    pub fn promotion(&self) -> Promotion {
        self.bughouse_rules.as_ref().map_or(Promotion::Upgrade, |r| r.promotion)
    }

    pub fn regicide_reason(&self) -> Vec<ChessVariant> {
        self.variants().into_iter().filter(|v| v.enables_regicide()).collect()
    }

    // If false, use normal chess rules: players are not allowed to leave the king undefended,
    // the king cannot pass through a square attacked by an enemy piece when castling, the game
    // end with a mate.
    // If true, there are no checks and mates. The game ends when the king is captured.
    pub fn regicide(&self) -> bool { !self.regicide_reason().is_empty() }

    // Conceptually we always allow a single preturn, but this may technically require several
    // preturns in game modes where each turn has multiple stages.
    pub fn max_preturns_per_board(&self) -> usize { if self.duck_chess { 2 } else { 1 } }

    pub fn variants(&self) -> Vec<ChessVariant> {
        let mut v = vec![];
        match self.fairy_pieces {
            FairyPieces::NoFairy => {}
            FairyPieces::Capablanca => {
                v.push(ChessVariant::Capablanca);
            }
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
        if self.atomic_chess {
            v.push(ChessVariant::AtomicChess);
        }
        if self.fog_of_war {
            v.push(ChessVariant::FogOfWar);
        }
        if let Some(bughouse_rules) = &self.bughouse_rules {
            if bughouse_rules.koedem {
                v.push(ChessVariant::Koedem);
            }
            if bughouse_rules.duplicate {
                v.push(ChessVariant::Duplicate);
            }
        }
        v
    }

    pub fn verify(&self) -> Result<(), String> {
        if let Some(bughouse_rules) = &self.bughouse_rules {
            let num_ranks = self.board_shape().num_rows as i8;
            let min_pawn_drop_rank = bughouse_rules.pawn_drop_ranks.min.to_one_based();
            let max_pawn_drop_rank = bughouse_rules.pawn_drop_ranks.max.to_one_based();
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

impl Rules {
    pub fn bughouse_rules(&self) -> Option<&BughouseRules> {
        self.chess_rules.bughouse_rules.as_ref()
    }
    pub fn bughouse_rules_mut(&mut self) -> Option<&mut BughouseRules> {
        self.chess_rules.bughouse_rules.as_mut()
    }

    pub fn verify(&self) -> Result<(), String> { self.chess_rules.verify() }
}

impl Promotion {
    pub fn to_pgn(&self) -> &'static str {
        match self {
            Promotion::Upgrade => "Upgrade",
            Promotion::Discard => "Discard",
            Promotion::Steal => "Steal",
        }
    }
    pub fn from_pgn(s: &str) -> Result<Self, ()> {
        match s {
            "Upgrade" => Ok(Promotion::Upgrade),
            "Discard" => Ok(Promotion::Discard),
            "Steal" => Ok(Promotion::Steal),
            _ => Err(()),
        }
    }
    pub fn to_human_readable(&self) -> &'static str { self.to_pgn() }
}

impl DropAggression {
    // Q. Is it ok that we use spaces for PGN values?
    pub fn to_pgn(&self) -> &'static str {
        match self {
            DropAggression::NoCheck => "No check",
            DropAggression::NoChessMate => "No chess mate",
            DropAggression::NoBughouseMate => "No bughouse mate",
            DropAggression::MateAllowed => "Mate allowed",
        }
    }
    pub fn from_pgn(s: &str) -> Result<Self, ()> {
        match s {
            "No check" => Ok(DropAggression::NoCheck),
            "No chess mate" => Ok(DropAggression::NoChessMate),
            "No bughouse mate" => Ok(DropAggression::NoBughouseMate),
            "Mate allowed" => Ok(DropAggression::MateAllowed),
            _ => Err(()),
        }
    }
    pub fn to_human_readable(&self) -> &'static str { self.to_pgn() }
}

impl PawnDropRanks {
    pub fn from_one_based(min: i8, max: i8) -> Self {
        assert!(min <= max, "Bad PawnDropRanks range: {min}-{max}");
        Self {
            min: SubjectiveRow::from_one_based(min),
            max: SubjectiveRow::from_one_based(max),
        }
    }
    // The most permissive pawn drop rules possible. In particular, it allows dropping pawns on the
    // first rank, which is almost never allowed.
    pub fn widest(board_shape: BoardShape) -> Self {
        Self::from_one_based(1, board_shape.num_rows as i8 - 1)
    }
    pub fn contains(&self, row: SubjectiveRow) -> bool { self.min <= row && row <= self.max }
    pub fn to_pgn(&self) -> String {
        format!("{}-{}", self.min.to_one_based(), self.max.to_one_based())
    }
    pub fn from_pgn(s: &str) -> Result<Self, ()> {
        let (min, max) = s.split_once('-').ok_or(())?;
        Ok(Self::from_one_based(min.parse().map_err(|_| ())?, max.parse().map_err(|_| ())?))
    }
    pub fn to_human_readable(&self) -> String { self.to_pgn() }
}

impl ChessVariant {
    pub fn enables_regicide(self) -> bool {
        use ChessVariant::*;
        match self {
            Capablanca | Accolade | FischerRandom | Duplicate => false,
            DuckChess | AtomicChess | FogOfWar | Koedem => true,
        }
    }

    pub fn to_pgn(self) -> &'static str {
        match self {
            ChessVariant::Capablanca => "Capablanca",
            ChessVariant::Accolade => "Accolade",
            ChessVariant::FischerRandom => "Chess960",
            ChessVariant::DuckChess => "DuckChess",
            ChessVariant::AtomicChess => "Atomic",
            // TODO: Should it be "DarkChess" of "FogOfWar"? Similarity with "DuckChess" is
            // confusing. If renaming, don't forget to update existing PGNs!
            ChessVariant::FogOfWar => "DarkChess",
            ChessVariant::Koedem => "Koedem",
            ChessVariant::Duplicate => "Duplicate",
        }
    }

    // Parses chess variant: as written by `to_pgn` or an alternative/historical name.
    pub fn from_pgn(s: &str) -> Option<Self> {
        match s {
            "Capablanca" => Some(ChessVariant::Capablanca),
            "Accolade" => Some(ChessVariant::Accolade),
            "Chess960" => Some(ChessVariant::FischerRandom),
            "DuckChess" => Some(ChessVariant::DuckChess),
            "Atomic" => Some(ChessVariant::AtomicChess),
            "DarkChess" | "FogOfWar" => Some(ChessVariant::FogOfWar),
            "Koedem" => Some(ChessVariant::Koedem),
            "Duplicate" => Some(ChessVariant::Duplicate),
            _ => None,
        }
    }

    pub fn to_human_readable(self) -> &'static str {
        match self {
            ChessVariant::Capablanca => "Capablanca chess",
            ChessVariant::Accolade => "Accolade",
            ChessVariant::FischerRandom => "Fischer random",
            ChessVariant::DuckChess => "Duck chess",
            ChessVariant::AtomicChess => "Atomic chess",
            ChessVariant::FogOfWar => "Fog of war",
            ChessVariant::Koedem => "Koedem",
            ChessVariant::Duplicate => "Duplicate",
        }
    }
}
