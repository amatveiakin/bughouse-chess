// TODO: Shrink WASM file size.

extern crate enum_map;
extern crate instant;
extern crate itertools;
extern crate serde_json;
extern crate strum;
extern crate wasm_bindgen;

extern crate bughouse_chess;

use std::sync::mpsc;

use enum_map::{EnumMap, enum_map};
use instant::Instant;
use itertools::Itertools;
use strum::{EnumIter, IntoEnumIterator};
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
        let game_was_active = matches!(self.state.contest_state(), ContestState::Game{ .. });
        self.state.process_server_event(serde_json::from_str(event).unwrap()).unwrap();
        if let ContestState::Game{ ref game_confirmed, .. } = self.state.contest_state() {
            if !game_was_active {
                let my_name = self.state.my_name();
                let (_, force) = game_confirmed.find_player(my_name).unwrap();
                render_grids(force == Force::Black);
                render_players();
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
            ContestState::Lobby{ players } => {
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
            ContestState::Game{ game_confirmed, local_turn, time_pair } => {
                let my_name = self.state.my_name();
                let now = Instant::now();
                let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
                let game = game_local(my_name, game_confirmed, local_turn);
                let (my_board_idx, my_force) = game.find_player(my_name).unwrap();
                for (board_idx, board) in game.boards() {
                    let is_primary = board_idx == my_board_idx;
                    let web_board_idx = if is_primary { WebBoard::Primary } else { WebBoard::Secondary };
                    let grid = board.grid();
                    for coord in Coord::all() {
                        let sq = document.get_element_by_id(&square_id(web_board_idx, coord)).unwrap();
                        let piece_str = match grid[coord] {
                            Some(piece) => String::from(to_unicode_char(piece.kind, piece.force)),
                            None => String::new(),
                        };
                        sq.set_text_content(Some(&piece_str));
                    }
                    for player_idx in WebPlayer::iter() {
                        use WebPlayer::*;
                        use WebBoard::*;
                        let force = match (player_idx, web_board_idx) {
                            (Bottom, Primary) | (Top, Secondary) => my_force,
                            (Top, Primary) | (Bottom, Secondary) => my_force.opponent(),
                        };
                        let id_suffix = format!("{}-{}", board_id(web_board_idx), player_id(player_idx));
                        let name_node = document.get_element_by_id(&format!("player-name-{}", id_suffix)).unwrap();
                        name_node.set_text_content(Some(&board.player(force).name));
                        let reserve_node = document.get_element_by_id(&format!("reserve-{}", id_suffix)).unwrap();
                        reserve_node.set_text_content(Some(&reserve_string(board.reserve(force), force)));
                        let clock_node = document.get_element_by_id(&format!("clock-{}", id_suffix)).unwrap();
                        update_clock(board.clock(), force, game_now, &clock_node).unwrap();
                    }
                }
                if game_confirmed.status() != BughouseGameStatus::Active {
                    info_string.set_text_content(Some(&format!("Game over: {:?}", game_confirmed.status())));
                } else {
                    info_string.set_text_content(None);
                }
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
enum WebBoard {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
enum WebPlayer {
    Top,
    Bottom,
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
    render_grids(false);
}

fn reserve_string(reserve: &Reserve, force: Force) -> String {
    let mut stacks = Vec::new();
    for (piece_kind, &amount) in reserve.iter() {
        if amount > 0 {
            stacks.push(String::from(to_unicode_char(piece_kind, force)).repeat(amount.into()));
        }
    }
    stacks.iter().join(" ")
}

fn div_ceil(a: u128, b: u128) -> u128 { (a + b - 1) / b }

// TODO: Dedup against console client
fn update_clock(clock: &Clock, force: Force, now: GameInstant, clock_node: &web_sys::Element) -> Result<(), JsValue> {
    let is_active = clock.active_force() == Some(force);
    let millis = clock.time_left(force, now).as_millis();
    let sec = millis / 1000;
    let separator = |s| if !is_active || millis % 1000 >= 500 { s } else { " " };
    let clock_str = if sec >= 20 {
        format!("{:02}{}{:02}", sec / 60, separator(":"), sec % 60)
    } else {
        format!("{:02}{}{}", sec, separator("."), div_ceil(millis, 100) % 10)
    };
    clock_node.set_text_content(Some(&clock_str));
    if is_active {
        clock_node.set_attribute("class", "clock clock-active")?;
    } else if millis == 0 {
        // Note. This will not apply to an active player, which is by design.
        // When the game is over, all clocks stop, so no player is active.
        // An active player can have zero time only in an online game client.
        // In this case we shouldn't paint the clock red (which means defeat)
        // before the server confirmed game result, because the game may have
        // ended earlier on the other board.
        clock_node.set_attribute("class", "clock clock-flag")?;
    } else {
        clock_node.set_attribute("class", "clock")?;
    }
    Ok(())
}

fn render_players() {
    for board_idx in WebBoard::iter() {
        for player_idx in WebPlayer::iter() {
            render_player(board_idx, player_idx).unwrap();
        }
    }
}

fn render_player(board_idx: WebBoard, player_idx: WebPlayer) -> Result<(), JsValue> {
    let document = web_document();
    let id_suffix = format!("{}-{}", board_id(board_idx), player_id(player_idx));
    let node_id = format!("player-{}", id_suffix);
    let node = document.get_element_by_id(&node_id).unwrap();
    remove_all_children(&node)?;

    let table = document.create_element("table")?;
    table.set_attribute("class", "player-view")?;
    let tr = document.create_element("tr")?;
    for obj in ["player-name", "reserve", "clock"] {
        let td = document.create_element("td")?;
        td.set_attribute("id", &format!("{}-{}", obj, id_suffix))?;
        td.set_attribute("class", obj)?;
        tr.append_child(&td)?;
    }
    table.append_child(&tr)?;
    node.append_child(&table)?;

    Ok(())
}

fn render_grids(flip_forces: bool) {
    for board_idx in WebBoard::iter() {
        render_grid(board_idx, flip_forces).unwrap();
    }
}

fn render_grid(board_idx: WebBoard, flip_forces: bool) -> Result<(), JsValue> {
    let document = web_document();
    let node_id = format!("board-{}", board_id(board_idx));
    let node = document.get_element_by_id(&node_id).unwrap();
    remove_all_children(&node)?;

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

fn board_id(idx: WebBoard) -> String {
    match idx {
        WebBoard::Primary => "primary",
        WebBoard::Secondary => "secondary",
    }.to_owned()
}

fn player_id(idx: WebPlayer) -> String {
    match idx {
        WebPlayer::Top => "top",
        WebPlayer::Bottom => "bottom",
    }.to_owned()
}

fn square_id(board_idx: WebBoard, coord: Coord) -> String {
    format!("{}-{}", board_id(board_idx), coord.to_algebraic())
}

fn square_color_class(row: Row, col: Col) -> String {
    if (row.to_zero_based() + col.to_zero_based()) % 2 == 0 {
        "sq-black".to_owned()
    } else {
        "sq-white".to_owned()
    }
}

fn inner_square_size_class(board_idx: WebBoard) -> String {
    format!("sq-in-{}", board_id(board_idx))
}

fn outer_square_size_class(board_idx: WebBoard) -> String {
    format!("sq-out-{}", board_id(board_idx))
}

fn to_unicode_char(piece_kind: PieceKind, force: Force) -> char {
    use self::PieceKind::*;
    use self::Force::*;
    match (force, piece_kind) {
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
    }
}
