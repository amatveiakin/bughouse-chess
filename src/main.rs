use std::io::{self, Write};
use std::time::{Instant, Duration};

use crossterm::{execute, terminal, cursor, event};
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
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    let start_time = Instant::now();

    let mut game = BughouseGame::new(chess_rules, bughouse_rules);
    let mut error_message: Option<String> = None;
    let mut keyboard_input = String::new();
    loop {
        // Don't clear the board to avoid blinking.
        execute!(stdout, cursor::MoveTo(0, 0))?;
        writeln!(stdout, "{}\n", tui::render_bughouse_game(&game))?;
        if game.status() != BughouseGameStatus::Active {
            assert!(error_message.is_none());
            let msg = format!("Game over: {:?}", game.status());
            writeln!(stdout, "{}", msg.with(style::Color::Blue))?;
            return Ok(());
        }
        execute!(stdout, terminal::Clear(terminal::ClearType::FromCursorDown))?;
        write!(stdout, "{}", keyboard_input)?;
        // Simulate cursor: real cursor blinking is broken with Show/Hide.
        if Instant::now().duration_since(start_time).as_millis() % 1000 < 500 {
            write!(stdout, "{}", "▂")?;
        }
        if let Some(ref err) = error_message {
            writeln!(stdout, "\n\n{}", err.clone().with(style::Color::Red))?;
        }
        writeln!(stdout, "\n\n{:?}", std::time::SystemTime::now())?;

        if event::poll(Duration::from_millis(100))? {
            if let event::Event::Key(event) = event::read()? {
                match event.code {
                    event::KeyCode::Char(ch) => {
                        keyboard_input.push(ch);
                    },
                    event::KeyCode::Backspace => {
                        keyboard_input.pop();
                    },
                    event::KeyCode::Enter => {
                        // TODO: Janitor for `keyboard_input.clear()`.
                        let command = keyboard_input.trim();
                        if command.trim() == "q" {
                            // TODO: Janitor for `LeaveAlternateScreen`.
                            execute!(stdout, terminal::LeaveAlternateScreen)?;
                            return Ok(())
                        };
                        let (board, turn) = command.split_at(1);
                        let board_idx = match board {
                            "<" => BughouseBoard::A,
                            ">" => BughouseBoard::B,
                            _ => {
                                error_message = Some("Should begin with < or >".to_owned());
                                keyboard_input.clear();
                                continue;
                            }
                        };
                        error_message = game.try_turn_from_algebraic(board_idx, &turn).err().map(|err| {
                            format!("Impossible move: {:?}", err)
                        });
                        keyboard_input.clear();
                    },
                    _ => {},
                }
            }
        }
    }
}
