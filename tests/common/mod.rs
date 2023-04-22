// Rust-upgrade (https://github.com/rust-lang/rust/issues/46379):
//   remove `#[allow(dead_code)]` before public functions.

use bughouse_chess::*;


#[derive(Clone, Copy, Debug)]
pub struct PieceMatcher {
    pub kind: PieceKind,
    pub force: PieceForce,
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
            force: bughouse_chess::PieceForce::$force,
            kind: bughouse_chess::PieceKind::$kind,
        }
    };
}


pub trait AutoTurnInput {
    fn to_turn_input(self) -> TurnInput;
}

impl AutoTurnInput for &str {
    fn to_turn_input(self) -> TurnInput { TurnInput::Algebraic(self.to_owned()) }
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
    ($from:ident -> $to:ident = $steal_piece_kind:ident $steal_piece_id:ident) => {
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::Move(bughouse_chess::TurnMove {
            from: bughouse_chess::Coord::$from,
            to: bughouse_chess::Coord::$to,
            promote_to: Some(PromotionTarget::Steal((
                bughouse_chess::PieceKind::$steal_piece_kind,
                $steal_piece_id,
            ))),
        }))
    };
    ($piece_kind:ident @ $to:ident) => {
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::Drop(bughouse_chess::TurnDrop {
            piece_kind: bughouse_chess::PieceKind::$piece_kind,
            to: bughouse_chess::Coord::$to,
        }))
    };
    (@ $to:ident) => {
        bughouse_chess::TurnInput::DragDrop(bughouse_chess::Turn::PlaceDuck(
            bughouse_chess::Coord::$to,
        ))
    };
}

#[allow(dead_code)]
pub fn algebraic_turn(algebraic: &str) -> TurnInput {
    bughouse_chess::TurnInput::Algebraic(algebraic.to_owned())
}


#[macro_export]
macro_rules! envoy {
    ($force:ident $board_idx:ident) => {
        bughouse_chess::BughouseEnvoy {
            board_idx: bughouse_chess::BughouseBoard::$board_idx,
            force: bughouse_chess::Force::$force,
        }
    };
}
