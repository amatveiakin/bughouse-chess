use derive_new::new;
use enum_map::Enum;
use serde::{Serialize, Deserialize};
use static_assertions::const_assert;
use strum::EnumIter;

use crate::force::Force;
use crate::util::{as_single_char, sort_two_desc};


// Improvement potential: Support pieces with asymmetric movements.
// Improvement potential: Support hoppers (e.g. Cannon).
// Improvement potential: Support pieces that move differently depending on whether they
//   capture a piece or not.
// Improvement potential: Support multistage pieces (e.g. Gryphon/Eagle), either as a
//   hard-coded "leaper + rider" combination, or more generally supported pieces that make
//   multiple moves in sequence.
//   Note that the later solution requires special support for limiting interactions between
//   move phases. E.g. a Gryphon moves one square diagonally followed by moving like a rook
//   (1X.n+), but the rook move must be directed outwards (away from where it started).
// Improvement potential: Support Joker (mimics the last move made by the opponent).
// Improvement potential: Support Orphan (moves like any enemy piece attacking it).
// Improvement potential: Support Reflecting Bishop. Perhaps limit to one reflection.
// Improvement potential: Add piece flags in addition to movement data. Use them to encode
//   all information about pieces. Add flags like: "royal", "promotable", "promotion_target",
//   "castling primary", "castling secondary", "en passant". Success criterion: `PieceKind`
//   is treated as opaque by the rest of the code (except rendering).
//   Q. How should flags interact with rules that e.g. allow to configure promotion targets?
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceMovement {
    Leap {
        shift: (u8, u8),
    },
    Ride {
        shift: (u8, u8),
        max_leaps: Option<u8>,  // always >= 2
    },
    LikePawn,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Enum, Serialize, Deserialize)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    Cardinal,  // == Bishop + Knight (a.k.a. Archbishop)
    Empress,   // == Rook + Knight
    Amazon,    // == Queen + Knight
    King,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PieceOrigin {
    Innate,
    Promoted,
    // For FairyPieces::Accolade. Contains original non-promoted pieces.
    Combined((PieceKind, PieceKind)),
    Dropped,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Serialize, Deserialize)]
pub struct PieceOnBoard {
    pub kind: PieceKind,
    pub origin: PieceOrigin,
    pub force: Force,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, EnumIter, Serialize, Deserialize)]
pub enum CastleDirection {
    ASide,
    HSide,
}

// Improvement potential: Compress into one byte - need to store lots of these.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PieceForRepetitionDraw {
    pub kind: PieceKind,
    pub force: Force,
}


// Use macros rather then functions to construct piece movements:
//   - This enables to use `const_assert!`
//   - Custom overloading syntax is nice.
//   - Result can be returned as `&'static` without additional tricks. In contract, limited
//     promotion capabilities of `const fn` don't allow to use it here (checked in Rust 1.66),
//     see https://github.com/rust-lang/const-eval/blob/master/promotion.md
//     (Note that it's possible to work around this with a macro that constructs a `const`
//     array and returns reference to it.)

macro_rules! leap {
    ($a:literal, $b:literal) => {
        {
            const_assert!($a <= $b);
            PieceMovement::Leap{ shift: ($a, $b) }
        }
    };
}

macro_rules! ride {
    ($a:literal, $b:literal) => {
        {
            const_assert!($a <= $b);
            PieceMovement::Ride{ shift: ($a, $b), max_leaps: None }
        }
    };
    ($a:literal, $b:literal, max_leaps=$max_leaps:literal) => {
        {
            const_assert!($a <= $b);
            // Q. Is this a good solution? Or should we remove leaps and always use rides instead?
            const_assert!($max_leaps > 1);  // Use `leap!` instead
            PieceMovement::Ride{ shift: ($a, $b), max_leaps: Some($max_leaps) }
        }
    };
}

// Improvement potential: Also generate `to_algebraic_for_move` returning `&'static str` instead
//   of `String`.
macro_rules! make_algebraic_mappings {
    ($($piece:path : $ch:literal,)*) => {
        // Should not be used to construct moves in algebraic notation, because it returns a
        // non-empty name for a pawn (use `to_algebraic_for_move` instead).
        pub fn to_full_algebraic(self) -> char {
            // Don't forget to change `PIECE_RE` in `algebraic_to_turn` if relaxing this condition:
            $( const_assert!('A' <= $ch && $ch <= 'Z'); )*

            match self { $($piece => $ch,)* }
        }

        pub fn from_algebraic_char(notation: char) -> Option<Self> {
            match notation {
                $($ch => Some($piece),)*
                _ => None
            }
        }
    }
}

