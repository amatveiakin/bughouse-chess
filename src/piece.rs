use derive_new::new;
use enum_map::Enum;

use crate::force::Force;


#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceOrigin {
    Innate,
    Promoted,
    Dropped,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, new)]
pub struct PieceOnBoard {
    pub kind: PieceKind,
    pub origin: PieceOrigin,
    pub rook_castling: Option<CastleDirection>,  // whether rook can be used to castle
    pub force: Force,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub enum CastleDirection {
    ASide,
    HSide,
}
