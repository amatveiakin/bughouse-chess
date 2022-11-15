// TODO: Shrink WASM file size.
// TODO: Consider: stop using websys at all, do all DOM manipulations in JS.
// TODO: Some additional indication that it's your turn outside of clock area.
//   Maybe even change background (add vignette) or something like that.
//   Better yet: subtler change by default and throbbing vignette on low time
//   like in action games!

extern crate console_error_panic_hook;
extern crate enum_map;
extern crate instant;
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

const RESERVE_HEIGHT: f64 = 1.5;  // total reserve area height, in squares
const RESERVE_PADDING: f64 = 0.25;  // padding between board and reserve, in squares
const BOARD_LEFT: f64 = 0.0;
const BOARD_TOP: f64 = 0.0;
// TODO: Viewbox size asserts.

// Mutable singleton should be ok since the relevant code is single-threaded.
// TODO: Consider wrapping into once_cell or thread_local for better safety.
static mut PIECE_PATH: Option<EnumMap<Force, EnumMap<PieceKind, String>>> = None;
static mut LAST_PANIC: String = String::new();

// Copied from console_error_panic_hook
#[wasm_bindgen]
extern {
    type Error;
    #[wasm_bindgen(constructor)]
    fn new() -> Error;
    #[wasm_bindgen(structural, method, getter)]
    fn stack(error: &Error) -> String;
}

// Optimization potential: Remove or shrink the panic hook when the client is stable.
#[wasm_bindgen]
pub fn set_panic_hook() {
    use std::panic;
    use std::sync::Once;
    static SET_HOOK: Once = Once::new();
    SET_HOOK.call_once(|| {
        panic::set_hook(Box::new(|panic_info| {
            // Log to the browser developer console. For more details see
            // https://github.com/rustwasm/console_error_panic_hook#readme
            console_error_panic_hook::hook(panic_info);

            // Generate error report to be sent to the server.
            let js_error = Error::new();
            let backtrace = js_error.stack();
            let event = BughouseClientEvent::ReportError(BughouseClientErrorReport::RustPanic {
                panic_info: panic_info.to_string(),
                backtrace,
            });
            unsafe {
                LAST_PANIC = serde_json::to_string(&event).unwrap();
            }
        }));
    });
}

#[wasm_bindgen]
pub fn last_panic() -> String {
    unsafe {
        LAST_PANIC.clone()
    }
}

#[wasm_bindgen]
pub struct RustError {
    message: String,
}
#[wasm_bindgen]
impl RustError {
    pub fn message(&self) -> String { self.message.clone() }
}

macro_rules! rust_error {
    ($($arg:tt)*) => {
        JsValue::from(RustError{ message: format!($($arg)*) })
    };
}

#[wasm_bindgen]
pub fn make_rust_error_event(error: RustError) -> String {
    let event = BughouseClientEvent::ReportError(BughouseClientErrorReport::RustError{
        message: error.message
    });
    serde_json::to_string(&event).unwrap()
}

#[wasm_bindgen]
pub fn make_unknown_error_event(message: String) -> String {
    let event = BughouseClientEvent::ReportError(BughouseClientErrorReport::UnknownError { message });
    serde_json::to_string(&event).unwrap()
}

#[wasm_bindgen]
pub struct JsEventMyNoop {}  // in contrast to `null`, indicates that event list is not over

#[wasm_bindgen]
pub struct JsEventVictory {}

#[wasm_bindgen]
pub struct JsEventDefeat {}

#[wasm_bindgen]
pub struct JsEventDraw {}

#[wasm_bindgen]
pub struct JsEventTurnMade {}

#[wasm_bindgen]
pub struct JsEventMyReserveRestocked {}

#[wasm_bindgen]
pub struct JsEventLowTime {}

#[wasm_bindgen]
pub struct JsEventGameExportReady {
    content: String,
}
#[wasm_bindgen]
impl JsEventGameExportReady {
    pub fn content(&self) -> String { self.content.clone() }
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
    pub fn new_client(my_name: &str, my_team: &str) -> JsResult<WebClient> {
        let my_team = match my_team {
            "red" => Some(Team::Red),
            "blue" => Some(Team::Blue),
            "" => None,
            _ => { return Err(rust_error!("Unexpected team: {}", my_team)); }
        };
        let (server_tx, server_rx) = mpsc::channel();
        Ok(WebClient {
            state: ClientState::new(my_name.to_owned(), my_team, server_tx),
            server_rx,
            rotate_boards: false,
        })
    }

