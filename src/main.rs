use std::io;

use bughouse_chess::*;


fn main() -> io::Result<()> {
    let chess_rules = ChessRules {
        starting_position: StartingPosition::Classic,
    };
    let bughouse_rules = BughouseRules {
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(7),
        drop_aggression: DropAggression::NoChessMate,
    };

    // let mut game = ChessGame::new(chess_rules);
    // println!("{}\n", tui::render_chess_game(&game));
    // loop {
    //     let mut buffer = String::new();
    //     let stdin = io::stdin();
    //     stdin.read_line(&mut buffer)?;
    //     if let Err(e) = game.try_turn_from_algebraic(&buffer) {
    //         println!("Impossible move: {:?}", e);
    //     } else {
    //         println!("{}\n", tui::render_chess_game(&game));
    //     }
    //     if game.status() != GameStatus::Active {
    //         println!("\n{:?}", game.status());
    //         return Ok(());
    //     }
    // }

    let mut game = BughouseGame::new(chess_rules, bughouse_rules);
    println!("{}\n", tui::render_bughouse_game(&game));
    loop {
        let mut buffer = String::new();
        let stdin = io::stdin();
        stdin.read_line(&mut buffer)?;
        let (board, turn) = buffer.split_at(1);
        if buffer.trim() == "q" {
            return Ok(())
        };
        let board_idx = match board {
            "<" => 0,
            ">" => 1,
            _ => {
                println!("Should begin with < or >");
                continue;
            }
        };
        if let Err(e) = game.try_turn_from_algebraic(board_idx, &turn) {
            println!("Impossible move: {:?}", e);
        } else {
            println!("{}\n", tui::render_bughouse_game(&game));
        }
        // if game.status() != GameStatus::Active {
        //     println!("\n{:?}", game.status());
        //     return Ok(());
        // }
    }
}
