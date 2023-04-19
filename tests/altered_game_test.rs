mod common;
use bughouse_chess::test_util::*;
use bughouse_chess::*;
use common::*;
use pretty_assertions::assert_eq;
use BughouseBoard::{A, B};


pub fn alg(s: &str) -> TurnInput { algebraic_turn(s) }

fn as_single_player(envoy: BughouseEnvoy) -> BughouseParticipant {
    BughouseParticipant::Player(BughousePlayer::SinglePlayer(envoy))
}

fn as_double_player(team: Team) -> BughouseParticipant {
    BughouseParticipant::Player(BughousePlayer::DoublePlayer(team))
}

fn default_game() -> BughouseGame {
    BughouseGame::new(
        MatchRules::unrated(),
        ChessRules::classic_blitz(),
        BughouseRules::chess_com(),
        &sample_bughouse_players(),
    )
}

fn fog_of_war_bughouse_game() -> BughouseGame {
    BughouseGame::new(
        MatchRules::unrated(),
        ChessRules {
            chess_variant: ChessVariant::FogOfWar,
            ..ChessRules::classic_blitz()
        },
        BughouseRules::chess_com(),
        &sample_bughouse_players(),
    )
}

macro_rules! turn_highlight {
    ($board_idx:ident $coord:ident : $layer:ident $family:ident $item:ident) => {
        TurnHighlight {
            board_idx: BughouseBoard::$board_idx,
            coord: Coord::$coord,
            layer: TurnHighlightLayer::$layer,
            family: TurnHighlightFamily::$family,
            item: TurnHighlightItem::$item,
        }
    };
}

const T0: GameInstant = GameInstant::game_start();


// Regression test: shouldn't panic if there's a drag depending on a preturn that was reverted,
// because another piece blocked the target square.
#[test]
fn drag_depends_on_preturn_to_blocked_square() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(Black A)), default_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("e6"), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(E6 -> E5), T0).unwrap();
    alt_game.start_drag_piece(A, PieceDragStart::Board(Coord::E5)).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("e5"), T0).unwrap();
    assert_eq!(
        alt_game.drag_piece_drop(Coord::E4, PieceKind::Queen),
        Err(PieceDragError::DragNoLongerPossible)
    );
}

// Regression test: shouldn't panic if there's a drag depending on a preturn that was reverted,
// because preturn piece was captured.
#[test]
fn drag_depends_on_preturn_with_captured_piece() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(Black A)), default_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("d5"), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(D5 -> D4), T0).unwrap();
    alt_game.start_drag_piece(A, PieceDragStart::Board(Coord::D4)).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("xd5"), T0).unwrap();
    assert_eq!(
        alt_game.drag_piece_drop(Coord::D3, PieceKind::Queen),
        Err(PieceDragError::DragNoLongerPossible)
    );
}

// It is not allowed to have more than one preturn. However a player can start dragging a
// piece while having a preturn and finish the drag after the preturn was upgraded to a
// regular local turn (or resolved altogether).
#[test]
fn start_drag_with_a_preturn() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.try_local_turn(A, drag_move!(E2 -> E3), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(E3 -> E4), T0).unwrap();
    alt_game.start_drag_piece(A, PieceDragStart::Board(Coord::E4)).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("e3"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("Nc6"), T0).unwrap();
    let drag_result = alt_game.drag_piece_drop(Coord::E5, PieceKind::Queen).unwrap();
    assert_eq!(drag_result, drag_move!(E4 -> E5));
}

// Regression test: keep local preturn after getting an opponent's turn.
// Original implementation missed it because it expected that the server always sends our
// preturn back together with the opponent's turn. And it does. When it *has* the preturn.
// But with the preturn still in-flight, a race condition happened.
#[test]
fn pure_preturn_persistent() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(Black A)), default_game());
    alt_game.try_local_turn(A, alg("e5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(Black Pawn)));
}

#[test]
fn preturn_invalidated() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.try_local_turn(A, alg("e5"), T0).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));

    alt_game.apply_remote_turn(envoy!(Black A), &alg("e5"), T0).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(Black Pawn)));
}