    pub fn join(&mut self) {
        self.state.join();
    }
    pub fn resign(&mut self) {
        self.state.resign();
    }
    pub fn toggle_ready(&mut self) {
        if let Some(is_ready) = self.state.is_ready() {
            self.state.set_ready(!is_ready);
        }
    }
    pub fn leave(&mut self) {
        self.state.leave();
    }
    pub fn reset(&mut self) {
        self.state.reset();
    }
    pub fn request_export(&mut self) -> JsResult<()> {
        let format = pgn::BughouseExportFormat{};
        self.state.request_export(format);
        Ok(())
    }

    fn make_turn(&mut self, turn_input: TurnInput) -> JsResult<()> {
        let turn_result = self.state.make_turn(turn_input);
        let info_string = web_document().get_existing_element_by_id("info-string")?;
        info_string.set_text_content(turn_result.as_ref().err().map(|err| format!("{:?}", err)).as_deref());
        Ok(())
    }

    pub fn make_turn_algebraic(&mut self, turn_algebraic: String) -> JsResult<()> {
        // if turn_algebraic == "panic" { panic!("Test panic!"); }
        // if turn_algebraic == "error" { return Err(rust_error!("Test Rust error")); }
        // if turn_algebraic == "bad" { return Err("Test unknown error".into()); }
        self.make_turn(TurnInput::Algebraic(turn_algebraic))
    }

    pub fn start_drag_piece(&mut self, source: &str) -> JsResult<()> {
        let Some(GameState{ ref mut alt_game, .. }) = self.state.game_state_mut() else {
            return Ok(());
        };
        let source = if let Some(piece) = source.strip_prefix("reserve-") {
            PieceDragStart::Reserve(PieceKind::from_algebraic(piece).unwrap())
        } else {
            let coord = Coord::from_algebraic(source);
            let board_orientation = get_board_orientation(WebBoard::Primary, self.rotate_boards);
            let display_coord = to_display_coord(coord, board_orientation);
            set_square_highlight("drag-start-highlight", WebBoard::Primary, Some(display_coord))?;
            PieceDragStart::Board(coord)
        };
        alt_game.start_drag_piece(source).map_err(|err| rust_error!("Drag&drop error: {:?}", err))?;
        Ok(())
    }
    pub fn drag_piece(&mut self, dest_x: f64, dest_y: f64) -> JsResult<()> {
        set_square_highlight("drag-over-highlight", WebBoard::Primary, position_to_square(dest_x, dest_y))
    }
    pub fn drag_piece_drop(&mut self, dest_x: f64, dest_y: f64, alternative_promotion: bool)
        -> JsResult<()>
    {
        let Some(GameState{ ref mut alt_game, .. }) = self.state.game_state_mut() else {
            return Ok(());
        };
        if let Some(dest_display) = position_to_square(dest_x, dest_y) {
            use PieceKind::*;
            let board_orientation = get_board_orientation(WebBoard::Primary, self.rotate_boards);
            let dest_coord = from_display_coord(dest_display, board_orientation);
            let promote_to = if alternative_promotion { Knight } else { Queen };
            match alt_game.drag_piece_drop(dest_coord, promote_to) {
                Ok(turn) => {
                    self.make_turn(turn)?;
                },
                Err(PieceDragError::DragNoLongerPossible) => {
                    // Ignore: this happen when dragged piece was captured by opponent.
                },
                Err(PieceDragError::Cancelled) => {
                    // Ignore: user cancelled the move by putting the piece back in place.
                },
                Err(err) => {
                    return Err(rust_error!("Drag&drop error: {:?}", err));
                },
            };
        } else {
            alt_game.abort_drag_piece();
        }
        Ok(())
    }
    pub fn abort_drag_piece(&mut self) -> JsResult<()> {
        if let Some(GameState{ ref mut alt_game, .. }) = self.state.game_state_mut() {
            alt_game.abort_drag_piece();
        }
        Ok(())
    }
    // Remove drag highlights. Should be called after drag_piece_drop/abort_drag_piece but
    // also in any case where a drag could become obsolete (e.g. dragged piece was captured
    // or it's position was reverted after the game finished).
    pub fn reset_drag_highlights(&self) -> JsResult<()> {
        reset_square_highlight("drag-start-highlight")?;
        reset_square_highlight("drag-over-highlight")?;
        Ok(())
    }
    pub fn drag_state(&self) -> String {
        (if let Some(GameState{ ref alt_game, .. }) = self.state.game_state() {
            if let Some(drag) = alt_game.piece_drag_state() {
                match drag.source {
                    PieceDragSource::Board(_) | PieceDragSource::Reserve => "yes",
                    PieceDragSource::Defunct => "defunct",
                }
            } else {
                "no"
            }
        } else {
            "no"
        }).to_owned()
    }

