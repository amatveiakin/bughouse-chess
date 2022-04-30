extern crate enum_map;
extern crate instant;
extern crate serde_json;
extern crate wasm_bindgen;

extern crate bughouse_chess;

use std::sync::mpsc;

use enum_map::{EnumMap, enum_map};
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
    primary_board: Option<BughouseBoard>,
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
            primary_board: None,
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
        let game_was_active = matches!(self.state.contest_state(), ContestState::Game{ .. });
        self.state.process_server_event(serde_json::from_str(event).unwrap()).unwrap();
        if !game_was_active {
            if let ContestState::Game{ ref game_confirmed, .. } = self.state.contest_state() {
                let my_name = self.state.my_name();
                let (board_idx, force) = game_confirmed.find_player(my_name).unwrap();
                render_game_area(force == Force::Black);
                self.primary_board = Some(board_idx);
            }
        }
    }

    pub fn next_outgoing_event(&mut self) -> Option<String> {
        match self.server_rx.try_recv() {
            Ok(event) => Some(serde_json::to_string(&event).unwrap()),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => panic!("Event channel disconnected"),
        }
    }

    pub fn update_state(&self) {
        let document = web_document();
        let info_string = document.get_element_by_id("info-string").unwrap();
        match self.state.contest_state() {
            ContestState::Uninitialized => {
                info_string.set_text_content(Some("Initializing..."));
            },
            ContestState::Lobby{ ref players } => {
                let mut teams: EnumMap<Team, Vec<String>> = enum_map!{ _ => vec![] };
                for p in players {
                    teams[p.team].push(p.name.clone());
                }
                info_string.set_text_content(Some(&format!(
                    "red: {}; blue: {}",
                    teams[Team::Red].join(", "),
                    teams[Team::Blue].join(", "),
                )));
            },
            ContestState::Game{ ref game_confirmed, ref local_turn, game_start } => {
                let my_name = self.state.my_name();
                let now = Instant::now();
                let _game_now = GameInstant::from_maybe_active_game(*game_start, now).approximate();
                let game = game_local(my_name, game_confirmed, local_turn);
                // TODO: Show clock and player names; show reserve
                for (board_idx, board) in game.boards() {
                    let is_primary = board_idx == self.primary_board.unwrap();
                    let web_board_idx = if is_primary { WebBoard::Primary } else { WebBoard::Secondary };
                    let grid = board.grid();
                    for coord in Coord::all() {
                        let sq = document.get_element_by_id(&square_id(web_board_idx, coord)).unwrap();
                        sq.set_text_content(Some(&String::from(to_unicode_char(&grid[coord]))));
                    }
                }
                info_string.set_text_content(None);
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WebBoard {
    Primary,
    Secondary,
}

#[derive(Debug)]
struct Square {
    classes: Vec<String>,
    id: Option<String>,
    text: Option<String>,
}

fn web_document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn remove_all_children(node: &web_sys::Node) -> Result<(), JsValue> {
    while let Some(child) = node.last_child() {
        node.remove_child(&child)?;
    }
    Ok(())
}

#[wasm_bindgen]
pub fn init_page() {
    render_game_area(false);
}

fn render_game_area(flip_forces: bool) {
    let document = web_document();
    let board_primary = document.get_element_by_id("board-primary").unwrap();
    let board_secondary = document.get_element_by_id("board-secondary").unwrap();
    render_grid(&board_primary, WebBoard::Primary, flip_forces).unwrap();
    render_grid(&board_secondary, WebBoard::Secondary, flip_forces).unwrap();
}

fn render_grid(node: &web_sys::Node, board_idx: WebBoard, flip_forces: bool) -> Result<(), JsValue> {
    let document = web_document();
    remove_all_children(node)?;
    let table = document.create_element("table")?;
    for line in make_grid(board_idx, flip_forces) {
        let tr = document.create_element("tr")?;
        for sq in line {
            let td = document.create_element("td")?;
            td.set_attribute("class", &sq.classes.join(" "))?;
            if let Some(id) = sq.id {
                td.set_attribute("id", &id)?;
            }
            td.set_text_content(sq.text.as_deref());
            tr.append_child(&td)?;
        }
        table.append_child(&tr)?;
    }
    node.append_child(&table)?;
    Ok(())
}

fn make_grid(board_idx: WebBoard, mut flip_forces: bool) -> Vec<Vec<Square>> {
    match board_idx {
        WebBoard::Primary => {},
        WebBoard::Secondary => flip_forces = !flip_forces,
    };
    let rows = match flip_forces {
        false => [vec![None], Row::all().rev().map(|v| Some(v)).collect(), vec![None]].concat(),
        true => [vec![None], Row::all().map(|v| Some(v)).collect(), vec![None]].concat(),
    };
    let cols = [vec![None], Col::all().map(|v| Some(v)).collect(), vec![None]].concat();
    rows.iter().map(|&row| {
        cols.iter().map(|&col| {
            match (row, col) {
                (Some(row), Some(col)) => Square {
                    classes: vec![square_color_class(row, col), inner_square_size_class(board_idx)],
                    id: Some(square_id(board_idx, Coord::new(row, col))),
                    text: None,
                },
                (Some(row), None) => Square {
                    classes: vec!["sq-side".to_owned(), outer_square_size_class(board_idx)],
                    id: None,
                    text: Some(String::from(row.to_algebraic())),
                },
                (None, Some(col)) => Square {
                    classes: vec!["sq-side".to_owned(), outer_square_size_class(board_idx)],
                    id: None,
                    text: Some(String::from(col.to_algebraic())),
                },
                (None, None) => Square {
                    classes: vec!["sq-corner".to_owned(), outer_square_size_class(board_idx)],
                    id: None,
                    text: None,
                },
            }
        }).collect()
    }).collect()
}

fn square_id(board_idx: WebBoard, coord: Coord) -> String {
    let prefix = match board_idx {
        WebBoard::Primary => "primary",
        WebBoard::Secondary => "secondary",
    };
    format!("{}-{}", prefix, coord.to_algebraic())
}

fn square_color_class(row: Row, col: Col) -> String {
    if (row.to_zero_based() + col.to_zero_based()) % 2 == 0 {
        "sq-black".to_owned()
    } else {
        "sq-white".to_owned()
    }
}

fn inner_square_size_class(board_idx: WebBoard) -> String {
    match board_idx {
        WebBoard::Primary => "sq-in-primary".to_owned(),
        WebBoard::Secondary => "sq-in-secondary".to_owned(),
    }
}

fn outer_square_size_class(board_idx: WebBoard) -> String {
    match board_idx {
        WebBoard::Primary => "sq-out-primary".to_owned(),
        WebBoard::Secondary => "sq-out-secondary".to_owned(),
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
