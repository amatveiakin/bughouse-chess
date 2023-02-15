// Test utilities that cannot be moved to the "tests" folder, because stress_test uses them.

use enum_map::{EnumMap, enum_map};

use crate::force::Force;
use crate::game::{BughouseBoard, BughouseEnvoy, BughousePlayer, PlayerInGame};


pub fn sample_chess_players() -> EnumMap<Force, String> {
    enum_map! {
        Force::White => "Alice".to_owned(),
        Force::Black => "Bob".to_owned(),
    }
}

pub fn sample_bughouse_players() -> Vec<PlayerInGame> {
    use Force::*;
    use BughouseBoard::*;
    let single_player = |force, board_idx| BughousePlayer::SinglePlayer(
        BughouseEnvoy{ board_idx, force }
    );
    vec! [
        PlayerInGame {
            name: "Alice".to_owned(),
            id: single_player(White, A),
        },
        PlayerInGame {
            name: "Bob".to_owned(),
            id: single_player(Black, A),
        },
        PlayerInGame {
            name: "Charlie".to_owned(),
            id: single_player(White, B),
        },
        PlayerInGame {
            name: "Dave".to_owned(),
            id: single_player(Black, B),
        },
    ]
}
