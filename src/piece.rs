use derive_new::new;
use enum_map::Enum;
use serde::{Deserialize, Serialize};
use static_assertions::const_assert;
use strum::EnumIter;

use crate::force::Force;
use crate::rules::{ChessRules, FairyPieces};
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
        max_leaps: Option<u8>, // always >= 2
    },
    LikePawn,
    FreeSquare, // move to any square; cannot capture
}

#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Enum, EnumIter, Serialize, Deserialize,
)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    Cardinal, // == Bishop + Knight (a.k.a. Archbishop)
    Empress,  // == Rook + Knight
    Amazon,   // == Queen + Knight
    King,
    Duck,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum PieceForce {
    White,
    Black,
    // Piece that does belong to either side. See also: `PieceKind::is_neutral`.
    // Note that for pieces in reserve the owner isn't tracked in quite the same
    // way as for pieces on board.
    Neutral,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PieceOrigin {
    Innate,
    Promoted,
    // For FairyPieces::Accolade. Contains original non-promoted pieces.
    Combined((PieceKind, PieceKind)),
    Dropped,
}

// Piece ID:
//   - Must be unique within a board. Older IDs must not be reused.
//   - Must be computed deterministically. Client/server interaction relies on the fact that all
//     parties will independently arrive to the same piece IDs.
//     (Q. What about piece IDs generated during preturns?)
//   - Should remain the same as long as pieces stay on the board and remains the same. Piece ID
//     should change if piece kind changes (e.g. because a pawn is promoted).
//   - Should not be exposed to the user (e.g. when exporting to PGN).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PieceId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, new, Serialize, Deserialize)]
pub struct PieceOnBoard {
    pub id: PieceId,
    pub kind: PieceKind,
    pub origin: PieceOrigin,
    pub force: PieceForce,
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
    pub force: PieceForce,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PieceReservable {
    // A piece can show up in reserve during the course of a normal play. UIs that support
    // drag&drop should reserve space for such pieces in order to reduce surprise.
    Always,

    // A piece can never be in reserve. Game engine could panic if it is.
    Never,

    // A piece can be in reserve only in special cases, e.g. before the game starts or after
    // the game ends. UIs should not reserve space for such pieces. Adding such a piece to
    // reserve in the middle of the game can lead to poor user experience when the user wants
    // to drag a piece, but it changes last moment.
    InSpecialCases,
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
    ($a:literal, $b:literal) => {{
        const_assert!($a <= $b);
        PieceMovement::Leap { shift: ($a, $b) }
    }};
}

macro_rules! ride {
    ($a:literal, $b:literal) => {{
        const_assert!($a <= $b);
        PieceMovement::Ride { shift: ($a, $b), max_leaps: None }
    }};
    ($a:literal, $b:literal, max_leaps=$max_leaps:literal) => {{
        const_assert!($a <= $b);
        // Q. Is this a good solution? Or should we remove leaps and always use rides instead?
        const_assert!($max_leaps > 1); // Use `leap!` instead
        PieceMovement::Ride {
            shift: ($a, $b),
            max_leaps: Some($max_leaps),
        }
    }};
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

impl PieceId {
    pub fn new() -> Self { PieceId(0) }
    pub fn tmp() -> Self { PieceId(u32::MAX) }
    pub fn inc(&mut self) -> Self {
        let id = *self;
        self.0 += 1;
        id
    }
}

impl Default for PieceId {
    fn default() -> Self { Self::new() }
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
            PieceKind::Duck => &[PieceMovement::FreeSquare],
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
        PieceKind::Duck : 'D',
    );

    pub fn to_algebraic_for_move(self) -> String {
        if self == PieceKind::Pawn {
            String::new()
        } else {
            self.to_full_algebraic().to_string()
        }
    }

    pub fn is_neutral(self) -> bool {
        use PieceKind::*;
        match self {
            Duck => true,
            Pawn | Knight | Bishop | Rook | Queen | Cardinal | Empress | Amazon | King => false,
        }
    }

    pub fn reserve_piece_force(self, reserve_owner: Force) -> PieceForce {
        if self.is_neutral() {
            PieceForce::Neutral
        } else {
            reserve_owner.into()
        }
    }

    pub fn reservable(self, rules: &ChessRules) -> PieceReservable {
        use FairyPieces::*;
        use PieceKind::*;
        match self {
            Pawn | Knight | Bishop | Rook | Queen => PieceReservable::Always,
            Cardinal | Empress => match rules.fairy_pieces {
                NoFairy | Accolade => PieceReservable::Never,
                Capablanca => PieceReservable::Always,
            },
            Amazon => PieceReservable::Never,
            King => {
                if rules.bughouse_rules.as_ref().is_some_and(|r| r.koedem) {
                    PieceReservable::Always
                } else {
                    // Before game start (demos) and after game over (regicide).
                    PieceReservable::InSpecialCases
                }
            }
            Duck => PieceReservable::InSpecialCases, // before game start
        }
    }

    pub fn can_be_upgrade_promotion_target(self, rules: &ChessRules) -> bool {
        use FairyPieces::*;
        use PieceKind::*;
        match self {
            Pawn | Amazon | King | Duck => false,
            Cardinal | Empress => match rules.fairy_pieces {
                NoFairy | Accolade => false,
                Capablanca => true,
            },
            Knight | Bishop | Rook | Queen => true,
        }
    }

    pub fn can_be_steal_promotion_target(self) -> bool {
        use PieceKind::*;
        match self {
            Pawn | King | Duck => false,
            Knight | Bishop | Rook | Queen | Cardinal | Empress | Amazon => true,
        }
    }

    pub fn destroyed_by_atomic_explosion(self) -> bool {
        use PieceKind::*;
        match self {
            Pawn | Duck => false,
            Knight | Bishop | Rook | Queen | Cardinal | Empress | Amazon | King => true,
        }
    }

    pub fn from_algebraic(notation: &str) -> Option<Self> {
        as_single_char(notation).and_then(Self::from_algebraic_char)
    }

    pub fn from_algebraic_ignore_case(notation: &str) -> Option<Self> {
        as_single_char(notation)
            .map(|ch| ch.to_ascii_uppercase())
            .and_then(Self::from_algebraic_char)
    }
}

pub fn accolade_combine_piece_kinds(first: PieceKind, second: PieceKind) -> Option<PieceKind> {
    use PieceKind::*;
    let base_piece = match (first, second) {
        (Knight, p) | (p, Knight) => p,
        _ => {
            return None;
        }
    };
    match base_piece {
        Knight => None,                      // knight itself
        Pawn | King | Duck => None,          // not combinable
        Cardinal | Empress | Amazon => None, // already combined
        Bishop => Some(Cardinal),
        Rook => Some(Empress),
        Queen => Some(Amazon),
    }
}

pub fn accolade_combine_pieces(
    id: PieceId, first: PieceOnBoard, second: PieceOnBoard,
) -> Option<PieceOnBoard> {
    let original_piece = |p: PieceOnBoard| match p.origin {
        PieceOrigin::Innate | PieceOrigin::Dropped => Some(p.kind),
        PieceOrigin::Promoted => Some(PieceKind::Pawn),
        PieceOrigin::Combined(_) => None,
    };
    let kind = accolade_combine_piece_kinds(first.kind, second.kind)?;
    let origin =
        PieceOrigin::Combined(sort_two_desc((original_piece(first)?, original_piece(second)?)));
    if first.force != second.force {
        return None;
    }
    let force = first.force;
    Some(PieceOnBoard { id, kind, origin, force })
}

pub fn piece_to_ascii(kind: PieceKind, force: PieceForce) -> char {
    let s = kind.to_full_algebraic();
    match force {
        PieceForce::Neutral => s,
        PieceForce::White => s.to_ascii_uppercase(),
        PieceForce::Black => s.to_ascii_lowercase(),
    }
}

pub fn piece_from_ascii(ch: char) -> Option<(PieceKind, PieceForce)> {
    let kind = PieceKind::from_algebraic_char(ch.to_ascii_uppercase())?;
    if kind.is_neutral() {
        Some((kind, PieceForce::Neutral))
    } else if ch.is_ascii_uppercase() {
        Some((kind, PieceForce::White))
    } else {
        Some((kind, PieceForce::Black))
    }
}

pub fn piece_to_pictogram(piece_kind: PieceKind, force: PieceForce) -> char {
    use self::PieceForce::*;
    use self::PieceKind::*;
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
        // Normally `(Neutral, Duck)` would suffice. However a duck could be considered
        // to have an owner when it's in reserve in the beginning of the game.
        (_, Duck) => '🦆',
        _ => piece_to_ascii(piece_kind, force),
    }
}

impl PieceForce {
    pub fn is_owned_by_or_neutral(self, force: Force) -> bool {
        self == force.into() || self == PieceForce::Neutral
    }
}

impl From<Force> for PieceForce {
    fn from(force: Force) -> Self {
        match force {
            Force::White => PieceForce::White,
            Force::Black => PieceForce::Black,
        }
    }
}

impl TryFrom<PieceForce> for Force {
    type Error = ();

    fn try_from(piece_force: PieceForce) -> Result<Self, Self::Error> {
        match piece_force {
            PieceForce::White => Ok(Force::White),
            PieceForce::Black => Ok(Force::Black),
            PieceForce::Neutral => Err(()),
        }
    }
}
