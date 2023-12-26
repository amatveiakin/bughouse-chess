#![feature(anonymous_lifetime_in_impl_trait)]
#![feature(let_chains)]
#![cfg_attr(feature = "strict", deny(warnings))]
// Suppress Rust analyzer diagnostics like:
//   Function `__wasm_bindgen_generated_WebClient_update_state` should have snake_case ...
#![allow(non_snake_case)]

extern crate console_error_panic_hook;
extern crate enum_map;
extern crate instant;
extern crate serde_json;
extern crate strum;
extern crate wasm_bindgen;

extern crate bughouse_chess;

mod bughouse_prelude;
mod html_collection_iterator;
mod rules_ui;
mod svg;
mod web_chat;
mod web_document;
mod web_element_ext;
mod web_error_handling;
mod web_util;

use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Duration;

use bughouse_chess::client::*;
use bughouse_chess::lobby::*;
use bughouse_chess::meter::*;
use bughouse_chess::session::*;
use enum_map::enum_map;
use html_collection_iterator::IntoHtmlCollectionIterator;
use instant::Instant;
use itertools::Itertools;
use strum::IntoEnumIterator;
use wasm_bindgen::prelude::*;
use web_document::{web_document, WebDocument};
use web_element_ext::WebElementExt;
use web_error_handling::{JsResult, RustError};
use web_sys::{ScrollBehavior, ScrollIntoViewOptions, ScrollLogicalPosition};
use web_util::scroll_to_bottom;

use crate::bughouse_prelude::*;


const RESERVE_HEIGHT: f64 = 1.5; // total reserve area height, in squares
const RESERVE_PADDING: f64 = 0.25; // padding between board and reserve, in squares
const TOTAL_FOG_TILES: u64 = 3;
const FOG_TILE_SIZE: f64 = 1.2;

#[wasm_bindgen(getter_with_clone)]
pub struct IgnorableError {
    pub message: String,
}

#[wasm_bindgen(getter_with_clone)]
pub struct KickedFromMatch {
    pub message: String,
}

#[wasm_bindgen(getter_with_clone)]
pub struct FatalError {
    pub message: String,
}

#[wasm_bindgen]
pub struct JsMeter {
    meter: Meter,
}

#[wasm_bindgen]
impl JsMeter {
    fn new(meter: Meter) -> Self { JsMeter { meter } }

    // Note. It is possible to have a u64 argument, but it's passed as BigInt:
    // https://rustwasm.github.io/docs/wasm-bindgen/reference/browser-support.html
    pub fn record(&self, value: f64) {
        assert!(value >= 0.0);
        self.meter.record(value as u64);
    }
}

#[wasm_bindgen(getter_with_clone)]
pub struct JsSession {
    pub status: String,
    pub user_name: String,
    pub email: String,
    pub registration_method: String,
}

#[wasm_bindgen]
pub struct JsEventNoop {} // in contrast to `null`, indicates that event list is not over

#[wasm_bindgen]
pub struct JsEventSessionUpdated {}

#[wasm_bindgen(getter_with_clone)]
pub struct JsEventMatchStarted {
    pub match_id: String,
}

#[wasm_bindgen]
pub struct JsEventGameStarted {}

#[wasm_bindgen(getter_with_clone)]
pub struct JsEventGameOver {
    pub result: String,
}

#[wasm_bindgen(getter_with_clone)]
pub struct JsEventPlaySound {
    pub audio: String,
    pub pan: f64,
}

#[wasm_bindgen(getter_with_clone)]
pub struct JsEventGameExportReady {
    pub content: String,
}


#[wasm_bindgen]
pub struct WebClient {
    // Improvement potential: Consider: in order to store additional information that
    //   is only relevant during game phase, add a generic `UserData` parameter to
    //   `MatchState::Game`. Could move `chalk_canvas` there, for example.
    state: ClientState,
}

#[wasm_bindgen]
impl WebClient {
    pub fn new_client(user_agent: String, time_zone: String) -> JsResult<WebClient> {
        Ok(WebClient {
            state: ClientState::new(user_agent, time_zone),
        })
    }

    pub fn session(&self) -> JsResult<JsValue> {
        use Session::*;
        let status = match self.state.session() {
            Unknown => "unknown",
            LoggedOut => "logged_out",
            LoggedIn(_) => "logged_in",
            GoogleOAuthRegistering(_) => "google_oauth_registering",
        };
        let user_name = match self.state.session() {
            Unknown | LoggedOut | GoogleOAuthRegistering(_) => String::new(),
            LoggedIn(UserInfo { user_name, .. }) => user_name.clone(),
        };
        let email = match self.state.session() {
            Unknown | LoggedOut => String::new(),
            LoggedIn(UserInfo { email, .. }) => email.clone().unwrap_or(String::new()),
            GoogleOAuthRegistering(GoogleOAuthRegistrationInfo { email }) => email.clone(),
        };
        let registration_method = match self.state.session() {
            Unknown | LoggedOut => String::new(),
            GoogleOAuthRegistering(_) => RegistrationMethod::GoogleOAuth.to_string(),
            LoggedIn(UserInfo { registration_method, .. }) => registration_method.to_string(),
        };
        Ok(JsSession {
            status: status.to_owned(),
            user_name,
            email,
            registration_method,
        }
        .into())
    }

    pub fn meter(&mut self, name: String) -> JsMeter { JsMeter::new(self.state.meter(name)) }

    pub fn got_server_welcome(&self) -> bool { self.state.got_server_welcome() }
    pub fn hot_reconnect(&mut self) { self.state.hot_reconnect(); }
    pub fn current_turnaround_time(&self) -> f64 {
        self.state.current_turnaround_time().as_secs_f64()
    }

    pub fn observer_status(&self) -> String {
        let Some(my_faction) = self.state.my_faction() else {
            return "no".to_owned();
        };
        match my_faction {
            Faction::Observer => "permanently",
            Faction::Fixed(_) | Faction::Random => {
                let my_id = self.state.my_id();
                if my_id == Some(BughouseParticipant::Observer) {
                    "temporary"
                } else {
                    "no"
                }
            }
        }
        .to_owned()
    }

    pub fn game_status(&self) -> String {
        if let Some(game_state) = self.state.game_state() {
            if game_state.alt_game.is_active() {
                "active"
            } else {
                "over"
            }
        } else {
            "none"
        }
        .to_owned()
    }

    pub fn lobby_waiting_explanation(&self) -> String {
        let Some(mtch) = self.state.mtch() else {
            return "".to_owned();
        };
        type Error = ParticipantsError;
        type Warning = ParticipantsWarning;
        let ParticipantsStatus { error, warning } =
            verify_participants(&mtch.rules, mtch.participants.iter());
        match (error, warning) {
            (Some(Error::NotEnoughPlayers), _) => "Not enough players",
            (Some(Error::TooManyPlayersTotal), _) => "Too many players",
            (Some(Error::EmptyTeam), _) => "A team is empty",
            (Some(Error::RatedDoublePlay), _) =>
                "Playing on two boards is only allowed in unrated matches",
            (Some(Error::NotReady) | None, Some(Warning::NeedToDoublePlayAndSeatOut)) =>
                "👉🏾 Can start, but some players will have to play on two boards while others will have to seat out",
            (Some(Error::NotReady) | None, Some(Warning::NeedToDoublePlay)) =>
                "👉🏾 Can start, but some players will have to play on two boards",
            (Some(Error::NotReady) | None, Some(Warning::NeedToSeatOut)) =>
                "👉🏾 Can start, but some players will have to seat out each game",
            (Some(Error::NotReady), None) => "👍🏾 Will start when everyone is ready",
            (None, None) => "",
        }.to_owned()
    }
    pub fn lobby_countdown_seconds_left(&self) -> Option<u32> {
        self.state.first_game_countdown_left().map(|d| d.as_secs_f64().ceil() as u32)
    }

    pub fn init_new_match_rules_body(&self) -> JsResult<()> {
        let server_options = self.state.server_options().ok_or_else(|| rust_error!())?;
        rules_ui::make_new_match_rules_body(server_options)?;
        update_new_match_rules_body()?;
        Ok(())
    }

