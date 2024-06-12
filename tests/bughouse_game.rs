mod common;

use std::cmp::Ordering;
use std::time::Duration;

use bughouse_chess::board::{DrawReason, TurnError, TurnInput, TurnMode, VictoryReason};
use bughouse_chess::clock::{ClockShowing, GameInstant, TimeBreakdown, TimeDifferenceBreakdown};
use bughouse_chess::coord::Coord;
use bughouse_chess::force::Force;
use bughouse_chess::game::{BughouseBoard, BughouseGame, BughouseGameStatus};
use bughouse_chess::piece::PieceKind;
use bughouse_chess::player::Team;
use bughouse_chess::role::Role;
use bughouse_chess::rules::{ChessRules, FairyPieces, MatchRules, Promotion, Rules};
use bughouse_chess::test_util::*;
use common::*;
use enum_map::EnumMap;
use rand::Rng;
use strum::IntoEnumIterator;


const T0: GameInstant = GameInstant::game_start();

pub fn alg(s: &str) -> TurnInput { algebraic_turn(s) }

fn default_rules() -> Rules {
    Rules {
        match_rules: MatchRules::unrated(),
        chess_rules: ChessRules::bughouse_rush(),
    }
}

fn default_game() -> BughouseGame {
    BughouseGame::new(default_rules(), Role::ServerOrStandalone, &sample_bughouse_players())
}

fn koedem_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().koedem = true;
    BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players())
}

fn make_turn(
    game: &mut BughouseGame, board_idx: BughouseBoard, turn_notation: &str,
) -> Result<(), TurnError> {
    let turn_input = TurnInput::Algebraic(turn_notation.to_owned());
    game.try_turn(board_idx, &turn_input, TurnMode::Normal, GameInstant::game_start())?;
    Ok(())
}

