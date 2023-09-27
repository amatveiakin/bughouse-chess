mod common;

use bughouse_chess::board::{DrawReason, TurnError, TurnInput, TurnMode, VictoryReason};
use bughouse_chess::clock::GameInstant;
use bughouse_chess::coord::Coord;
use bughouse_chess::force::Force;
use bughouse_chess::game::{BughouseBoard, BughouseGame, BughouseGameStatus};
use bughouse_chess::once_cell_regex;
use bughouse_chess::piece::PieceKind;
use bughouse_chess::player::Team;
use bughouse_chess::rules::{ChessRules, MatchRules, Promotion, Rules};
use bughouse_chess::test_util::*;
use common::*;
use itertools::Itertools;


const T0: GameInstant = GameInstant::game_start();

pub fn alg(s: &str) -> TurnInput { algebraic_turn(s) }

fn default_rules() -> Rules {
    Rules {
        match_rules: MatchRules::unrated(),
        chess_rules: ChessRules::bughouse_chess_com(),
    }
}

fn default_game() -> BughouseGame { BughouseGame::new(default_rules(), &sample_bughouse_players()) }

fn koedem_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().koedem = true;
    BughouseGame::new(rules, &sample_bughouse_players())
}

fn make_turn(
    game: &mut BughouseGame, board_idx: BughouseBoard, turn_notation: &str,
) -> Result<(), TurnError> {
    let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
    game.try_turn(board_idx, &turn_input, TurnMode::Normal, GameInstant::game_start())?;
    Ok(())
}

fn replay_log(game: &mut BughouseGame, log: &str) -> Result<(), TurnError> {
    let turn_number_re = once_cell_regex!(r"^(?:[0-9]+([AaBb])\.)?(.*)$");
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
        game.try_turn(board_idx, &turn_input, TurnMode::Normal, T0)?;
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
    assert_eq!(game.status(), BughouseGameStatus::Draw(DrawReason::ThreefoldRepetition));
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
fn steal_promotion_cannot_expose_opponent_king() {
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
            ",
        ),
        Err(TurnError::ExposingKingByStealing)
    );
}

#[test]
fn steal_promotion_cannot_expose_checked_king() {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    let game_str = "
        . . . . k . . .     K . . . r . . .
        P . . . . . . .     B . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     r . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . K . . .     . . . . . . . k
    ";
    let mut game = parse_ascii_bughouse(rules, game_str).unwrap();
    assert_eq!(
        game.try_turn(BughouseBoard::A, &alg("a8=Bh2"), TurnMode::Normal, T0),
        Err(TurnError::ExposingKingByStealing)
    );
}

#[test]
fn steal_promotion_cannot_expose_partner_king() {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    let game_str = "
        . . . . k . . .     . . . . . K . .
        P . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . K . . .     . R . N . k . .
    ";
    let mut game = parse_ascii_bughouse(rules, game_str).unwrap();
    assert_eq!(
        game.try_turn(BughouseBoard::A, &alg("a8=Ne8"), TurnMode::Normal, T0),
        Err(TurnError::ExposingPartnerKingByStealing)
    );
}

// This is an extreme corner case of stealing promotion: not only we are exposing our partner rather
// than the opponent, but also the number of pieces attacking the king hasn't changes: basically
// we'are swapping one attacking rook for another. Yet the steal is illegal, since we now have a
// piece, which is able to attack the king but weren't before.
#[test]
fn steal_promotion_cannot_expose_checked_partner_king() {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    let game_str = "
        . . . . k . . .     . . . . . K . .
        P . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . K . . .     . R . R . k . .
    ";
    let mut game = parse_ascii_bughouse(rules, game_str).unwrap();
    assert_eq!(
        game.try_turn(BughouseBoard::A, &alg("a8=Re8"), TurnMode::Normal, T0),
        Err(TurnError::ExposingPartnerKingByStealing)
    );
}

