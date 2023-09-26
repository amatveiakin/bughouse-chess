mod common;
use bughouse_chess::altered_game::{
    AlteredGame, PieceDragError, PieceDragStart, TurnHighlight, TurnHighlightFamily,
    TurnHighlightItem, TurnHighlightLayer,
};
use bughouse_chess::board::{TurnError, TurnInput, VictoryReason};
use bughouse_chess::clock::GameInstant;
use bughouse_chess::coord::Coord;
use bughouse_chess::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus, BughouseParticipant,
    BughousePlayer,
};
use bughouse_chess::piece::PieceKind;
use bughouse_chess::player::Team;
use bughouse_chess::rules::{ChessRules, MatchRules, Promotion, Rules};
use bughouse_chess::test_util::*;
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

fn default_rules() -> Rules {
    Rules {
        match_rules: MatchRules::unrated(),
        chess_rules: ChessRules::bughouse_chess_com(),
    }
}

fn default_game() -> BughouseGame {
    let rules = default_rules();
    BughouseGame::new(rules, &sample_bughouse_players())
}

fn stealing_promotion_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    BughouseGame::new(rules, &sample_bughouse_players())
}

fn duck_chess_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.chess_rules.duck_chess = true;
    BughouseGame::new(rules, &sample_bughouse_players())
}

fn fog_of_war_bughouse_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.chess_rules.fog_of_war = true;
    BughouseGame::new(rules, &sample_bughouse_players())
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

fn turn_highlights_sorted(alt_game: &AlteredGame) -> Vec<TurnHighlight> {
    let mut highlights = alt_game.turn_highlights();
    highlights.sort_by_key(|h| (h.board_idx, h.coord.row_col()));
    highlights
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
        alt_game.drag_piece_drop(A, Coord::E4, PieceKind::Queen),
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
        alt_game.drag_piece_drop(A, Coord::D3, PieceKind::Queen),
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
    let drag_result = alt_game.drag_piece_drop(A, Coord::E5, PieceKind::Queen).unwrap();
    assert_eq!(drag_result, Some(drag_move!(E4 -> E5)));
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
    assert_eq!(turn_highlights_sorted(&alt_game), vec![
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
fn stealing_promotion() {
    let mut alt_game =
        AlteredGame::new(as_single_player(envoy!(White A)), stealing_promotion_game());

    // The original promo-stealing code contained several bugs related to looking at the current
    // board rather than the other (e.g. when construction algebraic notation), so to make sure we
    // test that it doesn't happen:
    //   - Move the to-be-stolen piece to a different location on the target board;
    //   - Sacrifice the corresponding piece altogether on the original board (note that simply
    //     moving it to a different location is not sufficient: it has the same PieceId, so it may
    //     still be found).
    alt_game.apply_remote_turn(envoy!(White B), &drag_move!(B1 -> C3), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(B1 -> A3), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(B8 -> A6), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(A3 -> B5), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A6 -> B8), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(B5 -> C7), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(D8 -> C7), T0).unwrap();

    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H2 -> H4), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A7 -> A5), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H4 -> H5), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A5 -> A4), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H5 -> H6), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A4 -> A3), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H6 -> G7), T0).unwrap();

    assert!(alt_game.local_game().board(A).grid()[Coord::G7].is(piece!(White Pawn)));
    assert!(alt_game.local_game().board(A).grid()[Coord::F8].is(piece!(Black Bishop)));
    assert!(alt_game.local_game().board(B).grid()[Coord::C3].is(piece!(White Knight)));

    alt_game.start_drag_piece(A, PieceDragStart::Board(Coord::G7)).unwrap();
    assert!(alt_game.drag_piece_drop(A, Coord::F8, PieceKind::Queen).unwrap().is_none());
    let (input_board_idx, input) = alt_game.click_square(B, Coord::C3).unwrap();
    assert_eq!(input_board_idx, A);
    alt_game.try_local_turn(input_board_idx, input, T0).unwrap();

    assert!(alt_game.local_game().board(A).grid()[Coord::G7].is_none());
    assert!(alt_game.local_game().board(A).grid()[Coord::F8].is(piece!(White Knight)));
    assert!(alt_game.local_game().board(B).grid()[Coord::C3].is_none());
}

// Regression test: Cannot drag pawn onto piece when stealing promotion.
// (More generally, test that partial turns are verified as well.)
#[test]
fn stealing_promotion_cannot_move_pawn_onto_piece() {
    let mut alt_game =
        AlteredGame::new(as_single_player(envoy!(White A)), stealing_promotion_game());
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H2 -> H4), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A7 -> A5), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H4 -> H5), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A5 -> A4), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H5 -> H6), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A4 -> A3), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H6 -> G7), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A3 -> B2), T0).unwrap();
    alt_game.start_drag_piece(A, PieceDragStart::Board(Coord::G7)).unwrap();
    assert_eq!(
        alt_game.drag_piece_drop(A, Coord::G8, PieceKind::Queen),
        Err(PieceDragError::DragIllegal)
    );
    assert!(alt_game.local_game().board(A).grid()[Coord::G7].is(piece!(White Pawn)));
    assert!(alt_game.local_game().board(A).grid()[Coord::G8].is(piece!(Black Knight)));
}