    pub fn set_guest_player_name(&mut self, player_name: Option<String>) -> JsResult<()> {
        // Can never be certain if JS passes an empty string or null.
        let player_name = player_name.filter(|s| !s.is_empty());
        self.state.set_guest_player_name(player_name);
        Ok(())
    }
    pub fn new_match(&mut self) -> JsResult<()> {
        use rules_ui::*;
        let variants = new_match_rules_variants()?;
        let details = new_match_rules_form_data()?;

        // Chess variants
        let fairy_pieces = match variants.get(FAIRY_PIECES).unwrap().as_str() {
            "off" => FairyPieces::NoFairy,
            "accolade" => FairyPieces::Accolade,
            s => return Err(format!("Invalid fairy pieces: {s}").into()),
        };
        let starting_position = match variants.get(STARTING_POSITION).unwrap().as_str() {
            "off" => StartingPosition::Classic,
            "fischer-random" => StartingPosition::FischerRandom,
            s => return Err(format!("Invalid starting position: {s}").into()),
        };
        let duck_chess = match variants.get(DUCK_CHESS).unwrap().as_str() {
            "off" => false,
            "on" => true,
            s => return Err(format!("Invalid duck chess option: {s}").into()),
        };
        let atomic_chess = match variants.get(ATOMIC_CHESS).unwrap().as_str() {
            "off" => false,
            "on" => true,
            s => return Err(format!("Invalid atomic chess option: {s}").into()),
        };
        let fog_of_war = match variants.get(FOG_OF_WAR).unwrap().as_str() {
            "off" => false,
            "on" => true,
            s => return Err(format!("Invalid fog of war option: {s}").into()),
        };
        let koedem = match variants.get(KOEDEM).unwrap().as_str() {
            "off" => false,
            "on" => true,
            s => return Err(format!("Invalid koedem option: {s}").into()),
        };

        // Other chess rules
        let promotion = match details.get(PROMOTION).as_string().unwrap().as_str() {
            "upgrade" => Promotion::Upgrade,
            "discard" => Promotion::Discard,
            "steal" => Promotion::Steal,
            s => return Err(format!("Invalid promotion: {s}").into()),
        };
        let drop_aggression = match details.get(DROP_AGGRESSION).as_string().unwrap().as_str() {
            "no-check" => DropAggression::NoCheck,
            "no-chess-mate" => DropAggression::NoChessMate,
            "no-bughouse-mate" => DropAggression::NoBughouseMate,
            "mate-allowed" => DropAggression::MateAllowed,
            s => return Err(format!("Invalid drop aggression: {s}").into()),
        };

        let starting_time = details.get(STARTING_TIME).as_string().unwrap();
        let Some((Ok(starting_minutes), Ok(starting_seconds))) =
            starting_time.split(':').map(|v| v.parse::<u64>()).collect_tuple()
        else {
            return Err(format!("Invalid starting time: {starting_time}").into());
        };
        let starting_time = Duration::from_secs(starting_minutes * 60 + starting_seconds);

        let pawn_drop_ranks = details.get(PAWN_DROP_RANKS).as_string().unwrap();
        let Some((Some(min_pawn_drop_rank), Some(max_pawn_drop_rank))) = pawn_drop_ranks
            .split('-')
            .map(|v| v.parse::<i8>().ok().map(SubjectiveRow::from_one_based))
            .collect_tuple()
        else {
            return Err(format!("Invalid pawn drop ranks: {pawn_drop_ranks}").into());
        };

        // Non-chess rules
        let rated = match details.get(RATING).as_string().unwrap().as_str() {
            "rated" => true,
            "unrated" => false,
            s => return Err(format!("Invalid rating: {s}").into()),
        };

        // Combine everything together
        let match_rules = MatchRules { rated };
        let mut chess_rules = ChessRules {
            fairy_pieces,
            starting_position,
            duck_chess,
            atomic_chess,
            fog_of_war,
            time_control: TimeControl { starting_time },
            bughouse_rules: Some(BughouseRules {
                koedem,
                promotion,
                min_pawn_drop_rank,
                max_pawn_drop_rank,
                drop_aggression,
            }),
        };
        if chess_rules.regicide() {
            chess_rules.bughouse_rules.as_mut().unwrap().drop_aggression =
                DropAggression::MateAllowed;
        }
        let rules = Rules { match_rules, chess_rules };
        if let Err(message) = rules.verify() {
            return Err(IgnorableError { message }.into());
        }
        self.state.new_match(rules);
        Ok(())
    }

    pub fn join(&mut self, match_id: String) -> JsResult<()> {
        self.state.join(match_id);
        Ok(())
    }
    pub fn resign(&mut self) { self.state.resign(); }
    pub fn toggle_ready(&mut self) {
        if let Some(is_ready) = self.state.is_ready() {
            self.state.set_ready(!is_ready);
        }
    }
    pub fn next_faction(&mut self) { self.change_faction(|f| f + 1); }
    pub fn previous_faction(&mut self) { self.change_faction(|f| f - 1); }
    pub fn request_export(&mut self) -> JsResult<()> {
        let format = pgn::BughouseExportFormat {};
        self.state.request_export(format);
        Ok(())
    }
    pub fn leave_match(&mut self) { self.state.leave_match(); }

    pub fn execute_input(&mut self, input: &str) { self.state.execute_input(input); }
    pub fn clear_ephemeral_chat_items(&mut self) { self.state.clear_ephemeral_chat_items(); }
    pub fn show_command_result(&mut self, text: String) { self.state.show_command_result(text); }
    pub fn show_command_error(&mut self, text: String) { self.state.show_command_error(text); }

    pub fn click_board(&mut self, board_id: &str, x: f64, y: f64) -> JsResult<()> {
        // Note: cannot use "data-bughouse-location" attribute: squares are not the click targets
        // when obscured by the fog tiles.
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        let board_shape = alt_game.board_shape();
        let pos = DisplayFCoord { x, y };
        let Some(display_coord) = pos.to_square(board_shape) else {
            return Ok(());
        };
        let display_board_idx = parse_board_id(board_id)?;
        let board_orientation = get_board_orientation(display_board_idx, alt_game.perspective());
        let coord = from_display_coord(display_coord, board_shape, board_orientation).unwrap();
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        if let Some((input_board_idx, turn_input)) = alt_game.click_square(board_idx, coord) {
            let display_input_board_idx =
                get_display_board_index(input_board_idx, alt_game.perspective());
            _ = self.state.make_turn(display_input_board_idx, turn_input);
        }
        Ok(())
    }

    pub fn choose_promotion_upgrade(&mut self, board_id: &str, piece_kind: &str) -> JsResult<()> {
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        let display_board_idx = parse_board_id(board_id)?;
        let piece_kind = PieceKind::from_algebraic(piece_kind)
            .ok_or(rust_error!("Invalid piece kind: {piece_kind}"))?;
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        if let Some(turn_input) = alt_game.choose_promotion_upgrade(board_idx, piece_kind) {
            _ = self.state.make_turn(display_board_idx, turn_input);
        }
        Ok(())
    }

    pub fn start_drag_piece(&mut self, source: &str) -> JsResult<String> {
        let alt_game = self.state.alt_game_mut().ok_or_else(|| rust_error!())?;
        let board_shape = alt_game.board_shape();
        let (display_board_idx, source) = if let Some((display_board_idx, piece)) =
            parse_reserve_piece_id(source)
        {
            (display_board_idx, PieceDragStart::Reserve(piece))
        } else if let Some((display_board_idx, coord)) = parse_square_id(source) {
            let board_orientation =
                get_board_orientation(display_board_idx, alt_game.perspective());
            set_square_highlight(
                None,
                "drag-start-highlight",
                SquareHighlightLayer::Drag,
                display_board_idx,
                Some(to_display_coord(coord, board_shape, board_orientation)),
            )?;
            let board_idx = get_board_index(display_board_idx, alt_game.perspective());
            // Note. The name "nontrivial" part of `nontrivial_fairy_pieces` comes from the fact
            // that duck is, in some sense, a fairy piece, but not a one that requires move hints.
            // Improvement potential. More conistent legal moves highlighting. Perhaps, add a config
            //   with "Yes" / "No" / "If fairy chess" values.
            let rules = alt_game.chess_rules();
            let nontrivial_fairy_pieces = match rules.fairy_pieces {
                FairyPieces::NoFairy => false,
                FairyPieces::Accolade => true,
            };
            if nontrivial_fairy_pieces && !rules.fog_of_war {
                for dest in alt_game.local_game().board(board_idx).legal_turn_destinations(coord) {
                    set_square_highlight(
                        None,
                        "legal-move-highlight",
                        SquareHighlightLayer::Drag,
                        display_board_idx,
                        Some(to_display_coord(dest, board_shape, board_orientation)),
                    )?;
                }
            }
            (display_board_idx, PieceDragStart::Board(coord))
        } else {
            return Err(rust_error!("Illegal drag source: {source:?}"));
        };
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        match alt_game.start_drag_piece(board_idx, source) {
            Ok(_) => Ok(board_id(display_board_idx).to_owned()),
            Err(_) => {
                self.reset_drag_highlights()?;
                Ok("abort".to_owned())
            }
        }
    }

    pub fn drag_piece(&mut self, board_id: &str, x: f64, y: f64) -> JsResult<()> {
        let Some(GameState { alt_game, .. }) = self.state.game_state() else {
            return Err(rust_error!());
        };
        let board_shape = alt_game.board_shape();
        let display_board_idx = parse_board_id(board_id)?;
        let pos = DisplayFCoord { x, y };
        set_square_highlight(
            Some("drag-over-highlight"),
            "drag-over-highlight",
            SquareHighlightLayer::Drag,
            display_board_idx,
            pos.to_square(board_shape),
        )
    }

    pub fn drag_piece_drop(&mut self, board_id: &str, x: f64, y: f64) -> JsResult<()> {
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        let board_shape = alt_game.board_shape();
        let display_board_idx = parse_board_id(board_id)?;
        let pos = DisplayFCoord { x, y };
        if let Some(dest_display) = pos.to_square(board_shape) {
            use PieceDragError::*;
            let board_idx = get_board_index(display_board_idx, alt_game.perspective());
            let board_orientation =
                get_board_orientation(display_board_idx, alt_game.perspective());
            let dest_coord =
                from_display_coord(dest_display, board_shape, board_orientation).unwrap();
            match alt_game.drag_piece_drop(board_idx, dest_coord) {
                Ok(None) => {
                    // Probably a partial turn input. Awaiting completion.
                }
                Ok(Some(turn_input)) => {
                    _ = self.state.make_turn(display_board_idx, turn_input);
                }
                Err(CannotCastleDroppedKing) => {
                    self.state.show_turn_result(Err(TurnError::CannotCastleDroppedKing));
                }
                Err(DragIllegal) => {
                    // Ignore: tried to make an illegal move (this is usually checked later, but
                    // sometimes now).
                }
                Err(DragNoLongerPossible) => {
                    // Ignore: this happen when dragged piece was captured by opponent.
                }
                Err(Cancelled) => {
                    // Ignore: user cancelled the move by putting the piece back in place.
                }
                Err(err @ (DragForbidden | NoDragInProgress | PieceNotFound)) => {
                    return Err(rust_error!("Drag&drop error: {:?}", err));
                }
            };
        } else {
            alt_game.abort_drag_piece();
        }
        Ok(())
    }