#[test]
fn preturn_after_local_turn_persistent() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.try_local_turn(A, alg("e4"), T0).unwrap();
    alt_game.try_local_turn(A, alg("e5"), T0).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));

    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));

    alt_game.apply_remote_turn(envoy!(Black A), &alg("Nc6"), T0).unwrap();
    assert!(alt_game.local_game().board(A).grid()[Coord::E5].is(piece!(White Pawn)));
}

#[test]
fn two_preturns_forbidden() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.try_local_turn(A, drag_move!(E2 -> E4), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(D2 -> D4), T0).unwrap();
    assert_eq!(
        alt_game.try_local_turn(A, drag_move!(F2 -> F4), T0),
        Err(TurnError::PreturnLimitReached)
    );
}

#[test]
fn turn_highlights() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e3"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("d5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White B), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black B), &alg("d5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White B), &alg("xd5"), T0).unwrap();
    alt_game.try_local_turn(A, alg("e4"), T0).unwrap();
    alt_game.try_local_turn(A, alg("xd5"), T0).unwrap();
    let mut highlights = alt_game.turn_highlights();
    highlights.sort_by_key(|h| (h.board_idx, h.coord.row_col()));
    assert_eq!(highlights, vec![
        turn_highlight!(A E4 : BelowFog Preturn MoveFrom),
        turn_highlight!(A D5 : BelowFog Preturn MoveTo), // don't use `Capture` for preturns
        turn_highlight!(B E4 : BelowFog LatestTurn MoveFrom),
        turn_highlight!(B D5 : BelowFog LatestTurn Capture),
    ]);
}

#[test]
fn cannot_make_turns_on_other_board() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(Black A)), default_game());
    assert_eq!(alt_game.try_local_turn(B, drag_move!(E2 -> E4), T0), Err(TurnError::NotPlayer));
}

#[test]
fn double_play() {
    let mut alt_game = AlteredGame::new(as_double_player(Team::Red), default_game());
    alt_game.try_local_turn(A, drag_move!(E2 -> E4), T0).unwrap();
    alt_game.try_local_turn(B, drag_move!(D7 -> D5), T0).unwrap();
}

#[test]
fn preturn_fog_of_war() {
    let mut alt_game =
        AlteredGame::new(as_single_player(envoy!(Black A)), fog_of_war_bughouse_game());
    // Preturn piece itself should be visible, but it should not reveal other squares.
    alt_game.try_local_turn(A, drag_move!(E7 -> E5), T0).unwrap();
    assert!(!alt_game.fog_of_war_area(A).contains(&Coord::E5));
    assert!(alt_game.fog_of_war_area(A).contains(&Coord::E4));
    // Now that preturn has been promoted to a normal local turn, we should have full visibility.
    alt_game.apply_remote_turn(envoy!(White A), &alg("Nc3"), T0).unwrap();
    assert!(!alt_game.fog_of_war_area(A).contains(&Coord::E5));
    assert!(!alt_game.fog_of_war_area(A).contains(&Coord::E4));
}

#[test]
fn wayback_affects_fog_of_war() {
    let mut alt_game =
        AlteredGame::new(as_single_player(envoy!(White A)), fog_of_war_bughouse_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("d5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("xd5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("e5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("Qe2"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("Nc6"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("Qxe5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("Nf6"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("Qxe8"), T0).unwrap();
    assert_eq!(
        alt_game.status(),
        BughouseGameStatus::Victory(Team::Red, VictoryReason::Checkmate)
    );
    assert!(!alt_game.fog_of_war_area(A).contains(&Coord::D8));
    alt_game.wayback_to_turn(A, Some("00000002-w".to_owned()));
    assert!(alt_game.fog_of_war_area(A).contains(&Coord::D8));
}
