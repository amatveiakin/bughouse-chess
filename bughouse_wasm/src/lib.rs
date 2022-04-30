extern crate enum_map;
extern crate horrorshow;
extern crate instant;
extern crate serde_json;
extern crate wasm_bindgen;

extern crate bughouse_chess;

use std::sync::mpsc;

use enum_map::{EnumMap, enum_map};
use horrorshow::html;
use instant::Instant;
use wasm_bindgen::prelude::*;

use bughouse_chess::*;
use bughouse_chess::client::*;


#[wasm_bindgen]
pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call the
    // `set_panic_hook` function at least once during initialization, and then
    // we will get better error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct WebClient {
    state: ClientState,
    server_rx: mpsc::Receiver<BughouseClientEvent>,
}

#[wasm_bindgen]
impl WebClient {
    pub fn new_client(my_name: &str, my_team: &str) -> Self {
        let my_team = match my_team {
            "red" => Team::Red,
            "blue" => Team::Blue,
            _ => panic!("Unexpected team: {}", my_team),
        };
        let (server_tx, server_rx) = mpsc::channel();
        WebClient {
            state: ClientState::new(my_name.to_owned(), my_team, server_tx),
            server_rx,
        }
    }

    pub fn join(&mut self) {
        self.state.join();
    }
    pub fn resign(&mut self) {
        self.state.resign();
    }
    pub fn leave(&mut self) {
        self.state.leave();
    }
    pub fn make_turn(&mut self, turn_algebraic: String) -> Option<String> {
        self.state.make_turn(turn_algebraic).err().map(|err| {
            format!("{:?}", err)
        })
    }

    pub fn process_server_event(&mut self, event: &str) {
        self.state.process_server_event(serde_json::from_str(event).unwrap()).unwrap();
    }

    pub fn next_outgoing_event(&mut self) -> Option<String> {
        match self.server_rx.try_recv() {
            Ok(event) => Some(serde_json::to_string(&event).unwrap()),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => panic!("Event channel disconnected"),
        }
    }

    pub fn get_state(&self) -> String {
        match self.state.contest_state() {
            ContestState::Uninitialized => {
                "Initializing...".to_owned()
            },
            ContestState::Lobby{ ref players } => {
                let mut teams: EnumMap<Team, Vec<String>> = enum_map!{ _ => vec![] };
                for p in players {
                    teams[p.team].push(p.name.clone());
                }
                format!("{}", html! {
                    p { : "Red team:" }
                    ul {
                        @ for player in &teams[Team::Red] {
                            li { : player }
                        }
                    }
                    p { : "Blue team:" }
                    ul {
                        @ for player in &teams[Team::Blue] {
                            li { : player }
                        }
                    }
                })
            },
            ContestState::Game{ ref game_confirmed, ref local_turn, game_start } => {
                let now = Instant::now();
                let _game_now = GameInstant::from_maybe_active_game(*game_start, now).approximate();
                let game = game_local(self.state.my_name(), game_confirmed, local_turn);
                // TODO: Show both boards; show clock; flip as necessary; formatting.
                let grid = game.board(BughouseBoard::A).grid();
                format!("{}", html! {
                    table {
                        @ for row in Row::all() {
                            tr {
                                @ for col in Col::all() {
                                    td {
                                        : to_unicode_char(&grid[Coord::new(row, col)])
                                    }
                                }
                            }
                        }
                    }
                })
            },
        }
    }
}

fn to_unicode_char(piece: &Option<PieceOnBoard>) -> char {
    use self::PieceKind::*;
    use self::Force::*;
    match piece {
        Some(piece) => match (piece.force, piece.kind) {
            (White, Pawn) => '♙',
            (White, Knight) => '♘',
            (White, Bishop) => '♗',
            (White, Rook) => '♖',
            (White, Queen) => '♕',
            (White, King) => '♔',
            (Black, Pawn) => '♟',
            (Black, Knight) => '♞',
            (Black, Bishop) => '♝',
            (Black, Rook) => '♜',
            (Black, Queen) => '♛',
            (Black, King) => '♚',
        },
        None => ' ',
    }
}
