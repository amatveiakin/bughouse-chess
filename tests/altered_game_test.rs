mod common;

use std::time::Duration;

use BughouseBoard::{A, B};
use bughouse_chess::altered_game::{
    AlteredGame, ReservePieceHighlight, SquareHighlight, TurnHighlightFamily, TurnHighlightItem,
    TurnHighlightLayer, TurnInputResult, WaybackDestination,
};
use bughouse_chess::board::{TurnError, TurnInput, VictoryReason};
use bughouse_chess::clock::GameInstant;
use bughouse_chess::coord::Coord;
use bughouse_chess::envoy;
use bughouse_chess::force::Force;
use bughouse_chess::game::{
    BughouseBoard, BughouseEnvoy, BughouseGame, BughouseGameStatus, BughouseParticipant,
    BughousePlayer, TurnIndex,
};
use bughouse_chess::piece::PieceKind;
use bughouse_chess::player::Team;
use bughouse_chess::role::Role;
use bughouse_chess::rules::{ChessRules, FairyPieces, MatchRules, Promotion, Rules};
use bughouse_chess::test_util::*;
use common::*;
use pretty_assertions::assert_eq;


pub fn alg(s: &str) -> TurnInput { algebraic_turn(s) }

fn as_single_player(envoy: BughouseEnvoy) -> BughouseParticipant {
    BughouseParticipant::Player(BughousePlayer::SinglePlayer(envoy))
}

fn as_double_player(team: Team) -> BughouseParticipant {
    BughouseParticipant::Player(BughousePlayer::DoublePlayer(team))
}

fn default_rules() -> Rules {
    Rules {
        match_rules: MatchRules::unrated_public(),
        chess_rules: ChessRules::bughouse_international5(),
    }
}

fn default_game() -> BughouseGame {
    let rules = default_rules();
    BughouseGame::new(rules, Role::Client, &sample_bughouse_players())
}

fn stealing_promotion_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().promotion = Promotion::Steal;
    BughouseGame::new(rules, Role::Client, &sample_bughouse_players())
}

fn accolade_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.chess_rules.fairy_pieces = FairyPieces::Accolade;
    BughouseGame::new(rules, Role::Client, &sample_bughouse_players())
}

fn duck_chess_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.chess_rules.duck_chess = true;
    BughouseGame::new(rules, Role::Client, &sample_bughouse_players())
}

fn fog_of_war_bughouse_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.chess_rules.fog_of_war = true;
    BughouseGame::new(rules, Role::Client, &sample_bughouse_players())
}

fn koedem_game() -> BughouseGame {
    let mut rules = default_rules();
    rules.bughouse_rules_mut().unwrap().koedem = true;
    BughouseGame::new(rules, Role::Client, &sample_bughouse_players())
}

macro_rules! square_highlight {
    ($board_idx:ident $coord:ident : $layer:ident $family:ident $item:ident) => {
        SquareHighlight {
            board_idx: BughouseBoard::$board_idx,
            coord: Coord::$coord,
            layer: TurnHighlightLayer::$layer,
            family: TurnHighlightFamily::$family,
            item: TurnHighlightItem::$item,
        }
    };
}

macro_rules! reserve_piece_highlight {
    ($board_idx:ident $force:ident $piece_kind:ident) => {
        ReservePieceHighlight {
            board_idx: BughouseBoard::$board_idx,
            force: Force::$force,
            piece_kind: PieceKind::$piece_kind,
        }
    };
}

fn sort_square_highlights(mut highlights: Vec<SquareHighlight>) -> Vec<SquareHighlight> {
    highlights.sort_by_key(|h| (h.board_idx, h.coord.row_col()));
    highlights
}

fn sort_reserve_piece_highlights(
    mut highlights: Vec<ReservePieceHighlight>,
) -> Vec<ReservePieceHighlight> {
    highlights.sort_by_key(|h| (h.board_idx, h.force, h.piece_kind));
    highlights
}

#[track_caller]
fn square_highlights_sorted(alt_game: &AlteredGame) -> Vec<SquareHighlight> {
    let highlights = alt_game.turn_highlights();
    assert!(highlights.reserve_piece_highlights.is_empty());
    sort_square_highlights(highlights.square_highlights)
}

