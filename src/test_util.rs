// Test utilities that cannot be moved to the "tests" folder, because stress_test uses them.

use enum_map::{enum_map, EnumMap};
use instant::Duration;
use itertools::Itertools;
use rand::{Rng, SeedableRng};

use crate::board::{TurnError, TurnInput, TurnMode};
use crate::clock::GameInstant;
use crate::force::Force;
use crate::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughousePlayer, ChessGame, PlayerInGame,
};
use crate::once_cell_regex;


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

// Improvement potential: Allow whitespace after turn number.
pub fn replay_chess_log(
    game: &mut ChessGame, log: &str, time_per_turn: Duration,
) -> Result<(), TurnError> {
    let turn_number_re = once_cell_regex!(r"^(?:[0-9]+\.)?(.*)$");
    let mut t = Duration::ZERO;
    for turn_notation in log.split_whitespace() {
        let turn_notation =
            turn_number_re.captures(turn_notation).unwrap().get(1).unwrap().as_str();
        let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
        game.try_turn(&turn_input, TurnMode::Normal, GameInstant::from_duration(t))?;
        t += time_per_turn;
    }
    Ok(())
}

pub fn replay_bughouse_log(
    game: &mut BughouseGame, log: &str, time_per_turn: Duration,
) -> Result<(), TurnError> {
    let turn_number_re = once_cell_regex!(r"^(?:[0-9]+([AaBb])\.)?(.*)$");
    let mut t = Duration::ZERO;
    let mut words = log.split_whitespace().rev().collect_vec();
    while let Some(word) = words.pop() {
        use BughouseBoard::*;
        use Force::*;
        let caps = turn_number_re.captures(word).unwrap();
        let player_notation = caps.get(1).unwrap().as_str();
        let mut turn_notation = caps.get(2).unwrap().as_str();
        if turn_notation.is_empty() {
            // There was a whitespace after turn number.
            turn_notation = words.pop().unwrap();
        }
        let (board_idx, force) = match player_notation {
            "A" => (A, White),
            "a" => (A, Black),
            "B" => (B, White),
            "b" => (B, Black),
            _ => panic!("Unexpected bughouse player notation: {}", player_notation),
        };
        assert_eq!(game.board(board_idx).active_force(), force);
        let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
        game.try_turn(board_idx, &turn_input, TurnMode::Normal, GameInstant::from_duration(t))?;
        t += time_per_turn;
    }
    Ok(())
}
