extern crate derive_new;
extern crate enum_map;
extern crate itertools;
extern crate rand;

pub mod chess;  // TODO: Remove `pub` (it's for unused imports warning)
mod coord;
mod force;
mod grid;
mod janitor;
mod piece;
mod util;

use chess::*;
use coord::*;


fn main() {
    let chess_rules = ChessRules {
        starting_position: StartingPosition::Classic,
    };
    let bughouse_rules = BughouseRules {
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(7),
        drop_aggression: DropAggression::NoChessMate,
    };
    let mut game = ChessGame::new(chess_rules.clone());
    game.try_turn(Turn::Move(TurnMove{ from: Coord::E2, to: Coord::E4, promote_to: None })).unwrap();
    let mut game = BughouseGame::new(chess_rules, bughouse_rules);
    game.try_turn(0, Turn::Move(TurnMove{ from: Coord::E2, to: Coord::E4, promote_to: None })).unwrap();
}