pub fn replay_log(game: &mut BughouseGame, log: &str) -> Result<(), TurnError> {
    replay_bughouse_log(game, log, Duration::ZERO)
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

// Rust-upgrade: use Duration::abs_diff (https://github.com/rust-lang/rust/pull/117619).
fn duration_abs_diff(a: Duration, b: Duration) -> Duration {
    match a.cmp(&b) {
        Ordering::Less => b - a,
        Ordering::Equal => Duration::ZERO,
        Ordering::Greater => a - b,
    }
}

fn time_breakdown_to_duration(time_breakdown: TimeBreakdown) -> Duration {
    match time_breakdown {
        TimeBreakdown::NormalTime { minutes, seconds } => {
            Duration::from_secs((minutes * 60 + seconds).into())
        }
        TimeBreakdown::LowTime { seconds, deciseconds } => {
            Duration::from_millis((seconds * 1000 + deciseconds * 100).into())
        }
        TimeBreakdown::Unknown => panic!(),
    }
}

fn time_difference_breakdown_to_duration(time_breakdown: TimeDifferenceBreakdown) -> Duration {
    match time_breakdown {
        TimeDifferenceBreakdown::Minutes { minutes, seconds } => {
            Duration::from_secs((minutes * 60 + seconds).into())
        }
        TimeDifferenceBreakdown::Seconds { seconds } => Duration::from_secs(seconds.into()),
        TimeDifferenceBreakdown::Subseconds { seconds, deciseconds } => {
            Duration::from_millis((seconds * 1000 + deciseconds * 100).into())
        }
        TimeDifferenceBreakdown::Unknown => panic!(),
    }
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
    let mut game = BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
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
    let mut game = BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
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
    let mut game = BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
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
    let mut game = parse_ascii_bughouse(rules, Role::ServerOrStandalone, game_str).unwrap();
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
    let mut game = parse_ascii_bughouse(rules, Role::ServerOrStandalone, game_str).unwrap();
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
    let mut game = parse_ascii_bughouse(rules, Role::ServerOrStandalone, game_str).unwrap();
    assert_eq!(
        game.try_turn(BughouseBoard::A, &alg("a8=Re8"), TurnMode::Normal, T0),
        Err(TurnError::ExposingPartnerKingByStealing)
    );
}

#[test]
fn combined_piece_falls_apart_on_capture() {
    let mut rules = default_rules();
    rules.chess_rules.fairy_pieces = FairyPieces::Accolade;
    let game_str = "
        . . . . k . . .     . . . K . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . b . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . N . . . . .     . . . . . . . .
        R . . . K . . .     . . . k . . . .
    ";
    let mut game = parse_ascii_bughouse(rules, Role::ServerOrStandalone, game_str).unwrap();
    game.try_turn(BughouseBoard::A, &alg("Na1"), TurnMode::Normal, T0).unwrap();
    game.try_turn(BughouseBoard::A, &alg("Bxa1"), TurnMode::Normal, T0).unwrap();
    assert_eq!(
        game.board(BughouseBoard::B).reserve(Force::White).to_map(),
        [(PieceKind::Knight, 1), (PieceKind::Rook, 1)].into_iter().collect()
    );
}

#[test]
fn steal_promotion_preserves_piece_composition() {
    let mut rules = default_rules();
    rules.chess_rules.fairy_pieces = FairyPieces::Accolade;
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    let game_str = "
        . . . . k . . .     . . . K . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . b . .
        . . N . . . . .     . . . . . . . P
        R . . . K . . .     . . . k . . . .
    ";
    let mut game = parse_ascii_bughouse(rules, Role::ServerOrStandalone, game_str).unwrap();
    game.try_turn(BughouseBoard::A, &alg("Na1"), TurnMode::Normal, T0).unwrap();
    game.try_turn(BughouseBoard::B, &alg("Pa8=Ea1"), TurnMode::Normal, T0).unwrap();
    game.try_turn(BughouseBoard::B, &alg("Bxa8"), TurnMode::Normal, T0).unwrap();
    assert_eq!(
        game.board(BughouseBoard::A).reserve(Force::White).to_map(),
        [
            (PieceKind::Pawn, 1),
            (PieceKind::Knight, 1),
            (PieceKind::Rook, 1)
        ]
        .into_iter()
        .collect()
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
    let mut game = BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
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

// TODO: More atomic chess tests:
//   - King explosions;
//   - Simultaneous king explosions;
//   - Kings cannot capture;
//   - Explosions on the border of the board;
//   - Explosions plus pawn promotions;
//   - Explosions plus combined pieces;
//   - Explosions destroy pieces in the fog of war;
//   - Explosions does not destroy the duck.
#[test]
fn atomic_explosions() {
    let mut rules = default_rules();
    rules.chess_rules.atomic_chess = true;
    let mut game =
        BughouseGame::new(rules.clone(), Role::ServerOrStandalone, &sample_bughouse_players());
    replay_log(&mut game, "1A.Nc3 1a.e5  2A.Nd5 2a.f5  3A.Nxc7").unwrap();
    let expected_game_str = "
        r . . . k b n r     R N B K Q B N R
        p p . p . . p p     P P P P P P P P
        . . . . . . . .     . . . . . . . .
        . . . . p p . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        . . . . . . . .     . . . . . . . .
        P P P P P P P P     p p p p p p p p
        R . B Q K B N R     r n b k q b n r
    ";
    let expected_game =
        parse_ascii_bughouse(rules, Role::ServerOrStandalone, expected_game_str).unwrap();
    for board in BughouseBoard::iter() {
        assert_eq!(
            game.board(board).grid().without_ids(),
            expected_game.board(board).grid().without_ids()
        );
    }
    assert_eq!(
        game.board(BughouseBoard::B).reserve(Force::White).to_map(),
        [(PieceKind::Knight, 1)].into_iter().collect()
    );
    assert_eq!(
        game.board(BughouseBoard::B).reserve(Force::Black).to_map(),
        [
            (PieceKind::Pawn, 1),
            (PieceKind::Knight, 1),
            (PieceKind::Bishop, 1),
            (PieceKind::Queen, 1)
        ]
        .into_iter()
        .collect()
    );
}

// Unfortunately we cannot mock the boards, so the test has to pay the full price of executing the
// turns. Which does serve as a limiting factor since we are only able to do about 50k turns per
// second in debug mode at the time of writing. I considered instantiating two clocks manually, but
// this reduces the quality of the test since there is some non-trivial clock-related logic in the
// game class. Notably `BughouseGame::test_flag` tries to stop clock at a very precise moment to
// avoid time overflows.
#[test]
fn clock_showings_match() {
    let turn_white_1 = drag_move!(B1 -> C3);
    let turn_white_2 = drag_move!(C3 -> B1);
    let turn_black_1 = drag_move!(B8 -> C6);
    let turn_black_2 = drag_move!(C6 -> B8);
    let force_separator = |showing: ClockShowing| -> ClockShowing {
        ClockShowing { show_separator: true, ..showing }
    };
    let mut rng = deterministic_rng();
    const NUM_ITERATIONS: usize = 100;
    for _ in 0..NUM_ITERATIONS {
        use BughouseBoard::*;
        use Force::*;
        let mut rules = default_rules();
        // Should be enough to cover all time display styles.
        rules.chess_rules.time_control.starting_time = Duration::from_secs(120);
        // Enables regicide: check/mate evaluations are irrelevant and just slow thing down.
        rules.chess_rules.fog_of_war = true;
        let mut game =
            BughouseGame::new(rules, Role::ServerOrStandalone, &sample_bughouse_players());
        let mut t = Duration::ZERO;
        loop {
            // Increase the probability of small time increments.
            let dt = if rng.gen() {
                Duration::from_millis(rng.gen_range(0..1000))
            } else {
                Duration::from_millis(rng.gen_range(1000..20_000))
            };
            t += dt;
            let game_t = GameInstant::from_duration(t);
            let board = if rng.gen() { A } else { B };
            game.test_flag(game_t);
            if !game.is_active() {
                // Improvement potential. Test that time diff matches the time diagonally from the
                // player who ran out of time.
                break;
            }

            let board_total: EnumMap<_, _> = BughouseBoard::iter()
                .map(|b| {
                    (
                        b,
                        Force::iter()
                            .map(|f| {
                                time_breakdown_to_duration(
                                    game.board(b).clock().showing_for(f, game_t).time_breakdown,
                                )
                            })
                            .sum::<Duration>(),
                    )
                })
                .collect();
            {
                let showing_a_white = game.board(A).clock().showing_for(White, game_t);
                let showing_a_black = game.board(A).clock().showing_for(Black, game_t);
                let showing_b_white = game.board(B).clock().showing_for(White, game_t);
                let showing_b_black = game.board(B).clock().showing_for(Black, game_t);
                let threshold =
                    if matches!(showing_a_white.time_breakdown, TimeBreakdown::LowTime { .. })
                        && matches!(showing_a_black.time_breakdown, TimeBreakdown::LowTime { .. })
                        && matches!(showing_b_white.time_breakdown, TimeBreakdown::LowTime { .. })
                        && matches!(showing_b_black.time_breakdown, TimeBreakdown::LowTime { .. })
                    {
                        Duration::from_millis(100)
                    } else {
                        Duration::from_secs(2)
                    };
                assert!(
                    duration_abs_diff(board_total[A], board_total[B]) <= threshold,
                    "\n{}   {}\n{}   {}\n",
                    force_separator(showing_a_black).ui_string(),
                    force_separator(showing_b_white).ui_string(),
                    force_separator(showing_a_white).ui_string(),
                    force_separator(showing_b_black).ui_string(),
                );
            }

            let clock_a = game.board(A).clock();
            let clock_b = game.board(B).clock();
            for force in Force::iter() {
                let diff = clock_a.difference_for(force, clock_b, game_t);
                let showing_a = clock_a.showing_for(force, game_t);
                let showing_b = clock_b.showing_for(force, game_t);
                let diff_duration = time_difference_breakdown_to_duration(diff.time_breakdown);
                let expected_diff_duration = duration_abs_diff(
                    time_breakdown_to_duration(showing_a.time_breakdown),
                    time_breakdown_to_duration(showing_b.time_breakdown),
                );
                let threshold =
                    if matches!(diff.time_breakdown, TimeDifferenceBreakdown::Subseconds { .. })
                        && matches!(showing_a.time_breakdown, TimeBreakdown::LowTime { .. })
                        && matches!(showing_b.time_breakdown, TimeBreakdown::LowTime { .. })
                    {
                        Duration::from_millis(100)
                    } else {
                        Duration::from_secs(1)
                    };
                assert!(
                    duration_abs_diff(diff_duration, expected_diff_duration) <= threshold,
                    "{} - {}  vs  {}",
                    force_separator(showing_a).ui_string(),
                    force_separator(showing_b).ui_string(),
                    diff.ui_string().unwrap()
                );
            }

            let (turn_1, turn_2) = match game.board(board).active_force() {
                Force::White => (&turn_white_1, &turn_white_2),
                Force::Black => (&turn_black_1, &turn_black_2),
            };
            if game.try_turn(board, turn_1, TurnMode::Normal, game_t).is_err() {
                game.try_turn(board, turn_2, TurnMode::Normal, game_t).unwrap();
            }
            game.board_mut(board).reset_threefold_repetition_draw();
        }
    }
}
