use derive_new::new;
use enum_map::Enum;
use serde::{Serialize, Deserialize};
use strum::EnumIter;

use crate::force::Force;


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
    // non-empty name for a pawn (use `piece_to_algebraic_for_move` instead).
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

    // Improvement potential. Return Option, let the caller unwrap.
    pub fn from_algebraic(notation: &str) -> Self {
        match notation {
            "P" => PieceKind::Pawn,
            "N" => PieceKind::Knight,
            "B" => PieceKind::Bishop,
            "R" => PieceKind::Rook,
            "Q" => PieceKind::Queen,
            "K" => PieceKind::King,
            _ => panic!("Unknown piece: {}", notation),
        }
    }
}
