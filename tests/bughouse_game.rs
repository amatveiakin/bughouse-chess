mod common;

use lazy_static::lazy_static;
use regex::Regex;

use bughouse_chess::*;
use common::*;


fn bughouse_chess_com() -> BughouseGame {
    BughouseGame::new(ChessRules::classic_blitz(), BughouseRules::chess_com(), sample_bughouse_players())
}

fn make_turn(game: &mut BughouseGame, board_idx: BughouseBoard, turn_notation: &str)
    -> Result<(), TurnError>
{
    let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
    game.try_turn(board_idx, &turn_input, TurnMode::Normal, GameInstant::game_start())?;
    Ok(())
}

// Improvement potential: Allow whitespace after turn number.
fn replay_log(game: &mut BughouseGame, log: &str) -> Result<(), TurnError> {
    lazy_static! {
        static ref TURN_NUMBER_RE: Regex = Regex::new(r"^(?:[0-9]+([AaBb])\.)?(.*)$").unwrap();
    }
    let now = GameInstant::game_start();
    for turn_notation in log.split_whitespace() {
        use BughouseBoard::*;
        use Force::*;
        let captures = TURN_NUMBER_RE.captures(turn_notation).unwrap();
        let player_notation = captures.get(1).unwrap().as_str();
        let turn_notation = captures.get(2).unwrap().as_str();
        let (board_idx, force) = match player_notation {
            "A" => (A, White),
            "a" => (A, Black),
            "B" => (B, White),
            "b" => (B, Black),
            _ => panic!("Unexpected bughouse player notation: {}", player_notation),
        };
        assert_eq!(game.board(board_idx).active_force(), force);
        let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
        game.try_turn(board_idx, &turn_input, TurnMode::Normal, now)?;
    }
    Ok(())
}

fn replay_log_symmetric(game: &mut BughouseGame, log: &str) -> Result<(), TurnError> {
    let now = GameInstant::game_start();
    for turn_notation in log.split_whitespace() {
        let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
        game.try_turn(BughouseBoard::A, &turn_input, TurnMode::Normal, now)?;
        game.try_turn(BughouseBoard::B, &turn_input, TurnMode::Normal, now)?;
    }
    Ok(())
}

#[test]
fn no_castling_with_dropped_rook() {
    let mut game = bughouse_chess_com();
    replay_log(&mut game, "
        0A.g4  0a.h5
        0A.xh5  0a.Rxh5
        0A.Nf3  0a.Rxh2
        0A.Ng5  0a.Rxh1
        0A.Ne4  0a.Rg1
        0A.Ng3  0a.Rxf1
        0A.Nxf1  0a.e5
        0B.b4  0b.a5
        0B.xa5  0b.Rxa5
        0B.Nf3  0b.Rxa2
        0B.Nc3  0b.Rxa1
        0A.R@h8  0a.d5
    ").unwrap();
    assert_eq!(
        make_turn(&mut game, BughouseBoard::A, "0-0").err().unwrap(),
        TurnError::CastlingPieceHasMoved
    );
}

// Test that after dropping a piece the threefold repetition counter starts anew. Note that
// in this particular case this is harmful, because it leads to an infinite loops involving
// both boards. However this scenario is extremely rare. Much more common are cases when a
// position was repeated only on one board, but changes in reserves actually make the
// situation different.
#[test]
fn threefold_repetition_draw_prevented_by_drops() {
    let mut game = bughouse_chess_com();
    replay_log_symmetric(&mut game, "
        e4 b6
        Qf3 Ba6
        Bxa6 e5
        b3 Ba3
        Bc4 Ne7
        Bxa3 B@g6
    ").unwrap();
    for _ in 0..10 {
        replay_log_symmetric(&mut game, "
            Bxf7 Bxf7
            B@c4 B@g6
        ").unwrap();
    }
    assert!(game.status() == BughouseGameStatus::Active);
}

#[test]
fn threefold_repetition_draw_ignores_reserve() {
    let mut game = bughouse_chess_com();
    replay_log(&mut game, "
        1A.Nc3  1a.Nf6
        2A.Nb1  2a.Ng8
        3A.Nc3  3a.Nf6
        1B.e4  1b.d5
        2B.xd5
        4A.Nb1  4a.Ng8
    ").unwrap();
    assert!(game.status() == BughouseGameStatus::Draw(DrawReason::ThreefoldRepetition));
}
