mod common;

use bughouse_chess::test_util::*;
use bughouse_chess::*;
use common::*;


fn default_rules() -> Rules {
    Rules {
        match_rules: MatchRules::unrated(),
        chess_rules: ChessRules::bughouse_chess_com(),
    }
}

fn default_game() -> BughouseGame { BughouseGame::new(default_rules(), &sample_bughouse_players()) }

fn make_turn(
    game: &mut BughouseGame, board_idx: BughouseBoard, turn_notation: &str,
) -> Result<(), TurnError> {
    let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
    game.try_turn(board_idx, &turn_input, TurnMode::Normal, GameInstant::game_start())?;
    Ok(())
}

// Improvement potential: Allow whitespace after turn number.
fn replay_log(game: &mut BughouseGame, log: &str) -> Result<(), TurnError> {
    let turn_number_re = once_cell_regex!(r"^(?:[0-9]+([AaBb])\.)?(.*)$");
    let now = GameInstant::game_start();
    for turn_notation in log.split_whitespace() {
        use BughouseBoard::*;
        use Force::*;
        let captures = turn_number_re.captures(turn_notation).unwrap();
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
    let mut game = default_game();
    replay_log(
        &mut game,
        "
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
    ",
    )
    .unwrap();
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
    let mut game = default_game();
    replay_log_symmetric(
        &mut game,
        "
        e4 b6
        Qf3 Ba6
        Bxa6 e5
        b3 Ba3
        Bc4 Ne7
        Bxa3 B@g6
    ",
    )
    .unwrap();
    for _ in 0..10 {
        replay_log_symmetric(
            &mut game,
            "
            Bxf7 Bxf7
            B@c4 B@g6
        ",
        )
        .unwrap();
    }
    assert!(game.is_active());
}

#[test]
fn threefold_repetition_draw_ignores_reserve() {
    let mut game = default_game();
    replay_log(
        &mut game,
        "
        1A.Nc3  1a.Nf6
        2A.Nb1  2a.Ng8
        3A.Nc3  3a.Nf6
        1B.e4  1b.d5
        2B.xd5
        4A.Nb1  4a.Ng8
    ",
    )
    .unwrap();
    assert!(game.status() == BughouseGameStatus::Draw(DrawReason::ThreefoldRepetition));
}

#[test]
fn discard_promotion() {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Discard;
    let mut game = BughouseGame::new(rules, &sample_bughouse_players());
    replay_log(
        &mut game,
        "
        1A.a4  1a.h5
        2A.a5  2a.h4
        3A.a6  3a.h3
        4A.xb7  4a.g5
        5A.xc8=.
    ",
    )
    .unwrap();
    assert!(game.board(BughouseBoard::A).grid()[Coord::C8].is_none());
    assert_eq!(game.board(BughouseBoard::B).reserve(Force::White)[PieceKind::Pawn], 1);
}

// Test that promoted piece is not downgraded to a pawn on capture if it's promoted by stealing.
#[test]
fn steal_promotion_piece_goes_back_unchanged() {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    let mut game = BughouseGame::new(rules, &sample_bughouse_players());
    replay_log(
        &mut game,
        "
        1A.a4  1a.h5
        2A.a5  2a.h4
        3A.a6  3a.h3
        4A.xb7  4a.g5
        5A.xc8=Qd1
    ",
    )
    .unwrap();
    assert!(game.board(BughouseBoard::A).grid()[Coord::C8].is(piece!(White Queen)));
    assert_eq!(game.board(BughouseBoard::B).reserve(Force::White)[PieceKind::Pawn], 1);
    assert_eq!(game.board(BughouseBoard::B).reserve(Force::White)[PieceKind::Queen], 0);
    replay_log(&mut game, "5a.Qxc8").unwrap();
    assert_eq!(game.board(BughouseBoard::B).reserve(Force::White)[PieceKind::Pawn], 1);
    assert_eq!(game.board(BughouseBoard::B).reserve(Force::White)[PieceKind::Queen], 1);
}

#[test]
fn cannot_check_by_stealing() {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    let mut game = BughouseGame::new(rules, &sample_bughouse_players());
    assert_eq!(
        replay_log(
            &mut game,
            "
            1B.h4  1b.g5
            1B.xg5  2b.Nf6
            1B.Rxh7  2b.Nc6
            1B.Rxh8
            1A.a4  1a.h5
            2A.a5  2a.h4
            3A.a6  3a.h3
            4A.b4  4a.xg2
            5A.b5  5a.xh1=Bf8
            "
        ),
        Err(TurnError::CannotCheckByStealing)
    );
}