#[test]
fn koedem_basic() {
    let mut game = koedem_game();
    replay_log(
        &mut game,
        "
        1B.e4  1b.e5
        1A.f4  1a.e5
        2B.Bb5  2b.d5
        2A.e4  2a.Qh4
        3A.xe5  3a.Qxe1
        3B.K@h3  3b.xe4
        4B.Bxe8
        ",
    )
    .unwrap();
    assert_eq!(game.status(), BughouseGameStatus::Victory(Team::Blue, VictoryReason::Checkmate));
}

#[test]
fn koedem_castling() {
    let mut game = koedem_game();
    replay_log(
        &mut game,
        "
        1A. e4 1a. e6 2A. d4 2a. d6 3A. Bf4 3a. Qh4 4A. Bc4 4a. Be7 5A. Qe2 5a. Bd7
        6A. Nc3 6a. Nf6 7A. Nf3 7a. Nc6 1B. c4 1b. e5 2B. Qa4 2b. Qh4 3B. f3 3b. d6
        4B. Qxe8 4b. Qxe1 8A. K@d1 8a. K@d8
        ",
    )
    .unwrap();
    // Now we have only kings and rooks on ranks 1 and 8 on board A.
    // Should always try to castle the original king, not the dropped one.
    assert_eq!(
        game.try_turn(BughouseBoard::A, &alg("0-0-0"), TurnMode::Normal, T0),
        Err(TurnError::PathBlocked)
    );
    game.try_turn(BughouseBoard::A, &alg("0-0"), TurnMode::Normal, T0).unwrap();
    assert_eq!(
        game.try_turn(BughouseBoard::A, &alg("0-0-0"), TurnMode::Normal, T0),
        Err(TurnError::PathBlocked)
    );
    game.try_turn(BughouseBoard::A, &alg("0-0"), TurnMode::Normal, T0).unwrap();
}

// Normally the only turn one can do in Koedem while having a king in reserve is to drop the king.
// But relocating a duck is an exception.
#[test]
fn koedem_two_kings_and_a_duck() {
    let mut rules = default_rules();
    rules.chess_rules.duck_chess = true;
    rules.bughouse_rules_mut().unwrap().koedem = true;
    let mut game = BughouseGame::new(rules, &sample_bughouse_players());
    replay_log(
        &mut game,
        "
        1B. e4 1B. @d6 1b. e5 1b. @e7 2B. f4 2B. @f5 2b. Qh4 2b. @f2 3B. Bb5 3B. @c6
        3b. Qxe1 1A. K@d6 1A. @d5 1a. exd6 1a. @e7 2A. f4 2A. @c6 2a. Qh4 2a. @f2
        3A. Nf3 3A. @e7 3a. Qxe1 3b. @f1
        ",
    )
    .unwrap();

    // Must place the first king.
    assert_eq!(
        game.try_turn(BughouseBoard::B, &alg("Qf3"), TurnMode::Normal, T0),
        Err(TurnError::MustDropKingIfPossible)
    );
    game.try_turn(BughouseBoard::B, &alg("K@b3"), TurnMode::Normal, T0).unwrap();
    // We still have one more king in reserve, yet now we need to place a duck.
    game.try_turn(BughouseBoard::B, &alg("@c4"), TurnMode::Normal, T0).unwrap();

    game.try_turn(BughouseBoard::B, &alg("a5"), TurnMode::Normal, T0).unwrap();
    game.try_turn(BughouseBoard::B, &alg("@a4"), TurnMode::Normal, T0).unwrap();

    // Must place the second king.
    assert_eq!(
        game.try_turn(BughouseBoard::B, &alg("Qf3"), TurnMode::Normal, T0),
        Err(TurnError::MustDropKingIfPossible)
    );
    game.try_turn(BughouseBoard::B, &alg("K@c3"), TurnMode::Normal, T0).unwrap();
    game.try_turn(BughouseBoard::B, &alg("@b4"), TurnMode::Normal, T0).unwrap();
}
