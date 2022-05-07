// TODO: Shrink WASM file size.
// TODO: Consider: stop using websys at all, do all DOM manipulations in JS.
// TODO: Some additional indication that it's your turn outside of clock area.
//   Maybe even change background (add vignette) or something like that.
//   Better yet: subtler change by default and throbbing vignette on low time
//   like in action games!
// TODO: Sounds.

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


type JsResult<T> = Result<T, JsValue>;

static mut PIECE_PATH: Option<EnumMap<Force, EnumMap<PieceKind, String>>> = None;

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
    // TODO: Consider: in order to store additional information that is only relevant
    //   during game phase, add a generic `UserData` parameter to `ContestState::Game`.
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
    pub fn next_game(&mut self) {
        self.state.next_game();
    }
    pub fn leave(&mut self) {
        self.state.leave();
    }
    pub fn reset(&mut self) {
        self.state.reset();
    }
    pub fn make_turn_algebraic(&mut self, turn_algebraic: String) {
        let turn_result = self.state.make_turn(turn_algebraic);
        let info_string = web_document().get_existing_element_by_id("info-string").unwrap();
        info_string.set_text_content(turn_result.err().map(|err| format!("{:?}", err)).as_deref());
    }
    pub fn make_turn_drag_drop(&mut self, from: &str, to: &str, alternative_promotion: bool)
        -> JsResult<()>
    {
        if let ContestState::Game{ game_confirmed, local_turn, .. } = self.state.contest_state() {
            use PieceKind::*;
            let my_name = self.state.my_name();
            let game = game_local(my_name, game_confirmed, local_turn);
            if let Some(piece) = from.strip_prefix("reserve-") {
                self.make_turn_algebraic(format!("{}@{}", piece, to));
            } else {
                let (_, my_force) = game.find_player(my_name).unwrap();
                let board = game.player_board(my_name).unwrap();
                let from_coord = Coord::from_algebraic(from);
                let to_coord = Coord::from_algebraic(to);
                if let Some(piece) = board.grid()[from_coord] {
                    let last_row = SubjectiveRow::from_one_based(8).to_row(my_force);
                    let d_col = to_coord.col - from_coord.col;
                    let d_col_abs = d_col.abs();
                    let to_my_piece = if let Some(piece_to) = board.grid()[to_coord] {
                        piece_to.force == my_force
                    } else {
                        false
                    };
                    // Castling rules: drag the king at least two squares in the rook direction
                    // or onto a friendly piece. That later is required for Fischer random where
                    // a king could start on b1 or g1.
                    if piece.kind == King && (d_col_abs >= 2 || (d_col_abs >= 1 && to_my_piece)) {
                        if d_col > 0 {
                            self.make_turn_algebraic("0-0".to_owned());
                        } else {
                            self.make_turn_algebraic("0-0-0".to_owned());
                        }
                    } else if piece.kind == Pawn && to_coord.row == last_row {
                        let promote_to = if alternative_promotion { Knight } else { Queen };
                        let promotion_str = piece_notation(promote_to);
                        self.make_turn_algebraic(format!("{}{}/{}", from, to, promotion_str));
                    } else {
                        let piece_str = piece_notation(piece.kind);
                        self.make_turn_algebraic(format!("{}{}{}", piece_str, from, to));
                    }
                } else {
                    return Err(JsValue::from("Cannot make turn: no piece in the starting position"))
                }
            }
        } else {
            return Err(JsValue::from("Cannot make turn: no game in progress"))
        }
        Ok(())
    }

    pub fn process_server_event(&mut self, event: &str) -> JsResult<Option<String>> {
        let server_event = serde_json::from_str(event).unwrap();
        let notable_event = self.state.process_server_event(server_event).map_err(|err| {
            JsValue::from(format!("{:?}", err))
        })?;
        match notable_event {
            NotableEvent::None => {
                Ok(None)
            },
            NotableEvent::GameStarted => {
                if let ContestState::Game{ ref game_confirmed, .. } = self.state.contest_state() {
                    let info_string = web_document().get_existing_element_by_id("info-string").unwrap();
                    info_string.set_text_content(None);
                    let my_name = self.state.my_name();
                    let (_, force) = game_confirmed.find_player(my_name).unwrap();
                    render_grids(force == Force::Black);
                    Ok(Some("game_started".to_owned()))
                } else {
                    Err("No game in progress".into())
                }
            }
            NotableEvent::OpponentTurnMade(_) => {
                if let ContestState::Game{ .. } = self.state.contest_state() {
                    // TODO: Highlight last turn
                    return Ok(Some("opponent_turn_made".to_owned()));
                } else {
                    Err("No game in progress".into())
                }
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

    // TODO: Check exception passing and return `JsResult<()>`.
    pub fn update_state(&self) {
        let document = web_document();
        let info_string = document.get_existing_element_by_id("info-string").unwrap();
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
                // TODO: Reset boards, clock, etc.
            },
            ContestState::Game{ scores, game_confirmed, local_turn, .. } => {
                let my_name = self.state.my_name();
                let game = game_local(my_name, game_confirmed, local_turn);
                let (my_board_idx, my_force) = game.find_player(my_name).unwrap();
                for (board_idx, board) in game.boards() {
                    let is_primary = board_idx == my_board_idx;
                    let web_board_idx = if is_primary { WebBoard::Primary } else { WebBoard::Secondary };
                    let grid = board.grid();
                    for coord in Coord::all() {
                        // This is potentially quite slow, but should't spend too much time on it:
                        //   we'll have to switch to canvas anyway.
                        let node = document.get_existing_element_by_id(&piece_id(web_board_idx, coord)).unwrap();
                        let piece = grid[coord];
                        let draggable = is_primary && piece.map(|p| p.force) == Some(my_force);
                        if let Some(piece) = piece {
                            let filename = piece_path(piece.kind, piece.force);
                            node.set_attribute("src", &filename).unwrap();
                        } else {
                            node.set_attribute("src", transparent_1x1_image()).unwrap();
                        }
                        if draggable {
                            node.set_attribute("draggable", "true").unwrap();
                        } else {
                            node.set_attribute("draggable", "false").unwrap();
                        }
                    }
                    for player_idx in WebPlayer::iter() {
                        use WebPlayer::*;
                        use WebBoard::*;
                        let force = match (player_idx, web_board_idx) {
                            (Bottom, Primary) | (Top, Secondary) => my_force,
                            (Top, Primary) | (Bottom, Secondary) => my_force.opponent(),
                        };
                        let id_suffix = format!("{}-{}", board_id(web_board_idx), player_id(player_idx));
                        let name_node = document.get_existing_element_by_id(&format!("player-name-{}", id_suffix)).unwrap();
                        name_node.set_text_content(Some(&board.player(force).name));
                        let reserve_node = document.get_existing_element_by_id(&format!("reserve-{}", id_suffix)).unwrap();
                        let is_me = is_primary && force == my_force;
                        update_reserve(board.reserve(force), force, web_board_idx, is_me, &reserve_node).unwrap();
                    }
                }
                if game_confirmed.status() != BughouseGameStatus::Active {
                    info_string.set_text_content(Some(&format!("Game over: {:?}", game_confirmed.status())));
                }
                update_scores(&scores, self.state.my_team()).unwrap();
            },
        }
        self.update_clock();
    }

    pub fn update_clock(&self) {
        let document = web_document();
        if let ContestState::Game{ game_confirmed, local_turn, time_pair, .. } = self.state.contest_state() {
            let now = Instant::now();
            let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
            let my_name = self.state.my_name();
            let game = game_local(my_name, game_confirmed, local_turn);
            let (my_board_idx, my_force) = game.find_player(my_name).unwrap();
            for (board_idx, board) in game.boards() {
                let is_primary = board_idx == my_board_idx;
                let web_board_idx = if is_primary { WebBoard::Primary } else { WebBoard::Secondary };
                for player_idx in WebPlayer::iter() {
                    use WebPlayer::*;
                    use WebBoard::*;
                    let force = match (player_idx, web_board_idx) {
                        (Bottom, Primary) | (Top, Secondary) => my_force,
                        (Top, Primary) | (Bottom, Secondary) => my_force.opponent(),
                    };
                    let id_suffix = format!("{}-{}", board_id(web_board_idx), player_id(player_idx));
                    // TODO: Dedup against `update_state`. Everything except the two lines below
                    //   is copy-pasted from there.
                    let clock_node = document.get_existing_element_by_id(&format!("clock-{}", id_suffix)).unwrap();
                    update_clock(board.clock(), force, game_now, &clock_node).unwrap();
                }
            }
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

struct WebDocument(web_sys::Document);

impl WebDocument {
    fn get_existing_element_by_id(&self, element_id: &str) -> JsResult<web_sys::Element> {
        let element = self.0.get_element_by_id(element_id).ok_or_else(|| JsValue::from(format!(
            "Cannot find element \"{}\"", element_id
        )))?;
        if !element.is_object() {
            return Err(JsValue::from(format!("Element \"{}\" is not an object", element_id)));
        }
        Ok(element)
    }

    pub fn create_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        self.0.create_element(local_name)
    }
}

fn web_document() -> WebDocument {
    WebDocument(web_sys::window().unwrap().document().unwrap())
}

fn remove_all_children(node: &web_sys::Node) -> JsResult<()> {
    while let Some(child) = node.last_child() {
        node.remove_child(&child)?;
    }
    Ok(())
}

#[wasm_bindgen]
pub fn init_page(
    white_pawn: String,
    white_knight: String,
    white_bishop: String,
    white_rook: String,
    white_queen: String,
    white_king: String,
    black_pawn: String,
    black_knight: String,
    black_bishop: String,
    black_rook: String,
    black_queen: String,
    black_king: String,
) {
    use Force::*;
    use PieceKind::*;
    let mut piece_path = enum_map!{
        _ => enum_map!{ _ => String::new() }
    };
    piece_path[White][Pawn] = white_pawn;
    piece_path[White][Knight] = white_knight;
    piece_path[White][Bishop] = white_bishop;
    piece_path[White][Rook] = white_rook;
    piece_path[White][Queen] = white_queen;
    piece_path[White][King] = white_king;
    piece_path[Black][Pawn] = black_pawn;
    piece_path[Black][Knight] = black_knight;
    piece_path[Black][Bishop] = black_bishop;
    piece_path[Black][Rook] = black_rook;
    piece_path[Black][Queen] = black_queen;
    piece_path[Black][King] = black_king;
    unsafe {
        PIECE_PATH = Some(piece_path);
    }
    render_grids(false);
}

fn update_reserve(reserve: &Reserve, force: Force, board_idx: WebBoard,
    is_me: bool, reserve_node: &web_sys::Element)
    -> JsResult<()>
{
    let document = web_document();
    let class_base = format!("reserve-piece-{}", board_id(board_idx));
    remove_all_children(reserve_node)?;
    let add_separator = || -> JsResult<()> {
        let sep = document.create_element("div")?;
        sep.set_attribute("class", &format!("{}-separator", class_base))?;
        reserve_node.append_child(&sep)?;
        Ok(())
    };
    let mut need_separator = false;
    for (piece_kind, &amount) in reserve.iter() {
        if need_separator {
            add_separator()?;
            need_separator = false;
        }
        let filename = piece_path(piece_kind, force);
        for _ in 0..amount {
            need_separator = true;
            let wrapper = document.create_element("div")?;
            wrapper.set_attribute("class", &format!("{}-wrapper", class_base))?;
            let img = document.create_element("img")?;
            img.set_attribute("src", &filename)?;
            img.set_attribute("class", &class_base)?;
            if is_me {
                img.set_attribute("draggable", "true").unwrap();
                img.set_attribute("data-piece-kind", &reserve_piece_notation(piece_kind)).unwrap();
            }
            wrapper.append_child(&img)?;
            reserve_node.append_child(&wrapper)?;
        }
    }
    if need_separator {
        add_separator()?;
    }
    Ok(())
}

fn div_ceil(a: u128, b: u128) -> u128 { (a + b - 1) / b }

// TODO: Dedup against console client
fn update_clock(clock: &Clock, force: Force, now: GameInstant, clock_node: &web_sys::Element)
    -> JsResult<()>
{
    let is_active = clock.active_force() == Some(force);
    let millis = clock.time_left(force, now).as_millis();
    let sec = millis / 1000;
    let separator = |s| if !is_active || millis % 1000 >= 500 { s } else { " " };
    let low_time = sec < 20;
    let clock_str = if low_time {
        format!("{:02}{}{}", sec, separator("."), div_ceil(millis, 100) % 10)
    } else {
        format!("{:02}{}{:02}", sec / 60, separator(":"), sec % 60)
    };
    clock_node.set_text_content(Some(&clock_str));
    let mut classes = vec!["clock"];
    if !is_active && millis == 0 {
        // Note. When the game is over, all clocks stop, so no player is active.
        // An active player can have zero time only in an online game client.
        // In this case we shouldn't signal flag defeat before the server confirmed
        // game result, because the game may have ended earlier on the other board.
        classes.push("clock-flag");
    } else {
        classes.push(if is_active { "clock-active" } else { "clock-inactive" });
        if low_time {
            classes.push("clock-low-time");
        }
    }
    clock_node.set_attribute("class", &classes.join(" "))?;
    Ok(())
}

fn update_scores(scores: &EnumMap<Team, u32>, my_team: Team) -> JsResult<()> {
    let scores_normalized = scores.map(|_, v| (v as f64) / 2.0);
    let score_node = web_document().get_existing_element_by_id("score")?;
    score_node.set_text_content(Some(&format!(
        "{}\n⎯\n{}", scores_normalized[my_team.opponent()], scores_normalized[my_team]
    )));
    Ok(())
}

fn render_grids(flip_forces: bool) {
    for board_idx in WebBoard::iter() {
        render_grid(board_idx, flip_forces).unwrap();
    }
}

// TODO: Embed notation inside the squares.
fn render_grid(board_idx: WebBoard, mut flip_forces: bool) -> JsResult<()> {
    match board_idx {
        WebBoard::Primary => {},
        WebBoard::Secondary => flip_forces = !flip_forces,
    };
    let rows = match flip_forces {
        false => Row::all().rev().collect_vec(),
        true => Row::all().collect_vec(),
    };

    let document = web_document();
    let node_id = format!("board-container-{}", board_id(board_idx));
    let node = document.get_existing_element_by_id(&node_id)?;
    remove_all_children(&node)?;

    let table = document.create_element("table")?;
    table.set_attribute("id", &format!("board-{}", board_id(board_idx)))?;

    for row in rows {
        let tr = document.create_element("tr")?;
        for col in Col::all() {
            let coord = Coord::new(row, col);
            let td = document.create_element("td")?;
            let classes = [square_color_class(row, col), square_size_class(board_idx)];
            td.set_attribute("class", &classes.join(" "))?;
            let img = document.create_element("img")?;
            img.set_attribute("class", "stretch board-piece")?;
            img.set_attribute("id", &piece_id(board_idx, coord))?;
            img.set_attribute("src", transparent_1x1_image())?;
            td.append_child(&img)?;
            tr.append_child(&td)?;
        }
        table.append_child(&tr)?;
    }
    node.append_child(&table)?;
    Ok(())
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

fn piece_id(board_idx: WebBoard, coord: Coord) -> String {
    format!("{}-{}", board_id(board_idx), coord.to_algebraic())
}

fn square_color_class(row: Row, col: Col) -> String {
    if (row.to_zero_based() + col.to_zero_based()) % 2 == 0 {
        "sq-black".to_owned()
    } else {
        "sq-white".to_owned()
    }
}

fn square_size_class(board_idx: WebBoard) -> String {
    format!("sq-{}", board_id(board_idx))
}

fn piece_path(piece_kind: PieceKind, force: Force) -> String {
    unsafe {
        return PIECE_PATH.as_ref().unwrap()[force][piece_kind].clone();
    }
}

fn piece_notation(piece_kind: PieceKind) -> &'static str {
    use self::PieceKind::*;
    match piece_kind {
        Pawn => "",
        Knight => "N",
        Bishop => "B",
        Rook => "R",
        Queen => "Q",
        King => "K",
    }
}

fn reserve_piece_notation(piece_kind: PieceKind) -> &'static str {
    use self::PieceKind::*;
    match piece_kind {
        Pawn => "P",
        Knight => "N",
        Bishop => "B",
        Rook => "R",
        Queen => "Q",
        King => panic!("There should be no kings in reserve"),
    }
}

fn transparent_1x1_image() -> &'static str {
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII="
    // semi-transparent blue alternative for testing
    // "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
}
