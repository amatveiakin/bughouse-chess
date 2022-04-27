use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Instant, Duration};

use crossterm::{execute, terminal, cursor, event as term_event};
use crossterm::style::{self, Stylize};
use enum_map::{EnumMap, enum_map};
use itertools::Itertools;
use scopeguard::defer;

use bughouse_chess::*;
use bughouse_chess::client::*;


pub struct ClientConfig {
    pub server_address: String,
    pub player_name: String,
    pub team: String,
}

fn render(stdout: &mut io::Stdout, app_start_time: Instant, client_state: &ClientState)
    -> io::Result<()>
{
    let my_name = client_state.my_name();
    let now = Instant::now();
    execute!(stdout, cursor::MoveTo(0, 0))?;
    let mut highlight_input = false;
    let mut additional_message = None;
    match client_state.contest_state() {
        ContestState::Uninitialized => {
            execute!(stdout, terminal::Clear(terminal::ClearType::All))?;
            writeln!(stdout, "Loading...")?;
        },
        ContestState::Lobby{ ref players } => {
            execute!(stdout, terminal::Clear(terminal::ClearType::All))?;
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
        ContestState::Game{ ref game_confirmed, ref local_turn, ref game_start } => {
            let game_now = match game_start {
                Some(t) => GameInstant::new(*t, now),
                None => GameInstant::game_start(),
            };
            let game = game_local(my_name, game_confirmed, local_turn);
            writeln!(stdout, "{}\n", tui::render_bughouse_game(&game, game_now))?;
            // TODO: Don't clear the board to avoid blinking.
            execute!(stdout, terminal::Clear(terminal::ClearType::FromCursorDown))?;
            if game.status() == BughouseGameStatus::Active {
                highlight_input = game.player_is_active(my_name).unwrap();
            } else {
                additional_message = Some(
                    format!("Game over: {:?}", game.status()).with(style::Color::Magenta)
                );
            }
        },
    }

    // Simulate cursor: real cursor blinking is broken with Show/Hide.
    let show_cursor = now.duration_since(app_start_time).as_millis() % 1000 >= 500;
    let cursor = if show_cursor { '▂' } else { ' ' };
    let input_with_cursor = format!("{}{}", client_state.keyboard_input(), cursor);
    let input_style = if highlight_input { style::Color::White } else { style::Color::DarkGrey };
    // TODO: Show input on a fixed line regardless of client_status.
    write!(stdout, "{}", input_with_cursor.with(input_style))?;

    writeln!(stdout, "\n")?;
    if let Some(msg) = additional_message {
        writeln!(stdout, "{}", msg)?;
    }
    if let Some(ref err) = client_state.command_error() {
        writeln!(stdout, "{}", err.clone().with(style::Color::Red))?;
    }
    Ok(())
}

pub fn run(config: ClientConfig) -> io::Result<()> {
    let my_name = config.player_name.trim().to_owned();
    let my_team = match config.team.as_str() {
        "red" => Team::Red,
        "blue" => Team::Blue,
        _ => panic!("Unexpected team: {}", config.team),
    };
    let server_addr = (config.server_address.as_str(), network::PORT).to_socket_addrs().unwrap().collect_vec();
    println!("Connecting to {:?}...", server_addr);
    let net_stream = TcpStream::connect(&server_addr[..])?;
    // TODO: Test if nodelay helps.  Should it be set on both sides or just one?
    //   net_stream.set_nodelay(true)?;
    let mut net_in_stream = net_stream.try_clone()?;
    let mut net_out_stream = net_stream;
    std::mem::drop(config);

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
                &network::read_str(&mut net_in_stream).unwrap()).unwrap();
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

    let (server_tx, server_rx) = mpsc::channel();
    thread::spawn(move || {
        for ev in server_rx {
            network::write_obj(&mut net_out_stream, &ev).unwrap();
        }
    });

    let mut client_state = ClientState::new(my_name.to_owned(), my_team, server_tx);
    client_state.join();
    for event in rx {
        match client_state.apply_event(event) {
            EventReaction::Continue => {},
            EventReaction::ExitOk => {
                return Ok(());
            },
            EventReaction::ExitWithError(message) => {
                execute!(io::stdout(), terminal::LeaveAlternateScreen)?;
                writeln!(stdout, "Fatal error: {}", message)?;
                std::process::exit(1);
            }
        }
        render(&mut stdout, app_start_time, &client_state)?;
    }
    panic!("Unexpected end of events stream");
}
