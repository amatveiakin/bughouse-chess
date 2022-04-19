use std::io::{self, Write};
use std::time::{Instant, Duration};

use crossterm::{execute, terminal, cursor, event};
use crossterm::style::{self, Stylize};
use scopeguard::defer;

use bughouse_chess::*;


fn main() -> io::Result<()> {
    let chess_rules = ChessRules {
        starting_position: StartingPosition::Classic,
        time_control: TimeControl{ starting_time: Duration::from_secs(300) },
    };
    let bughouse_rules = BughouseRules {
        min_pawn_drop_row: SubjectiveRow::from_one_based(2),
        max_pawn_drop_row: SubjectiveRow::from_one_based(7),
        drop_aggression: DropAggression::NoChessMate,
    };

    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    defer!{ execute!(io::stdout(), terminal::LeaveAlternateScreen).unwrap(); };
    let start_time = Instant::now();

    let mut game = BughouseGame::new(chess_rules, bughouse_rules);
    let mut error_message: Option<String> = None;
    let mut keyboard_input = String::new();
    loop {
        let now = Instant::now();
        game.test_flag(now);
        // Don't clear the board to avoid blinking.
        execute!(stdout, cursor::MoveTo(0, 0))?;
        writeln!(stdout, "{}\n", tui::render_bughouse_game(&game, now))?;
        execute!(stdout, terminal::Clear(terminal::ClearType::FromCursorDown))?;
        write!(stdout, "{}", keyboard_input)?;
        // Simulate cursor: real cursor blinking is broken with Show/Hide.
        if now.duration_since(start_time).as_millis() % 1000 < 500 {
            write!(stdout, "{}", "▂")?;
        }
        writeln!(stdout, "\n")?;
        if game.status() != BughouseGameStatus::Active {
            let msg = format!("Game over: {:?}", game.status());
            writeln!(stdout, "{}", msg.with(style::Color::Blue))?;
        }
        if let Some(ref err) = error_message {
            writeln!(stdout, "{}", err.clone().with(style::Color::Red))?;
        }

        if event::poll(Duration::from_millis(100))? {
            let now = Instant::now();
            game.test_flag(now);
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
                            return Ok(())
                        };
                        if game.status() != BughouseGameStatus::Active {
                            keyboard_input.clear();
                            continue;
                        }
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
                        let turn_result = game.try_turn_from_algebraic(board_idx, &turn, now);
                        error_message = turn_result.err().map(|err| {
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