impl PieceKind {
    pub fn movements(self) -> &'static [PieceMovement] {
        match self {
            PieceKind::Pawn => &[PieceMovement::LikePawn],
            PieceKind::Knight => &[leap!(1, 2)],
            PieceKind::Bishop => &[ride!(1, 1)],
            PieceKind::Rook => &[ride!(0, 1)],
            PieceKind::Queen => &[ride!(1, 1), ride!(0, 1)],
            PieceKind::Cardinal => &[leap!(1, 2), ride!(1, 1)],
            PieceKind::Empress => &[leap!(1, 2), ride!(0, 1)],
            PieceKind::Amazon => &[leap!(1, 2), ride!(1, 1), ride!(0, 1)],
            PieceKind::King => &[leap!(1, 1), leap!(0, 1)],
        }
    }

    make_algebraic_mappings!(
        PieceKind::Pawn : 'P',
        PieceKind::Knight : 'N',
        PieceKind::Bishop : 'B',
        PieceKind::Rook : 'R',
        PieceKind::Queen : 'Q',
        PieceKind::Cardinal : 'C',
        PieceKind::Empress : 'E',
        PieceKind::Amazon : 'A',
        PieceKind::King : 'K',
    );

    pub fn to_algebraic_for_move(self) -> String {
        if self == PieceKind::Pawn {
            String::new()
        } else {
            self.to_full_algebraic().to_string()
        }
    }

    pub fn can_be_in_reserve(self) -> bool {
        use PieceKind::*;
        match self {
            Cardinal | Empress | Amazon | King => false,
            Pawn | Knight | Bishop | Rook | Queen => true,
        }
    }

    pub fn can_be_promotion_target(self) -> bool {
        use PieceKind::*;
        match self {
            Pawn | Cardinal | Empress | Amazon | King => false,
            Knight | Bishop | Rook | Queen => true,
        }
    }

    pub fn from_algebraic(notation: &str) -> Option<Self> {
        as_single_char(notation).and_then(Self::from_algebraic_char)
    }
}

pub fn accolade_combine_piece_kinds(first: PieceKind, second: PieceKind) -> Option<PieceKind> {
    use PieceKind::*;
    let base_piece = match (first, second) {
        (Knight, p) | (p, Knight) => p,
        _ => { return None; },
    };
    match base_piece {
        Knight => None,                       // knight itself
        Pawn | King => None,                  // not combinable
        Cardinal | Empress | Amazon => None,  // already combined
        Bishop => Some(Cardinal),
        Rook => Some(Empress),
        Queen => Some(Amazon),
    }
}

pub fn accolade_combine_pieces(first: PieceOnBoard, second: PieceOnBoard) -> Option<PieceOnBoard> {
    let original_piece = |p: PieceOnBoard| match p.origin {
        PieceOrigin::Innate | PieceOrigin::Dropped => Some(p.kind),
        PieceOrigin::Promoted => Some(PieceKind::Pawn),
        PieceOrigin::Combined(_) => None,
    };
    let kind = accolade_combine_piece_kinds(first.kind, second.kind)?;
    let origin = PieceOrigin::Combined(sort_two_desc((original_piece(first)?, original_piece(second)?)));
    if first.force != second.force {
        return None;
    }
    let force = first.force;
    return Some(PieceOnBoard{ kind, origin, force });
}

pub fn piece_to_pictogram(piece_kind: PieceKind, force: Force) -> char {
    use self::PieceKind::*;
    use self::Force::*;
    match (force, piece_kind) {
        (White, Pawn) => '♙',
        (White, Knight) => '♘',
        (White, Bishop) => '♗',
        (White, Rook) => '♖',
        (White, Queen) => '♕',
        (White, King) => '♔',
        (Black, Pawn) => '♟',
        (Black, Knight) => '♞',
        (Black, Bishop) => '♝',
        (Black, Rook) => '♜',
        (Black, Queen) => '♛',
        (Black, King) => '♚',
        (White, _) => piece_kind.to_full_algebraic().to_ascii_uppercase(),
        (Black, _) => piece_kind.to_full_algebraic().to_ascii_lowercase(),
    }
}
