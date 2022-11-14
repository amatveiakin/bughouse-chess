
mod common;

use bughouse_chess::*;
use common::*;

// Regression test: shouldn't panic if there's a drag depending on a local turn that was reverted.
#[test]
fn drag_depends_on_reverted_local_turn() {
    let game_start = BughouseGame::new(ChessRules::classic_blitz(), BughouseRules::chess_com(), sample_bughouse_players());
    let my_id = BughouseParticipantId::Player(BughousePlayerId{ board_idx: BughouseBoard::A, force: Force::White });
    let mut alt_game = AlteredGame::new(my_id, game_start);
    assert!(alt_game.try_local_turn(&drag_move!(E2 -> E4), GameInstant::game_start()).is_ok());
    assert!(alt_game.start_drag_piece(PieceDragStart::Board(Coord::E4)).is_ok());
    let _game = alt_game.local_game();
    alt_game.set_status(BughouseGameStatus::Victory(Team::Red, VictoryReason::Resignation), GameInstant::game_start());
    let _game = alt_game.local_game();  // the point of the test is to verify that it doesn't crash
}
