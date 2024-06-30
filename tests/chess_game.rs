mod common;

use std::time::Duration;

use bughouse_chess::board::{ChessGameStatus, TurnError, TurnMode, VictoryReason};
use bughouse_chess::clock::GameInstant;
use bughouse_chess::coord::Coord;
use bughouse_chess::force::Force;
use bughouse_chess::game::ChessGame;
use bughouse_chess::piece::PieceKind;
use bughouse_chess::role::Role;
use bughouse_chess::rules::{ChessRules, MatchRules, Rules, StartingPosition};
use bughouse_chess::starter::EffectiveStartingPosition;
use bughouse_chess::test_util::*;
use common::*;
use itertools::Itertools;


fn chess_with_rules(chess_rules: ChessRules) -> ChessGame {
    ChessGame::new(
        Rules {
            match_rules: MatchRules::unrated(),
            chess_rules,
        },
        Role::ServerOrStandalone,
        sample_chess_players(),
    )
}

fn chess_classic() -> ChessGame { chess_with_rules(ChessRules::chess_blitz_5()) }

fn chess960_from_short_fen(pieces: &str) -> ChessGame {
    let chess_rules = ChessRules {
        starting_position: StartingPosition::FischerRandom,
        ..ChessRules::chess_blitz_5()
    };
    let pieces = pieces
        .chars()
        .map(|ch| PieceKind::from_algebraic_char(ch).unwrap())
        .collect_vec();
    let starting_position = EffectiveStartingPosition::FischerRandom(pieces);
    ChessGame::new_with_starting_position(
        Rules {
            match_rules: MatchRules::unrated(),
            chess_rules,
        },
        Role::ServerOrStandalone,
        starting_position,
        sample_chess_players(),
    )
}

pub fn replay_log(game: &mut ChessGame, log: &str) -> Result<(), TurnError> {
    replay_chess_log(game, log, Duration::ZERO)
}

fn replay_log_from_start(log: &str) -> Result<(), TurnError> {
    replay_log(&mut chess_classic(), log)
}


#[test]
fn capture_notation() {
    // Capture marks + capture = ok.
    replay_log_from_start("1.Nc3 d5 2.Nxd5").unwrap();
    replay_log_from_start("1.e4 d5 2.xd5").unwrap();
    replay_log_from_start("1.e4 Nc6 2.e5 d5 3.xd6").unwrap();

    // No capture marks + capture = ok (capture mark is optional).
    replay_log_from_start("1.Nc3 d5 2.Nd5").unwrap();
    replay_log_from_start("1.e4 d5 2.d5").unwrap();
    replay_log_from_start("1.e4 Nc6 2.e5 d5 3.d6").unwrap();

    // Capture marks + no capture = fail (capture mark requires capture).
    assert_eq!(
        replay_log_from_start("1.xe3").unwrap_err(),
        TurnError::CaptureNotationRequiresCapture
    );
    assert_eq!(
        replay_log_from_start("1.Nxf3").unwrap_err(),
        TurnError::CaptureNotationRequiresCapture
    );
}

#[test]
fn wikipedia_example() {
    let mut game = chess_classic();
    replay_log(
        &mut game,
        "
        1.Nf3 Nf6 2.c4 g6 3.Nc3 Bg7 4.d4 O-O 5.Bf4 d5
        6.Qb3 dxc4 7.Qxc4 c6 8.e4 Nbd7 9.Rd1 Nb6 10.Qc5 Bg4
        11.Bg5 Na4 12.Qa3 Nxc3 13.bxc3 Nxe4 14.Bxe7 Qb6 15.Bc4 Nxc3
        16.Bc5 Rfe8+ 17.Kf1 Be6 18.Bxb6 Bxc4+ 19.Kg1 Ne2+ 20.Kf1 Nxd4+
        21.Kg1 Ne2+ 22.Kf1 Nc3+ 23.Kg1 axb6 24.Qb4 Ra4 25.Qxb6 Nxd1
        26.h3 Rxa2 27.Kh2 Nxf2 28.Re1 Rxe1 29.Qd8+ Bf8 30.Nxe1 Bd5
        31.Nf3 Ne4 32.Qb8 b5 33.h4 h5 34.Ne5 Kg7 35.Kg1 Bc5+
        36.Kf1 Ng3+ 37.Ke1 Bb4+ 38.Kd1 Bb3+ 39.Kc1 Ne2+ 40.Kb1 Nc3+
        41.Kc1 Rc2#
    ",
    )
    .unwrap();
    assert_eq!(game.status(), ChessGameStatus::Victory(Force::Black, VictoryReason::Checkmate));
}

#[test]
fn chess960_first_move_castle() {
    let mut game = chess960_from_short_fen("RBNNBKRQ");
    replay_log(&mut game, "1.0-0").unwrap();
}

#[test]
fn chess960_drag_king_onto_rook_castle() {
    let mut game = chess960_from_short_fen("RBNNBKRQ");
    game.try_turn(&drag_move!(F1 -> G1), TurnMode::InOrder, GameInstant::game_start())
        .unwrap();
    assert!(game.board().grid()[Coord::F1].is(piece!(White Rook)));
    assert!(game.board().grid()[Coord::G1].is(piece!(White King)));
}

#[test]
fn king_capture() {
    let rules = ChessRules {
        fog_of_war: true,
        ..ChessRules::chess_blitz_5()
    };
    let mut game = chess_with_rules(rules);
    replay_log(&mut game, "1.Nc3 a6 2.Nd5 a5 3.N×c7 a4 4.N×e8").unwrap();
    assert_eq!(game.status(), ChessGameStatus::Victory(Force::White, VictoryReason::Checkmate));
}

#[test]
fn fog_of_war_en_passant() {
    let rules = ChessRules {
        fog_of_war: true,
        ..ChessRules::chess_blitz_5()
    };
    let mut game = chess_with_rules(rules);
    replay_log(&mut game, "1.e4 a6 2.e5 d5 3.×d6").unwrap();
}

#[test]
fn duck_chess_en_passant() {
    let rules = ChessRules {
        duck_chess: true,
        ..ChessRules::chess_blitz_5()
    };
    let mut game = chess_with_rules(rules);
    replay_log(&mut game, "1.e4 @h6 a6 @h3  2.e5 @h6 d5 @h3  3.×d6").unwrap();
}

#[test]
fn duck_cannot_stay_in_place() {
    let rules = ChessRules {
        duck_chess: true,
        ..ChessRules::chess_blitz_5()
    };
    let mut game = chess_with_rules(rules);
    assert_eq!(replay_log(&mut game, "1.e4 @d4 d5 @d4"), Err(TurnError::MustChangeDuckPosition));
}
