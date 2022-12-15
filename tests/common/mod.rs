// Rust-upgrade (https://github.com/rust-lang/rust/issues/46379):
//   remove `#[allow(dead_code)]` before public functions.

use enum_map::{EnumMap, enum_map};

use bughouse_chess::*;


#[derive(Clone, Copy, Debug)]
pub struct PieceMatcher {
    pub kind: PieceKind,
    pub force: Force,
}

pub trait PieceIs {
    fn is(self, matcher: PieceMatcher) -> bool;
}

impl PieceIs for Option<PieceOnBoard> {
    fn is(self, matcher: PieceMatcher) -> bool {
        if let Some(piece) = self {
            piece.kind == matcher.kind && piece.force == matcher.force
        } else {
            false
        }
    }
}

#[macro_export]
macro_rules! piece {
    ($force:ident $kind:ident) => {
        common::PieceMatcher {
            force: bughouse_chess::Force::$force,
            kind: bughouse_chess::PieceKind::$kind
        }
    };
}


pub trait AutoTurnInput {
    fn to_turn_input(self) -> TurnInput;
}

impl AutoTurnInput for &str {
    fn to_turn_input(self) -> TurnInput {
        TurnInput::Algebraic(self.to_owned())
    }
}

impl AutoTurnInput for TurnInput {
    fn to_turn_input(self) -> TurnInput { self }
}

#[macro_export]
macro_rules! drag_move {
    ($from:ident -> $to:ident) => {
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::Move(bughouse_chess::TurnMove {
            from: bughouse_chess::Coord::$from,
            to: bughouse_chess::Coord::$to,
            promote_to: None,
        }))
    };
}

#[allow(dead_code)]
pub fn algebraic_turn(algebraic: &str) -> TurnInput {
    bughouse_chess::TurnInput::Algebraic(algebraic.to_owned())
}


#[macro_export]
macro_rules! seating {
    ($force:ident $board_idx:ident) => {
        bughouse_chess::BughousePlayerId {
            board_idx: bughouse_chess::BughouseBoard::$board_idx,
            force: bughouse_chess::Force::$force,
        }
    };
}

#[allow(dead_code)]
pub fn sample_chess_players() -> EnumMap<Force, String> {
    enum_map! {
        Force::White => "Alice".to_owned(),
        Force::Black => "Bob".to_owned(),
    }
}

#[allow(dead_code)]
pub fn sample_bughouse_players() -> Vec<PlayerInGame> {
    use Force::*;
    use BughouseBoard::*;
    vec! [
        PlayerInGame {
            name: "Alice".to_owned(),
            id: BughousePlayerId{ force: White, board_idx: A }
        },
        PlayerInGame {
            name: "Bob".to_owned(),
            id: BughousePlayerId{ force: Black, board_idx: A }
        },
        PlayerInGame {
            name: "Charlie".to_owned(),
            id: BughousePlayerId{ force: White, board_idx: B }
        },
        PlayerInGame {
            name: "Dave".to_owned(),
            id: BughousePlayerId{ force: Black, board_idx: B }
        },
    ]
}
