use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Instant, Duration};

use crossterm::{execute, terminal, cursor, event as term_event};
use crossterm::style::{self, Stylize};
use enum_map::{EnumMap, enum_map};
use itertools::Itertools;
use scopeguard::defer;

use bughouse_chess::*;


pub struct ClientConfig {
    pub server_address: String,
    pub player_name: String,
    pub team: String,
}

enum IncomingEvent {
    Network(BughouseServerEvent),
    Terminal(term_event::Event),
    Tick,
}

enum ClientState {
    Uninitialized,
    Error { message: String },
    Lobby { players: Vec<Player> },
    Game {
        game: BughouseGame,  // local state; may contain moves not confirmed by the server yet
        game_confirmed: Option<BughouseGame>,  // state from the server, if different from `game`
        game_start: Option<Instant>,
    },
}

fn make_join_event(player_name: &str, team_name: &str) -> BughouseClientEvent {
    BughouseClientEvent::Join {
        player_name: player_name.to_owned(),
        team: match team_name {
            "red" => Team::Red,
            "blue" => Team::Blue,
            _ => panic!("Unexpected team: {}", team_name),
        }
    }
}

pub fn client_main(config: ClientConfig) -> io::Result<()> {
    let my_name = config.player_name.trim();
    let server_addr = (config.server_address.as_str(), network::PORT).to_socket_addrs().unwrap().collect_vec();
    println!("Connecting to {:?}...", server_addr);
    let mut net_stream = TcpStream::connect(&server_addr[..])?;
    // TODO: Test if nodelay helps.  Should it be set on both sides or just one?
    //   net_stream.set_nodelay(true)?;
    let mut net_read_stream = net_stream.try_clone()?;
    network::write_obj(&mut net_stream, &make_join_event(my_name, &config.team))?;
    #[allow(unused_variables)] let config = ();  // shouldn't be used anymore;  TODO: how to do this properly?

    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    defer!{ execute!(io::stdout(), terminal::LeaveAlternateScreen).unwrap(); };
    let app_start_time = Instant::now();

    let (tx, rx) = mpsc::channel();
    let tx_net = tx.clone();
    let tx_local = tx.clone();
    let tx_tick = tx;
    thread::spawn(move || {
        loop {
            let ev = network::parse_obj::<BughouseServerEvent>(
                &network::read_str(&mut net_read_stream).unwrap()).unwrap();
            tx_net.send(IncomingEvent::Network(ev)).unwrap();
        }
    });
    thread::spawn(move || {
        loop {
            let ev = term_event::read().unwrap();
            tx_local.send(IncomingEvent::Terminal(ev)).unwrap();
        }
    });
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            tx_tick.send(IncomingEvent::Tick).unwrap();
        }
    });

    let mut client_state = ClientState::Uninitialized;
    let mut error_message: Option<String> = None;
    let mut keyboard_input = String::new();
    for event in rx {
        // TODO: What should happen to error_message after state transition?
        let mut command_to_execute = None;
        match event {
            IncomingEvent::Terminal(term_event) => {
                if let term_event::Event::Key(key_event) = term_event {
                    match key_event.code {
                        term_event::KeyCode::Char(ch) => {
                            keyboard_input.push(ch);
                        },
                        term_event::KeyCode::Backspace => {
                            keyboard_input.pop();
                        },
                        term_event::KeyCode::Enter => {
                            command_to_execute = Some(keyboard_input.trim().to_owned());
                            keyboard_input.clear();
                        },
                        _ => {},
                    }
                }
            },
            IncomingEvent::Network(net_event) => {
                use BughouseServerEvent::*;
                match net_event {
                    Error{ message } => {
                        // TODO: Should all errors be final?
                        client_state = ClientState::Error{ message };
                    },
                    LobbyUpdated{ players } => {
                        client_state = ClientState::Lobby{ players };
                    },
                    GameStarted{ chess_rules, bughouse_rules, starting_grid, players } => {
                        let player_map = BughouseGame::make_player_map(
                            players.iter().map(|(p, board_idx)| (Rc::new(p.clone()), *board_idx))
                        );
                        client_state = ClientState::Game {
                            game: BughouseGame::new_with_grid(
                                chess_rules, bughouse_rules, starting_grid, player_map
                            ),
                            game_confirmed: None,
                            game_start: None,
                        };
                    },
                    TurnMade{ player_name, turn_algebraic, time, game_status } => {
                        if let ClientState::Game{
                            ref mut game, ref mut game_confirmed, ref mut game_start
                        } = client_state {
                            assert!(game.status() == BughouseGameStatus::Active, "Cannot make turn: game over");
                            if game_start.is_none() {
                                // TODO: Sync client/server times better; consider NTP
                                *game_start = Some(Instant::now());
                            }
                            if player_name == my_name {
                                *game = game_confirmed.take().unwrap();
                            }
                            let turn_result = game.try_turn_by_player_from_algebraic(
                                &player_name, &turn_algebraic, time
                            );
                            turn_result.unwrap_or_else(|err| {
                                panic!("Impossible turn: {}, error: {:?}", turn_algebraic, err)
                            });
                            assert_eq!(game_status, game.status());
                        } else {
                            panic!("Cannot make turn: no game in progress")
                        }
                    },
                    GameOver{ time, game_status } => {
                        if let ClientState::Game{ ref mut game, .. } = client_state {
                            assert!(game.status() == BughouseGameStatus::Active);
                            assert!(game_status != BughouseGameStatus::Active);
                            // TODO: Make sure this is synced with flag.
                            game.set_status(game_status, time);
                        } else {
                            panic!("Cannot record game result: no game in progress")
                        }
                    },
                }
            },
            IncomingEvent::Tick => {
                // Any event triggers repaint, so no additional action is required.
            },
        }

        if let Some(cmd) = command_to_execute {
            if cmd == "quit" {
                network::write_obj(&mut net_stream, &BughouseClientEvent::Leave)?;
                return Ok(());
            }
            if let ClientState::Game{ ref mut game, ref mut game_confirmed, .. } = client_state {
                if game.player_is_active(my_name).unwrap() {
                    assert!(game_confirmed.is_none());
                    *game_confirmed = Some(game.clone());
                    // Don't try to advance the clock: server is the source of truth for flag defeat.
                    // TODO: Fix time recorded in order to show accurate local time before the server confirmed the move.
                    //   Problem: need to avoid recording flag defeat prematurely.
                    let clock = game.player_board(my_name).unwrap().clock();
                    let turn_start = clock.turn_start().unwrap_or(GameInstant::game_start());
                    let turn_result = game.try_turn_by_player_from_algebraic(
                        my_name, &cmd, turn_start
                    );
                    match turn_result {
                        Ok(_) => {
                            error_message = None;
                            network::write_obj(&mut net_stream, &BughouseClientEvent::MakeTurn {
                                turn_algebraic: cmd
                            })?;
                        },
                        Err(err) => {
                            *game_confirmed = None;
                            // TODO: FIX: Screen is not updated while an error is shown.
                            error_message = Some(format!("Illegal turn: {:?}", err));
                        },
                    }
                } else {
                    keyboard_input = cmd;
                }
            }
        }

        let now = Instant::now();
        execute!(stdout, cursor::MoveTo(0, 0))?;
        // TODO: Don't clear the board to avoid blinking.
        execute!(stdout, terminal::Clear(terminal::ClearType::FromCursorDown))?;
        let mut highlight_input = false;
        match client_state {
            ClientState::Uninitialized => {
                writeln!(stdout, "Loading...")?;
            },
            ClientState::Error{ ref message } => {
                writeln!(stdout, "{}", message)?;
            },
            ClientState::Lobby{ ref players } => {
                let mut teams: EnumMap<Team, Vec<String>> = enum_map!{ _ => vec![] };
                for p in players {
                    teams[p.team].push(p.name.clone());
                }
                for (team, team_players) in teams {
                    writeln!(stdout, "Team {:?}:", team)?;
                    let color = match team {
                        Team::Red => style::Color::Red,
                        Team::Blue => style::Color::Blue,
                    };
                    for p in team_players {
                        writeln!(stdout, "  {} {}", "•".with(color), p)?;
                    }
                    writeln!(stdout, "")?;
                }
            },
            ClientState::Game{ ref game, ref game_start, .. } => {
                let game_now = match game_start {
                    Some(t) => GameInstant::new(*t, now),
                    None => GameInstant::game_start(),
                };
                writeln!(stdout, "{}\n", tui::render_bughouse_game(&game, game_now))?;
                if game.status() == BughouseGameStatus::Active {
                    highlight_input = game.player_is_active(my_name).unwrap();
                } else {
                    let msg = format!("Game over: {:?}", game.status());
                    writeln!(stdout, "{}", msg.with(style::Color::Magenta))?;
                }
            },
        }

        // Simulate cursor: real cursor blinking is broken with Show/Hide.
        let show_cursor = now.duration_since(app_start_time).as_millis() % 1000 >= 500;
        let cursor = if show_cursor { '▂' } else { ' ' };
        let input_style = if highlight_input { style::Color::White } else { style::Color::DarkGrey };
        // TODO: Show input on a fixed line regardless of client_status.
        write!(stdout, "{}", format!("{}{}", keyboard_input, cursor).with(input_style))?;

        writeln!(stdout, "\n")?;
        if let Some(ref err) = error_message {
            writeln!(stdout, "{}", err.clone().with(style::Color::Red))?;
        }
    }
    panic!("Unexpected end of events stream");
}
