use derive_new::new;
use enum_map::Enum;
use serde::{Serialize, Deserialize};
use strum::EnumIter;

use crate::force::Force;
use crate::util::as_single_char;


#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Enum, Serialize, Deserialize)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PieceOrigin {
    Innate,
    Promoted,
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

impl PieceKind {
    // Should not be used to construct moves in algebraic notation, because it returns a
    // non-empty name for a pawn (use `to_algebraic_for_move` instead).
    pub fn to_full_algebraic(self) -> char {
        match self {
            PieceKind::Pawn => 'P',
            PieceKind::Knight => 'N',
            PieceKind::Bishop => 'B',
            PieceKind::Rook => 'R',
            PieceKind::Queen => 'Q',
            PieceKind::King => 'K',
        }
    }

    pub fn to_algebraic_for_move(self) -> &'static str {
        match self {
            PieceKind::Pawn => "",
            PieceKind::Knight => "N",
            PieceKind::Bishop => "B",
            PieceKind::Rook => "R",
            PieceKind::Queen => "Q",
            PieceKind::King => "K",
        }
    }

    pub fn from_algebraic_char(notation: char) -> Option<Self> {
        match notation {
            'P' => Some(PieceKind::Pawn),
            'N' => Some(PieceKind::Knight),
            'B' => Some(PieceKind::Bishop),
            'R' => Some(PieceKind::Rook),
            'Q' => Some(PieceKind::Queen),
            'K' => Some(PieceKind::King),
            _ => None,
        }
    }

    pub fn from_algebraic(notation: &str) -> Option<Self> {
        as_single_char(notation).and_then(Self::from_algebraic_char)
    }
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
    }
}