fn turn_highlights_sorted(
    alt_game: &AlteredGame,
) -> (Vec<SquareHighlight>, Vec<ReservePieceHighlight>) {
    let highlights = alt_game.turn_highlights();
    (
        sort_square_highlights(highlights.square_highlights),
        sort_reserve_piece_highlights(highlights.reserve_piece_highlights),
    )
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
    alt_game.start_drag_piece(A, loc!(E5)).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("e5"), T0).unwrap();
    assert_eq!(
        alt_game.drag_piece_drop(A, Coord::E4),
        TurnInputResult::Error(TurnError::Defunct)
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
    alt_game.start_drag_piece(A, loc!(D4)).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("xd5"), T0).unwrap();
    assert_eq!(
        alt_game.drag_piece_drop(A, Coord::D3),
        TurnInputResult::Error(TurnError::Defunct)
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
    alt_game.start_drag_piece(A, loc!(E4)).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("e3"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("Nc6"), T0).unwrap();
    assert_eq!(
        alt_game.drag_piece_drop(A, Coord::E5),
        TurnInputResult::Turn((A, drag_move!(E4 -> E5)))
    );
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
    assert_eq!(square_highlights_sorted(&alt_game), vec![
        square_highlight!(A E4 : BelowFog Preturn MoveFrom),
        square_highlight!(A D5 : BelowFog Preturn MoveTo), // don't use `Capture` for preturns
        square_highlight!(B E4 : BelowFog LatestTurn MoveFrom),
        square_highlight!(B D5 : BelowFog LatestTurn Capture),
    ]);
}

#[test]
fn multiple_turn_highlights_per_square() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), accolade_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("Rg1"), T0).unwrap();
    alt_game.try_local_turn(A, alg("Ef3"), T0).unwrap();
    alt_game.start_drag_piece(A, loc!(F3)).unwrap();
    assert_eq!(
        square_highlights_sorted(&alt_game),
        sort_square_highlights(vec![
            // drag start
            square_highlight!(A F3 : AboveFog PartialTurn DragStart),
            // rook moves
            square_highlight!(A A3 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A B3 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A C3 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A D3 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A E3 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A G3 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A H3 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A F4 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A F5 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A F6 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A F7 : AboveFog PartialTurn LegalDestination),
            // knight moves
            square_highlight!(A G1 : AboveFog PartialTurn LegalDestination), // <--
            square_highlight!(A H4 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A G5 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A E5 : AboveFog PartialTurn LegalDestination),
            square_highlight!(A D4 : AboveFog PartialTurn LegalDestination),
            // preturn highlight still active
            square_highlight!(A G1 : BelowFog Preturn MoveFrom), // <--
            square_highlight!(A F3 : BelowFog Preturn MoveTo),
        ])
    );
}

#[test]
fn click_moves() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    assert_eq!(alt_game.click(A, loc!(E2)), TurnInputResult::Noop);
    assert_eq!(square_highlights_sorted(&alt_game), vec![
        square_highlight!(A E2 : BelowFog PartialTurn MoveFrom)
    ]);
    assert_eq!(alt_game.click(A, loc!(E4)), TurnInputResult::Turn((A, drag_move!(E2 -> E4))));
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("e5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White B), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black B), &alg("d5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White B), &alg("Nc3"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black B), &alg("xe4"), T0).unwrap();
    assert_eq!(alt_game.click(A, loc!(White Pawn)), TurnInputResult::Noop);
    assert_eq!(
        turn_highlights_sorted(&alt_game),
        (
            vec![
                square_highlight!(A E5 : BelowFog LatestTurn MoveTo),
                square_highlight!(A E7 : BelowFog LatestTurn MoveFrom),
                square_highlight!(B E4 : BelowFog LatestTurn Capture),
                square_highlight!(B D5 : BelowFog LatestTurn MoveFrom),
            ],
            vec![reserve_piece_highlight!(A White Pawn)]
        )
    );
    assert_eq!(alt_game.click(A, loc!(D5)), TurnInputResult::Turn((A, drag_move!(Pawn @ D5))));
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

    alt_game.start_drag_piece(A, loc!(G7)).unwrap();
    assert_eq!(alt_game.drag_piece_drop(A, Coord::F8), TurnInputResult::Noop);
    let TurnInputResult::Turn((input_board_idx, input)) = alt_game.click(B, loc!(C3)) else {
        panic!();
    };
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
    alt_game.start_drag_piece(A, loc!(G7)).unwrap();
    assert_eq!(
        alt_game.drag_piece_drop(A, Coord::G8),
        TurnInputResult::Error(TurnError::PathBlocked)
    );
    assert!(alt_game.local_game().board(A).grid()[Coord::G7].is(piece!(White Pawn)));
    assert!(alt_game.local_game().board(A).grid()[Coord::G8].is(piece!(Black Knight)));
}

