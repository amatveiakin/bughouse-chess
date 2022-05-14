use derive_new::new;
use enum_map::Enum;
use serde::{Serialize, Deserialize};

use crate::force::Force;


#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum, Serialize, Deserialize)]
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
    pub rook_castling: Option<CastleDirection>,  // whether rook can be used to castle
    pub force: Force,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum, Serialize, Deserialize)]
pub enum CastleDirection {
    ASide,
    HSide,
}

// Should not be used to construct moves in algebraic notation, because it returns a
// non-empty name for a pawn.
pub fn piece_to_full_algebraic(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Pawn => "P",
        PieceKind::Knight => "N",
        PieceKind::Bishop => "B",
        PieceKind::Rook => "R",
        PieceKind::Queen => "Q",
        PieceKind::King => "K",
    }
}

pub fn piece_to_algebraic_for_move(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Pawn => "",
        _ => piece_to_full_algebraic(kind)
    }
}

// Improvement potential. Return Option, let the caller unwrap.
pub fn piece_from_algebraic(notation: &str) -> PieceKind {
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
