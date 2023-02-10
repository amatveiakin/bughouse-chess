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

#[wasm_bindgen(getter_with_clone)]
pub struct RustError { pub message: String }

#[wasm_bindgen(getter_with_clone)]
pub struct IgnorableError { pub message: String }

#[wasm_bindgen(getter_with_clone)]
pub struct FatalError { pub message: String }

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
pub struct JsEventNoop {}  // in contrast to `null`, indicates that event list is not over

#[wasm_bindgen(getter_with_clone)]
pub struct JsEventContestStarted { pub contest_id: String }

#[wasm_bindgen]
pub struct JsEventGameStarted {}

#[wasm_bindgen(getter_with_clone)]
pub struct JsEventGameOver { pub result: String }

#[wasm_bindgen]
pub struct JsEventTurnMade {}

#[wasm_bindgen]
pub struct JsEventMyReserveRestocked {}

#[wasm_bindgen]
pub struct JsEventLowTime {}

#[wasm_bindgen(getter_with_clone)]
pub struct JsEventGameExportReady { pub content: String }


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

    pub fn current_turnaround_time(&self) -> Option<f64> {
        self.state.current_turnaround_time().map(|t| t.as_secs_f64())
    }

    pub fn observer_status(&self) -> String {
        let Some(my_faction) = self.state.my_faction() else {
            return "no".to_owned();
        };
        match my_faction {
            Faction::Observer => "permanently",
            Faction::Fixed(_) | Faction::Random => {
                let my_id = self.state.my_id();
                if matches!(my_id, Some(BughouseParticipantId::Observer(_))) {
                    "temporary"
                } else {
                    "no"
                }
            }
        }.to_owned()
    }

    pub fn game_status(&self) -> String {
        if let Some(game_state) = self.state.game_state() {
            if game_state.alt_game.status() == BughouseGameStatus::Active {
                "active"
            } else {
                "over"
            }
        } else {
            "none"
        }.to_owned()
    }

    pub fn lobby_waiting_explanation(&self) -> String {
        let Some(contest) = self.state.contest() else {
            return "".to_owned();
        };
        // Improvement potential: Factor out common code or send the status from the server
        //   to ensure there are no discrepancies between server logic and UI messages.
        let num_players = contest.participants.iter().filter(|p| p.faction != Faction::Observer).count();
        if num_players < TOTAL_PLAYERS {
            "Not enough players"
        } else {
            "Waiting for players to be ready"
        }.to_owned()
    }
    pub fn lobby_countdown_seconds_left(&self) -> Option<u32> {
        self.state.first_game_countdown_left().map(|d| d.as_secs_f64().ceil() as u32)
    }

    pub fn new_contest(
        &mut self,
        player_name: &str,
        teaming: &str,
        starting_position: &str,
        starting_time: &str,
        drop_aggression: &str,
        pawn_drop_rows: &str,
        rating: &str,
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
        let rated = match rating {
            "rated" => true,
            "unrated" => false,
            _ => return Err(format!("Invalid rating: {rating}").into()),
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

        let contest_rules = ContestRules {
            rated,
        };
        let chess_rules = ChessRules {
            starting_position,
            time_control: TimeControl {
                starting_time,
            },
        };
        let bughouse_rules = BughouseRules {
            teaming,
            min_pawn_drop_row: SubjectiveRow::from_one_based(min_pawn_drop_row).unwrap(),
            max_pawn_drop_row: SubjectiveRow::from_one_based(max_pawn_drop_row).unwrap(),
            drop_aggression,
        };
        let rules = Rules {
            contest_rules,
            chess_rules,
            bughouse_rules,
        };
        self.state.new_contest(rules, player_name.to_owned());
        Ok(())
    }

    pub fn join(&mut self, contest_id: String, my_name: String) {
        self.state.join(contest_id, my_name);
    }
    pub fn resign(&mut self) {
        self.state.resign();
    }
    pub fn toggle_ready(&mut self) {
        if let Some(is_ready) = self.state.is_ready() {
            self.state.set_ready(!is_ready);
        }
    }
    pub fn next_faction(&mut self) {
        self.change_faction(|f| f + 1);
    }
    pub fn previous_faction(&mut self) {
        self.change_faction(|f| f - 1);
    }
    pub fn request_export(&mut self) -> JsResult<()> {
        let format = pgn::BughouseExportFormat{};
        self.state.request_export(format);
        Ok(())
    }

    fn make_turn(&mut self, turn_input: TurnInput) -> JsResult<()> {
        let turn_result = self.state.make_turn(turn_input);
        // Improvement potential: Human-readable error messages (and/or visual hints).
        //   Ideally also include rule-dependent context, e.g. "Illegal drop position:
        //   pawns can be dropped onto ranks 2–6 counting from the player".
        let game_message = web_document().get_existing_element_by_id("game-message")?;
        game_message.set_text_content(turn_result.as_ref().err().map(|err| format!("{:?}", err)).as_deref());
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
            let coord = Coord::from_algebraic(source).unwrap();
            let board_orientation = get_board_orientation(DisplayBoard::Primary, alt_game.perspective());
            set_square_highlight(
                None, "drag-start-highlight", SquareHighlightLayer::Drag,
                DisplayBoard::Primary, Some(to_display_coord(coord, board_orientation))
            )?;
            let board_idx = get_board_index(DisplayBoard::Primary, alt_game.my_id());
            for dest in alt_game.local_game().board(board_idx).legal_move_destinations(coord) {
                set_square_highlight(
                    None, "legal-move-highlight", SquareHighlightLayer::Drag,
                    DisplayBoard::Primary, Some(to_display_coord(dest, board_orientation))
                )?;
            }
            PieceDragStart::Board(coord)
        };
        alt_game.start_drag_piece(source).map_err(|err| rust_error!("Drag&drop error: {:?}", err))?;
        Ok(())
    }
    pub fn drag_piece(&mut self, x: f64, y: f64) -> JsResult<()> {
        let pos = DisplayFCoord{ x, y };
        set_square_highlight(
            Some("drag-over-highlight"), "drag-over-highlight", SquareHighlightLayer::Drag,
            DisplayBoard::Primary, pos.to_square()
        )
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
            let dest_coord = from_display_coord(dest_display, board_orientation).unwrap();
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
        clear_square_highlight_layer(SquareHighlightLayer::Drag)
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

    pub fn process_server_event(&mut self, event: &str) -> JsResult<bool> {
        let server_event = serde_json::from_str(event).unwrap();
        let updated_needed = !matches!(server_event, BughouseServerEvent::Pong);
        self.state.process_server_event(server_event).map_err(|err| {
            match err {
                EventError::IgnorableError(message) => IgnorableError{ message }.into(),
                EventError::FatalError(message) => FatalError{ message }.into(),
                EventError::InternalEvent(message) => rust_error!("{message}"),
            }
        })?;
        Ok(updated_needed)
    }

    pub fn next_notable_event(&mut self) -> JsResult<JsValue> {
        match self.state.next_notable_event() {
            Some(NotableEvent::ContestStarted(contest_id)) => {
                let rules_node = web_document().get_existing_element_by_id("lobby-rules")?;
                rules_node.set_text_content(Some(&self.state.contest().unwrap().rules.to_human_readable()));
                Ok(JsEventContestStarted{ contest_id }.into())
            },
            Some(NotableEvent::GameStarted) => {
                let Some(GameState{ ref alt_game, .. }) = self.state.game_state() else {
                    return Err(rust_error!("No game in progress"));
                };
                let game_message = web_document().get_existing_element_by_id("game-message")?;
                game_message.set_text_content(None);
                let my_id = alt_game.my_id();
                let is_observer = matches!(my_id, BughouseParticipantId::Observer(_));
                render_grids(alt_game.perspective())?;
                setup_participation_mode(is_observer)?;
                for display_board_idx in DisplayBoard::iter() {
                    scroll_log_to_bottom(display_board_idx)?;
                }
                Ok(JsEventGameStarted{}.into())
            },
            Some(NotableEvent::GameOver(game_status)) => {
                let result = match game_status {
                    SubjectiveGameResult::Victory => "victory",
                    SubjectiveGameResult::Defeat => "defeat",
                    SubjectiveGameResult::Draw => "draw",
                }.to_owned();
                Ok(JsEventGameOver{ result }.into())
            },
            Some(NotableEvent::TurnMade(player_id)) => {
                let Some(GameState{ ref alt_game, .. }) = self.state.game_state() else {
                    return Err(rust_error!("No game in progress"));
                };
                let display_board_idx = get_display_board_index(player_id.board_idx, alt_game.my_id());
                scroll_log_to_bottom(display_board_idx)?;
                if let BughouseParticipantId::Player(my_player_id) = alt_game.my_id() {
                    if player_id.board_idx == my_player_id.board_idx {
                        return Ok(JsEventTurnMade{}.into());
                    }
                }
                Ok(JsEventNoop{}.into())
            }
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
        let game_message = document.get_existing_element_by_id("game-message")?;
        self.update_clock()?;
        let Some(contest) = self.state.contest() else {
            return Ok(());
        };
        update_observers(&contest.participants)?;
        let Some(GameState{ ref alt_game, .. }) = contest.game_state else {
            update_lobby(&contest)?;
            return Ok(());
        };
        // Improvement potential: Better readiness status display.
        let teaming = contest.rules.bughouse_rules.teaming;
        let game = alt_game.local_game();
        let my_id = alt_game.my_id();
        update_scores(&contest.scores, &contest.participants, game.status(), teaming, my_id)?;
        for (board_idx, board) in game.boards() {
            let is_piece_draggable = |force| {
                let BughouseParticipantId::Player(my_player_id) = my_id else {
                    return false;
                };
                board_idx == my_player_id.board_idx && force == my_player_id.force
            };
            let display_board_idx = get_display_board_index(board_idx, my_id);
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
                    if is_piece_draggable(piece.force) {
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
            for force in Force::iter() {
                let player_idx = get_display_player(force, board_orientation);
                let name_node = document.get_existing_element_by_id(
                    &player_name_node_id(display_board_idx, player_idx)
                )?;
                let player_name = board.player_name(force);
                let player = contest.participants.iter().find(|p| p.name == *player_name).unwrap();
                // TODO: Show teams for the upcoming game in individual mode.
                let show_readiness = game.status() != BughouseGameStatus::Active && teaming == Teaming::FixedTeams;
                let player_string = participant_string(&player, show_readiness);
                name_node.set_text_content(Some(&player_string));
                let is_draggable = is_piece_draggable(force);
                update_reserve(board.reserve(force), force, display_board_idx, player_idx, is_draggable)?;
            }
            let latest_turn = game.turn_log().iter().rev()
                .find(|record| record.player_id.board_idx == board_idx);
            {
                let latest_turn_highlight = latest_turn
                    .filter(|record| BughouseParticipantId::Player(record.player_id) != my_id)
                    .map(|record| &record.turn_expanded);
                self.set_turn_highlights(TurnHighlight::LatestTurn, latest_turn_highlight, display_board_idx)?;
            }
            if display_board_idx == DisplayBoard::Primary {
                let pre_turn_highlight = latest_turn
                    .filter(|record| record.mode == TurnMode::Preturn)
                    .map(|record| &record.turn_expanded);
                self.set_turn_highlights(TurnHighlight::Preturn, pre_turn_highlight, display_board_idx)?;
            }
            update_turn_log(&game, board_idx, display_board_idx)?;
        }
        document.body()?.class_list().toggle_with_force("active-player", is_clock_ticking(&game, my_id))?;
        self.repaint_chalk()?;
        if alt_game.status() != BughouseGameStatus::Active {
            // Safe to use `game_confirmed` here, because there could be no local status
            // changes after game over.
            game_message.set_text_content(Some(&alt_game.game_confirmed().outcome()));
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
        for (board_idx, board) in game.boards() {
            let display_board_idx = get_display_board_index(board_idx, alt_game.my_id());
            let board_orientation = get_board_orientation(display_board_idx, alt_game.perspective());
            for force in Force::iter() {
                let player_idx = get_display_player(force, board_orientation);
                let clock_node = document.get_existing_element_by_id(&clock_node_id(display_board_idx, player_idx))?;
                render_clock(board.clock(), force, game_now, &clock_node)?;
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

    fn change_faction(&mut self, faction_modifier: impl Fn(i32) -> i32) {
        let Some(contest) = self.state.contest() else {
            return;
        };
        let allowed_factions = contest.rules.bughouse_rules.teaming.allowed_factions();
        let current = allowed_factions.iter().position(|&f| f == contest.my_faction).unwrap();
        let new = faction_modifier(current.try_into().unwrap());
        let new = new.rem_euclid(allowed_factions.len().try_into().unwrap());
        let new: usize = new.try_into().unwrap();
        self.state.set_faction(allowed_factions[new]);
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

    fn set_turn_highlights(
        &self, highlight: TurnHighlight, turn: Option<&TurnExpanded>, board_idx: DisplayBoard
    ) -> JsResult<()>
    {
        let (id_prefix, class_prefix) = match highlight {
            TurnHighlight::LatestTurn => (format!("latest-{}", board_id(board_idx)), "latest"),
            TurnHighlight::Preturn => ("pre".to_owned(), "pre"),
        };
        // Optimization potential: do not reset highlights that stay in place.
        //   ... or replace with a single clear_square_highlight_layer and remove all IDs.
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
            for (suffix, coord) in turn_highlights(turn) {
                let id = format!("{}-{}", id_prefix, suffix);
                let class = format!("{}-{}", class_prefix, suffix);
                set_square_highlight(
                    Some(&id), &class, SquareHighlightLayer::Turn,
                    board_idx, Some(to_display_coord(coord, board_orientation))
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SquareHighlightLayer {
    Turn,  // last turn, preturn
    Drag,  // drag start, drag hover, legal moves
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TurnHighlight {
    Preturn,
    LatestTurn,
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

fn scroll_to_bottom(e: &web_sys::Element) {
    // Do not try to compute the real scroll position, as it is very slow!
    // See the comment in `update_turn_log`.
    e.set_scroll_top(1_000_000_000);
}

fn scroll_log_to_bottom(board_idx: DisplayBoard) -> JsResult<()> {
    let e = web_document().get_existing_element_by_id(&turn_log_scroll_area_node_id(board_idx))?;
    scroll_to_bottom(&e);
    Ok(())
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
    let document = web_document();
    let lobby_participants_node = document.get_existing_element_by_id("lobby-participants")?;
    remove_all_children(&lobby_participants_node)?;
    for p in &contest.participants {
        let is_me = p.name == contest.my_name;
        add_lobby_participant_node(p, is_me, &lobby_participants_node)?;
    }
    document.get_existing_element_by_id("lobby-contest-id")?.set_text_content(Some(&contest.contest_id));
    Ok(())
}

// Note. If present, `id` must be unique across both boards.
fn set_square_highlight(
    id: Option<&str>, class: &str, layer: SquareHighlightLayer, board_idx: DisplayBoard, coord: Option<DisplayCoord>
) -> JsResult<()> {
    let document = web_document();
    if let Some(coord) = coord {
        let node = id.and_then(|id| document.get_element_by_id(id));
        let highlight_layer = document.get_existing_element_by_id(
            &square_highlight_layer_id(layer, board_idx)
        )?;
        let node = node.ok_or(JsValue::UNDEFINED).or_else(|_| -> JsResult<web_sys::Element> {
            let node = document.create_svg_element("rect")?;
            if let Some(id) = id {
                node.set_attribute("id", id)?;
            }
            node.set_attribute("class", class)?;
            node.set_attribute("width", "1")?;
            node.set_attribute("height", "1")?;
            highlight_layer.append_child(&node)?;
            Ok(node)
        })?;
        let pos = DisplayFCoord::square_pivot(coord);
        node.set_attribute("x", &pos.x.to_string())?;
        node.set_attribute("y", &pos.y.to_string())?;
    } else {
        let Some(id) = id else {
            return Err(rust_error!(r#"Cannot reset square highlight without ID; class is "{class}""#));
        };
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

fn clear_square_highlight_layer(layer: SquareHighlightLayer) -> JsResult<()> {
    let document = web_document();
    for board_idx in DisplayBoard::iter() {
        let layer = document.get_existing_element_by_id(&square_highlight_layer_id(layer, board_idx))?;
        remove_all_children(&layer)?;
    }
    Ok(())
}

// Improvement potential: Find a better icon for connection problems.
// Improvement potential: Add a tooltip explaining the meaning of the icons.
fn participant_prefix(p: &Participant, show_readiness: bool) -> &'static str {
    if p.faction == Faction::Observer {
        return "👀 "
    }
    if !p.is_online {
        return "⚠️ ";
    }
    if show_readiness {
        return if p.is_ready { "☑ " } else { "☐ " };
    }
    ""
}

fn participant_string(p: &Participant, show_readiness: bool) -> String {
    format!("{}{}", participant_prefix(p, show_readiness), p.name)
}

fn make_menu_icon(images: &[&str]) -> JsResult<web_sys::Element> {
    let document = web_document();
    let svg_node = document.create_svg_element("svg")?;
    svg_node.set_attribute("viewBox", "0 0 10 10")?;
    svg_node.set_attribute("class", "lobby-icon")?;
    for img in images {
        let use_node = document.create_svg_element("use")?;
        use_node.set_attribute("href", &format!("#{img}"))?;
        use_node.set_attribute("class", img)?;
        svg_node.append_child(&use_node)?;
    }
    Ok(svg_node)
}

fn add_lobby_participant_node(p: &Participant, is_me: bool, parent: &web_sys::Element) -> JsResult<()> {
    let document = web_document();
    let add_relation_class = |node: &web_sys::Element| {
        node.class_list().add_1(if is_me { "lobby-me" } else { "lobby-other" })
    };
    {
        let name_node = document.create_element("div")?;
        name_node.set_attribute("class", "lobby-name")?;
        add_relation_class(&name_node)?;
        name_node.set_text_content(Some(&p.name));
        parent.append_child(&name_node)?;
    }
    {
        let faction_node = match p.faction {
            Faction::Fixed(Team::Red) => make_menu_icon(&["faction-red"])?,
            Faction::Fixed(Team::Blue) => make_menu_icon(&["faction-blue"])?,
            Faction::Random => make_menu_icon(&["faction-random"])?,
            Faction::Observer => make_menu_icon(&["faction-observer"])?,
        };
        add_relation_class(&faction_node)?;
        if is_me {
            faction_node.set_id("my-faction");
        }
        parent.append_child(&faction_node)?;
    }
    {
        // TODO: Remove readiness for observers.
        let readiness_node = match p.is_ready {
            false => make_menu_icon(&["readiness-checkbox"])?,
            true => make_menu_icon(&["readiness-checkbox", "readiness-checkmark"])?,
        };
        add_relation_class(&readiness_node)?;
        if is_me {
            readiness_node.set_id("my-readiness");
        }
        parent.append_child(&readiness_node)?;
    }
    Ok(())
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

fn update_reserve(
    reserve: &Reserve, force: Force, board_idx: DisplayBoard, player_idx: DisplayPlayer, is_draggable: bool
) -> JsResult<()> {
    let piece_kind_sep = 1.0;
    let reserve_iter = reserve.iter()
        .filter(|(kind, _)| *kind != PieceKind::King)
        .map(|(piece_kind, &amount)| (piece_kind, amount));
    render_reserve(force, board_idx, player_idx, is_draggable, piece_kind_sep, reserve_iter)
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

fn render_clock(clock: &Clock, force: Force, now: GameInstant, clock_node: &web_sys::Element)
    -> JsResult<()>
{
    let ClockShowing{ is_active, show_separator, out_of_time, time_breakdown } = clock.showing_for(force, now);
    let separator = |s| if show_separator { s } else { " " };
    let clock_str = match time_breakdown {
        TimeBreakdown::NormalTime{ minutes, seconds } => format!("{:02}{}{:02}", minutes, separator(":"), seconds),
        TimeBreakdown::LowTime{ seconds, deciseconds } => format!("{:02}{}{}", seconds, separator("."), deciseconds),
    };
    clock_node.set_text_content(Some(&clock_str));
    let mut classes = vec!["clock"];
    if out_of_time {
        classes.push("clock-flag");
    } else {
        classes.push(if is_active { "clock-active" } else { "clock-inactive" });
        if matches!(time_breakdown, TimeBreakdown::LowTime{ .. }) {
            classes.push("clock-low-time");
        }
    }
    clock_node.set_attribute("class", &classes.join(" "))?;
    Ok(())
}

fn update_scores(
    scores: &Scores, participants: &[Participant], game_status: BughouseGameStatus,
    teaming: Teaming, my_id: BughouseParticipantId
) -> JsResult<()> {
    let normalize = |score: u32| (score as f64) / 2.0;
    let team_node = web_document().get_existing_element_by_id("score-team")?;
    let individual_node = web_document().get_existing_element_by_id("score-individual")?;
    match teaming {
        Teaming::FixedTeams => {
            // TODO: Display "0:0" score before the first game.
            assert!(scores.per_player.is_empty());
            let my_team = my_id.visual_team();
            team_node.set_text_content(Some(&format!(
                "{}\n⎯\n{}",
                normalize(*scores.per_team.get(&my_team.opponent()).unwrap_or(&0)),
                normalize(*scores.per_team.get(&my_team).unwrap_or(&0)),
            )));
            individual_node.set_text_content(None);
        },
        Teaming::IndividualMode => {
            assert!(scores.per_team.is_empty());
            let show_readiness = game_status != BughouseGameStatus::Active;
            let scores = scores.per_player.iter().map(|(name, score)| {
                let participant = participants.iter().find(|p| p.name == *name).unwrap();
                (
                    name,
                    format!("{}: {}", participant_string(participant, show_readiness), normalize(*score))
                )
            });
            let scores = scores
                .sorted_by_key(|(name, _)| name.clone())  // TODO: Can we do without `clone()`?
                .map(|(_, display_string)| display_string)
                .join("\n");
            team_node.set_text_content(None);
            individual_node.set_text_content(Some(&scores));
        }
    }
    Ok(())
}

fn update_observers(participants: &[Participant]) -> JsResult<()> {
    let observers_node = web_document().get_existing_element_by_id("observers")?;
    let text = participants.iter()
        .filter(|p| p.faction == Faction::Observer)
        .map(|p| participant_string(p, false))
        .join("\n");
    observers_node.set_text_content(Some(&text));
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
    let log_node = document.get_existing_element_by_id(&turn_log_node_id(display_board_idx))?;
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
    // Note. The log will be scrolled to bottom whenever a turn is made on a given board (see
    // `NotableEvent::TurnMade` handler). Another strategy would've been to keep the log scrolled
    // to bottom if it was already there. I found two ways of doing this, but unfortunately none
    // of them works well:
    //   - This could done in JS, using this code to find if an element is at bottom:
    //        e.scroll_top() >= e.scroll_height() - e.client_height() - 1
    //     (as https://developer.mozilla.org/en-US/docs/Web/API/Element/scrollHeight#determine_if_an_element_has_been_totally_scrolled suggests)
    //     But the test is very slow. It made the entire `update_state` an order of magnitued slower,
    //     increasing update time from 1-10 ms to 10-100 ms.
    //   - This could be done in CSS, via `scroll-snap-type`:
    //       https://stackoverflow.com/a/60546366/3092679
    //     but the snap range is too large (especially in Firefox), so it becomes very hard to browse
    //     the log.
    Ok(())
}

fn setup_participation_mode(observer: bool) -> JsResult<()> {
    let body = web_document().body()?;
    body.class_list().toggle_with_force("observer", observer)?;
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

    add_layer(square_highlight_layer_id(SquareHighlightLayer::Turn, board_idx))?;
    add_layer(chalk_highlight_layer_id(board_idx))?;
    add_layer(piece_layer_id(board_idx))?;
    // Place drag highlight layer above pieces to allow legal move highlight for captures.
    // Note that the dragged piece will still be above the highlight.
    add_layer(square_highlight_layer_id(SquareHighlightLayer::Drag, board_idx))?;
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

fn clock_node_id(board_idx: DisplayBoard, player_idx: DisplayPlayer) -> String {
    format!("clock-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn turn_log_scroll_area_node_id(board_idx: DisplayBoard) -> String {
    format!("turn-log-scroll-area-{}", board_id(board_idx))
}

fn turn_log_node_id(board_idx: DisplayBoard) -> String {
    format!("turn-log-{}", board_id(board_idx))
}

fn piece_layer_id(board_idx: DisplayBoard) -> String {
    format!("piece-layer-{}", board_id(board_idx))
}

fn square_highlight_layer_id(layer: SquareHighlightLayer, board_idx: DisplayBoard) -> String {
    let layer_id = match layer {
        SquareHighlightLayer::Drag => "drag",
        SquareHighlightLayer::Turn => "turn",
    };
    format!("{}-highlight-layer-{}", layer_id, board_id(board_idx))
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
