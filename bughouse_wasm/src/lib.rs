// TODO: Shrink WASM file size.
// TODO: Consider: stop using websys at all, do all DOM manipulations in JS.
// TODO: Some additional indication that it's your turn outside of clock area.
//   Maybe even change background (add vignette) or something like that.
//   Better yet: subtler change by default and throbbing vignette on low time
//   like in action games!
// TODO: Sounds.

extern crate enum_map;
extern crate instant;
extern crate serde_json;
extern crate strum;
extern crate wasm_bindgen;

extern crate bughouse_chess;

use std::sync::mpsc;

use enum_map::{EnumMap, enum_map};
use instant::Instant;
use strum::{EnumIter, IntoEnumIterator};
use wasm_bindgen::prelude::*;

use bughouse_chess::*;
use bughouse_chess::client::*;


type JsResult<T> = Result<T, JsValue>;

const RESERVE_HEIGHT: f64 = 1.5;  // in squares
const BOARD_TOP: f64 = RESERVE_HEIGHT;
const BOARD_BOTTOM: f64 = BOARD_TOP + NUM_ROWS as f64;
// TODO: Viewbox size assert.

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
    rotate_boards: bool,
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
            rotate_boards: false,
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
    pub fn make_turn_drag_drop(&mut self, from: &str, to_x: f64, to_y: f64, alternative_promotion: bool)
        -> JsResult<()>
    {
        if let Some(to_display) = position_to_square(to_x, to_y) {
            if let ContestState::Game{ game_confirmed, local_turn, .. } = self.state.contest_state() {
                use PieceKind::*;
                let my_name = self.state.my_name();
                let game = game_local(my_name, game_confirmed, local_turn);
                let board_orientation = get_board_orientation(WebBoard::Primary, self.rotate_boards);
                let to_coord = from_display_coord(to_display, board_orientation);
                let to = to_coord.to_algebraic();
                if let Some(piece) = from.strip_prefix("reserve-") {
                    self.make_turn_algebraic(format!("{}@{}", piece, to));
                } else {
                    let (_, my_force) = game.find_player(my_name).unwrap();
                    let board = game.player_board(my_name).unwrap();
                    let from_coord = Coord::from_algebraic(from);
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
                    self.rotate_boards = force == Force::Black;
                    render_grids(self.rotate_boards);
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
                    let board_orientation = get_board_orientation(web_board_idx, self.rotate_boards);
                    let svg = document.get_existing_element_by_id(&board_node_id(web_board_idx)).unwrap();
                    let grid = board.grid();
                    for coord in Coord::all() {
                        let node_id = piece_id(web_board_idx, coord);
                        let node = document.get_element_by_id(&node_id);
                        let piece = grid[coord];
                        if let Some(piece) = piece {
                            let display_coord = to_display_coord(coord, board_orientation);
                            let node = node.unwrap_or_else(|| {
                                let node = make_piece_node(&node_id).unwrap();
                                svg.append_child(&node).unwrap();
                                node
                            });
                            // TODO: Re-create the image when a piece is captured.
                            //   Otherwise you are going to continue dragging enemy piece!
                            let filename = piece_path(piece.kind, piece.force);
                            let (x, y) = square_position(display_coord);
                            node.set_attribute("x", &x.to_string()).unwrap();
                            node.set_attribute("y", &y.to_string()).unwrap();
                            node.set_attribute("href", &filename).unwrap();
                            node.set_attribute("data-bughouse-location", &coord.to_algebraic()).unwrap();
                            let draggable = is_primary && piece.force == my_force;
                            if draggable {
                                node.set_attribute("class", "draggable").unwrap();
                            } else {
                                node.remove_attribute("class").unwrap();
                            }
                        } else {
                            if let Some(node) = node {
                                node.remove();
                            }
                        }
                    }
                    for player_idx in WebPlayer::iter() {
                        use WebPlayer::*;
                        use WebBoard::*;
                        let force = match (player_idx, web_board_idx) {
                            (Bottom, Primary) | (Top, Secondary) => my_force,
                            (Top, Primary) | (Bottom, Secondary) => my_force.opponent(),
                        };
                        let name_node = document.get_existing_element_by_id(
                            &player_name_node_id(web_board_idx, player_idx)
                        ).unwrap();
                        name_node.set_text_content(Some(&board.player(force).name));
                        update_reserve(board.reserve(force), force, web_board_idx, player_idx).unwrap();
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BoardOrientation {
    Normal,
    Rotated,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct DisplayCoord {
    x: u8,
    y: u8,
}

struct WebDocument(web_sys::Document);

impl WebDocument {
    fn get_element_by_id(&self, element_id: &str) -> Option<web_sys::Element> {
        self.0.get_element_by_id(element_id)
    }
    fn get_existing_element_by_id(&self, element_id: &str) -> JsResult<web_sys::Element> {
        let element = self.0.get_element_by_id(element_id).ok_or_else(|| JsValue::from(format!(
            "Cannot find element \"{}\"", element_id
        )))?;
        if !element.is_object() {
            return Err(JsValue::from(format!("Element \"{}\" is not an object", element_id)));
        }
        Ok(element)
    }

    pub fn create_svg_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        self.0.create_element_ns(Some("http://www.w3.org/2000/svg"), local_name)
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

fn update_reserve(reserve: &Reserve, force: Force, board_idx: WebBoard, player_idx: WebPlayer)
    -> JsResult<()>
{
    let is_me = (board_idx == WebBoard::Primary) && (player_idx == WebPlayer::Bottom);
    let document = web_document();
    let reserve_node = document.get_existing_element_by_id(&reserve_node_id(board_idx, player_idx)).unwrap();
    // TODO: What would this do if a reserve piece is being dragged?
    remove_all_children(&reserve_node)?;

    let num_piece: f64 = reserve.iter().map(|(_, &amount)| amount as f64).sum();
    let num_kind = reserve.iter().filter(|(_, &amount)| amount > 0).count() as f64;
    let max_width = NUM_COLS as f64;
    let kind_sep = 1.0;
    let total_kind_sep_width = kind_sep * (num_kind - 1.0);
    let piece_sep = f64::min(
        0.5,
        (max_width - total_kind_sep_width) / (num_piece - num_kind)
    );
    let width = total_kind_sep_width + (num_piece - num_kind) * piece_sep;

    let mut x = (max_width - width - 1.0) / 2.0;  // center reserve
    let y = reserve_y_pos(player_idx);
    for (piece_kind, &amount) in reserve.iter().filter(|(_, &amount)| amount > 0) {
        let filename = piece_path(piece_kind, force);
        let location = format!("reserve-{}", reserve_piece_notation(piece_kind));
        for iter in 0..amount {
            if iter > 0 {
                x += piece_sep;
            }
            let node = document.create_svg_element("image")?;
            node.set_attribute("href", &filename)?;
            node.set_attribute("data-bughouse-location", &location).unwrap();
            node.set_attribute("x", &x.to_string())?;
            node.set_attribute("y", &y.to_string())?;
            node.set_attribute("width", "1")?;
            node.set_attribute("height", "1")?;
            if is_me {
                node.set_attribute("class", "draggable").unwrap();
            }
            reserve_node.append_child(&node)?;
        }
        x += kind_sep;
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

fn render_grids(rotate_boards: bool) {
    for board_idx in WebBoard::iter() {
        render_grid(board_idx, rotate_boards).unwrap();
    }
}

fn render_grid(board_idx: WebBoard, rotate_boards: bool) -> JsResult<()> {
    let board_orientation = get_board_orientation(board_idx, rotate_boards);
    let text_h_padding = 0.07;
    let text_v_padding = 0.09;

    let make_board_rect = |document: &WebDocument| -> JsResult<web_sys::Element> {
        let rect = document.create_svg_element("rect")?;
        let (x, y) = square_position(DisplayCoord{ x: 0, y: 0 });
        rect.set_attribute("x", &x.to_string())?;
        rect.set_attribute("y", &y.to_string())?;
        rect.set_attribute("width", &NUM_COLS.to_string())?;
        rect.set_attribute("height", &NUM_ROWS.to_string())?;
        Ok(rect)
    };

    let document = web_document();
    let svg = document.get_existing_element_by_id(&board_node_id(board_idx))?;
    remove_all_children(&svg)?;

    let shadow = make_board_rect(&document)?;
    shadow.set_attribute("class", "board-shadow")?;
    svg.append_child(&shadow)?;

    for row in Row::all() {
        for col in Col::all() {
            let sq = document.create_svg_element("rect")?;
            let display_coord = to_display_coord(Coord::new(row, col), board_orientation);
            let (x, y) = square_position(display_coord);
            sq.set_attribute("x", &x.to_string())?;
            sq.set_attribute("y", &y.to_string())?;
            sq.set_attribute("width", "1")?;
            sq.set_attribute("height", "1")?;
            sq.set_attribute("class", &square_color_class(row, col))?;
            svg.append_child(&sq)?;
            if display_coord.x == 0 {
                let caption = document.create_svg_element("text")?;
                caption.set_text_content(Some(&String::from(row.to_algebraic())));
                caption.set_attribute("x", &(x + text_h_padding).to_string())?;
                caption.set_attribute("y", &(y + text_v_padding).to_string())?;
                caption.set_attribute("alignment-baseline", "hanging")?;
                caption.set_attribute("class", &square_text_color_class(row, col))?;
                svg.append_child(&caption)?;
            }
            if display_coord.y == NUM_ROWS - 1 {
                let caption = document.create_svg_element("text")?;
                caption.set_text_content(Some(&String::from(col.to_algebraic())));
                caption.set_attribute("x", &(x + 1.0 - text_h_padding).to_string())?;
                caption.set_attribute("y", &(y + 1.0 - text_v_padding).to_string())?;
                caption.set_attribute("text-anchor", "end")?;
                caption.set_attribute("class", &square_text_color_class(row, col))?;
                svg.append_child(&caption)?;
            }
        }
    }

    let border = make_board_rect(&document)?;
    border.set_attribute("class", "board-border")?;
    svg.append_child(&border)?;

    for player_idx in WebPlayer::iter() {
        let reserve = document.create_svg_element("g")?;
        reserve.set_attribute("id", &reserve_node_id(board_idx, player_idx))?;
        reserve.set_attribute("class", "reserve")?;
        svg.append_child(&reserve)?;
    }
    Ok(())
}

fn make_piece_node(id: &str) -> JsResult<web_sys::Element> {
    let node = web_document().create_svg_element("image")?;
    node.set_attribute("id", id)?;
    node.set_attribute("width", "1")?;
    node.set_attribute("height", "1")?;
    return Ok(node);
}

fn get_board_orientation(board_idx: WebBoard, rotate_180: bool) -> BoardOrientation {
    match (board_idx, rotate_180) {
        (WebBoard::Primary, false) | (WebBoard::Secondary, true) => BoardOrientation::Normal,
        (WebBoard::Primary, true) | (WebBoard::Secondary, false) => BoardOrientation::Rotated,
    }
}

fn to_display_coord(coord: Coord, board_orientation: BoardOrientation) -> DisplayCoord {
    match board_orientation {
        BoardOrientation::Normal => DisplayCoord {
            x: coord.col.to_zero_based(),
            y: NUM_ROWS - coord.row.to_zero_based() - 1,
        },
        BoardOrientation::Rotated => DisplayCoord {
            x: NUM_COLS - coord.col.to_zero_based() - 1,
            y: coord.row.to_zero_based(),
        },
    }
}

fn from_display_coord(coord: DisplayCoord, board_orientation: BoardOrientation) -> Coord {
    match board_orientation {
        BoardOrientation::Normal => Coord {
            row: Row::from_zero_based(NUM_ROWS - coord.y - 1),
            col: Col::from_zero_based(coord.x),
        },
        BoardOrientation::Rotated => Coord {
            row: Row::from_zero_based(coord.y),
            col: Col::from_zero_based(NUM_COLS - coord.x - 1),
        },
    }
}

// position of the top-left corner of a square
fn square_position(coord: DisplayCoord) -> (f64, f64) {
    return (
        f64::from(coord.x),
        f64::from(coord.y) + BOARD_TOP,
    );
}

fn position_to_square(x: f64, y: f64) -> Option<DisplayCoord> {
    let x = x as i32;
    let y = (y - BOARD_TOP) as i32;
    if 0 <= x && x < NUM_COLS as i32 && 0 <= y && y < NUM_ROWS as i32 {
        Some(DisplayCoord{ x: x.try_into().unwrap(), y: y.try_into().unwrap() })
    } else {
        None
    }
}

fn board_id(idx: WebBoard) -> String {
    match idx {
        WebBoard::Primary => "primary",
        WebBoard::Secondary => "secondary",
    }.to_owned()
}

fn board_node_id(idx: WebBoard) -> String {
    format!("board-{}", board_id(idx))
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

fn player_name_node_id(board_idx: WebBoard, player_idx: WebPlayer) -> String {
    format!("player-name-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn reserve_node_id(board_idx: WebBoard, player_idx: WebPlayer) -> String {
    format!("reserve-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn reserve_y_pos(player_idx: WebPlayer) -> f64 {
    let reserve_padding = (RESERVE_HEIGHT - 1.0) / 2.0;
    match player_idx {
        WebPlayer::Top => reserve_padding,
        WebPlayer::Bottom => BOARD_BOTTOM + reserve_padding,
    }
}

fn square_text_color_class(row: Row, col: Col) -> String {
    if (row.to_zero_based() + col.to_zero_based()) % 2 == 0 {
        "on-sq-black".to_owned()
    } else {
        "on-sq-white".to_owned()
    }
}

fn square_color_class(row: Row, col: Col) -> String {
    if (row.to_zero_based() + col.to_zero_based()) % 2 == 0 {
        "sq-black".to_owned()
    } else {
        "sq-white".to_owned()
    }
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