    pub fn cancel_preturn(&mut self) {
        self.state.cancel_preturn();
    }

    pub fn process_server_event(&mut self, event: &str) -> JsResult<()> {
        let server_event = serde_json::from_str(event).unwrap();
        self.state.process_server_event(server_event).map_err(|err| {
            rust_error!("{:?}", err)
        })
    }

    pub fn next_notable_event(&mut self) -> JsResult<JsValue> {
        match self.state.next_notable_event() {
            Some(NotableEvent::GameStarted) => {
                let Some(GameState{ ref mut alt_game, .. }) = self.state.game_state_mut() else {
                    return Err(rust_error!("No game in progress"));
                };
                let info_string = web_document().get_existing_element_by_id("info-string")?;
                info_string.set_text_content(None);
                let my_id = alt_game.my_id();
                let is_observer = matches!(my_id, BughouseParticipantId::Observer(_));
                self.rotate_boards = my_id.display_force() == Force::Black;
                render_grids(self.rotate_boards)?;
                setup_participation_mode(is_observer)?;
                Ok(JsEventMyNoop{}.into())
            },
            Some(NotableEvent::GameOver(game_status)) => {
                match game_status {
                    SubjectiveGameResult::Victory => Ok(JsEventVictory{}.into()),
                    SubjectiveGameResult::Defeat => Ok(JsEventDefeat{}.into()),
                    SubjectiveGameResult::Draw => Ok(JsEventDraw{}.into()),
                }
            },
            Some(NotableEvent::MyTurnMade) => Ok(JsEventTurnMade{}.into()),
            Some(NotableEvent::OpponentTurnMade) => Ok(JsEventTurnMade{}.into()),
            Some(NotableEvent::MyReserveRestocked) => Ok(JsEventMyReserveRestocked{}.into()),
            Some(NotableEvent::LowTime) => Ok(JsEventLowTime{}.into()),
            Some(NotableEvent::GameExportReady(content)) => Ok(JsEventGameExportReady{ content }.into()),
            None => Ok(JsValue::NULL),
        }
    }

    pub fn next_outgoing_event(&mut self) -> Option<String> {
        match self.server_rx.try_recv() {
            Ok(event) => Some(serde_json::to_string(&event).unwrap()),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => panic!("Event channel disconnected"),
        }
    }

    pub fn refresh(&mut self) {
        self.state.refresh();
    }

