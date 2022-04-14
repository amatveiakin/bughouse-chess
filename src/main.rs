use std::io;

use bughouse_chess::*;


fn main() -> io::Result<()> {
    let chess_rules = ChessRules {
        starting_position: StartingPosition::Classic,
    };
    let _bughouse_rules = BughouseRules {
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(7),
        drop_aggression: DropAggression::NoChessMate,
    };

    let mut game = ChessGame::new(chess_rules.clone());
    println!("{}\n", game.render_as_unicode());
    loop {
        let mut buffer = String::new();
        let stdin = io::stdin();
        stdin.read_line(&mut buffer)?;
        if let Err(e) = game.try_replay_log(&buffer) {
            println!("Impossible move: {:?}", e);
        } else {
            println!("{}\n", game.render_as_unicode());
        }
        if game.status() != GameStatus::Active {
            println!("\n{:?}", game.status());
            return Ok(());
        }
    }

    // let mut game = BughouseGame::new(chess_rules, bughouse_rules);
    // game.try_turn(0, Turn::Move(TurnMove{ from: Coord::E2, to: Coord::E4, promote_to: None })).unwrap();
}