#[test]
// Stealing promotion is unique in that it can make a local in-order turn invalid.
fn stealing_promotion_invalidates_local_turn() {
    let mut alt_game =
        AlteredGame::new(as_single_player(envoy!(White B)), stealing_promotion_game());
    let steal_target = alt_game.local_game().board(B).grid()[Coord::B1].unwrap();
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
        .apply_remote_turn(envoy!(White A), &drag_move!(G7 -> F8 = steal_target), T0)
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
    assert_eq!(square_highlights_sorted(&alt_game), vec![
        square_highlight!(A D5 : BelowFog LatestTurn MoveTo),
        square_highlight!(A D7 : BelowFog LatestTurn MoveFrom),
    ]);

    // Both the normal piece and the duck should be highlighted when the megaturn is competed.
    alt_game.apply_remote_turn(envoy!(Black A), &alg("@c3"), T0).unwrap();
    assert_eq!(square_highlights_sorted(&alt_game), vec![
        square_highlight!(A C3 : BelowFog LatestTurn MoveTo),
        square_highlight!(A D5 : BelowFog LatestTurn MoveTo),
        square_highlight!(A D7 : BelowFog LatestTurn MoveFrom),
    ]);

    // Hint that it's time to move the duck now.
    alt_game.apply_remote_turn(envoy!(White A), &alg("Nf3"), T0).unwrap();
    assert_eq!(square_highlights_sorted(&alt_game), vec![
        square_highlight!(A C3 : BelowFog PartialTurn MoveFrom),
    ]);
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
    let game = BughouseGame::new(rules, Role::Client, &sample_bughouse_players());
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), game);
    alt_game.apply_remote_turn(envoy!(White A), &alg("e3"), T0).unwrap();
    assert!(alt_game.fog_of_war_area(A).contains(&Coord::D6));
    alt_game.apply_remote_turn(envoy!(White A), &alg("@d6"), T0).unwrap();
    assert!(!alt_game.fog_of_war_area(A).contains(&Coord::D6));
}