    pub fn update_state(&self) -> JsResult<()> {
        let document = web_document();
        let info_string = document.get_existing_element_by_id("info-string")?;
        self.update_clock()?;
        let Some(contest) = self.state.contest() else {
            return Ok(());
        };
        update_scores(&contest.scores, contest.teaming, self.state.my_team())?;
        let Some(GameState{ ref alt_game, .. }) = contest.game_state else {
            update_lobby(&contest)?;
            return Ok(());
        };
        // TODO: Better readiness status display.
        let game = alt_game.local_game();
        let my_id = alt_game.my_id();
        let my_display_board_idx = my_id.display_board_idx();
        let my_display_force = my_id.display_force();
        for (board_idx, board) in game.boards() {
            let is_primary = board_idx == my_display_board_idx;
            let web_board_idx = if is_primary { WebBoard::Primary } else { WebBoard::Secondary };
            let board_orientation = get_board_orientation(web_board_idx, self.rotate_boards);
            let svg = document.get_existing_element_by_id(&board_node_id(web_board_idx))?;
            let grid = board.grid();
            for coord in Coord::all() {
                let node_id = piece_id(web_board_idx, coord);
                let node = document.get_element_by_id(&node_id);
                let piece = grid[coord];
                if let Some(piece) = piece {
                    let display_coord = to_display_coord(coord, board_orientation);
                    let node = match node {
                        Some(v) => v,
                        None => {
                            let v = make_piece_node(&node_id)?;
                            svg.append_child(&v)?;
                            v
                        },
                    };
                    let filename = piece_path(piece.kind, piece.force);
                    let (x, y) = square_position(display_coord);
                    node.set_attribute("x", &x.to_string())?;
                    node.set_attribute("y", &y.to_string())?;
                    node.set_attribute("href", &filename)?;
                    node.set_attribute("data-bughouse-location", &coord.to_algebraic())?;
                    let mut draggable = false;
                    if let BughouseParticipantId::Player(my_player_id) = my_id {
                        draggable = is_primary && piece.force == my_player_id.force;
                    }
                    if draggable {
                        node.set_attribute("class", "draggable")?;
                    } else {
                        node.remove_attribute("class")?;
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
                    (Bottom, Primary) | (Top, Secondary) => my_display_force,
                    (Top, Primary) | (Bottom, Secondary) => my_display_force.opponent(),
                };
                let name_node = document.get_existing_element_by_id(
                    &player_name_node_id(web_board_idx, player_idx)
                )?;
                let player_name = &board.player(force).name;
                let player_string = if game.status() == BughouseGameStatus::Active {
                    player_name.clone()
                } else {
                    // TODO: Fix this on server side instead: send the full list of players even
                    //   if somebody went offline.
                    if let Some(player) = contest.players.iter().find(|p| p.name == *player_name) {
                        player_with_readiness_status(&player)
                    } else {
                        player_name.clone()
                    }
                };
                name_node.set_text_content(Some(&player_string));
                update_reserve(board.reserve(force), force, web_board_idx, player_idx)?;
            }
            let latest_turn = game.turn_log().iter().rev()
                .find(|record| record.player_id.board_idx == board_idx);
            {
                let latest_turn_highlight = latest_turn
                    .filter(|record| BughouseParticipantId::Player(record.player_id) != my_id)
                    .map(|record| &record.turn_expanded);
                let hightlight_id = format!("latest-{}", board_id(web_board_idx));
                self.set_turn_highlights(&hightlight_id, latest_turn_highlight, web_board_idx)?;
            }
            if web_board_idx == WebBoard::Primary {
                let pre_turn_highlight = latest_turn
                    .filter(|record| record.mode == TurnMode::Preturn)
                    .map(|record| &record.turn_expanded);
                self.set_turn_highlights("pre", pre_turn_highlight, web_board_idx)?;
            }
        }
        if alt_game.status() != BughouseGameStatus::Active {
            // TODO: Print "victory / defeat" instead of team color.
            info_string.set_text_content(Some(&format!("Game over: {:?}", alt_game.status())));
        }
        Ok(())
    }

    pub fn update_clock(&self) -> JsResult<()> {
        let document = web_document();
        let Some(GameState{ ref alt_game, time_pair, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let now = Instant::now();
        let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
        let game = alt_game.local_game();
        let my_id = alt_game.my_id();
        let my_display_board_idx = my_id.display_board_idx();
        let my_display_force = my_id.display_force();
        for (board_idx, board) in game.boards() {
            let is_primary = board_idx == my_display_board_idx;
            let web_board_idx = if is_primary { WebBoard::Primary } else { WebBoard::Secondary };
            for player_idx in WebPlayer::iter() {
                use WebPlayer::*;
                use WebBoard::*;
                let force = match (player_idx, web_board_idx) {
                    (Bottom, Primary) | (Top, Secondary) => my_display_force,
                    (Top, Primary) | (Bottom, Secondary) => my_display_force.opponent(),
                };
                let id_suffix = format!("{}-{}", board_id(web_board_idx), player_id(player_idx));
                // TODO: Dedup against `update_state`. Everything except the two lines below
                //   is copy-pasted from there.
                let clock_node = document.get_existing_element_by_id(&format!("clock-{}", id_suffix))?;
                update_clock(board.clock(), force, game_now, &clock_node)?;
            }
        }
        Ok(())
    }

    fn set_turn_highlights(&self, id_prefix: &str, turn: Option<&TurnExpanded>, board_idx: WebBoard)
        -> JsResult<()>
    {
        // Optimization potential: do not reset highlights that stay in place.
        reset_square_highlight(&format!("{}-turn-from", id_prefix))?;
        reset_square_highlight(&format!("{}-turn-to", id_prefix))?;
        reset_square_highlight(&format!("{}-turn-from-extra", id_prefix))?;
        reset_square_highlight(&format!("{}-turn-to-extra", id_prefix))?;
        reset_square_highlight(&format!("{}-drop-to", id_prefix))?;
        reset_square_highlight(&format!("{}-capture", id_prefix))?;
        let board_orientation = get_board_orientation(board_idx, self.rotate_boards);
        if let Some(turn) = turn {
            for (id_suffix, coord) in turn_highlights(turn) {
                let id = format!("{}-{}", id_prefix, id_suffix);
                set_square_highlight(&id, board_idx, Some(to_display_coord(coord, board_orientation)))?;
            }
        }
        Ok(())
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
    fn body(&self) -> JsResult<web_sys::HtmlElement> {
        self.0.body().ok_or_else(|| rust_error!("Cannot find document body"))
    }

    fn get_element_by_id(&self, element_id: &str) -> Option<web_sys::Element> {
        self.0.get_element_by_id(element_id)
    }
    fn get_existing_element_by_id(&self, element_id: &str) -> JsResult<web_sys::Element> {
        let element = self.0.get_element_by_id(element_id).ok_or_else(|| rust_error!(
            "Cannot find element \"{}\"", element_id
        ))?;
        if !element.is_object() {
            return Err(rust_error!("Element \"{}\" is not an object", element_id));
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
) -> JsResult<()> {
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
    render_grids(false)?;
    render_starting()?;
    Ok(())
}

fn update_lobby(contest: &Contest) -> JsResult<()> {
    let info_string = web_document().get_existing_element_by_id("info-string")?;
    // TODO: Show teams for the news game in individual mode.
    match contest.teaming {
        Teaming::FixedTeams => {
            let mut teams: EnumMap<Team, Vec<String>> = enum_map!{ _ => vec![] };
            for p in &contest.players {
                teams[p.fixed_team.unwrap()].push(player_with_readiness_status(p));
            }
            info_string.set_text_content(Some(&format!(
                "red:\n{}\nblue:\n{}",
                teams[Team::Red].join("\n"),
                teams[Team::Blue].join("\n"),
            )));
        },
        Teaming::IndividualMode => {
            info_string.set_text_content(Some(&contest.players.iter().map(|p| {
                assert!(p.fixed_team.is_none());
                player_with_readiness_status(p)
            }).join("\n")))
        },
    }
    // TODO: Reset boards, clock, etc.
    Ok(())
}

// Note. Each `id` should unambiguously correspond to a fixed board.
// TODO: Separate highlight layers based on z-order: put drag highlight above the rest.
fn set_square_highlight(id: &str, board_idx: WebBoard, coord: Option<DisplayCoord>) -> JsResult<()> {
    let document = web_document();
    if let Some(coord) = coord {
        let node = document.get_element_by_id(id);
        let highlight_layer = document.get_existing_element_by_id(&square_highlight_layer(board_idx))?;
        let node = node.ok_or(JsValue::UNDEFINED).or_else(|_| -> JsResult<web_sys::Element> {
            let node = document.create_svg_element("rect")?;
            node.set_attribute("id", id)?;
            node.set_attribute("width", "1")?;
            node.set_attribute("height", "1")?;
            highlight_layer.append_child(&node)?;
            Ok(node)
        })?;
        let (x, y) = square_position(coord);
        node.set_attribute("x", &x.to_string())?;
        node.set_attribute("y", &y.to_string())?;
    } else {
        reset_square_highlight(id)?;
    }
    Ok(())
}

fn reset_square_highlight(id: &str) -> JsResult<()> {
    let document = web_document();
    let node = document.get_element_by_id(id);
    if let Some(node) = node {
        node.remove();
    }
    Ok(())
}

fn player_with_readiness_status(p: &Player) -> String {
    format!(
        "{} {}",
        if p.is_ready { "☑" } else { "☐" },
        p.name
    )
}

// Renders reserve.
// Leaves space for missing piece kinds too. This makes reserve piece positions more or
// less fixed, thus reducing the chance of grabbing the wrong piece after a last-moment
// reserve update.
fn render_reserve(
    force: Force, board_idx: WebBoard, player_idx: WebPlayer, draggable: bool,
    piece_kind_sep: f64, reserve_iter: impl Iterator<Item = (PieceKind, u8)> + Clone
) -> JsResult<()>
{
    let document = web_document();
    let reserve_node = document.get_existing_element_by_id(&reserve_node_id(board_idx, player_idx))?;
    // Does not interfere with dragging a reserve piece, because dragged piece is re-parented
    // to board SVG.
    remove_all_children(&reserve_node)?;

    let num_piece: u8 = reserve_iter.clone().map(|(_, amount)| amount).sum();
    if num_piece == 0 {
        return Ok(());
    }
    let num_piece = num_piece as f64;
    let num_kind = reserve_iter.clone().count() as f64;
    let num_nonempty_kind = reserve_iter.clone().filter(|&(_, amount)| amount > 0).count() as f64;
    let max_width = NUM_COLS as f64;
    let total_kind_sep_width = piece_kind_sep * (num_kind - 1.0);
    let piece_sep = f64::min(
        0.5,
        (max_width - total_kind_sep_width) / (num_piece - num_nonempty_kind)
    );
    assert!(piece_sep > 0.0, "{:?}", reserve_iter.collect_vec());
    let width = total_kind_sep_width + (num_piece - num_nonempty_kind) * piece_sep;

    let mut x = (max_width - width - 1.0) / 2.0;  // center reserve
    let y = reserve_y_pos(player_idx);
    for (piece_kind, amount) in reserve_iter {
        let filename = piece_path(piece_kind, force);
        let location = format!("reserve-{}", piece_kind.to_full_algebraic());
        for iter in 0..amount {
            if iter > 0 {
                x += piece_sep;
            }
            let node = document.create_svg_element("image")?;
            node.set_attribute("href", &filename)?;
            node.set_attribute("data-bughouse-location", &location)?;
            node.set_attribute("x", &x.to_string())?;
            node.set_attribute("y", &y.to_string())?;
            node.set_attribute("width", "1")?;
            node.set_attribute("height", "1")?;
            if draggable {
                node.set_attribute("class", "draggable")?;
            }
            reserve_node.append_child(&node)?;
        }
        x += piece_kind_sep;
    }
    Ok(())
}

fn update_reserve(reserve: &Reserve, force: Force, board_idx: WebBoard, player_idx: WebPlayer)
    -> JsResult<()>
{
    let is_me = (board_idx == WebBoard::Primary) && (player_idx == WebPlayer::Bottom);
    let piece_kind_sep = 1.0;
    let reserve_iter = reserve.iter()
        .filter(|(kind, _)| *kind != PieceKind::King)
        .map(|(piece_kind, &amount)| (piece_kind, amount));
    render_reserve(force, board_idx, player_idx, is_me, piece_kind_sep, reserve_iter)
}

fn render_starting() -> JsResult<()> {
    use PieceKind::*;
    use Force::*;
    use WebBoard::*;
    use WebPlayer::*;
    let reserve = [
        (Pawn, 8),
        (Knight, 2),
        (Bishop, 2),
        (Rook, 2),
        (Queen, 1),
        (King, 1),
    ];
    let draggable = false;
    let piece_kind_sep = 0.75;
    let reserve_iter = reserve.iter().copied();
    render_reserve(White, Primary, Bottom, draggable, piece_kind_sep, reserve_iter.clone())?;
    render_reserve(Black, Primary, Top, draggable, piece_kind_sep, reserve_iter.clone())?;
    render_reserve(Black, Secondary, Bottom, draggable, piece_kind_sep, reserve_iter.clone())?;
    render_reserve(White, Secondary, Top, draggable, piece_kind_sep, reserve_iter.clone())?;
    Ok(())
}

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
        format!("{:02}{}{}", sec, separator("."), util::div_ceil_u128(millis, 100) % 10)
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

fn update_scores(scores: &Scores, teaming: Teaming, my_team: Option<Team>) -> JsResult<()> {
    let normalize = |score: u32| (score as f64) / 2.0;
    let team_node = web_document().get_existing_element_by_id("score-team")?;
    let individual_node = web_document().get_existing_element_by_id("score-individual")?;
    match teaming {
        Teaming::FixedTeams => {
            assert!(scores.per_player.is_empty());
            let my_team = my_team.unwrap();
            team_node.set_text_content(Some(&format!(
                "{}\n⎯\n{}",
                normalize(*scores.per_team.get(&my_team.opponent()).unwrap_or(&0)),
                normalize(*scores.per_team.get(&my_team).unwrap_or(&0)),
            )));
            individual_node.set_text_content(None);
        },
        Teaming::IndividualMode => {
            assert!(scores.per_team.is_empty());
            let mut score_vec: Vec<_> = scores.per_player.iter().map(|(player, score)| {
                format!("{}: {}", player, normalize(*score))
            }).collect();
            score_vec.sort();
            team_node.set_text_content(None);
            individual_node.set_text_content(Some(&score_vec.join("\n")));
        }
    }
    Ok(())
}

fn render_grids(rotate_boards: bool) -> JsResult<()> {
    for board_idx in WebBoard::iter() {
        render_grid(board_idx, rotate_boards)?;
    }
    Ok(())
}

fn setup_participation_mode(observer: bool) -> JsResult<()> {
    let body = web_document().body()?;
    if observer {
        body.class_list().add_1("observer")?
    } else {
        body.class_list().remove_1("observer")?
    }
    Ok(())
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
                caption.set_attribute("dominant-baseline", "hanging")?;
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

    // Layer for square highlight that should be displayed below pieces.
    let highlight_layer = document.create_svg_element("g")?;
    highlight_layer.set_attribute("id", &square_highlight_layer(board_idx))?;
    svg.append_child(&highlight_layer)?;

    let border = make_board_rect(&document)?;
    border.set_attribute("class", "board-border")?;
    svg.append_child(&border)?;

    for player_idx in WebPlayer::iter() {
        let reserve = document.create_svg_element("g")?;
        reserve.set_attribute("id", &reserve_node_id(board_idx, player_idx))?;
        reserve.set_attribute("class", "reserve")?;
        let reserve_container = document.get_existing_element_by_id(
            &reserve_container_id(board_idx, player_idx)
        )?;
        reserve_container.append_child(&reserve)?;
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

fn turn_highlights(turn_expanded: &TurnExpanded) -> Vec<(&'static str, Coord)> {
    let mut highlights = vec![];
    if let Some(relocation) = turn_expanded.relocation {
        let (from, to) = relocation;
        highlights.push(("turn-from", from));
        highlights.push(("turn-to", to));
    }
    if let Some(relocation_extra) = turn_expanded.relocation_extra {
        let (from, to) = relocation_extra;
        highlights.push(("turn-from-extra", from));
        highlights.push(("turn-to-extra", to));
    }
    if let Some(drop) = turn_expanded.drop {
        highlights.push(("drop-to", drop));
    }
    if let Some(capture) = turn_expanded.capture {
        highlights.retain(|(_, coord)| *coord != capture.from);
        highlights.push(("capture", capture.from));
    }
    highlights
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
        f64::from(coord.x) + BOARD_LEFT,
        f64::from(coord.y) + BOARD_TOP,
    );
}

fn position_to_square(x: f64, y: f64) -> Option<DisplayCoord> {
    let x = (x - BOARD_LEFT) as i32;
    let y = (y - BOARD_TOP) as i32;
    if 0 <= x && x < NUM_COLS as i32 && 0 <= y && y < NUM_ROWS as i32 {
        // Improvement potential: clamp instead of asserting the values are in range.
        // Who knows if all browsers guarantee click coords cannot be 0.00001px away?
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

fn reserve_container_id(board_idx: WebBoard, player_idx: WebPlayer) -> String {
    format!("reserve-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn reserve_node_id(board_idx: WebBoard, player_idx: WebPlayer) -> String {
    format!("reserve-group-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn square_highlight_layer(board_idx: WebBoard) -> String {
    format!("square-highlight-layer-{}", board_id(board_idx))
}

fn reserve_y_pos(player_idx: WebPlayer) -> f64 {
    match player_idx {
        WebPlayer::Top => RESERVE_HEIGHT - 1.0 - RESERVE_PADDING,
        WebPlayer::Bottom => RESERVE_PADDING,
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
