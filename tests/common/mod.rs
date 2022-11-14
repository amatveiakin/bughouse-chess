use std::rc::Rc;

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

#[allow(dead_code)]  // Rust-upgrade (https://github.com/rust-lang/rust/issues/46379): remove
pub fn sample_chess_players() -> EnumMap<Force, Rc<PlayerInGame>> {
    enum_map! {
        Force::White => Rc::new(PlayerInGame{ name: "Alice".to_owned(), team: Team::Red }),
        Force::Black => Rc::new(PlayerInGame{ name: "Bob".to_owned(), team: Team::Blue }),
    }
}

#[allow(dead_code)]  // Rust-upgrade (https://github.com/rust-lang/rust/issues/46379): remove
pub fn sample_bughouse_players() -> EnumMap<BughouseBoard, EnumMap<Force, Rc<PlayerInGame>>> {
    enum_map! {
        BughouseBoard::A => enum_map! {
            Force::White => Rc::new(PlayerInGame{ name: "Alice".to_owned(), team: Team::Red }),
            Force::Black => Rc::new(PlayerInGame{ name: "Bob".to_owned(), team: Team::Blue }),
        },
        BughouseBoard::B => enum_map! {
            Force::White => Rc::new(PlayerInGame{ name: "Charlie".to_owned(), team: Team::Blue }),
            Force::Black => Rc::new(PlayerInGame{ name: "Dave".to_owned(), team: Team::Red }),
        }
    }
}
