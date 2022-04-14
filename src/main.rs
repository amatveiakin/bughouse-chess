use std::io::{self, Write};

use crossterm::{execute, terminal, cursor};
use crossterm::style::{self, Stylize};

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

    let mut stdout = io::stdout();
    let mut game = BughouseGame::new(chess_rules, bughouse_rules);
    let mut error_message: Option<String> = None;
    loop {
        execute!(stdout, terminal::Clear(terminal::ClearType::All), cursor::MoveTo(0, 0))?;
        writeln!(stdout, "{}\n", tui::render_bughouse_game(&game))?;
        if game.status() != BughouseGameStatus::Active {
            assert!(error_message.is_none());
            let msg = format!("Game over: {:?}", game.status());
            writeln!(stdout, "{}", msg.with(style::Color::Blue))?;
            return Ok(());
        }
        if let Some(err) = error_message {
            execute!(stdout, cursor::SavePosition)?;
            writeln!(stdout, "\n\n{}", err.with(style::Color::Red))?;
            execute!(stdout, cursor::RestorePosition)?;
        }
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
                error_message = Some("Should begin with < or >".to_owned());
                continue;
            }
        };
        error_message = game.try_turn_from_algebraic(board_idx, &turn).err().map(|err| {
            format!("Impossible move: {:?}", err)
        });
    }
}