#[test]
fn wayback_navigation() {
    use WaybackDestination::*;
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("a4"), T0).unwrap(); // TurnIndex == 0
    alt_game.apply_remote_turn(envoy!(White B), &alg("h4"), T0).unwrap(); // TurnIndex == 1
    alt_game.apply_remote_turn(envoy!(Black A), &alg("a5"), T0).unwrap(); // TurnIndex == 2
    alt_game.apply_remote_turn(envoy!(White A), &alg("b4"), T0).unwrap(); // TurnIndex == 3
    alt_game.apply_remote_turn(envoy!(Black B), &alg("h5"), T0).unwrap(); // TurnIndex == 4
    alt_game.apply_remote_turn(envoy!(Black A), &alg("b5"), T0).unwrap(); // TurnIndex == 5
    alt_game.set_status(BughouseGameStatus::Victory(Team::Red, VictoryReason::Resignation), T0);

    assert_eq!(alt_game.wayback_state().turn_index(), None);
    assert_eq!(alt_game.wayback_state().display_turn_index(), Some(TurnIndex(5)));

    assert_eq!(alt_game.wayback_to(Previous, None), Some(TurnIndex(4)));
    assert_eq!(alt_game.wayback_to(Previous, None), Some(TurnIndex(3)));
    assert_eq!(alt_game.wayback_to(Next, None), Some(TurnIndex(4)));
    assert_eq!(alt_game.wayback_to(First, None), Some(TurnIndex(0)));
    assert_eq!(alt_game.wayback_to(Previous, None), Some(TurnIndex(0)));
    assert_eq!(alt_game.wayback_to(Last, None), None);
    assert_eq!(alt_game.wayback_to(Next, None), None);

    assert_eq!(alt_game.wayback_to(Previous, Some(A)), Some(TurnIndex(3)));
    assert_eq!(alt_game.wayback_to(Previous, Some(A)), Some(TurnIndex(2)));
    assert_eq!(alt_game.wayback_to(Previous, Some(A)), Some(TurnIndex(0)));
    assert_eq!(alt_game.wayback_to(Next, Some(A)), Some(TurnIndex(2)));
    assert_eq!(alt_game.wayback_to(First, Some(A)), Some(TurnIndex(0)));
    assert_eq!(alt_game.wayback_to(Previous, Some(A)), Some(TurnIndex(0)));
    assert_eq!(alt_game.wayback_to(Last, Some(A)), None);
    assert_eq!(alt_game.wayback_to(Next, Some(A)), None);

    assert_eq!(alt_game.wayback_to(Index(Some(TurnIndex(4))), None), Some(TurnIndex(4)));
    assert_eq!(alt_game.wayback_to(Previous, Some(B)), Some(TurnIndex(1)));
    assert_eq!(alt_game.wayback_to(Next, Some(B)), Some(TurnIndex(4)));
    assert_eq!(alt_game.wayback_to(First, Some(B)), Some(TurnIndex(1)));
    assert_eq!(alt_game.wayback_to(Previous, Some(B)), Some(TurnIndex(1)));
    assert_eq!(alt_game.wayback_to(Last, Some(B)), Some(TurnIndex(4)));
    assert_eq!(alt_game.wayback_to(Next, Some(B)), Some(TurnIndex(4)));

    // Check what happens when going to a turn on a given board while starting with a turn on
    // another board. Particularly interesting is the behavior of `Previous`, see a comment in
    // `AltGame::wayback_to`.
    assert_eq!(alt_game.wayback_to(Last, None), None);
    assert_eq!(alt_game.wayback_to(Previous, Some(B)), Some(TurnIndex(1)));
    assert_eq!(alt_game.wayback_to(First, None), Some(TurnIndex(0)));
    assert_eq!(alt_game.wayback_to(Next, Some(B)), Some(TurnIndex(1)));
    assert_eq!(alt_game.wayback_to(Index(Some(TurnIndex(1))), None), Some(TurnIndex(1)));
    assert_eq!(alt_game.wayback_to(Next, Some(A)), Some(TurnIndex(2)));
    assert_eq!(alt_game.wayback_to(Index(Some(TurnIndex(4))), None), Some(TurnIndex(4)));
    assert_eq!(alt_game.wayback_to(Previous, Some(A)), Some(TurnIndex(2)));
}

#[test]
fn wayback_turn_highlight() {
    let mut alt_game = AlteredGame::new(as_single_player(envoy!(White A)), default_game());
    alt_game.apply_remote_turn(envoy!(White A), &alg("e4"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(Black A), &alg("e5"), T0).unwrap();
    alt_game.apply_remote_turn(envoy!(White A), &alg("Nc3"), T0).unwrap();
    // Normally we don't highlight turns by the current player ...
    assert!(square_highlights_sorted(&alt_game).is_empty());
    alt_game.apply_remote_turn(envoy!(Black A), &alg("Nf6"), T0).unwrap();
    alt_game.set_status(BughouseGameStatus::Victory(Team::Red, VictoryReason::Resignation), T0);

    alt_game.wayback_to(WaybackDestination::Index(Some(TurnIndex(2))), None);
    // ... but we do if we're waybacking.
    assert_eq!(square_highlights_sorted(&alt_game), vec![
        square_highlight!(A B1 : BelowFog LatestTurn MoveFrom),
        square_highlight!(A C3 : BelowFog LatestTurn MoveTo),
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
    alt_game.wayback_to(WaybackDestination::Index(Some(TurnIndex(2))), None);
    assert!(alt_game.fog_of_war_area(A).contains(&Coord::D8));
}

// Regression fix: the client crashed when playing on two boards and invalidating your own preturn
// by making a move on the other board.
#[test]
fn invalidate_own_preturn() {
    let game_log = "1B.Nc3 1b.h6 2B.Nb5 2b.h5 3B.Nxc7 3b.h4 1A.e4 1a.e5";
    let mut game = koedem_game();
    replay_bughouse_log(&mut game, game_log, Duration::ZERO).unwrap();

    let mut alt_game = AlteredGame::new(as_double_player(Team::Blue), game);
    alt_game.try_local_turn(A, drag_move!(G8 -> F6), T0).unwrap();
    alt_game.try_local_turn(B, drag_move!(C7 -> E8), T0).unwrap();
    alt_game.local_game();
}
