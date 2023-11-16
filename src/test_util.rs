// Test utilities that cannot be moved to the "tests" folder, because stress_test uses them.

use enum_map::{enum_map, EnumMap};
use rand::{Rng, SeedableRng};

use crate::force::Force;
use crate::game::{BughouseBoard, BughouseEnvoy, BughousePlayer, PlayerInGame};


// In theory random tests verify statistical properties that should always hold, but let's fix
// the seed to avoid sporadic failures.
pub fn deterministic_rng() -> impl Rng { rand::rngs::StdRng::from_seed([0; 32]) }

pub fn sample_chess_players() -> EnumMap<Force, String> {
    enum_map! {
        Force::White => "Alice".to_owned(),
        Force::Black => "Bob".to_owned(),
    }
}

pub fn sample_bughouse_players() -> Vec<PlayerInGame> {
    use BughouseBoard::*;
    use Force::*;
    let single_player =
        |force, board_idx| BughousePlayer::SinglePlayer(BughouseEnvoy { board_idx, force });
    vec![
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