    pub fn abort_drag_piece(&mut self) -> JsResult<()> {
        if let Some(alt_game) = self.state.alt_game_mut() {
            if alt_game.piece_drag_state() != PieceDragState::NoDrag {
                alt_game.abort_drag_piece();
            }
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
        (if let Some(GameState { ref alt_game, .. }) = self.state.game_state() {
            match alt_game.piece_drag_state() {
                PieceDragState::NoDrag => "no",
                PieceDragState::Dragging { .. } => "yes",
                PieceDragState::Defunct => "defunct",
            }
        } else {
            "no"
        })
        .to_owned()
    }

    pub fn cancel_preturn(&mut self, board_id: &str) -> JsResult<()> {
        self.state.cancel_preturn(parse_board_id(board_id)?);
        Ok(())
    }

    pub fn is_chalk_active(&self) -> bool {
        self.state.chalk_canvas().map_or(false, |c| c.is_painting())
    }
    pub fn chalk_down(
        &mut self, board_node: &str, x: f64, y: f64, alternative_mode: bool,
    ) -> JsResult<()> {
        let Some(GameState { alt_game, .. }) = self.state.game_state() else {
            return Ok(());
        };
        if alt_game.is_active() {
            return Ok(());
        }
        let Some(canvas) = self.state.chalk_canvas_mut() else {
            return Ok(());
        };
        let board_idx = parse_board_node_id(board_node)?;
        canvas.chalk_down(board_idx, DisplayFCoord { x, y }, alternative_mode);
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_move(&mut self, x: f64, y: f64) -> JsResult<()> {
        let Some(canvas) = self.state.chalk_canvas_mut() else {
            return Ok(());
        };
        canvas.chalk_move(DisplayFCoord { x, y });
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_up(&mut self, x: f64, y: f64) -> JsResult<()> {
        let Some(canvas) = self.state.chalk_canvas_mut() else {
            return Ok(());
        };
        let Some((board_idx, mark)) = canvas.chalk_up(DisplayFCoord { x, y }) else {
            return Ok(());
        };
        self.state.add_chalk_mark(board_idx, mark);
        self.repaint_chalk()?;
        Ok(())
    }
    pub fn chalk_abort(&mut self) -> JsResult<()> {
        let Some(canvas) = self.state.chalk_canvas_mut() else {
            return Ok(());
        };
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
        let Some(GameState { alt_game, chalkboard, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let document = web_document();
        for board_idx in DisplayBoard::iter() {
            document
                .get_existing_element_by_id(&chalk_highlight_layer_id(board_idx))?
                .remove_all_children();
            document
                .get_existing_element_by_id(&chalk_drawing_layer_id(board_idx))?
                .remove_all_children();
        }
        for (player_name, drawing) in chalkboard.all_drawings() {
            let owner = self.state.relation_to(player_name);
            for board_idx in DisplayBoard::iter() {
                for mark in drawing.board(get_board_index(board_idx, alt_game.perspective())) {
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
        self.state.process_server_event(server_event).map_err(|err| -> JsValue {
            match err {
                EventError::Ignorable(message) => IgnorableError { message }.into(),
                EventError::KickedFromMatch(message) => KickedFromMatch { message }.into(),
                EventError::Fatal(message) => FatalError { message }.into(),
                EventError::Internal(message) => RustError { message }.into(),
            }
        })?;
        Ok(updated_needed)
    }

    pub fn next_notable_event(&mut self) -> JsResult<JsValue> {
        match self.state.next_notable_event() {
            Some(NotableEvent::SessionUpdated) => Ok(JsEventSessionUpdated {}.into()),
            Some(NotableEvent::MatchStarted(match_id)) => {
                init_lobby(&self.state.mtch().unwrap())?;
                Ok(JsEventMatchStarted { match_id }.into())
            }
            Some(NotableEvent::GameStarted) => {
                let Some(GameState { ref alt_game, .. }) = self.state.game_state() else {
                    return Err(rust_error!());
                };
                // Improvement potential. Add an <hr> style separator between games in chat.
                let my_id = alt_game.my_id();
                render_boards(alt_game.board_shape(), alt_game.perspective())?;
                setup_participation_mode(my_id)?;
                web_chat::render_chat_reference_tooltip(my_id, self.state.team_chat_enabled())?;
                for display_board_idx in DisplayBoard::iter() {
                    scroll_log_to_bottom(display_board_idx)?;
                }
                Ok(JsEventGameStarted {}.into())
            }
            Some(NotableEvent::GameOver(game_status)) => {
                let result = match game_status {
                    SubjectiveGameResult::Victory => "victory",
                    SubjectiveGameResult::Defeat => "defeat",
                    SubjectiveGameResult::Draw => "draw",
                    // Improvement potential. Add a separate sound for `Observation`.
                    SubjectiveGameResult::Observation => "draw",
                }
                .to_owned();
                Ok(JsEventGameOver { result }.into())
            }
            Some(NotableEvent::TurnMade(envoy)) => {
                let Some(GameState { ref alt_game, .. }) = self.state.game_state() else {
                    return Err(rust_error!());
                };
                let display_board_idx =
                    get_display_board_index(envoy.board_idx, alt_game.perspective());
                scroll_log_to_bottom(display_board_idx)?;
                if alt_game.my_id().plays_on_board(envoy.board_idx)
                    || alt_game.my_id().is_observer()
                {
                    return Ok(JsEventPlaySound {
                        audio: "turn".to_owned(),
                        pan: self.get_game_audio_pan(envoy.board_idx)?,
                    }
                    .into());
                }
                Ok(JsEventNoop {}.into())
            }
            Some(NotableEvent::MyReserveRestocked(board_idx)) => Ok(JsEventPlaySound {
                audio: "reserve_restocked".to_owned(),
                pan: self.get_game_audio_pan(board_idx)?,
            }
            .into()),
            Some(NotableEvent::PieceStolen) => Ok(JsEventPlaySound {
                audio: "piece_stolen".to_owned(),
                pan: 0.,
            }
            .into()),
            Some(NotableEvent::LowTime(board_idx)) => Ok(JsEventPlaySound {
                audio: "low_time".to_owned(),
                pan: self.get_game_audio_pan(board_idx)?,
            }
            .into()),
            Some(NotableEvent::GameExportReady(content)) => {
                Ok(JsEventGameExportReady { content }.into())
            }
            None => Ok(JsValue::NULL),
        }
    }

    pub fn next_outgoing_event(&mut self) -> Option<String> {
        self.state
            .next_outgoing_event()
            .map(|event| serde_json::to_string(&event).unwrap())
    }

    pub fn refresh(&mut self) { self.state.refresh(); }

    pub fn update_state(&self) -> JsResult<()> {
        let document = web_document();
        self.update_clock()?;
        let Some(mtch) = self.state.mtch() else {
            return Ok(());
        };
        update_observers(&mtch.participants)?;
        let Some(GameState { game_index, ref alt_game, .. }) = mtch.game_state else {
            update_lobby(mtch)?;
            return Ok(());
        };
        let game = alt_game.local_game();
        let board_shape = alt_game.board_shape();
        let my_id = alt_game.my_id();
        let perspective = alt_game.perspective();
        update_scores(&mtch.scores, &mtch.participants, game.status())?;
        for (board_idx, board) in game.boards() {
            let is_piece_draggable = |piece_force| {
                my_id
                    .envoy_for(board_idx)
                    .map_or(false, |e| board.can_potentially_move_piece(e.force, piece_force))
            };
            let is_glowing_duck = |piece: PieceOnBoard| {
                alt_game.is_my_duck_turn(board_idx) && piece.kind == PieceKind::Duck
            };
            let is_glowing_steal = |coord: Coord| {
                let Some((input_board_idx, partial_input)) = alt_game.partial_turn_input() else {
                    return false;
                };
                let Some(envoy) = my_id.envoy_for(input_board_idx) else {
                    return false;
                };
                if !matches!(partial_input, PartialTurnInput::StealPromotion { .. }) {
                    return false;
                }
                board_idx == input_board_idx.other()
                    && board.stealing_result(coord, envoy.force).is_ok()
            };
            let upgrade_promotion_target = if let Some((input_board_idx, partial_input)) =
                alt_game.partial_turn_input()
                && let PartialTurnInput::UpgradePromotion { to, .. } = partial_input
                && board_idx == input_board_idx
            {
                Some(to)
            } else {
                None
            };
            let my_force = my_id.envoy_for(board_idx).map(|e| e.force);
            let see_though_fog = alt_game.see_though_fog();
            let empty_area = HashSet::new();
            let fog_render_area = alt_game.fog_of_war_area(board_idx);
            let fog_cover_area = if see_though_fog { &empty_area } else { &fog_render_area };
            let display_board_idx = get_display_board_index(board_idx, perspective);
            let board_orientation = get_board_orientation(display_board_idx, perspective);
            let board_node =
                document.get_existing_element_by_id(&board_node_id(display_board_idx))?;
            let piece_layer =
                document.get_existing_element_by_id(&piece_layer_id(display_board_idx))?;
            let fog_of_war_layer =
                document.get_existing_element_by_id(&fog_of_war_layer_id(display_board_idx))?;
            let grid = board.grid();
            for coord in board_shape.coords() {
                let display_coord = to_display_coord(coord, board_shape, board_orientation);
                {
                    let node_id = fog_of_war_id(display_board_idx, coord);
                    if fog_render_area.contains(&coord) {
                        let sq_hash = calculate_hash(&(&mtch.match_id, board_idx, coord));
                        let fog_tile = sq_hash % TOTAL_FOG_TILES + 1;
                        let node = ensure_square_node(
                            display_coord,
                            &fog_of_war_layer,
                            &node_id,
                            FOG_TILE_SIZE,
                        )?;
                        node.set_attribute("href", &format!("#fog-{fog_tile}"))?;
                        node.remove_attribute("class")?;
                        // Improvement potential. To make fog look more varied, add variants:
                        //   let variant = (sq_hash / TOTAL_FOG_TILES) % 4;
                        //   node.class_list().add_1(&format!("fog-variant-{variant}"))?;
                        // and alter the variants. Ideas:
                        //   - Rotate the tiles 90, 180 or 270 degrees. Problem: don't know how to
                        //     rotate <use> element around center.
                        //     https://stackoverflow.com/questions/15138801/rotate-rectangle-around-its-own-center-in-svg
                        //     did not work.
                        //   - Shift colors somehow. Problem: tried `hue-rotate` and `saturate`, but
                        //     it's either unnoticeable or too visisble. Ideal would be to rotate hue
                        //     within bluish color range.
                    } else {
                        document.get_element_by_id(&node_id).inspect(|n| n.remove());
                    }
                }
                {
                    let node_id = square_id(display_board_idx, coord);
                    let node = ensure_square_node(display_coord, &piece_layer, &node_id, 1.0)?;
                    if !fog_cover_area.contains(&coord)
                        && let Some(piece) = grid[coord]
                    {
                        let filename = if let ChessGameStatus::Victory(winner, reason) =
                            board.status()
                            && reason == VictoryReason::Checkmate
                            && piece.kind == PieceKind::King
                            && piece.force == winner.opponent().into()
                        {
                            broken_king_path(piece.force)
                        } else {
                            piece_path(piece.kind, piece.force)
                        };
                        node.set_attribute("href", filename)?;
                        node.remove_attribute("class")?;
                        node.class_list()
                            .toggle_with_force("draggable", is_piece_draggable(piece.force))?;
                        node.class_list()
                            .toggle_with_force("glowing-duck", is_glowing_duck(piece))?;
                        node.class_list()
                            .toggle_with_force("glowing-steal", is_glowing_steal(coord))?;
                    } else {
                        node.set_attribute("href", "#transparent")?;
                        node.remove_attribute("class")?;
                    }
                }
            }
            render_upgrade_promotion_selector(
                my_force,
                display_board_idx,
                upgrade_promotion_target
                    .map(|c| to_display_coord(c, board_shape, board_orientation)),
            )?;
            fog_of_war_layer
                .class_list()
                .toggle_with_force("see-though-fog", see_though_fog)?;
            for force in Force::iter() {
                let player_idx = get_display_player(force, board_orientation);
                let p_node = document.get_existing_element_by_id(&player_name_node_id(
                    display_board_idx,
                    player_idx,
                ))?;
                let player_name = board.player_name(force);
                let player = mtch.participants.iter().find(|p| p.name == *player_name).unwrap();
                // TODO: Show teams for the upcoming game in individual mode.
                let show_readiness = false;
                let name_content = participant_node(player, show_readiness)?;
                p_node.replace_children_with_node_1(&name_content);
                let is_draggable = is_piece_draggable(force.into());
                update_reserve(
                    board.reserve(force),
                    force,
                    display_board_idx,
                    player_idx,
                    is_draggable,
                    game.chess_rules(),
                )?;
            }
            let wayback = alt_game.wayback(board_idx);
            board_node.class_list().toggle_with_force("wayback", wayback.active())?;
            update_turn_log(&game, my_id, board_idx, display_board_idx, wayback)?;
        }
        self.update_turn_highlights()?;
        document
            .body()?
            .class_list()
            .toggle_with_force("active-player", is_clock_ticking(&game, my_id))?;
        let chat_node = web_document().get_existing_element_by_id("chat-text-area")?;
        web_chat::update_chat(
            &chat_node,
            &mtch.chat.items(&mtch.my_name, game.chess_rules(), Some(game_index)),
        )?;
        self.repaint_chalk()?;
        Ok(())
    }

    // Improvement potential. Time difference is the same for all players (modulo sign). Consider
    // showing it only once, e.g. add a colored hourglass/progressbar somewhere in the middle.
    pub fn update_clock(&self) -> JsResult<()> {
        let Some(GameState { ref alt_game, time_pair, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let now = Instant::now();
        let game_now = GameInstant::from_pair_game_maybe_active(*time_pair, now);
        let game = alt_game.local_game();
        let wayback_active =
            BughouseBoard::iter().any(|board_idx| alt_game.wayback(board_idx).active());
        for (board_idx, board) in game.boards() {
            let display_board_idx = get_display_board_index(board_idx, alt_game.perspective());
            let board_orientation =
                get_board_orientation(display_board_idx, alt_game.perspective());
            for force in Force::iter() {
                let team = get_bughouse_team(board_idx, force);
                let player_idx = get_display_player(force, board_orientation);
                let clock = board.clock();
                let other_clock = game.board(board_idx.other()).clock();
                let show_diff = match alt_game.my_id() {
                    BughouseParticipant::Player(player) => player.team() == team,
                    BughouseParticipant::Observer => true,
                };
                let diff = show_diff.then(|| clock.difference_for(force, other_clock, game_now));
                render_clock(
                    clock.showing_for(force, game_now),
                    diff,
                    display_board_idx,
                    player_idx,
                    wayback_active,
                )?;
            }
        }
        Ok(())
    }

    pub fn meter_stats(&self) -> String {
        self.state
            .read_meter_stats()
            .iter()
            .sorted_by_key(|(metric, _)| metric.as_str())
            .map(|(metric, stats)| format!("{metric}: {stats}"))
            .join("\n")
    }

    pub fn wayback_to_turn(&mut self, board_id: &str, turn_idx: Option<String>) -> JsResult<()> {
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        if alt_game.is_active() {
            return Ok(());
        }
        let display_board_idx = parse_board_id(board_id)?;
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        alt_game.wayback_to_turn(board_idx, turn_idx);
        Ok(())
    }

    pub fn on_vertical_arrow_key_down(&mut self, key: &str, ctrl: bool, alt: bool) -> JsResult<()> {
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        if alt_game.is_active() {
            return Ok(());
        }
        let display_board_idx = match alt {
            false => DisplayBoard::Primary,
            true => DisplayBoard::Secondary,
        };
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        match (key, ctrl) {
            ("ArrowDown", false) => alt_game.wayback_to_next(board_idx),
            ("ArrowDown", true) => alt_game.wayback_to_last(board_idx),
            ("ArrowUp", false) => alt_game.wayback_to_previous(board_idx),
            ("ArrowUp", true) => alt_game.wayback_to_first(board_idx),
            _ => {}
        };
        let node = alt_game.wayback(board_idx).turn_index().and_then(|index| {
            web_document().get_element_by_id(&turn_record_node_id(display_board_idx, index))
        });
        if let Some(node) = node {
            node.scroll_into_view_with_scroll_into_view_options(
                ScrollIntoViewOptions::new()
                    .behavior(ScrollBehavior::Instant)
                    .block(ScrollLogicalPosition::Nearest),
            );
        }
        Ok(())
    }

    pub fn readonly_rules_body(&self) -> JsResult<web_sys::Element> {
        let mtch = self.state.mtch().ok_or_else(|| rust_error!())?;
        let node = web_document().create_element("div")?;
        node.append_element(
            make_match_caption_body(&mtch)?.with_classes(["readonly-rules-match-caption"])?,
        )?;
        node.append_new_element("hr")?;
        node.append_element(rules_ui::make_readonly_rules_body(&mtch.rules)?)?;
        Ok(node)
    }

    fn change_faction(&mut self, faction_modifier: impl Fn(i32) -> i32) {
        let Some(mtch) = self.state.mtch() else {
            return;
        };
        let current = ALL_FACTIONS.iter().position(|&f| f == mtch.my_faction).unwrap();
        let new = faction_modifier(current.try_into().unwrap());
        let new = new.rem_euclid(ALL_FACTIONS.len().try_into().unwrap());
        let new: usize = new.try_into().unwrap();
        self.state.set_faction(ALL_FACTIONS[new]);
    }

    fn render_chalk_mark(
        &self, board_idx: DisplayBoard, owner: PlayerRelation, mark: &ChalkMark,
    ) -> JsResult<()> {
        use PlayerRelation::*;
        let Some(GameState { alt_game, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let document = web_document();
        let board_shape = alt_game.board_shape();
        let orientation = get_board_orientation(board_idx, alt_game.perspective());
        match mark {
            ChalkMark::Arrow { from, to } => {
                let layer =
                    document.get_existing_element_by_id(&chalk_drawing_layer_id(board_idx))?;
                let from =
                    DisplayFCoord::square_center(to_display_coord(*from, board_shape, orientation));
                let to =
                    DisplayFCoord::square_center(to_display_coord(*to, board_shape, orientation));
                let node = document.create_svg_element("line")?;
                let d = normalize_vec(to - from);
                let from = from + mult_vec(d, 0.3);
                let to = to + mult_vec(d, -0.45);
                node.set_attribute("x1", &from.x.to_string())?;
                node.set_attribute("y1", &from.y.to_string())?;
                node.set_attribute("x2", &to.x.to_string())?;
                node.set_attribute("y2", &to.y.to_string())?;
                node.set_attribute(
                    "class",
                    &["chalk-arrow", &chalk_line_color_class(owner)].join(" "),
                )?;
                layer.append_child(&node)?;
            }
            ChalkMark::FreehandLine { points } => {
                let layer =
                    document.get_existing_element_by_id(&chalk_drawing_layer_id(board_idx))?;
                let node = document.create_svg_element("polyline")?;
                let points = points
                    .iter()
                    .map(|&q| {
                        let p = to_display_fcoord(q, board_shape, orientation);
                        format!("{},{}", p.x, p.y)
                    })
                    .join(" ");
                node.set_attribute("points", &points)?;
                node.set_attribute(
                    "class",
                    &["chalk-freehand-line", &chalk_line_color_class(owner)].join(" "),
                )?;
                layer.append_child(&node)?;
            }
            ChalkMark::SquareHighlight { coord } => {
                let layer =
                    document.get_existing_element_by_id(&chalk_highlight_layer_id(board_idx))?;
                let node = document.create_svg_element("polygon")?;
                let p =
                    DisplayFCoord::square_pivot(to_display_coord(*coord, board_shape, orientation));
                // Note. The corners are chosen so that they corresponds to the seating, as seen
                // by the current player. Another approach would be to have one highlight element,
                // <use> it here and rotate in CSS based on class.
                let points = match owner {
                    Myself => vec![p + (0., 1.), p + (0.5, 1.), p + (0., 0.5)],
                    Opponent => vec![p + (0., 0.), p + (0., 0.5), p + (0.5, 0.)],
                    Partner => vec![p + (1., 1.), p + (1., 0.5), p + (0.5, 1.)],
                    Diagonal => vec![p + (1., 0.), p + (0.5, 0.), p + (1., 0.5)],
                    Other => vec![
                        p + (0.5, 0.1),
                        p + (0.1, 0.5),
                        p + (0.5, 0.9),
                        p + (0.9, 0.5),
                    ],
                };
                let points = points.iter().map(|&p| format!("{},{}", p.x, p.y)).join(" ");
                node.set_attribute("points", &points)?;
                node.set_attribute(
                    "class",
                    &["chalk-square-highlight", &chalk_square_color_class(owner)].join(" "),
                )?;
                layer.append_child(&node)?;
            }
        }
        Ok(())
    }

    fn update_turn_highlights(&self) -> JsResult<()> {
        // Optimization potential: do not reset highlights that stay in place.
        clear_square_highlight_layer(SquareHighlightLayer::Turn)?;
        clear_square_highlight_layer(SquareHighlightLayer::TurnAbove)?;
        let Some(GameState { ref alt_game, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let board_shape = alt_game.board_shape();
        for h in alt_game.turn_highlights() {
            let class = format!("turn-highlight {}", turn_highlight_class_id(&h));
            let display_board_idx = get_display_board_index(h.board_idx, alt_game.perspective());
            let board_orientation =
                get_board_orientation(display_board_idx, alt_game.perspective());
            let layer = turn_highlight_layer(h.layer);
            let display_coord = to_display_coord(h.coord, board_shape, board_orientation);
            set_square_highlight(None, &class, layer, display_board_idx, Some(display_coord))?;
        }
        Ok(())
    }

    fn get_game_audio_pan(&self, board_idx: BughouseBoard) -> JsResult<f64> {
        let Some(GameState { ref alt_game, .. }) = self.state.game_state() else {
            return Err(rust_error!());
        };
        let display_board_idx = get_display_board_index(board_idx, alt_game.perspective());
        get_audio_pan(alt_game.my_id(), display_board_idx)
    }
}

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShapeRendering {
    // Use by default.
    Normal,
    // Use for layers with board squared to avoid anti-aliasing artifacts.
    CrispEdges,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SquareHighlightLayer {
    Turn,      // last turn, preturn
    TurnAbove, // like `Turn`, but above the fog of war
    Drag,      // drag start, drag hover, legal moves
}

fn scroll_log_to_bottom(board_idx: DisplayBoard) -> JsResult<()> {
    let e = web_document().get_existing_element_by_id(&turn_log_scroll_area_node_id(board_idx))?;
    scroll_to_bottom(&e);
    Ok(())
}


#[wasm_bindgen]
pub fn init_page() -> JsResult<()> {
    generate_svg_markers()?;
    render_starting()?;
    web_chat::render_chat_reference_tooltip(BughouseParticipant::Observer, false)?;
    web_chat::render_chat_reference_dialog()?;
    Ok(())
}

#[wasm_bindgen]
pub fn update_new_match_rules_body() -> JsResult<()> {
    use rules_ui::*;
    let variants = new_match_rules_variants()?;
    let duck_chess = variants.get(DUCK_CHESS).unwrap() == "on";
    let atomic_chess = variants.get(ATOMIC_CHESS).unwrap() == "on";
    let fog_of_war = variants.get(FOG_OF_WAR).unwrap() == "on";
    let koedem = variants.get(KOEDEM).unwrap() == "on";
    // Should mirror `ChessRules::regicide`. Could've constructed `ChessRules` and called it
    // directly, but doing so could fail due to unrelated problems, e.g. errors in "starting time"
    // format.
    let regicide = duck_chess || atomic_chess || fog_of_war || koedem;
    for node in web_document().get_elements_by_class_name(REGICIDE_CLASS) {
        node.class_list().toggle_with_force("display-none", !regicide)?;
    }
    for node in web_document().get_elements_by_class_name(&rule_setting_class(DROP_AGGRESSION)) {
        node.class_list().toggle_with_force("display-none", regicide)?;
    }
    Ok(())
}

fn new_match_rules_variants() -> JsResult<HashMap<String, String>> {
    let body = web_document().get_existing_element_by_id("cc-rule-variants")?;
    let buttons = body.get_elements_by_class_name("rule-variant-button");
    let mut variants = HashMap::new();
    for button in buttons.into_iterator() {
        if !button.class_list().contains("display-none") {
            let name = button.get_attribute("data-variant-name").unwrap();
            let value = button.get_attribute("data-variant-value").unwrap();
            assert!(variants.insert(name, value).is_none());
        }
    }
    Ok(variants)
}

fn new_match_rules_form_data() -> JsResult<web_sys::FormData> {
    let node = web_document().get_existing_element_by_id("menu-create-match-page")?;
    web_sys::FormData::new_with_form(&node.dyn_into()?)
}

#[wasm_bindgen]
pub fn git_version() -> String { my_git_version!().to_owned() }

fn make_match_caption_body(mtch: &Match) -> JsResult<web_sys::Element> {
    let prefix = if mtch.rules.match_rules.rated {
        "Rated match "
    } else {
        "Unrated match "
    };
    let node = web_document().create_element("div")?;
    node.append_text_span(prefix, [])?;
    node.append_text_span(&mtch.match_id, ["lobby-match-id"])?;
    Ok(node)
}

// Try to keep ordering in sync with "New match" dialog.
// Improvement potential: Add tooltips (similarly to match creation dialog).
fn init_lobby(mtch: &Match) -> JsResult<()> {
    web_document()
        .get_existing_element_by_id("lobby-match-caption")?
        .set_children([make_match_caption_body(mtch)?])?;
    let rules_body = rules_ui::make_readonly_rules_body(&mtch.rules)?;
    let rules_node = web_document().get_existing_element_by_id("lobby-rules")?;
    rules_node.replace_children_with_node_1(&rules_body);
    Ok(())
}

fn update_lobby(mtch: &Match) -> JsResult<()> {
    let lobby_participants_node =
        web_document().get_existing_element_by_id("lobby-participants")?;
    lobby_participants_node.remove_all_children();
    for p in &mtch.participants {
        let is_me = p.name == mtch.my_name;
        add_lobby_participant_node(p, is_me, &lobby_participants_node)?;
    }
    Ok(())
}

fn ensure_square_node(
    display_coord: DisplayCoord, layer: &web_sys::Element, node_id: &str, size: f64,
) -> JsResult<web_sys::Element> {
    let node = web_document().ensure_svg_node("use", node_id, layer)?;
    let shift = (size - 1.0) / 2.0;
    let pos = DisplayFCoord::square_pivot(display_coord);
    node.set_attribute("x", &(pos.x - shift).to_string())?;
    node.set_attribute("y", &(pos.y - shift).to_string())?;
    node.set_attribute("data-bughouse-location", node_id)?;
    Ok(node)
}

// Note. If present, `id` must be unique across both boards.
fn set_square_highlight(
    id: Option<&str>, class: &str, layer: SquareHighlightLayer, board_idx: DisplayBoard,
    coord: Option<DisplayCoord>,
) -> JsResult<()> {
    let document = web_document();
    if let Some(coord) = coord {
        let node = id.and_then(|id| document.get_element_by_id(id));
        let highlight_layer =
            document.get_existing_element_by_id(&square_highlight_layer_id(layer, board_idx))?;
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
            return Err(rust_error!(
                r#"Cannot reset square highlight without ID; class is "{class}""#
            ));
        };
        if let Some(node) = document.get_element_by_id(id) {
            node.remove();
        }
    }
    Ok(())
}

fn clear_square_highlight_layer(layer: SquareHighlightLayer) -> JsResult<()> {
    let document = web_document();
    for board_idx in DisplayBoard::iter() {
        document
            .get_existing_element_by_id(&square_highlight_layer_id(layer, board_idx))?
            .remove_all_children();
    }
    Ok(())
}

// Improvement potential: Find a better icon for connection problems.
// Improvement potential: Add a tooltip explaining the meaning of the icons.
fn participant_prefix(p: &Participant, show_readiness: bool) -> &'static str {
    if p.faction == Faction::Observer {
        return "👀 ";
    }
    if !p.is_online {
        return "⚠️ ";
    }
    if show_readiness {
        return if p.is_ready { "☑ " } else { "☐ " };
    }
    ""
}

fn get_text_width(s: &str) -> JsResult<u32> {
    let canvas = web_document()
        .get_existing_element_by_id("canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let context = canvas
        .get_context("2d")?
        .ok_or_else(|| rust_error!("Canvas 2D context missing"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;
    Ok(context.measure_text(&s)?.width() as u32)
}

fn participant_node(p: &Participant, show_readiness: bool) -> JsResult<web_sys::Element> {
    let width = get_text_width(&p.name)?;
    // Context. Player name limit is 16 characters. String consisting of 'W' repeated 16 times
    // measured 151px on my laptop. 'W' is usually the widest latter in common Latin, but you could
    // go wider with 'Ǆ' and even wider with non-Latin characters. So this solution might be
    // insufficient in case of complete outliers, but it should work for all realistic cases.
    let class = match width {
        140.. => "participant-name-xxxl",
        120.. => "participant-name-xxl",
        100.. => "participant-name-xl",
        80.. => "participant-name-l",
        _ => "participant-name-m",
    };
    let node = web_document().create_element("span")?.with_classes(["nowrap"])?;
    node.append_text_span(participant_prefix(p, show_readiness), [])?;
    node.append_text_span(&p.name, [class])?;
    Ok(node)
}

fn svg_icon(image: &str, width: u32, height: u32, classes: &[&str]) -> JsResult<web_sys::Element> {
    let document = web_document();
    let svg_node = document.create_svg_element("svg")?;
    svg_node.set_attribute("viewBox", &format!("0 0 {width} {height}"))?;
    svg_node.set_attribute("class", &classes.iter().join(" "))?;
    let use_node = document.create_svg_element("use")?;
    use_node.set_attribute("href", image)?;
    svg_node.append_child(&use_node)?;
    Ok(svg_node)
}

// Standalone chess piece icon to be used outside of SVG area.
fn make_piece_icon(
    piece_kind: PieceKind, force: PieceForce, classes: &[&str],
) -> JsResult<web_sys::Element> {
    svg_icon(piece_path(piece_kind, force), 1, 1, classes)
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

fn add_lobby_participant_node(
    p: &Participant, is_me: bool, parent: &web_sys::Element,
) -> JsResult<()> {
    let document = web_document();
    let add_relation_class = |node: &web_sys::Element| {
        node.class_list().add_1(if is_me { "lobby-me" } else { "lobby-other" })
    };
    {
        let registered_user_node = match p.is_registered_user {
            false => make_menu_icon(&[])?,
            true => make_menu_icon(&["registered-user"])?,
        };
        registered_user_node.class_list().add_1("registered-user-icon")?;
        if p.is_registered_user {
            let title_node = document.create_svg_element("title")?;
            title_node.set_text_content(Some("This is a registered user account."));
            registered_user_node.append_child(&title_node)?;
        }
        parent.append_child(&registered_user_node)?;
    }
    {
        let width_class = match get_text_width(&p.name)? {
            140.. => "lobby-name-xl",
            120.. => "lobby-name-l",
            _ => "lobby-name-m",
        };
        let name_node = document.create_element("div")?;
        name_node.set_attribute("class", &format!("lobby-name {width_class}"))?;
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
        let readiness_node = match (p.faction, p.is_ready) {
            (Faction::Observer, _) => make_menu_icon(&[])?,
            (_, false) => make_menu_icon(&["readiness-checkbox"])?,
            (_, true) => make_menu_icon(&["readiness-checkbox", "readiness-checkmark"])?,
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
    board_shape: BoardShape, piece_kind_sep: f64,
    reserve_iter: impl Iterator<Item = (PieceKind, u8)> + Clone,
) -> JsResult<()> {
    let document = web_document();
    let reserve_node =
        document.get_existing_element_by_id(&reserve_node_id(board_idx, player_idx))?;
    // Does not interfere with dragging a reserve piece, because dragged piece is re-parented
    // to board SVG.
    reserve_node.remove_all_children();

    let num_piece: u8 = reserve_iter.clone().map(|(_, amount)| amount).sum();
    if num_piece == 0 {
        return Ok(());
    }
    let num_piece = num_piece as f64;
    let num_kind = reserve_iter.clone().count() as f64;
    let num_nonempty_kind = reserve_iter.clone().filter(|&(_, amount)| amount > 0).count() as f64;
    let max_width = board_shape.num_cols as f64;
    let total_kind_sep_width = piece_kind_sep * (num_kind - 1.0);
    let piece_sep =
        f64::min(0.5, (max_width - total_kind_sep_width) / (num_piece - num_nonempty_kind));
    assert!(piece_sep > 0.0, "{:?}", reserve_iter.collect_vec());
    let width = total_kind_sep_width + (num_piece - num_nonempty_kind) * piece_sep;

    let mut x = (max_width - width - 1.0) / 2.0; // center reserve
    let y = reserve_y_pos(player_idx);
    for (piece_kind, amount) in reserve_iter {
        let filename = piece_path(piece_kind, force.into());
        for iter in 0..amount {
            if iter > 0 {
                x += piece_sep;
            }
            let node = document.create_svg_element("use")?;
            node.set_attribute("href", filename)?;
            node.set_attribute("data-bughouse-location", &reserve_piece_id(board_idx, piece_kind))?;
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
    reserve: &Reserve, force: Force, board_idx: DisplayBoard, player_idx: DisplayPlayer,
    is_draggable: bool, chess_rules: &ChessRules,
) -> JsResult<()> {
    let piece_kind_sep = 1.0;
    let reserve_iter = reserve
        .iter()
        .filter(|(kind, &amount)| {
            match kind.reservable(chess_rules) {
                // Leave space for all `PieceReservable::Always` pieces, so that the icons
                // don't shift too much and the user does not misclick after receiving a new
                // reserve piece.
                PieceReservable::Always => true,
                PieceReservable::Never => {
                    assert!(amount == 0);
                    false
                }
                PieceReservable::InSpecialCases => amount > 0,
            }
        })
        .map(|(kind, &amount)| (kind, amount));
    render_reserve(
        force,
        board_idx,
        player_idx,
        is_draggable,
        chess_rules.board_shape(),
        piece_kind_sep,
        reserve_iter,
    )
}

fn render_upgrade_promotion_selector(
    force: Option<Force>, display_board_idx: DisplayBoard,
    upgrade_promotion_target: Option<DisplayCoord>,
) -> JsResult<()> {
    use std::f64::consts::PI;
    const PIECE_SIZE: f64 = 1.0;
    // Make central circle in promotion UI cover the entire square. Of course, 0.7 is slightly less
    // than sqrt(2), but there's also stroke width.
    const INNER_RADIUS: f64 = 0.7;
    const OUTER_RADIUS: f64 = 1.7;
    const MID_RADIUS: f64 = (INNER_RADIUS + OUTER_RADIUS) / 2.0;

    let document = web_document();
    let layer =
        document.get_existing_element_by_id(&promotion_target_layer_id(display_board_idx))?;
    let Some(display_coord) = upgrade_promotion_target else {
        layer.remove_all_children();
        return Ok(());
    };
    let force = force.unwrap();

    let promotion_targets = PieceKind::iter()
        .filter(|&kind| kind.can_be_upgrade_promotion_target())
        .collect_vec();
    let primary_target = PieceKind::Queen;
    assert!(promotion_targets.contains(&primary_target));
    let secondary_targets = promotion_targets
        .into_iter()
        .filter(|&kind| kind != primary_target)
        .collect_vec();
    let bg_node_id = |kind: PieceKind| format!("promotion-bg-{}", kind.to_full_algebraic());
    let make_fg_node = |piece: PieceKind, x: f64, y: f64| -> JsResult<()> {
        let id = format!("promotion-fg-{}", piece.to_full_algebraic());
        let node = document.ensure_svg_node("use", &id, &layer)?;
        node.set_attribute("class", "promotion-target-fg")?;
        node.set_attribute("x", &(x - PIECE_SIZE / 2.0).to_string())?;
        node.set_attribute("y", &(y - PIECE_SIZE / 2.0).to_string())?;
        node.set_attribute("href", &piece_path(piece, force.into()))?;
        Ok(())
    };
    let pos = DisplayFCoord::square_center(display_coord);

    let num_steps = secondary_targets.len();
    let start_rad = PI;
    let end_rad = 2.0 * PI;
    let step_rad = (end_rad - start_rad) / (num_steps as f64);
    for (i, &piece) in secondary_targets.iter().enumerate() {
        let from_rad = start_rad + (i as f64) * step_rad;
        let to_rad = from_rad + step_rad;
        let mid_rad = (from_rad + to_rad) / 2.0;
        let piece_pos = svg::polar_to_cartesian(pos.x, pos.y, MID_RADIUS, mid_rad);
        let bg_node = document.ensure_svg_node("path", &bg_node_id(piece), &layer)?;
        bg_node.set_attribute(
            "d",
            &svg::ring_arc_path(pos.x, pos.y, INNER_RADIUS, OUTER_RADIUS, from_rad, to_rad),
        )?;
        bg_node.set_attribute("data-promotion-target", &piece.to_full_algebraic().to_string())?;
        bg_node.set_attribute("class", "promotion-target-bg promotion-target-bg-secondary")?;
        make_fg_node(piece, piece_pos.0, piece_pos.1)?;
    }

    let bg_node = document.ensure_svg_node("circle", &bg_node_id(primary_target), &layer)?;
    bg_node.set_attribute("cx", &pos.x.to_string())?;
    bg_node.set_attribute("cy", &pos.y.to_string())?;
    bg_node.set_attribute("r", &INNER_RADIUS.to_string())?;
    bg_node
        .set_attribute("data-promotion-target", &primary_target.to_full_algebraic().to_string())?;
    bg_node.set_attribute("class", "promotion-target-bg promotion-target-bg-primary")?;
    make_fg_node(primary_target, pos.x, pos.y)?;

    Ok(())
}

fn render_starting() -> JsResult<()> {
    use PieceKind::*;
    let board_shape = BoardShape { num_rows: 8, num_cols: 8 };
    let perspective = Perspective::for_participant(BughouseParticipant::Observer);
    render_boards(board_shape, perspective)?;
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
    for force in Force::iter() {
        for board_idx in DisplayBoard::iter() {
            let board_orientation = get_board_orientation(board_idx, perspective);
            let player_idx = get_display_player(force, board_orientation);
            render_reserve(
                force,
                board_idx,
                player_idx,
                draggable,
                board_shape,
                piece_kind_sep,
                reserve_iter.clone(),
            )?;
        }
    }
    Ok(())
}

// Differs from `BughouseGame::envoy_is_active` in that it returns false for White before game start.
fn is_clock_ticking(game: &BughouseGame, participant_id: BughouseParticipant) -> bool {
    for envoy in participant_id.envoys() {
        if game.board(envoy.board_idx).clock().active_force() == Some(envoy.force) {
            return true;
        }
    }
    false
}

fn render_clock(
    showing: ClockShowing, diff: Option<ClockDifference>, display_board_idx: DisplayBoard,
    player_idx: DisplayPlayer, wayback_active: bool,
) -> JsResult<()> {
    let document = web_document();
    let clock_node =
        document.get_existing_element_by_id(&clock_node_id(display_board_idx, player_idx))?;
    clock_node.set_text_content(Some(&showing.ui_string()));
    let mut classes = vec!["clock"];
    if showing.out_of_time {
        classes.push("clock-flag");
    } else {
        classes.push(if showing.is_active {
            "clock-active"
        } else {
            "clock-inactive"
        });
        if matches!(showing.time_breakdown, TimeBreakdown::LowTime { .. }) {
            classes.push("clock-low-time");
        }
    }
    clock_node.set_attribute("class", &classes.join(" "))?;

    let diff_node = document.ensure_node(
        "div",
        &clock_diff_node_id(display_board_idx, player_idx),
        &clock_node,
    )?;
    diff_node.class_list().add_1("clock-difference")?;
    if let Some(diff) = diff {
        match player_idx {
            DisplayPlayer::Top => diff_node.class_list().add_1("clock-difference-top")?,
            DisplayPlayer::Bottom => diff_node.class_list().add_1("clock-difference-bot")?,
        }
        diff_node
            .class_list()
            .toggle_with_force("clock-difference-lt", diff.comparison == Ordering::Less)?;
        diff_node
            .class_list()
            .toggle_with_force("clock-difference-eq", diff.comparison == Ordering::Equal)?;
        diff_node
            .class_list()
            .toggle_with_force("clock-difference-gt", diff.comparison == Ordering::Greater)?;
        diff_node.set_text_content(Some(&diff.ui_string()));
        diff_node.class_list().toggle_with_force("display-none", wayback_active)?;
    } else {
        diff_node.class_list().toggle_with_force("display-none", true)?;
    }

    Ok(())
}

fn update_scores(
    scores: &Option<Scores>, participants: &[Participant], game_status: BughouseGameStatus,
) -> JsResult<()> {
    let show_readiness = !game_status.is_active();
    let table = web_document().create_element("table")?;
    match scores {
        None => {}
        Some(Scores::PerTeam(score_map)) => {
            let mut team_players = enum_map! { _ => vec![] };
            for p in participants {
                match p.faction {
                    Faction::Fixed(team) => team_players[team].push(p),
                    Faction::Random => panic!("Unexpected Faction::Random with Scores::PerTeam"),
                    Faction::Observer => {}
                }
            }
            for (team, players) in team_players {
                let team_size = players.len();
                let mut first = true;
                for p in players.iter().sorted_by_key(|p| &p.name) {
                    let p_node = participant_node(p, show_readiness)?;
                    let tr = table.append_new_element("tr")?;
                    if first {
                        first = false;
                        let score = score_map[team].as_f64();
                        tr.append_new_element("td")?
                            .with_classes(["score-player-name", "score-first-player-name"])?
                            .append_child(&p_node)?;
                        {
                            let td =
                                tr.append_new_element("td")?.with_classes(["team-score-value"])?;
                            td.set_text_content(Some(&score.to_string()));
                            td.set_attribute("rowspan", &team_size.to_string())?;
                        }
                    } else {
                        tr.append_new_element("td")?
                            .with_classes(["score-player-name"])?
                            .append_child(&p_node)?;
                    }
                }
            }
        }
        Some(Scores::PerPlayer(score_map)) => {
            for (name, score) in score_map.iter().sorted_by_key(|(name, _)| *name) {
                let score = score.as_f64();
                let p = participants.iter().find(|p| p.name == *name).unwrap();
                let p_node = participant_node(p, show_readiness)?;
                let tr = table.append_new_element("tr")?;
                tr.append_new_element("td")?
                    .with_classes(["score-player-name"])?
                    .append_child(&p_node)?;
                tr.append_new_element("td")?
                    .with_classes(["individual-score-value"])?
                    .set_text_content(Some(&score.to_string()));
            }
        }
    }
    let score_node = web_document().get_existing_element_by_id("score-body")?;
    score_node.replace_children_with_node_1(&table);
    Ok(())
}

fn update_observers(participants: &[Participant]) -> JsResult<()> {
    let observers_node = web_document().get_existing_element_by_id("observers")?;
    observers_node.remove_all_children();
    for p in participants {
        if p.faction == Faction::Observer {
            let node = observers_node.append_new_element("div")?;
            let p_node = participant_node(p, false)?;
            node.append_child(&p_node)?;
        }
    }
    Ok(())
}

fn render_boards(board_shape: BoardShape, perspective: Perspective) -> JsResult<()> {
    for board_idx in DisplayBoard::iter() {
        render_board(board_idx, board_shape, perspective)?;
    }
    Ok(())
}

fn update_turn_log(
    game: &BughouseGame, my_id: BughouseParticipant, board_idx: BughouseBoard,
    display_board_idx: DisplayBoard, wayback: WaybackState,
) -> JsResult<()> {
    let board_shape = game.board_shape();
    let document = web_document();
    let log_scroll_area_node =
        document.get_existing_element_by_id(&turn_log_scroll_area_node_id(display_board_idx))?;
    log_scroll_area_node
        .class_list()
        .toggle_with_force("wayback", wayback.active())?;
    let log_node = document.get_existing_element_by_id(&turn_log_node_id(display_board_idx))?;
    log_node.remove_all_children();
    let mut prev_number = 0;
    let mut prev_index = String::new();
    for record in game.turn_log().iter() {
        if record.envoy.board_idx == board_idx {
            let force = record.envoy.force;
            let mut turn_number_str = String::new();
            if prev_number != record.number {
                turn_number_str = format!("{}.", record.number);
                prev_number = record.number;
            }
            let index = record.index();
            let is_in_fog = game.chess_rules().fog_of_war
                && game.is_active()
                && my_id.as_player().map_or(false, |p| p.team() != record.envoy.team());
            let algebraic = if is_in_fog {
                record.turn_expanded.algebraic.format_in_the_fog(board_shape)
            } else {
                record
                    .turn_expanded
                    .algebraic
                    .format(board_shape, AlgebraicCharset::AuxiliaryUnicode)
            };
            let (algebraic, captures) = match record.mode {
                TurnMode::Normal => (algebraic, record.turn_expanded.captures.clone()),
                TurnMode::Preturn => (
                    format!("({})", algebraic),
                    vec![], // don't show captures for preturns: too unpredictable and messes with braces
                ),
            };

            const LOG_PIECE_WIDTH: u32 = 5;
            // The "one plus" part nicely accounts for the fact that there is a separator in
            // addition to captured pieces and, on the one hand, it is smaller that a single piece,
            // but on the other hand, pieces overlap, so the very first piece takes more space than
            // each next one.
            let width_estimate =
                get_text_width(&algebraic)? + LOG_PIECE_WIDTH * (1 + captures.len() as u32);
            let width_class = match width_estimate {
                55.. => "log-record-xl",
                50.. => "log-record-l",
                _ => "",
            };

            let line_node = document.create_element("div")?;
            line_node.set_attribute("id", &turn_record_node_id(display_board_idx, &index))?;
            line_node.set_attribute(
                "class",
                &format!("log-turn-record log-turn-record-{} {width_class}", force_id(force)),
            )?;
            line_node.set_attribute("data-turn-index", &index)?;
            if Some(index.as_str()) == wayback.turn_index() {
                if wayback.active() {
                    line_node.class_list().add_1("wayback-current-turn-active")?;
                } else {
                    line_node.class_list().add_1("wayback-current-turn-inactive")?;
                }
            }
            prev_index = index;

            line_node.append_text_span(&turn_number_str, ["log-turn-number"])?;
            line_node.append_text_span(&algebraic, ["log-algebraic"])?;

            if !captures.is_empty() {
                line_node.append_text_span("·", ["log-capture-separator"])?;
                for capture in captures.iter() {
                    let capture_classes = [
                        "log-piece",
                        &format!("log-piece-{}", piece_force_id(capture.force)),
                    ];
                    let capture_node =
                        make_piece_icon(capture.piece_kind, capture.force, &capture_classes)?;
                    line_node.append_child(&capture_node)?;
                }
            }

            log_node.append_child(&line_node)?;
        } else {
            // Add records for turns made on the other board when appropriate.
            if !record.turn_expanded.steals.is_empty() {
                let line_node = document.create_element("div")?;
                // No need to add a width class: steal records are always small.
                line_node.set_attribute("class", "log-turn-record log-turn-record-intervention")?;
                // Clicking on the steal will send you the previous turn on this board.
                line_node.set_attribute("data-turn-index", &prev_index)?;

                line_node.append_span(["log-turn-number"])?;

                let stealing_hand_node = svg_icon("#stealing-hand", 150, 100, &["log-steal-icon"])?;
                line_node.append_child(&stealing_hand_node)?;

                for steal in record.turn_expanded.steals.iter() {
                    let capture_classes = [
                        "log-piece",
                        &format!("log-piece-{}", piece_force_id(steal.force)),
                    ];
                    let steal_node =
                        make_piece_icon(steal.piece_kind, steal.force, &capture_classes)?;
                    line_node.append_child(&steal_node)?;
                }

                log_node.append_child(&line_node)?;
            }
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

fn setup_participation_mode(participant_id: BughouseParticipant) -> JsResult<()> {
    use BughousePlayer::*;
    let (is_symmetric, is_observer) = match participant_id {
        BughouseParticipant::Observer => (true, true),
        BughouseParticipant::Player(SinglePlayer(_)) => (false, false),
        BughouseParticipant::Player(DoublePlayer(_)) => (true, false),
    };
    let body = web_document().body()?;
    body.class_list().toggle_with_force("symmetric", is_symmetric)?;
    body.class_list().toggle_with_force("observer", is_observer)?;
    Ok(())
}

fn render_grid(
    board_idx: DisplayBoard, board_shape: BoardShape, perspective: Perspective,
) -> JsResult<()> {
    let text_h_padding = 0.07;
    let text_v_padding = 0.09;
    let board_orientation = get_board_orientation(board_idx, perspective);
    let document = web_document();
    let layer = document.get_existing_element_by_id(&square_grid_layer_id(board_idx))?;
    for row in board_shape.rows() {
        for col in board_shape.cols() {
            let sq = document.create_svg_element("rect")?;
            let display_coord =
                to_display_coord(Coord::new(row, col), board_shape, board_orientation);
            let DisplayFCoord { x, y } = DisplayFCoord::square_pivot(display_coord);
            sq.set_attribute("x", &x.to_string())?;
            sq.set_attribute("y", &y.to_string())?;
            sq.set_attribute("width", "1")?;
            sq.set_attribute("height", "1")?;
            sq.set_attribute("class", square_color_class(row, col))?;
            layer.append_child(&sq)?;
            if display_coord.x == 0 {
                let caption = document.create_svg_element("text")?;
                caption.set_text_content(Some(&String::from(row.to_algebraic(board_shape))));
                caption.set_attribute("x", &(x + text_h_padding).to_string())?;
                caption.set_attribute("y", &(y + text_v_padding).to_string())?;
                caption.set_attribute("dominant-baseline", "hanging")?;
                caption.set_attribute("class", square_text_color_class(row, col))?;
                layer.append_child(&caption)?;
            }
            if display_coord.y == board_shape.num_rows as i8 - 1 {
                let caption = document.create_svg_element("text")?;
                caption.set_text_content(Some(&String::from(col.to_algebraic(board_shape))));
                caption.set_attribute("x", &(x + 1.0 - text_h_padding).to_string())?;
                caption.set_attribute("y", &(y + 1.0 - text_v_padding).to_string())?;
                caption.set_attribute("text-anchor", "end")?;
                caption.set_attribute("class", square_text_color_class(row, col))?;
                layer.append_child(&caption)?;
            }
        }
    }
    Ok(())
}

fn render_board(
    board_idx: DisplayBoard, board_shape: BoardShape, perspective: Perspective,
) -> JsResult<()> {
    let BoardShape { num_rows, num_cols } = board_shape;
    let make_board_rect = |document: &WebDocument| -> JsResult<web_sys::Element> {
        let rect = document.create_svg_element("rect")?;
        let pos = DisplayFCoord::square_pivot(DisplayCoord { x: 0, y: 0 });
        rect.set_attribute("x", &pos.x.to_string())?;
        rect.set_attribute("y", &pos.y.to_string())?;
        rect.set_attribute("width", &num_cols.to_string())?;
        rect.set_attribute("height", &num_rows.to_string())?;
        Ok(rect)
    };

    let document = web_document();
    let svg = document.get_existing_element_by_id(&board_node_id(board_idx))?;
    svg.set_attribute("viewBox", &format!("0 0 {num_cols} {num_rows}"))?;
    svg.remove_all_children();

    let add_layer = |id: String, shape_rendering: ShapeRendering| -> JsResult<()> {
        let layer = document.create_svg_element("g")?;
        layer.set_attribute("id", &id)?;
        // TODO: Less hacky way to do this.
        if let Some(class) = id.strip_suffix("-primary").or(id.strip_suffix("-secondary")) {
            layer.set_attribute("class", class)?;
        }
        match shape_rendering {
            ShapeRendering::Normal => {}
            ShapeRendering::CrispEdges => {
                layer.set_attribute("shape-rendering", "crispEdges")?;
            }
        }
        svg.append_child(&layer)?;
        Ok(())
    };

    let shadow = make_board_rect(&document)?;
    shadow.set_attribute("class", "board-shadow")?;
    svg.append_child(&shadow)?;

    add_layer(square_grid_layer_id(board_idx), ShapeRendering::CrispEdges)?;
    render_grid(board_idx, board_shape, perspective)?;

    let border = make_board_rect(&document)?;
    border.set_attribute("class", "board-border")?;
    svg.append_child(&border)?;

    add_layer(
        square_highlight_layer_id(SquareHighlightLayer::Turn, board_idx),
        ShapeRendering::CrispEdges,
    )?;
    add_layer(chalk_highlight_layer_id(board_idx), ShapeRendering::CrispEdges)?;
    add_layer(piece_layer_id(board_idx), ShapeRendering::Normal)?;
    add_layer(fog_of_war_layer_id(board_idx), ShapeRendering::Normal)?;
    // Highlight layer for squares inside the fog of war.
    add_layer(
        square_highlight_layer_id(SquareHighlightLayer::TurnAbove, board_idx),
        ShapeRendering::CrispEdges,
    )?;
    // Place drag highlight layer above pieces to allow legal move highlight for captures.
    // Note that the dragged piece will still be above the highlight.
    add_layer(
        square_highlight_layer_id(SquareHighlightLayer::Drag, board_idx),
        ShapeRendering::Normal,
    )?;
    add_layer(chalk_drawing_layer_id(board_idx), ShapeRendering::Normal)?;
    add_layer(promotion_target_layer_id(board_idx), ShapeRendering::Normal)?;

    for player_idx in DisplayPlayer::iter() {
        let reserve = document.create_svg_element("g")?;
        reserve.set_attribute("id", &reserve_node_id(board_idx, player_idx))?;
        reserve.set_attribute("class", "reserve")?;
        let reserve_container =
            document.get_existing_element_by_id(&reserve_container_id(board_idx, player_idx))?;
        // Note that reserve height is also encoded in CSS.
        reserve_container.set_attribute("viewBox", &format!("0 0 {num_cols} {RESERVE_HEIGHT}"))?;
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

fn force_id(force: Force) -> &'static str {
    match force {
        Force::White => "white",
        Force::Black => "black",
    }
}

fn piece_force_id(force: PieceForce) -> &'static str {
    match force {
        PieceForce::White => "white",
        PieceForce::Black => "black",
        PieceForce::Neutral => "neutral",
    }
}

fn board_id(idx: DisplayBoard) -> &'static str {
    match idx {
        DisplayBoard::Primary => "primary",
        DisplayBoard::Secondary => "secondary",
    }
}
fn parse_board_id(id: &str) -> JsResult<DisplayBoard> {
    match id {
        "primary" => Ok(DisplayBoard::Primary),
        "secondary" => Ok(DisplayBoard::Secondary),
        _ => Err(format!(r#"Invalid board: "{id}""#).into()),
    }
}

fn board_node_id(idx: DisplayBoard) -> String { format!("board-{}", board_id(idx)) }
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

fn square_id(board_idx: DisplayBoard, coord: Coord) -> String {
    format!("{}-{}", board_id(board_idx), coord.to_id())
}
fn parse_square_id(id: &str) -> Option<(DisplayBoard, Coord)> {
    let (board_idx, coord) = id.split('-').collect_tuple()?;
    let board_idx = parse_board_id(board_idx).ok()?;
    let coord = Coord::from_id(coord)?;
    Some((board_idx, coord))
}

fn reserve_piece_id(board_idx: DisplayBoard, piece_kind: PieceKind) -> String {
    format!("reserve-{}-{}", board_id(board_idx), piece_kind.to_full_algebraic())
}
fn parse_reserve_piece_id(id: &str) -> Option<(DisplayBoard, PieceKind)> {
    let (reserve_literal, board_idx, piece_kind) = id.split('-').collect_tuple()?;
    if reserve_literal != "reserve" {
        return None;
    }
    let board_idx = parse_board_id(board_idx).ok()?;
    let piece_kind = PieceKind::from_algebraic(piece_kind)?;
    Some((board_idx, piece_kind))
}

fn fog_of_war_id(board_idx: DisplayBoard, coord: Coord) -> String {
    format!("fog-{}-{}", board_id(board_idx), coord.to_id())
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

fn clock_diff_node_id(board_idx: DisplayBoard, player_idx: DisplayPlayer) -> String {
    format!("clock-diff-{}-{}", board_id(board_idx), player_id(player_idx))
}

fn turn_log_scroll_area_node_id(board_idx: DisplayBoard) -> String {
    format!("turn-log-scroll-area-{}", board_id(board_idx))
}

fn turn_log_node_id(board_idx: DisplayBoard) -> String {
    format!("turn-log-{}", board_id(board_idx))
}

fn turn_record_node_id(board_idx: DisplayBoard, index: &str) -> String {
    format!("turn-record-{}-{index}", board_id(board_idx))
}

fn square_grid_layer_id(board_idx: DisplayBoard) -> String {
    format!("square-grid-layer-{}", board_id(board_idx))
}

fn piece_layer_id(board_idx: DisplayBoard) -> String {
    format!("piece-layer-{}", board_id(board_idx))
}

fn fog_of_war_layer_id(board_idx: DisplayBoard) -> String {
    format!("fog-of-war-layer-{}", board_id(board_idx))
}

fn turn_highlight_layer(layer: TurnHighlightLayer) -> SquareHighlightLayer {
    match layer {
        TurnHighlightLayer::AboveFog => SquareHighlightLayer::TurnAbove,
        TurnHighlightLayer::BelowFog => SquareHighlightLayer::Turn,
    }
}
fn turn_highlight_class_id(h: &TurnHighlight) -> String {
    let family = match h.family {
        TurnHighlightFamily::PartialTurn => "partial",
        TurnHighlightFamily::LatestTurn => "latest",
        TurnHighlightFamily::Preturn => "pre",
    };
    let item = match h.item {
        TurnHighlightItem::MoveFrom => "from",
        TurnHighlightItem::MoveTo => "to",
        TurnHighlightItem::Drop => "drop",
        TurnHighlightItem::Capture => "capture",
    };
    let layer = match h.layer {
        TurnHighlightLayer::AboveFog => "-above",
        TurnHighlightLayer::BelowFog => "",
    };
    format!("{}-turn-{}{}", family, item, layer)
}

fn square_highlight_layer_id(layer: SquareHighlightLayer, board_idx: DisplayBoard) -> String {
    let layer_id = match layer {
        SquareHighlightLayer::Turn => "turn",
        SquareHighlightLayer::TurnAbove => "turn-above",
        SquareHighlightLayer::Drag => "drag",
    };
    format!("{}-highlight-layer-{}", layer_id, board_id(board_idx))
}

fn chalk_highlight_layer_id(board_idx: DisplayBoard) -> String {
    format!("chalk-highlight-layer-{}", board_id(board_idx))
}

fn chalk_drawing_layer_id(board_idx: DisplayBoard) -> String {
    format!("chalk-drawing-layer-{}", board_id(board_idx))
}

fn promotion_target_layer_id(board_idx: DisplayBoard) -> String {
    format!("promotion-target-layer-{}", board_id(board_idx))
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

fn piece_path(piece_kind: PieceKind, force: PieceForce) -> &'static str {
    use PieceForce::*;
    use PieceKind::*;
    match (force, piece_kind) {
        (White, Pawn) => "#white-pawn",
        (White, Knight) => "#white-knight",
        (White, Bishop) => "#white-bishop",
        (White, Rook) => "#white-rook",
        (White, Queen) => "#white-queen",
        (White, Cardinal) => "#white-cardinal",
        (White, Empress) => "#white-empress",
        (White, Amazon) => "#white-amazon",
        (White, King) => "#white-king",
        (Black, Pawn) => "#black-pawn",
        (Black, Knight) => "#black-knight",
        (Black, Bishop) => "#black-bishop",
        (Black, Rook) => "#black-rook",
        (Black, Queen) => "#black-queen",
        (Black, Cardinal) => "#black-cardinal",
        (Black, Empress) => "#black-empress",
        (Black, Amazon) => "#black-amazon",
        (Black, King) => "#black-king",
        (_, Duck) => "#duck",
        (Neutral, _) => panic!("There is no neutral representation for {piece_kind:?}"),
    }
}

fn broken_king_path(force: PieceForce) -> &'static str {
    match force {
        PieceForce::White => "#white-king-broken",
        PieceForce::Black => "#black-king-broken",
        PieceForce::Neutral => panic!("King cannot be neutral"),
    }
}

// Note. This function must return only a limited set of values, because JS will create a permament
// `PannerNode` for each pan ever played.
fn get_audio_pan(my_id: BughouseParticipant, display_board_idx: DisplayBoard) -> JsResult<f64> {
    use BughouseParticipant::*;
    use BughousePlayer::*;
    match (my_id, display_board_idx) {
        (Player(SinglePlayer(_)), DisplayBoard::Primary) => Ok(0.),
        (Player(SinglePlayer(_)), DisplayBoard::Secondary) => Err(rust_error!()),
        (Player(DoublePlayer(_)) | Observer, DisplayBoard::Primary) => Ok(-1.),
        (Player(DoublePlayer(_)) | Observer, DisplayBoard::Secondary) => Ok(1.),
    }
}
