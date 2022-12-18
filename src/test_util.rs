// Test utilities that cannot be moved to the "tests" folder, because stress_test uses them.

use enum_map::{EnumMap, enum_map};

use crate::force::Force;
use crate::game::{BughouseBoard, BughousePlayerId, PlayerInGame};


pub fn sample_chess_players() -> EnumMap<Force, String> {
    enum_map! {
        Force::White => "Alice".to_owned(),
        Force::Black => "Bob".to_owned(),
    }
}

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
