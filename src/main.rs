extern crate derive_new;
extern crate enum_map;
extern crate itertools;

pub mod chess;  // TODO: Remove `pub` (it's for unused imports warning)
mod janitor;
mod util;

use chess::*;


fn main() {
    let rules = BughouseRules{
        starting_position: StartingPosition::Classic,
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(7),
        drop_aggression: DropAggression::NoChessMate,
    };
    let _board = Board::new(rules);
}