#[test]
// Stealing promotion is unique in that it can make a local in-order turn invalid.
fn stealing_promotion_invalidates_local_turn() {
    let mut alt_game =
        AlteredGame::new(as_single_player(envoy!(White B)), stealing_promotion_game());
    let steal_target_id = alt_game.local_game().board(B).grid()[Coord::B1].unwrap().id;
    alt_game.try_local_turn(B, drag_move!(B1 -> C3), T0).unwrap();

    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H2 -> H4), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A7 -> A5), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H4 -> H5), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A5 -> A4), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H5 -> H6), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A4 -> A3), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(H6 -> G7), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &drag_move!(A3 -> B2), T0).unwrap();
    alt_game
        .apply_remote_turn(envoy!(White A), &drag_move!(G7 -> F8 = Knight steal_target_id), T0)
        .unwrap();

    assert!(alt_game.local_game().board(B).grid()[Coord::B1].is_none());
    assert!(alt_game.local_game().board(B).grid()[Coord::C3].is_none());
    // The new local turn should not be considered preturn, because the old local turn was cancelled.
    alt_game.try_local_turn(B, drag_move!(E2 -> E4), T0).unwrap();
    assert_eq!(alt_game.num_preturns_on_board(B), 0);
}

#[test]
fn cannot_move_duck_instead_of_piece() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), duck_chess_game());
    assert_eq!(
        alt_game.try_local_turn(A, drag_move!(@ A6), T0),
        Err(TurnError::MustMovePieceBeforeDuck)
    );
    alt_game.try_local_turn(A, drag_move!(E2 -> E4), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(@ B6), T0).unwrap();
    // Must order must be followed for preturns as well:
    assert_eq!(
        alt_game.try_local_turn(A, drag_move!(@ C6), T0),
        Err(TurnError::MustMovePieceBeforeDuck)
    );
}

#[test]
fn cannot_move_piece_instead_of_duck() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), duck_chess_game());
    alt_game.try_local_turn(A, drag_move!(E2 -> E4), T0).unwrap();
    assert_eq!(
        alt_game.try_local_turn(A, drag_move!(D2 -> D4), T0),
        Err(TurnError::MustPlaceDuck)
    );
    alt_game.try_local_turn(A, drag_move!(@ A6), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(F2 -> F4), T0).unwrap();
    // Must order must be followed for preturns as well:
    assert_eq!(
        alt_game.try_local_turn(A, drag_move!(G2 -> G4), T0),
        Err(TurnError::MustPlaceDuck)
    );
}

#[test]
fn two_preturns_allowed_in_duck_chess() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), duck_chess_game());
    alt_game.try_local_turn(A, drag_move!(E2 -> E4), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(@ A6), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(D2 -> D4), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(@ B6), T0).unwrap();
    // Third preturn is still not allowed.
    assert_eq!(
        alt_game.try_local_turn(A, drag_move!(F2 -> F4), T0),
        Err(TurnError::PreturnLimitReached)
    );
}

#[test]
fn intermixing_turns_duck_chess() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(Black A)), duck_chess_game());
    alt_game.apply_remote_turn(envoy!(White A), &drag_move!(E2 -> E4), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(E7 -> E5), T0).unwrap();
    alt_game.try_local_turn(A, drag_move!(@ A6), T0).unwrap();
}

#[test]
fn turn_highlights_in_duck_chess() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), duck_chess_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("@c6"), T0).unwrap();

    // Highlight the first part of the megaturn as soon as it's made.
    alt_game.apply_remote_turn(envoy!(Black A), &alg("d5"), T0).unwrap();
    assert_eq!(turn_highlights_sorted(&alt_game), vec![
        turn_highlight!(A D5 : BelowFog LatestTurn MoveTo),
        turn_highlight!(A D7 : BelowFog LatestTurn MoveFrom),
    ]);

    // Both the normal piece and the duck should be highlighted when the megaturn is competed.
    alt_game.apply_remote_turn(envoy!(Black A), &alg("@c3"), T0).unwrap();
    assert_eq!(turn_highlights_sorted(&alt_game), vec![
        turn_highlight!(A C3 : BelowFog LatestTurn Drop),
        turn_highlight!(A D5 : BelowFog LatestTurn MoveTo),
        turn_highlight!(A D7 : BelowFog LatestTurn MoveFrom),
    ]);

    // No highlights in the midst of the current player turn.
    alt_game.apply_remote_turn(envoy!(White A), &alg("Nf3"), T0).unwrap();
    assert!(turn_highlights_sorted(&alt_game).is_empty());
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
fn duck_visible_in_the_fog() {
    let mut rules = default_rules();
    rules.chess_rules.duck_chess = true;
    rules.chess_rules.fog_of_war = true;
    let game = BughouseGame::new(rules, &sample_bughouse_players());
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), game);
    alt_game.apply_remote_turn(envoy!(White A), &alg("e3"), T0).unwrap();
    assert!(alt_game.fog_of_war_area(A).contains(&Coord::D6));
    alt_game.apply_remote_turn(envoy!(White A), &alg("@d6"), T0).unwrap();
    assert!(!alt_game.fog_of_war_area(A).contains(&Coord::D6));
}

#[test]
fn wayback_turn_highlight() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("e5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("Nc3"), T0).unwrap();
    // Normally we don't highlight turns by the current player ...
    assert!(turn_highlights_sorted(&alt_game).is_empty());
    alt_game.apply_remote_turn(envoy!(Black A), &alg("Nf6"), T0).unwrap();
    alt_game.set_status(BughouseGameStatus::Victory(Team::Red, VictoryReason::Resignation), T0);

    alt_game.wayback_to_turn(A, Some("00000002-w".to_owned()));
    // ... but we do if we're waybacking.
    assert_eq!(turn_highlights_sorted(&alt_game), vec![
        turn_highlight!(A B1 : BelowFog LatestTurn MoveFrom),
        turn_highlight!(A C3 : BelowFog LatestTurn MoveTo),
    ]);
    // Sanity check: verify that we went to the right turn.
    assert!(alt_game.local_game().board(A).grid()[Coord::C3].is_some());
    assert!(alt_game.local_game().board(A).grid()[Coord::F6].is_none());
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
