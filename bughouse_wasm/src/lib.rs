extern crate console_error_panic_hook;
extern crate enum_map;
extern crate instant;
extern crate serde_json;
extern crate strum;
extern crate wasm_bindgen;

extern crate bughouse_chess;

use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;

use chain_cmp::chmp;
use enum_map::{EnumMap, enum_map};
use instant::Instant;
use itertools::Itertools;
use strum::IntoEnumIterator;
use wasm_bindgen::prelude::*;

use bughouse_chess::*;
use bughouse_chess::client::*;
use bughouse_chess::meter::*;


type JsResult<T> = Result<T, JsValue>;

const RESERVE_HEIGHT: f64 = 1.5;  // total reserve area height, in squares
const RESERVE_PADDING: f64 = 0.25;  // padding between board and reserve, in squares

// The client is single-threaded, so wrapping all mutable singletons in `thread_local!` seems ok.
thread_local! {
    static LAST_PANIC: RefCell<String> = RefCell::new(String::new());
}

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
            LAST_PANIC.with(|cell| *cell.borrow_mut() = serde_json::to_string(&event).unwrap());
        }));
    });
}

#[wasm_bindgen]
pub fn last_panic() -> String {
    LAST_PANIC.with(|cell| cell.borrow().clone())
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
pub struct JsMeter {
    meter: Meter,
}

#[wasm_bindgen]
impl JsMeter {
    fn new(meter: Meter) -> Self { JsMeter{ meter } }

    // Note. It is possible to have a u64 argument, but it's passed as BigInt:
    // https://rustwasm.github.io/docs/wasm-bindgen/reference/browser-support.html
    pub fn record(&self, value: f64) {
        assert!(value >= 0.0);
        self.meter.record(value as u64);
    }
}

#[wasm_bindgen]
pub struct JsEventMyNoop {}  // in contrast to `null`, indicates that event list is not over

#[wasm_bindgen]
pub struct JsEventContestStarted { contest_id: String }

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
pub struct JsEventGameExportReady { content: String }

#[wasm_bindgen]
impl JsEventContestStarted {
    pub fn contest_id(&self) -> String { self.contest_id.clone() }
}

#[wasm_bindgen]
impl JsEventGameExportReady {
    pub fn content(&self) -> String { self.content.clone() }
}


#[wasm_bindgen]
pub struct WebClient {
    // Improvement potential: Consider: in order to store additional information that
    //   is only relevant during game phase, add a generic `UserData` parameter to
    //   `ContestState::Game`. Could move `chalk_canvas` there, for example.
    state: ClientState,
    server_rx: mpsc::Receiver<BughouseClientEvent>,
}

#[wasm_bindgen]
impl WebClient {
    pub fn new_client(user_agent: String, time_zone: String) -> JsResult<WebClient> {
        let (server_tx, server_rx) = mpsc::channel();
        Ok(WebClient {
            state: ClientState::new(user_agent, time_zone, server_tx),
            server_rx,
        })
    }

    pub fn meter(&mut self, name: String) -> JsMeter {
        JsMeter::new(self.state.meter(name))
    }

    pub fn new_contest(
        &mut self,
        player_name: &str,
        teaming: &str,
        starting_position: &str,
        starting_time: &str,
        drop_aggression: &str,
        pawn_drop_rows: &str,
    ) -> JsResult<()> {
        let teaming = match teaming {
            "fixed-teams" => Teaming::FixedTeams,
            "individual-mode" => Teaming::IndividualMode,
            _ => return Err(format!("Invalid teaming: {teaming}").into()),
        };
        let starting_position = match starting_position {
            "classic" => StartingPosition::Classic,
            "fischer-random" => StartingPosition::FischerRandom,
            _ => return Err(format!("Invalid starting position: {starting_position}").into()),
        };
        let drop_aggression = match drop_aggression {
            "no-check" => DropAggression::NoCheck,
            "no-chess-mate" => DropAggression::NoChessMate,
            "no-bughouse-mate" => DropAggression::NoBughouseMate,
            "mate-allowed" => DropAggression::MateAllowed,
            _ => return Err(format!("Invalid drop aggression: {drop_aggression}").into()),
        };

        let Some((Ok(starting_minutes), Ok(starting_seconds))) = starting_time
            .split(':')
            .map(|v| v.parse::<u64>())
            .collect_tuple()
        else {
            return Err(format!("Invalid starting time: {starting_time}").into());
        };
        let starting_time = Duration::from_secs(starting_minutes * 60 + starting_seconds);

        let Some((Ok(min_pawn_drop_row), Ok(max_pawn_drop_row))) = pawn_drop_rows
            .split('-')
            .map(|v| v.parse::<u8>())
            .collect_tuple()
        else {
            return Err(format!("Invalid pawn drop rows: {pawn_drop_rows}").into());
        };
        if !chmp!(1 <= min_pawn_drop_row <= max_pawn_drop_row <= 7) {
            return Err(format!("Invalid pawn drop rows: {pawn_drop_rows}").into());
        }

        let chess_rules = ChessRules {
            starting_position,
            time_control: TimeControl {
                starting_time,
            },
        };
        let bughouse_rules = BughouseRules {
            teaming,
            min_pawn_drop_row: SubjectiveRow::from_one_based(min_pawn_drop_row),
            max_pawn_drop_row: SubjectiveRow::from_one_based(max_pawn_drop_row),
            drop_aggression,
        };
        self.state.new_contest(chess_rules, bughouse_rules, player_name.to_owned());
        Ok(())
    }

    pub fn join(&mut self, contest_id: String, my_name: String) {
        self.state.join(contest_id, my_name);
    }
    pub fn set_team(&mut self, team: &str) -> JsResult<()> {
        let team = match team {
            "red" => Team::Red,
            "blue" => Team::Blue,
            _ => {
                let info_string = web_document().get_existing_element_by_id("info-string")?;
                info_string.set_text_content(Some(r#"Supported teams are "red" and "blue""#));
                return Ok(());
            }
        };
        self.state.set_team(team);
        Ok(())
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
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        let source = if let Some(piece) = source.strip_prefix("reserve-") {
            PieceDragStart::Reserve(PieceKind::from_algebraic(piece).unwrap())
        } else {
            let coord = Coord::from_algebraic(source);
            let board_orientation = get_board_orientation(DisplayBoard::Primary, alt_game.perspective());
            let display_coord = to_display_coord(coord, board_orientation);
            set_square_highlight("drag-start-highlight", DisplayBoard::Primary, Some(display_coord))?;
            PieceDragStart::Board(coord)
        };
        alt_game.start_drag_piece(source).map_err(|err| rust_error!("Drag&drop error: {:?}", err))?;
        Ok(())
    }
    pub fn drag_piece(&mut self, x: f64, y: f64) -> JsResult<()> {
        let pos = DisplayFCoord{ x, y };
        set_square_highlight("drag-over-highlight", DisplayBoard::Primary, pos.to_square())
    }
    pub fn drag_piece_drop(&mut self, x: f64, y: f64, alternative_promotion: bool)
        -> JsResult<()>
    {
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        let pos = DisplayFCoord{ x, y };
        if let Some(dest_display) = pos.to_square() {
            use PieceKind::*;
            let board_orientation = get_board_orientation(DisplayBoard::Primary, alt_game.perspective());
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
        if let Some(alt_game) = self.state.alt_game_mut() {
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

    pub fn is_chalk_active(&self) -> bool {
        self.state.chalk_canvas().map_or(false, |c| c.is_painting())
    }
    pub fn chalk_down(&mut self, board_node: &str, x: f64, y: f64, alternative_mode: bool) -> JsResult<()> {
        let Some(GameState{ alt_game, .. }) = self.state.game_state() else { return Ok(()); };
        if alt_game.status() == BughouseGameStatus::Active {
            return Ok(());
        }
        let Some(canvas) = self.state.chalk_canvas_mut() else { return Ok(()); };
        let board_idx = parse_board_node_id(board_node)?;
        canvas.chalk_down(board_idx, DisplayFCoord{ x, y }, alternative_mode);
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_move(&mut self, x: f64, y: f64) -> JsResult<()> {
        let Some(canvas) = self.state.chalk_canvas_mut() else { return Ok(()); };
        canvas.chalk_move(DisplayFCoord{ x, y });
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_up(&mut self, x: f64, y: f64) -> JsResult<()> {
        let Some(canvas) = self.state.chalk_canvas_mut() else { return Ok(()); };
        let Some((board_idx, mark)) = canvas.chalk_up(DisplayFCoord{ x, y }) else { return Ok(()); };
        self.state.add_chalk_mark(board_idx, mark);
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_abort(&mut self) -> JsResult<()> {
        let Some(canvas) = self.state.chalk_canvas_mut() else { return Ok(()); };
        canvas.chalk_abort();
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_remove_last(&mut self, board_node: &str) -> JsResult<()> {
        let board_idx = parse_board_node_id(board_node)?;
        self.state.remove_last_chalk_mark(board_idx);
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_clear(&mut self, board_node: &str) -> JsResult<()> {
        let board_idx = parse_board_node_id(board_node)?;
        self.state.clear_chalk_drawing(board_idx);
        self.repaint_chalk()?;
        Ok(())
    }

    pub fn repaint_chalk(&self) -> JsResult<()> {
        // Improvement potential: Meter.
        // Improvement potential: Repaint only the current mark while drawing.
        let Some(GameState{ alt_game, chalkboard, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let my_id = alt_game.my_id();
        let document = web_document();
        for board_idx in DisplayBoard::iter() {
            let layer = document.get_existing_element_by_id(&chalk_highlight_layer_id(board_idx))?;
            remove_all_children(&layer)?;
            let layer = document.get_existing_element_by_id(&chalk_drawing_layer_id(board_idx))?;
            remove_all_children(&layer)?;
        }
        for (player_name, drawing) in chalkboard.all_drawings() {
            let owner = self.state.relation_to(player_name);
            for board_idx in DisplayBoard::iter() {
                for mark in drawing.board(get_board_index(board_idx, my_id)) {
                    self.render_chalk_mark(board_idx, owner, mark)?;
                }
            }
        }
        if let Some(canvas) = self.state.chalk_canvas() {
            if let Some((board_idx, mark)) = canvas.current_painting() {
                self.render_chalk_mark(*board_idx, PlayerRelation::Myself, mark)?;
            }
        }
        Ok(())
    }

    pub fn process_server_event(&mut self, event: &str) -> JsResult<()> {
        let server_event = serde_json::from_str(event).unwrap();
        self.state.process_server_event(server_event).map_err(|err| {
            rust_error!("{:?}", err)
        })
    }

    pub fn next_notable_event(&mut self) -> JsResult<JsValue> {
        match self.state.next_notable_event() {
            Some(NotableEvent::ContestStarted(contest_id)) => Ok(JsEventContestStarted{ contest_id }.into()),
            Some(NotableEvent::GameStarted) => {
                let Some(GameState{ ref alt_game, .. }) = self.state.game_state() else {
                    return Err(rust_error!("No game in progress"));
                };
                let info_string = web_document().get_existing_element_by_id("info-string")?;
                info_string.set_text_content(None);
                let my_id = alt_game.my_id();
                let is_observer = matches!(my_id, BughouseParticipantId::Observer(_));
                render_grids(alt_game.perspective())?;
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

    pub fn refresh(&mut self) -> JsResult<()> {
        self.state.refresh();
        if !self.state.is_connection_ok() {
            let info_string = web_document().get_existing_element_by_id("info-string")?;
            info_string.set_text_content(Some("🔌 Connection problem!\nConsider reloading the page."));
        }
        Ok(())
    }

    pub fn update_state(&self) -> JsResult<()> {
        let document = web_document();
        let info_string = document.get_existing_element_by_id("info-string")?;
        self.update_clock()?;
        let Some(contest) = self.state.contest() else {
            return Ok(());
        };
        update_scores(&contest.scores, contest.bughouse_rules.teaming, contest.my_team)?;
        let Some(GameState{ ref alt_game, .. }) = contest.game_state else {
            update_lobby(&contest)?;
            return Ok(());
        };
        // TODO: Better readiness status display.
        let game = alt_game.local_game();
        let my_id = alt_game.my_id();
        let my_display_board_idx = my_id.visual_board_idx();
        let my_display_force = my_id.visual_force();
        for (board_idx, board) in game.boards() {
            let is_primary = board_idx == my_display_board_idx;
            let display_board_idx = if is_primary { DisplayBoard::Primary } else { DisplayBoard::Secondary };
            let board_orientation = get_board_orientation(display_board_idx, alt_game.perspective());
            let piece_layer = document.get_existing_element_by_id(&piece_layer_id(display_board_idx))?;
            let grid = board.grid();
            for coord in Coord::all() {
                let node_id = piece_id(display_board_idx, coord);
                let node = document.get_element_by_id(&node_id);
                let piece = grid[coord];
                if let Some(piece) = piece {
                    let display_coord = to_display_coord(coord, board_orientation);
                    let node = match node {
                        Some(v) => v,
                        None => {
                            let v = web_document().create_svg_element("use")?;
                            v.set_attribute("id", &node_id)?;
                            piece_layer.append_child(&v)?;
                            v
                        },
                    };
                    let filename = piece_path(piece.kind, piece.force);
                    let pos = DisplayFCoord::square_pivot(display_coord);
                    node.set_attribute("x", &pos.x.to_string())?;
                    node.set_attribute("y", &pos.y.to_string())?;
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
            for player_idx in DisplayPlayer::iter() {
                use DisplayPlayer::*;
                use DisplayBoard::*;
                let force = match (player_idx, display_board_idx) {
                    (Bottom, Primary) | (Top, Secondary) => my_display_force,
                    (Top, Primary) | (Bottom, Secondary) => my_display_force.opponent(),
                };
                let name_node = document.get_existing_element_by_id(
                    &player_name_node_id(display_board_idx, player_idx)
                )?;
                let player_name = board.player_name(force);
                let player = contest.players.iter().find(|p| p.name == *player_name).unwrap();
                let player_string = if game.status() == BughouseGameStatus::Active {
                    player_string(&player)
                } else {
                    player_string_with_readiness(&player)
                };
                name_node.set_text_content(Some(&player_string));
                update_reserve(board.reserve(force), force, display_board_idx, player_idx)?;
            }
            let latest_turn = game.turn_log().iter().rev()
                .find(|record| record.player_id.board_idx == board_idx);
            {
                let latest_turn_highlight = latest_turn
                    .filter(|record| BughouseParticipantId::Player(record.player_id) != my_id)
                    .map(|record| &record.turn_expanded);
                let hightlight_id = format!("latest-{}", board_id(display_board_idx));
                self.set_turn_highlights(&hightlight_id, latest_turn_highlight, display_board_idx)?;
            }
            if display_board_idx == DisplayBoard::Primary {
                let pre_turn_highlight = latest_turn
                    .filter(|record| record.mode == TurnMode::Preturn)
                    .map(|record| &record.turn_expanded);
                self.set_turn_highlights("pre", pre_turn_highlight, display_board_idx)?;
            }
            update_turn_log(&game, board_idx, display_board_idx)?;
        }
        if is_clock_ticking(&game, my_id) {
            document.body()?.class_list().add_1("active-player")?
        } else {
            document.body()?.class_list().remove_1("active-player")?
        }
        self.repaint_chalk()?;
        if alt_game.status() != BughouseGameStatus::Active {
            // Safe to use `game_confirmed` here, because there could be no local status
            // changes after game over.
            info_string.set_text_content(Some(&alt_game.game_confirmed().outcome()));
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
        let my_display_board_idx = my_id.visual_board_idx();
        let my_display_force = my_id.visual_force();
        for (board_idx, board) in game.boards() {
            let is_primary = board_idx == my_display_board_idx;
            let display_board_idx = if is_primary { DisplayBoard::Primary } else { DisplayBoard::Secondary };
            for player_idx in DisplayPlayer::iter() {
                use DisplayPlayer::*;
                use DisplayBoard::*;
                let force = match (player_idx, display_board_idx) {
                    (Bottom, Primary) | (Top, Secondary) => my_display_force,
                    (Top, Primary) | (Bottom, Secondary) => my_display_force.opponent(),
                };
                let id_suffix = format!("{}-{}", board_id(display_board_idx), player_id(player_idx));
                // TODO: Dedup against `update_state`. Everything except the two lines below
                //   is copy-pasted from there.
                let clock_node = document.get_existing_element_by_id(&format!("clock-{}", id_suffix))?;
                update_clock(board.clock(), force, game_now, &clock_node)?;
            }
        }
        Ok(())
    }

    pub fn meter_stats(&self) -> String {
        self.state.read_meter_stats().iter()
            .sorted_by_key(|(metric, _)| metric.as_str())
            .map(|(metric, stats)| format!("{metric}: {stats}"))
            .join("\n")
    }

    fn render_chalk_mark(
        &self, board_idx: DisplayBoard, owner: PlayerRelation, mark: &ChalkMark
    ) -> JsResult<()> {
        use PlayerRelation::*;
        let Some(GameState{ alt_game, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let document = web_document();
        let orientation = get_board_orientation(board_idx, alt_game.perspective());
        match mark {
            ChalkMark::Arrow{ from, to } => {
                let layer = document.get_existing_element_by_id(&chalk_drawing_layer_id(board_idx))?;
                let from = DisplayFCoord::square_center(to_display_coord(*from, orientation));
                let to = DisplayFCoord::square_center(to_display_coord(*to, orientation));
                let node = document.create_svg_element("line")?;
                let d = normalize_vec(to - from);
                let from = from + mult_vec(d, 0.3);
                let to = to + mult_vec(d, -0.45);
                node.set_attribute("x1", &from.x.to_string())?;
                node.set_attribute("y1", &from.y.to_string())?;
                node.set_attribute("x2", &to.x.to_string())?;
                node.set_attribute("y2", &to.y.to_string())?;
                node.set_attribute("class", &["chalk-arrow", &chalk_line_color_class(owner)].join(" "))?;
                layer.append_child(&node)?;
            },
            ChalkMark::FreehandLine{ points } => {
                let layer = document.get_existing_element_by_id(&chalk_drawing_layer_id(board_idx))?;
                let node = document.create_svg_element("polyline")?;
                let points = points.iter().map(|&q| {
                    let p = to_display_fcoord(q, orientation);
                    format!("{},{}", p.x, p.y)
                }).join(" ");
                node.set_attribute("points", &points)?;
                node.set_attribute("class", &["chalk-freehand-line", &chalk_line_color_class(owner)].join(" "))?;
                layer.append_child(&node)?;
            },
            ChalkMark::SquareHighlight{ coord } => {
                let layer = document.get_existing_element_by_id(&chalk_highlight_layer_id(board_idx))?;
                let node = document.create_svg_element("polygon")?;
                let p = DisplayFCoord::square_pivot(to_display_coord(*coord, orientation));
                // Note. The corners are chosen so that they corresponds to the seating, as seen
                // by the current player. Another approach would be to have one highlight element,
                // <use> it here and rotate in CSS based on class.
                let points = match owner {
                    Myself   => vec![ p + (0., 1.), p + (0.5, 1.), p + (0., 0.5) ],
                    Opponent => vec![ p + (0., 0.), p + (0., 0.5), p + (0.5, 0.) ],
                    Partner  => vec![ p + (1., 1.), p + (1., 0.5), p + (0.5, 1.) ],
                    Diagonal => vec![ p + (1., 0.), p + (0.5, 0.), p + (1., 0.5) ],
                    Other    => vec![ p + (0.5, 0.1), p + (0.1, 0.5), p + (0.5, 0.9), p + (0.9, 0.5) ],
                };
                let points = points.iter().map(|&p| format!("{},{}", p.x, p.y)).join(" ");
                node.set_attribute("points", &points)?;
                node.set_attribute("class", &["chalk-square-highlight", &chalk_square_color_class(owner)].join(" "))?;
                layer.append_child(&node)?;
            },
        }
        Ok(())
    }

    fn set_turn_highlights(&self, id_prefix: &str, turn: Option<&TurnExpanded>, board_idx: DisplayBoard)
        -> JsResult<()>
    {
        // Optimization potential: do not reset highlights that stay in place.
        reset_square_highlight(&format!("{}-turn-from", id_prefix))?;
        reset_square_highlight(&format!("{}-turn-to", id_prefix))?;
        reset_square_highlight(&format!("{}-turn-from-extra", id_prefix))?;
        reset_square_highlight(&format!("{}-turn-to-extra", id_prefix))?;
        reset_square_highlight(&format!("{}-drop-to", id_prefix))?;
        reset_square_highlight(&format!("{}-capture", id_prefix))?;
        let Some(GameState{ ref alt_game, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let board_orientation = get_board_orientation(board_idx, alt_game.perspective());
        if let Some(turn) = turn {
            for (id_suffix, coord) in turn_highlights(turn) {
                let id = format!("{}-{}", id_prefix, id_suffix);
                set_square_highlight(&id, board_idx, Some(to_display_coord(coord, board_orientation)))?;
            }
        }
        Ok(())
    }
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

    pub fn create_element(&self, local_name: &str) -> JsResult<web_sys::Element> {
        self.0.create_element(local_name)
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

fn is_scrolled_to_bottom(e: &web_sys::Element) -> bool {
    let eps = 1;
    e.scroll_top() >= e.scroll_height() - e.client_height() - eps
}

fn scroll_to_bottom(e: &web_sys::Element) {
    e.set_scroll_top(e.scroll_height() - e.client_height());
}


#[wasm_bindgen]
pub fn init_page() -> JsResult<()> {
    generate_svg_markers()?;
    render_grids(Perspective::PlayAsWhite)?;
    render_starting()?;
    Ok(())
}

#[wasm_bindgen]
pub fn git_version() -> String {
    my_git_version!().to_owned()
}

fn update_lobby(contest: &Contest) -> JsResult<()> {
    let info_string = web_document().get_existing_element_by_id("info-string")?;
    // TODO: Show teams for the new game in individual mode.
    let player_info = match contest.bughouse_rules.teaming {
        Teaming::FixedTeams => {
            // TODO: Allow observers in fixed teams mode; rename "teamless"/"unassigned" to "observer".
            let mut teamless = vec![];
            let mut teams: EnumMap<Team, Vec<String>> = enum_map!{ _ => vec![] };
            for p in &contest.players {
                let s = format!("{}\n", player_string_with_readiness(p));
                if let Some(fixed_team) = p.fixed_team {
                    teams[fixed_team].push(s);
                } else {
                    teamless.push(s);
                }
            }
            format!(
                "{}{}red:\n{}blue:\n{}",
                if teamless.is_empty() { "" } else { "unassigned:\n" },
                teamless.join(""),
                teams[Team::Red].join(""),
                teams[Team::Blue].join(""),
            )
        },
        Teaming::IndividualMode => {
            contest.players.iter().map(|p| {
                assert!(p.fixed_team.is_none());
                player_string_with_readiness(p)
            }).join("\n")
        },
    };
    let contest_id = &contest.contest_id;
    info_string.set_text_content(Some(&format!("Contest {contest_id}\n{player_info}")));
    Ok(())
}

// Note. Each `id` should unambiguously correspond to a fixed board.
// TODO: Separate layer for drag highlight, to put it above last turn highlight.
fn set_square_highlight(id: &str, board_idx: DisplayBoard, coord: Option<DisplayCoord>) -> JsResult<()> {
    let document = web_document();
    if let Some(coord) = coord {
        let node = document.get_element_by_id(id);
        let highlight_layer = document.get_existing_element_by_id(&square_highlight_layer_id(board_idx))?;
        let node = node.ok_or(JsValue::UNDEFINED).or_else(|_| -> JsResult<web_sys::Element> {
            let node = document.create_svg_element("rect")?;
            node.set_attribute("id", id)?;
            node.set_attribute("width", "1")?;
            node.set_attribute("height", "1")?;
            highlight_layer.append_child(&node)?;
            Ok(node)
        })?;
        let pos = DisplayFCoord::square_pivot(coord);
        node.set_attribute("x", &pos.x.to_string())?;
        node.set_attribute("y", &pos.y.to_string())?;
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

// Improvement potential: Find a better icon for connection problems.
// Improvement potential: Add a tooltip explaining the meaning of the icon.
fn player_string(p: &Player) -> String {
    let icon = if p.is_online { "" } else { "⚠️ " };
    format!("{}{}", icon, p.name)
}

fn player_string_with_readiness(p: &Player) -> String {
    let icon = if p.is_online {
        if p.is_ready { "☑ " } else { "☐ " }
    } else {
        "⚠️ "
    };
    format!("{}{}", icon, p.name)
}

// Renders reserve.
// Leaves space for missing piece kinds too. This makes reserve piece positions more or
// less fixed, thus reducing the chance of grabbing the wrong piece after a last-moment
// reserve update.
fn render_reserve(
    force: Force, board_idx: DisplayBoard, player_idx: DisplayPlayer, draggable: bool,
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
            let node = document.create_svg_element("use")?;
            node.set_attribute("href", &filename)?;
            node.set_attribute("data-bughouse-location", &location)?;
            node.set_attribute("x", &x.to_string())?;
            node.set_attribute("y", &y.to_string())?;
            if draggable {
                node.set_attribute("class", "draggable")?;
            }
            reserve_node.append_child(&node)?;
        }
        x += piece_kind_sep;
    }
    Ok(())
}

fn update_reserve(reserve: &Reserve, force: Force, board_idx: DisplayBoard, player_idx: DisplayPlayer)
    -> JsResult<()>
{
    let is_me = (board_idx == DisplayBoard::Primary) && (player_idx == DisplayPlayer::Bottom);
    let piece_kind_sep = 1.0;
    let reserve_iter = reserve.iter()
        .filter(|(kind, _)| *kind != PieceKind::King)
        .map(|(piece_kind, &amount)| (piece_kind, amount));
    render_reserve(force, board_idx, player_idx, is_me, piece_kind_sep, reserve_iter)
}

fn render_starting() -> JsResult<()> {
    use PieceKind::*;
    use Force::*;
    use DisplayBoard::*;
    use DisplayPlayer::*;
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

// Similar to `BughouseGame.player_is_active`, but returns false before game started.
fn is_clock_ticking(game: &BughouseGame, participant_id: BughouseParticipantId) -> bool {
    let BughouseParticipantId::Player(player_id) = participant_id else {
        return false;
    };
    game.board(player_id.board_idx).clock().active_force() == Some(player_id.force)
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
            let my_team = my_team.unwrap_or_else(|| {
                // A player may be without a team only before the contest first game begun.
                assert!(scores.per_team.values().all(|&v| v == 0));
                // Return a team at random to show zeros in the desired format.
                Team::Red
            });
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

fn render_grids(perspective: Perspective) -> JsResult<()> {
    for board_idx in DisplayBoard::iter() {
        render_grid(board_idx, perspective)?;
    }
    Ok(())
}

fn update_turn_log(
    game: &BughouseGame, board_idx: BughouseBoard, display_board_idx: DisplayBoard
) -> JsResult<()> {
    let document = web_document();
    let log_container_node = document.get_existing_element_by_id(&turn_log_container_node_id(display_board_idx))?;
    let log_node = document.get_existing_element_by_id(&turn_log_node_id(display_board_idx))?;
    let was_at_bottom = is_scrolled_to_bottom(&log_container_node);
    remove_all_children(&log_node)?;
    for record in game.turn_log().iter() {
        if record.player_id.board_idx == board_idx {
            let force = force_id(record.player_id.force);
            let node = document.create_element("div")?;
            node.set_text_content(Some(&record.to_log_entry()));
            node.set_attribute("class", &format!("turn-record turn-record-{force}"))?;
            log_node.append_child(&node)?;
        }
    }
    // Keep log scrolled to bottom if it's already there. It's also possible to snap scrolling
    // with CSS `scroll-snap-type` (https://stackoverflow.com/a/60546366/3092679), but the snap
    // range is too large (especially in Firefox), so it becomes very hard to browse the log.
    if was_at_bottom {
        scroll_to_bottom(&log_container_node);
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

fn render_grid(board_idx: DisplayBoard, perspective: Perspective) -> JsResult<()> {
    let board_orientation = get_board_orientation(board_idx, perspective);
    let text_h_padding = 0.07;
    let text_v_padding = 0.09;

    let make_board_rect = |document: &WebDocument| -> JsResult<web_sys::Element> {
        let rect = document.create_svg_element("rect")?;
        let pos = DisplayFCoord::square_pivot(DisplayCoord{ x: 0, y: 0 });
        rect.set_attribute("x", &pos.x.to_string())?;
        rect.set_attribute("y", &pos.y.to_string())?;
        rect.set_attribute("width", &NUM_COLS.to_string())?;
        rect.set_attribute("height", &NUM_ROWS.to_string())?;
        Ok(rect)
    };

    let document = web_document();
    let svg = document.get_existing_element_by_id(&board_node_id(board_idx))?;
    svg.set_attribute("viewBox", &format!("0 0 {NUM_COLS} {NUM_ROWS}"))?;
    remove_all_children(&svg)?;

    let shadow = make_board_rect(&document)?;
    shadow.set_attribute("class", "board-shadow")?;
    svg.append_child(&shadow)?;

    for row in Row::all() {
        for col in Col::all() {
            let sq = document.create_svg_element("rect")?;
            let display_coord = to_display_coord(Coord::new(row, col), board_orientation);
            let DisplayFCoord{ x, y } = DisplayFCoord::square_pivot(display_coord);
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

    let add_layer = |id: String| -> JsResult<()> {
        let layer = document.create_svg_element("g")?;
        layer.set_attribute("id", &id)?;
        svg.append_child(&layer)?;
        Ok(())
    };

    add_layer(square_highlight_layer_id(board_idx))?;
    add_layer(chalk_highlight_layer_id(board_idx))?;
    add_layer(piece_layer_id(board_idx))?;
    add_layer(chalk_drawing_layer_id(board_idx))?;

    let border = make_board_rect(&document)?;
    border.set_attribute("class", "board-border")?;
    svg.append_child(&border)?;

    for player_idx in DisplayPlayer::iter() {
        let reserve = document.create_svg_element("g")?;
        reserve.set_attribute("id", &reserve_node_id(board_idx, player_idx))?;
        reserve.set_attribute("class", "reserve")?;
        let reserve_container = document.get_existing_element_by_id(
            &reserve_container_id(board_idx, player_idx)
        )?;
        // Note that reserve height is also encoded in CSS.
        reserve_container.set_attribute("viewBox", &format!("0 0 {NUM_COLS} {RESERVE_HEIGHT}"))?;
        reserve_container.append_child(&reserve)?;
    }
    Ok(())
}

fn generate_svg_markers() -> JsResult<()> {
    let document = web_document();
    let svg_defs = document.get_existing_element_by_id("svg-defs")?;
    for relation in PlayerRelation::iter() {
        // These definition are identical, but having multiple copies allows us to color them
        // differently in css. Yep, that's the only way to have multiple arrowhear colors in SVG
        // (although it might be changed in SVG2):
        // https://stackoverflow.com/questions/16664584/changing-an-svg-markers-color-css
        let marker = document.create_svg_element("marker")?;
        marker.set_attribute("id", &arrowhead_id(relation))?;
        marker.set_attribute("viewBox", "0 0 10 10")?;
        marker.set_attribute("refX", "5")?;
        marker.set_attribute("refY", "5")?;
        marker.set_attribute("markerWidth", "2.5")?;
        marker.set_attribute("markerHeight", "2.5")?;
        marker.set_attribute("orient", "auto-start-reverse")?;
        let path = document.create_svg_element("path")?;
        path.set_attribute("d", "M 4 0 L 10 5 L 4 10 z")?;
        marker.append_child(&path)?;
        svg_defs.append_child(&marker)?;
    }
    Ok(())
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

fn force_id(force: Force) -> &'static str {
    match force {
        Force::White => "white",
        Force::Black => "black",
    }
}

fn board_id(idx: DisplayBoard) -> &'static str {
    match idx {
        DisplayBoard::Primary => "primary",
        DisplayBoard::Secondary => "secondary",
    }
}

fn board_node_id(idx: DisplayBoard) -> String {
    format!("board-{}", board_id(idx))
}

fn parse_board_node_id(id: &str) -> JsResult<DisplayBoard> {
    match id {
        "board-primary" => Ok(DisplayBoard::Primary),
        "board-secondary" => Ok(DisplayBoard::Secondary),
        _ => Err(format!(r#"Invalid board node: "{id}""#).into()),
    }
}

fn player_id(idx: DisplayPlayer) -> &'static str {
    match idx {
        DisplayPlayer::Top => "top",
        DisplayPlayer::Bottom => "bottom",
    }
}

fn piece_id(board_idx: DisplayBoard, coord: Coord) -> String {
    format!("{}-{}", board_id(board_idx), coord.to_algebraic())
}

fn player_name_node_id(board_idx: DisplayBoard, player_idx: DisplayPlayer) -> String {
    format!("player-name-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn reserve_container_id(board_idx: DisplayBoard, player_idx: DisplayPlayer) -> String {
    format!("reserve-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn reserve_node_id(board_idx: DisplayBoard, player_idx: DisplayPlayer) -> String {
    format!("reserve-group-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn turn_log_container_node_id(board_idx: DisplayBoard) -> String {
    format!("turn-log-container-{}", board_id(board_idx))
}

fn turn_log_node_id(board_idx: DisplayBoard) -> String {
    format!("turn-log-{}", board_id(board_idx))
}

fn piece_layer_id(board_idx: DisplayBoard) -> String {
    format!("piece-layer-{}", board_id(board_idx))
}

fn square_highlight_layer_id(board_idx: DisplayBoard) -> String {
    format!("square-highlight-layer-{}", board_id(board_idx))
}

fn chalk_highlight_layer_id(board_idx: DisplayBoard) -> String {
    format!("chalk-highlight-layer-{}", board_id(board_idx))
}

fn chalk_drawing_layer_id(board_idx: DisplayBoard) -> String {
    format!("chalk-drawing-layer-{}", board_id(board_idx))
}

fn participant_relation_id(owner: PlayerRelation) -> &'static str {
    match owner {
        PlayerRelation::Myself => "myself",
        PlayerRelation::Opponent => "opponent",
        PlayerRelation::Partner => "partner",
        PlayerRelation::Diagonal => "diagonal",
        PlayerRelation::Other => "other",
    }
}

fn arrowhead_id(owner: PlayerRelation) -> String {
    format!("arrowhead-{}", participant_relation_id(owner))
}

fn chalk_line_color_class(owner: PlayerRelation) -> String {
    format!("chalk-line-{}", participant_relation_id(owner))
}

fn chalk_square_color_class(owner: PlayerRelation) -> String {
    format!("chalk-square-{}", participant_relation_id(owner))
}

fn reserve_y_pos(player_idx: DisplayPlayer) -> f64 {
    match player_idx {
        DisplayPlayer::Top => RESERVE_HEIGHT - 1.0 - RESERVE_PADDING,
        DisplayPlayer::Bottom => RESERVE_PADDING,
    }
}

fn square_text_color_class(row: Row, col: Col) -> &'static str {
    if (row.to_zero_based() + col.to_zero_based()) % 2 == 0 {
        "on-sq-black"
    } else {
        "on-sq-white"
    }
}

fn square_color_class(row: Row, col: Col) -> &'static str {
    if (row.to_zero_based() + col.to_zero_based()) % 2 == 0 {
        "sq-black"
    } else {
        "sq-white"
    }
}

fn piece_path(piece_kind: PieceKind, force: Force) -> &'static str {
    use PieceKind::*;
    use Force::*;
    match (force, piece_kind) {
        (White, Pawn) => "#white-pawn",
        (White, Knight) => "#white-knight",
        (White, Bishop) => "#white-bishop",
        (White, Rook) => "#white-rook",
        (White, Queen) => "#white-queen",
        (White, King) => "#white-king",
        (Black, Pawn) => "#black-pawn",
        (Black, Knight) => "#black-knight",
        (Black, Bishop) => "#black-bishop",
        (Black, Rook) => "#black-rook",
        (Black, Queen) => "#black-queen",
        (Black, King) => "#black-king",
    }
}
