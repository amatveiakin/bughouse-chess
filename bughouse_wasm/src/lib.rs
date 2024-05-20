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
use std::str::FromStr;
use std::time::Duration;

use bughouse_chess::client::*;
use bughouse_chess::client_chat::cannot_start_game_message;
use bughouse_chess::lobby::*;
use bughouse_chess::meter::*;
use bughouse_chess::session::*;
use enum_map::enum_map;
use html_collection_iterator::IntoHtmlCollectionIterator;
use instant::Instant;
use itertools::Itertools;
use strum::{EnumIter, IntoEnumIterator};
use time::macros::{datetime, format_description, offset};
use time::{OffsetDateTime, UtcOffset};
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

#[wasm_bindgen]
pub struct JsEventArchiveGameLoaded {
    pub game_id: i64,
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
        let user_name = self.state.session().user_name().unwrap_or("").to_string();
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

    pub fn fixed_teams(&self) -> bool { self.state.teaming() == Some(Teaming::FixedTeams) }
    pub fn my_active_faction(&self) -> String {
        self.state.my_active_faction().map_or("none", faction_id).to_owned()
    }
    pub fn my_desired_faction(&self) -> String {
        self.state.my_desired_faction().map_or("none", faction_id).to_owned()
    }
    pub fn observer_status(&self) -> String {
        let Some(my_faction) = self.state.my_active_faction() else {
            return "no".to_owned();
        };
        match my_faction {
            Faction::Observer => "permanently",
            Faction::Fixed(_) | Faction::Random => {
                let my_id = self.state.my_id();
                if my_id.is_some_and(|id| !id.is_player()) {
                    "temporary"
                } else {
                    "no"
                }
            }
        }
        .to_owned()
    }

    pub fn game_status(&self) -> String {
        let Some(mtch) = self.state.mtch() else {
            return "none".to_owned();
        };
        if mtch.is_archive_game_view() {
            "archive".to_owned()
        } else if let Some(game_state) = &mtch.game_state {
            if game_state.alt_game.is_active() {
                "active".to_owned()
            } else {
                "over".to_owned()
            }
        } else {
            "none".to_owned()
        }
    }

    pub fn lobby_waiting_explanation(&self) -> String {
        let Some(mtch) = self.state.mtch() else {
            return "".to_owned();
        };
        let particpants_status = verify_participants(&mtch.rules, mtch.participants.iter());
        match particpants_status {
            ParticipantsStatus::CanStart { players_ready, warning } => {
                match (players_ready, warning) {
                    (_, Some(ParticipantsWarning::NeedToDoublePlayAndSeatOut)) => {
                        "👉🏾 Can start, but some players will have to play on two boards while others will have to seat out"
                    }
                    (_, Some(ParticipantsWarning::NeedToDoublePlay)) => {
                        "👉🏾 Can start, but some players will have to play on two boards"
                    }
                    (_, Some(ParticipantsWarning::NeedToSeatOut)) => {
                        "👉🏾 Can start, but some players will have to seat out each game"
                    }
                    (false, None) => "👍🏾 Will start when everyone is ready",
                    (true, None) => "",
                }
            }
            ParticipantsStatus::CannotStart(error) => match error {
                ParticipantsError::NotEnoughPlayers => "Not enough players",
                ParticipantsError::EmptyTeam => "A team is empty",
                ParticipantsError::RatedDoublePlay => {
                    "Playing on two boards is only allowed in unrated matches"
                }
            },
        }
        .to_owned()
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
                pawn_drop_ranks: PawnDropRanks {
                    min: min_pawn_drop_rank,
                    max: max_pawn_drop_rank,
                },
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
    pub fn leave_match(&mut self) { self.state.leave_match(); }

    pub fn change_faction_ingame(&mut self, faction: &str) -> JsResult<()> {
        let faction = parse_faction_id(faction)?;
        self.state.set_faction(faction);
        Ok(())
    }

    pub fn execute_input(&mut self, input: &str) { self.state.execute_input(input); }
    pub fn clear_ephemeral_chat_items(&mut self) { self.state.clear_ephemeral_chat_items(); }
    pub fn show_command_result(&mut self, text: String) { self.state.show_command_result(text); }
    pub fn show_command_error(&mut self, text: String) { self.state.show_command_error(text); }

    // Why do we need both `click_element` and `click_board`? We need `click_element` because it is
    // used to click on the reserve. It is also conceivable that in the future we want to allow
    // clicking pieces that are (temporarily) shifted or oversized. We need `click_board` because
    // the right way to click on empty squares. Introducing an element for each empty square would
    // not work, because for example fog tiles are larger than the squares.
    pub fn click_element(&mut self, source: &str) -> JsResult<()> {
        let (display_board_idx, loc) = parse_location_id(source)
            .ok_or_else(|| rust_error!("Illegal click source: {source:?}"))?;
        let alt_game = self.state.alt_game_mut().ok_or_else(|| rust_error!())?;
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        let turn_or_error = alt_game.click(board_idx, loc);
        self.state.apply_turn_or_error(turn_or_error);
        Ok(())
    }
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
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        let board_orientation = get_board_orientation(display_board_idx, alt_game.perspective());
        let coord = from_display_coord(display_coord, board_shape, board_orientation).unwrap();
        let turn_or_error = alt_game.click(board_idx, Location::Square(coord));
        self.state.apply_turn_or_error(turn_or_error);
        Ok(())
    }

    pub fn board_hover(&mut self, board_id: &str, x: f64, y: f64) -> JsResult<()> {
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        let display_board_idx = parse_board_id(board_id)?;
        let board_orientation = get_board_orientation(display_board_idx, alt_game.perspective());
        let board_shape = alt_game.board_shape();
        let pos = DisplayFCoord { x, y };
        let board_idx = get_board_index(display_board_idx, alt_game.perspective());
        let highlight_coord = if alt_game.highlight_square_on_hover(board_idx) {
            pos.to_square(board_shape)
        } else {
            None
        };
        set_square_drag_over_highlight(
            display_board_idx,
            highlight_coord,
            board_shape,
            board_orientation,
        )?;
        Ok(())
    }

    pub fn choose_promotion_upgrade(&mut self, piece_kind: &str) -> JsResult<()> {
        let Some(alt_game) = self.state.alt_game_mut() else {
            return Ok(());
        };
        let piece_kind = PieceKind::from_algebraic(piece_kind)
            .ok_or_else(|| rust_error!("Invalid piece kind: {piece_kind}"))?;
        let turn_or_error = alt_game.choose_promotion_upgrade(piece_kind);
        self.state.apply_turn_or_error(turn_or_error);
        Ok(())
    }

    pub fn start_drag_piece(&mut self, source: &str) -> JsResult<String> {
        let (display_board_idx, source) = parse_location_id(source)
            .ok_or_else(|| rust_error!("Illegal drag source: {source:?}"))?;
        let alt_game = self.state.alt_game_mut().ok_or_else(|| rust_error!())?;
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
        let board_orientation = get_board_orientation(display_board_idx, alt_game.perspective());
        let pos = DisplayFCoord { x, y };
        set_square_drag_over_highlight(
            display_board_idx,
            pos.to_square(board_shape),
            board_shape,
            board_orientation,
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
            let board_idx = get_board_index(display_board_idx, alt_game.perspective());
            let board_orientation =
                get_board_orientation(display_board_idx, alt_game.perspective());
            let dest_coord =
                from_display_coord(dest_display, board_shape, board_orientation).unwrap();
            let turn_or_error = alt_game.drag_piece_drop(board_idx, dest_coord);
            self.state.apply_turn_or_error(turn_or_error);
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
        clear_square_highlight_layer(SquareHighlightLayer::Ephemeral)
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
        self.state.process_server_event(server_event).map_err(client_error_to_js)?;
        Ok(updated_needed)
    }

    pub fn next_notable_event(&mut self) -> JsResult<JsValue> {
        match self.state.next_notable_event() {
            Some(NotableEvent::SessionUpdated) => Ok(JsEventSessionUpdated {}.into()),
            Some(NotableEvent::MatchStarted(match_id)) => {
                reset_chat()?;
                init_lobby(&self.state.mtch().unwrap())?;
                Ok(JsEventMatchStarted { match_id }.into())
            }
            Some(NotableEvent::GameStarted) => {
                self.init_game_view()?;
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
            Some(NotableEvent::WaybackStateUpdated(wayback)) => {
                scroll_to_wayback_turn(wayback);
                Ok(JsEventNoop {}.into())
            }
            Some(NotableEvent::GotArchiveGameList(games)) => {
                let user_name = self.state.session().user_name();
                render_archive_game_list(Some(games), user_name)?;
                if let Some(game_id) = self.state.archive_game_id() {
                    highlight_archive_game_row(game_id)?;
                }
                Ok(JsEventNoop {}.into())
            }
            Some(NotableEvent::ArchiveGameLoaded(game_id)) => {
                // TODO: Reset when going back to main menu.
                reset_chat()?;
                self.init_game_view()?;
                highlight_archive_game_row(game_id)?;
                Ok(JsEventArchiveGameLoaded { game_id }.into())
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
        let hash_seed = match &mtch.origin {
            MatchOrigin::ActiveMatch(match_id) => match_id.clone(),
            MatchOrigin::ArchiveGame(game_id) => game_id.to_string(),
        };
        let Some(GameState { game_index, ref alt_game, .. }) = mtch.game_state else {
            update_lobby(mtch)?;
            return Ok(());
        };
        let game = alt_game.local_game();
        let board_shape = alt_game.board_shape();
        let my_id = alt_game.my_id();
        let perspective = alt_game.perspective();
        let wayback = alt_game.wayback_state();
        update_participants_and_scores(
            &mtch.scores,
            &mtch.participants,
            game.status(),
            mtch.is_active_match(),
        )?;
        for (board_idx, board) in game.boards() {
            let my_force = my_id.envoy_for(board_idx).map(|e| e.force);
            let is_my_duck_turn = alt_game.is_my_duck_turn(board_idx);
            let is_piece_draggable = |piece_force: PieceForce| {
                my_id
                    .envoy_for(board_idx)
                    .map_or(false, |e| piece_force.is_owned_by_or_neutral(e.force))
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
                    // Improvement potential: Maybe pre-create fog tiles and just show/hide them
                    //   here?
                    let node_id = fog_of_war_id(display_board_idx, coord);
                    if fog_render_area.contains(&coord) {
                        document.ensure_svg_node("use", &node_id, &fog_of_war_layer, |node| {
                            let sq_hash = calculate_hash(&(&hash_seed, board_idx, coord));
                            let fog_tile = sq_hash % TOTAL_FOG_TILES + 1;
                            let shift = (FOG_TILE_SIZE - 1.0) / 2.0;
                            let pos = DisplayFCoord::square_pivot(display_coord);
                            node.set_attribute("x", &(pos.x - shift).to_string())?;
                            node.set_attribute("y", &(pos.y - shift).to_string())?;
                            node.set_attribute("href", &format!("#fog-{fog_tile}"))?;
                            Ok(())
                        })?;
                        // Improvement potential. To make fog look more varied, add variants:
                        //   let variant = (sq_hash / TOTAL_FOG_TILES) % 4;
                        //   node.class_list().add_1(&format!("fog-variant-{variant}"))?;
                        // and alter the variants. Ideas:
                        //   - Rotate the tiles 90, 180 or 270 degrees. Problem: don't know how to
                        //     rotate <use> element around center.
                        //     https://stackoverflow.com/questions/15138801/rotate-rectangle-around-its-own-center-in-svg
                        //     did not work.
                        //   - Shift colors somehow. Problem: tried `hue-rotate` and `saturate`, but
                        //     it's either unnoticeable or too visisble. Ideal would be to rotate
                        //     hue within bluish color range.
                    } else {
                        document.get_element_by_id(&node_id).inspect(|n| n.remove());
                    }
                }
                {
                    let node_id = square_id(display_board_idx, coord);
                    let node = document.ensure_svg_node("use", &node_id, &piece_layer, |node| {
                        const SIZE: f64 = 1.0;
                        let shift = (SIZE - 1.0) / 2.0;
                        let pos = DisplayFCoord::square_pivot(display_coord);
                        node.set_attribute("x", &(pos.x - shift).to_string())?;
                        node.set_attribute("y", &(pos.y - shift).to_string())?;
                        node.set_attribute("data-bughouse-location", &node_id)?;
                        Ok(())
                    })?;
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
                        node.class_list()
                            .toggle_with_force("draggable", is_piece_draggable(piece.force))?;
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
            board_node.class_list().toggle_with_force("duck-turn", is_my_duck_turn)?;
            board_node.class_list().toggle_with_force("wayback", wayback.active())?;
            update_turn_log(&game, my_id, board_idx, display_board_idx, &wayback)?;
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
        update_cannot_start_alert(mtch)?;
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
                    BughouseParticipant::Observer(_) => true,
                };
                let diff = show_diff.then(|| clock.difference_for(force, other_clock, game_now));
                render_clock(
                    clock.showing_for(force, game_now),
                    diff,
                    display_board_idx,
                    player_idx,
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

    pub fn shared_wayback_enabled(&self) -> bool { self.state.shared_wayback_enabled() }
    pub fn toggle_shared_wayback(&mut self) {
        self.state.set_shared_wayback(!self.shared_wayback_enabled());
    }

    pub fn wayback_to_turn(&mut self, turn_idx: Option<String>) -> JsResult<()> {
        let turn_idx = turn_idx.map(|idx| TurnIndex::from_str(&idx).unwrap());
        self.state.wayback_to(WaybackDestination::Index(turn_idx), None);
        Ok(())
    }

    pub fn on_vertical_arrow_key_down(
        &mut self, key: &str, ctrl: bool, shift: bool, alt: bool,
    ) -> JsResult<()> {
        let Some(GameState { alt_game, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let display_board_idx = match (shift, alt) {
            (false, false) => None,
            (true, false) => Some(DisplayBoard::Primary),
            (_, true) => Some(DisplayBoard::Secondary),
        };
        let board_idx = display_board_idx.map(|idx| get_board_index(idx, alt_game.perspective()));
        let destination = match (key, ctrl) {
            ("ArrowDown", false) => WaybackDestination::Next,
            ("ArrowDown", true) => WaybackDestination::Last,
            ("ArrowUp", false) => WaybackDestination::Previous,
            ("ArrowUp", true) => WaybackDestination::First,
            _ => return Ok(()),
        };
        self.state.wayback_to(destination, board_idx);
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

    pub fn view_archive_game_list(&mut self) {
        // TODO: Add a loading indicator when updating an existing list.
        self.state.view_archive_game_list();
    }

    pub fn view_archive_game_content(&mut self, game_id: &str) -> JsResult<()> {
        self.state
            .view_archive_game_content(game_id.parse().unwrap())
            .map_err(client_error_to_js)
    }

    pub fn get_game_bpgn(&mut self) -> Option<String> { self.state.get_game_bpgn() }

    fn change_faction(&mut self, faction_modifier: impl Fn(i32) -> i32) {
        let Some(mtch) = self.state.mtch() else {
            return;
        };
        let current = ALL_FACTIONS.iter().position(|&f| f == mtch.my_desired_faction).unwrap();
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

    // Must be called after `update_reserve`, because the latter will recreate reserve nodes and
    // thus remove highlight classes.
    fn update_turn_highlights(&self) -> JsResult<()> {
        // Optimization potential: do not reset highlights that stay in place.
        web_document().purge_class_name("reserve-highlight")?;
        clear_square_highlight_layer(SquareHighlightLayer::Turn)?;
        clear_square_highlight_layer(SquareHighlightLayer::TurnAbove)?;
        let Some(GameState { ref alt_game, .. }) = self.state.game_state() else {
            return Ok(());
        };
        let board_shape = alt_game.board_shape();
        let perspective = alt_game.perspective();
        let highlights = alt_game.turn_highlights();
        for h in highlights.square_highlights {
            let class = square_highlight_class_id(&h);
            let display_board_idx = get_display_board_index(h.board_idx, perspective);
            let orientation = get_board_orientation(display_board_idx, perspective);
            let layer = turn_highlight_layer(h.layer);
            let display_coord = to_display_coord(h.coord, board_shape, orientation);
            set_square_highlight(
                None,
                &class,
                layer,
                display_board_idx,
                Some(display_coord),
                board_shape,
                orientation,
            )?;
        }
        for h in highlights.reserve_piece_highlights {
            let display_board_idx = get_display_board_index(h.board_idx, perspective);
            let node = web_document().get_existing_element_by_id(&reserve_piece_id(
                display_board_idx,
                h.force,
                h.piece_kind,
            ))?;
            node.class_list().add_1("reserve-highlight")?;
        }
        Ok(())
    }

    fn init_game_view(&self) -> JsResult<()> {
        let Some(GameState { ref alt_game, .. }) = self.state.game_state() else {
            return Err(rust_error!());
        };
        let my_id = alt_game.my_id();
        render_boards(alt_game.board_shape(), alt_game.perspective())?;
        setup_participation_mode(my_id)?;
        // TODO: Actualize chat tooltip for game archive.
        // Improvement potential. Add an <hr> style separator between games in chat.
        web_chat::render_chat_reference_tooltip(my_id, self.state.team_chat_enabled())?;
        for display_board_idx in DisplayBoard::iter() {
            scroll_log_to_bottom(display_board_idx)?;
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

fn client_error_to_js(err: ClientError) -> JsValue {
    match err {
        ClientError::Ignorable(message) => IgnorableError { message }.into(),
        ClientError::KickedFromMatch(message) => KickedFromMatch { message }.into(),
        ClientError::Fatal(message) => FatalError { message }.into(),
        ClientError::Internal(message) => RustError { message }.into(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, EnumIter)]
enum TurnRecordBoard {
    Main,
    Auxiliary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShapeRendering {
    // Use by default.
    Normal,
    // Use for layers with board squared to avoid anti-aliasing artifacts.
    CrispEdges,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PointerEvents {
    // Can be target of mouse and touch events.
    Auto,
    // Ignored by mouse and touch events.
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SquareHighlightLayer {
    // Last turn, preturn, partial turn input. Derived from `AlteredGame::turn_highlights`.
    Turn,
    // Like `Turn`, but above the fog of war. Derived from `AlteredGame::turn_highlights`.
    TurnAbove,
    // For things that change very quickly, to avoid going through the `AlteredGame` and updating a
    // lot of state.
    Ephemeral,
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
    web_chat::render_chat_reference_tooltip(BughouseParticipant::default_observer(), false)?;
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
    if let Some(match_id) = mtch.match_id() {
        node.append_text_span(match_id, ["lobby-match-id"])?;
    }
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

// Note. If present, `id` must be unique across both boards.
fn set_square_highlight(
    id: Option<&str>, class: &str, layer: SquareHighlightLayer, board_idx: DisplayBoard,
    display_coord: Option<DisplayCoord>, board_shape: BoardShape, orientation: BoardOrientation,
) -> JsResult<()> {
    let document = web_document();
    if let Some(display_coord) = display_coord {
        let coord = from_display_coord(display_coord, board_shape, orientation);
        let node = id.and_then(|id| document.get_element_by_id(id));
        let highlight_layer =
            document.get_existing_element_by_id(&square_highlight_layer_id(layer, board_idx))?;
        let node = node.ok_or(JsValue::UNDEFINED).or_else(|_| -> JsResult<web_sys::Element> {
            let node = document.create_svg_element("rect")?;
            if let Some(id) = id {
                node.set_attribute("id", id)?;
            }
            node.set_attribute("width", "1")?;
            node.set_attribute("height", "1")?;
            highlight_layer.append_child(&node)?;
            Ok(node)
        })?;
        let pos = DisplayFCoord::square_pivot(display_coord);
        node.set_attribute("x", &pos.x.to_string())?;
        node.set_attribute("y", &pos.y.to_string())?;
        node.set_attribute("class", "")?;
        node.class_list().add_1(class)?;
        if let Some(coord) = coord {
            let color_class = match coord.color() {
                Force::White => format!("{}-onwhite", class),
                Force::Black => format!("{}-onblack", class),
            };
            node.class_list().add_1(&color_class)?;
        }
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

fn set_square_drag_over_highlight(
    display_board_idx: DisplayBoard, display_coord: Option<DisplayCoord>, board_shape: BoardShape,
    orientation: BoardOrientation,
) -> JsResult<()> {
    set_square_highlight(
        Some("ephemeral-dragover-highlight"),
        "ephemeral-dragover-highlight",
        SquareHighlightLayer::Ephemeral,
        display_board_idx,
        display_coord,
        board_shape,
        orientation,
    )
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

// TODO: Use SVG images instead.
// Improvement potential: Add a tooltip explaining the meaning of the icons.
// Improvement potential: Add small red/blue icons for fixed teams in individual mode.
fn participant_status_icon(p: &Participant, show_readiness: bool) -> &'static str {
    match (p.active_faction, p.is_online) {
        (Faction::Observer, true) => "👀",
        (Faction::Observer, false) => "⮐",
        (Faction::Fixed(_) | Faction::Random, false) => "⚠️",
        (Faction::Fixed(_) | Faction::Random, true) => {
            if show_readiness {
                if p.is_ready {
                    "☑"
                } else {
                    "☐"
                }
            } else {
                ""
            }
        }
    }
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
    let width_class = match width {
        140.. => "participant-name-xxxl",
        120.. => "participant-name-xxl",
        100.. => "participant-name-xl",
        80.. => "participant-name-l",
        _ => "participant-name-m",
    };
    let node = web_document().create_element("span")?.with_classes(["participant-item"])?;
    node.append_text_span(participant_status_icon(p, show_readiness), ["participant-status-icon"])?;
    node.append_text_span(&p.name, ["participant-name", width_class])?;
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
        let faction_node = match p.active_faction {
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
        let readiness_node = match (p.active_faction, p.is_ready) {
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
        let id = reserve_piece_id(board_idx, force, piece_kind);
        let group_node = reserve_node.append_new_svg_element("g")?;
        group_node.set_id(&id);
        group_node.class_list().add_1("reserve-piece-group")?;
        for iter in 0..amount {
            if iter > 0 {
                x += piece_sep;
            }
            let node = group_node.append_new_svg_element("use")?;
            node.set_attribute("href", filename)?;
            node.set_attribute("data-bughouse-location", &id)?;
            node.set_attribute("x", &x.to_string())?;
            node.set_attribute("y", &y.to_string())?;
            if draggable {
                node.class_list().add_1("draggable")?;
            }
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
        let node = document.ensure_svg_node("use", &id, &layer, |node| {
            node.set_attribute("class", "promotion-target-fg")?;
            node.set_attribute("href", &piece_path(piece, force.into()))?;
            Ok(())
        })?;
        node.set_attribute("x", &(x - PIECE_SIZE / 2.0).to_string())?;
        node.set_attribute("y", &(y - PIECE_SIZE / 2.0).to_string())?;
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
        let bg_node = document.ensure_svg_node("path", &bg_node_id(piece), &layer, |bg_node| {
            bg_node
                .set_attribute("data-promotion-target", &piece.to_full_algebraic().to_string())?;
            bg_node.set_attribute("class", "promotion-target-bg promotion-target-bg-secondary")?;
            Ok(())
        })?;
        bg_node.set_attribute(
            "d",
            &svg::ring_arc_path(pos.x, pos.y, INNER_RADIUS, OUTER_RADIUS, from_rad, to_rad),
        )?;
        make_fg_node(piece, piece_pos.0, piece_pos.1)?;
    }

    let bg_node =
        document.ensure_svg_node("circle", &bg_node_id(primary_target), &layer, |bg_node| {
            bg_node.set_attribute(
                "data-promotion-target",
                &primary_target.to_full_algebraic().to_string(),
            )?;
            bg_node.set_attribute("class", "promotion-target-bg promotion-target-bg-primary")?;
            Ok(())
        })?;
    bg_node.set_attribute("cx", &pos.x.to_string())?;
    bg_node.set_attribute("cy", &pos.y.to_string())?;
    bg_node.set_attribute("r", &INNER_RADIUS.to_string())?;
    make_fg_node(primary_target, pos.x, pos.y)?;

    Ok(())
}

fn render_starting() -> JsResult<()> {
    use PieceKind::*;
    let board_shape = BoardShape { num_rows: 8, num_cols: 8 };
    let perspective = Perspective::for_participant(BughouseParticipant::default_observer());
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
    render_archive_game_list(None, None)?;
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
    player_idx: DisplayPlayer,
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
    if let Some(diff) = diff
        && let Some(diff_ui_string) = diff.ui_string()
    {
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
        diff_node.set_text_content(Some(&diff_ui_string));
        diff_node.class_list().toggle_with_force("display-none", false)?;
    } else {
        diff_node.class_list().toggle_with_force("display-none", true)?;
    }

    Ok(())
}

fn update_participants_and_scores(
    scores: &Option<Scores>, participants: &[Participant], game_status: BughouseGameStatus,
    is_active_match: bool,
) -> JsResult<()> {
    let show_readiness = !game_status.is_active() && is_active_match;
    let table = web_document().create_element("table")?;
    let mut observers = vec![];
    match scores {
        None => {}
        Some(Scores::PerTeam(score_map)) => {
            table.class_list().add_1("team-score-table")?;
            let mut team_players = enum_map! { _ => vec![] };
            for p in participants {
                match p.active_faction {
                    Faction::Fixed(team) => team_players[team].push(p),
                    Faction::Random => panic!("Unexpected Faction::Random with Scores::PerTeam"),
                    Faction::Observer => observers.push(p),
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
        Some(Scores::PerPlayer) => {
            // TODO: More robust seat out detection to identify obscure cases like playing 1 on 3.
            table.class_list().add_1("individual-score-table")?;
            let mut players;
            (players, observers) = participants
                .iter()
                .partition(|p| p.games_played > 0 || p.active_faction.is_player());
            players.sort_by_key(|p| (!p.active_faction.is_player(), p.games_played == 0));
            for p in players {
                let score = p.individual_score.as_f64().to_string();
                let (score_whole, score_fraction) = match score.split_once('.') {
                    Some((whole, fraction)) => (whole.to_owned(), format!(".{}", fraction)),
                    None => (score, "".to_owned()),
                };
                let p_node = participant_node(&p, show_readiness)?;
                let tr = table.append_new_element("tr")?;
                tr.append_new_element("td")?
                    .with_classes(["score-player-name"])?
                    .append_child(&p_node)?;
                tr.append_new_element("td")?
                    .with_classes(["individual-score-value"])?
                    .with_text_content(&score_whole);
                tr.append_new_element("td")?
                    .with_classes(["individual-score-fraction"])?
                    .with_text_content(&score_fraction);
                tr.append_new_element("td")?
                    .with_classes(["individual-score-total"])?
                    .with_text_content(&format!("/{}", p.games_played));
            }
        }
    }
    let score_node = web_document().get_existing_element_by_id("score-body")?;
    score_node.replace_children_with_node_1(&table);

    let observers_node = web_document().get_existing_element_by_id("observers")?;
    observers_node.remove_all_children();
    for p in observers {
        let node = observers_node.append_new_element("div")?;
        let p_node = participant_node(p, false)?;
        node.append_child(&p_node)?;
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
    display_board_idx: DisplayBoard, wayback: &WaybackState,
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
    for record in game.turn_log().iter() {
        let index = record.index;
        let line_node = document.create_element("div")?;
        if Some(index) == wayback.display_turn_index() {
            if wayback.active() {
                line_node.class_list().add_1("wayback-current-turn-active")?;
            } else {
                line_node.class_list().add_1("wayback-current-turn-inactive")?;
            }
        }
        line_node.set_attribute("data-turn-index", &index.to_string())?;

        if record.envoy.board_idx == board_idx {
            let force = record.envoy.force;
            let mut turn_number_str = String::new();
            if prev_number != record.local_number {
                turn_number_str = format!("{}.", record.local_number);
                prev_number = record.local_number;
            }
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
                _ => "log-record-m",
            };

            line_node.set_attribute("id", &turn_record_node_id(index, TurnRecordBoard::Main))?;
            line_node.class_list().add_3(
                "log-turn-record",
                &format!("log-turn-record-{}", force_id(force)),
                width_class,
            )?;

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
        } else {
            // Add records for turns made on the other board when appropriate. Otherwise, add a
            // ghost record. During wayback it shows timestamp on this board when a turn on another
            // board is active.
            line_node
                .set_attribute("id", &turn_record_node_id(index, TurnRecordBoard::Auxiliary))?;
            if !record.turn_expanded.steals.is_empty() {
                // No need to add a width class: steal records are always small.
                line_node
                    .class_list()
                    .add_2("log-turn-record", "log-turn-record-intervention")?;

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
            } else {
                line_node.class_list().add_1("log-turn-ghost-record")?;
            }
        }
        log_node.append_child(&line_node)?;
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

fn scroll_to_wayback_turn(wayback: WaybackState) {
    for turn_record_board in TurnRecordBoard::iter() {
        let node = wayback.display_turn_index().and_then(|index| {
            web_document().get_element_by_id(&turn_record_node_id(index, turn_record_board))
        });
        if let Some(node) = node {
            node.scroll_into_view_with_scroll_into_view_options(
                ScrollIntoViewOptions::new()
                    .behavior(ScrollBehavior::Instant)
                    .block(ScrollLogicalPosition::Nearest),
            );
        }
    }
}

fn setup_participation_mode(participant_id: BughouseParticipant) -> JsResult<()> {
    use BughousePlayer::*;
    let (is_symmetric, is_observer) = match participant_id {
        BughouseParticipant::Observer(_) => (true, true),
        BughouseParticipant::Player(SinglePlayer(_)) => (false, false),
        BughouseParticipant::Player(DoublePlayer(_)) => (true, false),
    };
    let body = web_document().body()?;
    body.class_list().toggle_with_force("symmetric", is_symmetric)?;
    body.class_list().toggle_with_force("observer", is_observer)?;
    Ok(())
}

fn update_cannot_start_alert(mtch: &Match) -> JsResult<()> {
    let particpants_status = verify_participants(&mtch.rules, mtch.participants.iter());
    let alert = match particpants_status {
        ParticipantsStatus::CanStart { .. } => None,
        ParticipantsStatus::CannotStart(error) => Some(cannot_start_game_message(error)),
    };
    _ = alert;
    let alert_node = web_document().get_existing_element_by_id("cannot-start-alert")?;
    alert_node.set_text_content(alert);
    alert_node
        .class_list()
        .toggle_with_force("visibility-hidden", alert.is_none())?;
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
            let coord = Coord::new(row, col);
            let display_coord = to_display_coord(coord, board_shape, board_orientation);
            let DisplayFCoord { x, y } = DisplayFCoord::square_pivot(display_coord);
            sq.set_attribute("x", &x.to_string())?;
            sq.set_attribute("y", &y.to_string())?;
            sq.set_attribute("width", "1")?;
            sq.set_attribute("height", "1")?;
            sq.set_attribute("class", square_color_class(coord.color()))?;
            layer.append_child(&sq)?;
            if display_coord.x == 0 {
                let caption = document.create_svg_element("text")?;
                caption.set_text_content(Some(&String::from(row.to_algebraic(board_shape))));
                caption.set_attribute("x", &(x + text_h_padding).to_string())?;
                caption.set_attribute("y", &(y + text_v_padding).to_string())?;
                caption.set_attribute("dominant-baseline", "hanging")?;
                caption.set_attribute("class", square_text_color_class(coord.color()))?;
                layer.append_child(&caption)?;
            }
            if display_coord.y == board_shape.num_rows as i8 - 1 {
                let caption = document.create_svg_element("text")?;
                caption.set_text_content(Some(&String::from(col.to_algebraic(board_shape))));
                caption.set_attribute("x", &(x + 1.0 - text_h_padding).to_string())?;
                caption.set_attribute("y", &(y + 1.0 - text_v_padding).to_string())?;
                caption.set_attribute("text-anchor", "end")?;
                caption.set_attribute("class", square_text_color_class(coord.color()))?;
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

    let add_layer = |id: String,
                     shape_rendering: ShapeRendering,
                     pointer_events: PointerEvents|
     -> JsResult<()> {
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
        match pointer_events {
            PointerEvents::Auto => {}
            PointerEvents::None => {
                layer.set_attribute("pointer-events", "none")?;
            }
        }
        svg.append_child(&layer)?;
        Ok(())
    };

    let shadow = make_board_rect(&document)?;
    shadow.set_attribute("class", "board-shadow")?;
    svg.append_child(&shadow)?;

    add_layer(square_grid_layer_id(board_idx), ShapeRendering::CrispEdges, PointerEvents::Auto)?;
    render_grid(board_idx, board_shape, perspective)?;

    let border = make_board_rect(&document)?;
    border.set_attribute("class", "board-border")?;
    svg.append_child(&border)?;

    add_layer(
        square_highlight_layer_id(SquareHighlightLayer::Turn, board_idx),
        ShapeRendering::CrispEdges,
        PointerEvents::None,
    )?;
    add_layer(
        chalk_highlight_layer_id(board_idx),
        ShapeRendering::CrispEdges,
        PointerEvents::None,
    )?;
    add_layer(piece_layer_id(board_idx), ShapeRendering::Normal, PointerEvents::Auto)?;
    add_layer(fog_of_war_layer_id(board_idx), ShapeRendering::Normal, PointerEvents::Auto)?;
    // Highlight layer for squares inside the fog of war.
    add_layer(
        square_highlight_layer_id(SquareHighlightLayer::TurnAbove, board_idx),
        ShapeRendering::CrispEdges,
        PointerEvents::None,
    )?;
    // Place drag highlight layer above pieces to allow legal move highlight for captures.
    // Note that the dragged piece will still be above the highlight.
    add_layer(
        square_highlight_layer_id(SquareHighlightLayer::Ephemeral, board_idx),
        ShapeRendering::CrispEdges,
        PointerEvents::None,
    )?;
    add_layer(chalk_drawing_layer_id(board_idx), ShapeRendering::Normal, PointerEvents::None)?;
    add_layer(
        promotion_target_layer_id(board_idx),
        ShapeRendering::Normal,
        PointerEvents::Auto,
    )?;

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

fn reset_chat() -> JsResult<()> {
    let chat_node = web_document().get_existing_element_by_id("chat-text-area")?;
    chat_node.remove_all_children();
    Ok(())
}

fn render_archive_game_list(
    games: Option<Vec<FinishedGameDescription>>, user_name: Option<&str>,
) -> JsResult<()> {
    let document = web_document();
    let make_player_node = |name: &str| -> JsResult<web_sys::Element> {
        let node = document.create_element("span")?;
        if Some(name) == user_name {
            Ok(node.with_classes(["game-archive-me"])?.with_text_content("Me"))
        } else {
            Ok(node.with_text_content(name))
        }
    };
    let make_team_td = |player_names: Vec<String>| -> JsResult<web_sys::Element> {
        let td = document.create_element("td")?;
        match player_names.len() {
            0 => return Err(rust_error!()),
            1 => {
                td.append_element(make_player_node(&player_names[0])?)?;
                td.append_text_span(&" ×2", ["game-archive-double-play"])?;
            }
            2 => {
                td.append_element(make_player_node(&player_names[0])?)?;
                td.append_text_span(", ", [])?;
                td.append_element(make_player_node(&player_names[1])?)?;
            }
            _ => return Err(rust_error!()),
        }
        Ok(td)
    };
    let table = document.get_existing_element_by_id("archive-game-table")?;
    table.remove_all_children();
    {
        let thead = table.append_new_element("thead")?;
        let tr = thead.append_new_element("tr")?;
        tr.append_new_element("th")?.with_text_content("Game time");
        tr.append_new_element("th")?
            .with_text_content("R")
            .with_attribute("title", "Whether the game was rated")?;
        tr.append_new_element("th")?
            .with_text_content("Teammates")
            .with_attribute("title", "Your team (White, Black)")?;
        tr.append_new_element("th")?
            .with_text_content("Opponents")
            .with_attribute("title", "Opposing team (White, Black)")?;
        tr.append_new_element("th")?.with_text_content("Result");
        tr.append_new_element("th")?
            .with_attribute("title", "Hover the icon to preview game, click to open")?;
    }
    let tbody = table.append_new_element("tbody")?;
    let Some(games) = games else {
        tbody
            .append_new_element("div")?
            .with_classes(["game-archive-placeholder-message"])?
            .with_text_content("Loading ")
            .append_animated_dots()?;
        return Ok(());
    };
    if games.is_empty() {
        tbody
            .append_new_element("div")?
            .with_classes(["game-archive-placeholder-message"])?
            .with_text_content("You games will appear here.");
    }
    for game in games.into_iter().rev() {
        let game_view_available;
        let (result, result_class) = match game.result {
            SubjectiveGameResult::Victory => ("Won", "game-archive-result-victory"),
            SubjectiveGameResult::Defeat => ("Lost", "game-archive-result-defeat"),
            SubjectiveGameResult::Draw => ("Draw", "game-archive-result-draw"),
            SubjectiveGameResult::Observation => ("—", "game-archive-result-observation"),
        };
        let tr = tbody
            .append_new_element("tr")?
            .with_id(&archive_game_row_id(game.game_id))
            .with_classes([result_class])?;
        {
            let time_offset = UtcOffset::current_local_offset().unwrap_or(offset!(UTC));
            let game_start_utc = OffsetDateTime::from(game.game_start_time);
            // Before this date our BPGNs didn't contain enough information to parse the game.
            game_view_available = game_start_utc >= datetime!(2023-03-01 00:00:00 UTC);
            let game_start_local = game_start_utc.to_offset(time_offset);
            let today = OffsetDateTime::now_utc().to_offset(time_offset);
            let start_time = if game_start_local.date() == today.date() {
                game_start_local.format(format_description!("[hour]:[minute]")).unwrap()
            } else {
                game_start_local.format(format_description!("[year]-[month]-[day]")).unwrap()
            };
            tr.append_new_element("td")?.set_text_content(Some(&start_time));
        }
        {
            let rated_td = tr.append_new_element("td")?;
            if game.rated {
                rated_td.set_text_content(Some("⚔️"));
            }
        }
        tr.append_element(make_team_td(game.teammates)?)?;
        tr.append_element(make_team_td(game.opponents)?)?;
        tr.append_new_element("td")?.with_text_content(result);
        {
            let view_td = tr.append_new_element("td")?;
            if game_view_available {
                view_td
                    .with_text_content("👀")
                    .with_classes(["view-archive-game"])?
                    .with_attribute("archive-game-id", &game.game_id.to_string())?;
            }
        }
    }
    Ok(())
}

fn highlight_archive_game_row(game_id: i64) -> JsResult<()> {
    let document = web_document();
    document.purge_class_name("game-archive-hovered-row")?;
    if let Some(row_node) = document.get_element_by_id(&archive_game_row_id(game_id)) {
        row_node.class_list().add_1("game-archive-hovered-row")?;
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
fn parse_force_id(id: &str) -> JsResult<Force> {
    match id {
        "white" => Ok(Force::White),
        "black" => Ok(Force::Black),
        _ => Err(format!(r#"Invalid force: "{id}""#).into()),
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

fn faction_id(faction: Faction) -> &'static str {
    match faction {
        Faction::Fixed(Team::Red) => "team_red",
        Faction::Fixed(Team::Blue) => "team_blue",
        Faction::Random => "random",
        Faction::Observer => "observer",
    }
}
fn parse_faction_id(id: &str) -> JsResult<Faction> {
    match id {
        "team_red" => Ok(Faction::Fixed(Team::Red)),
        "team_blue" => Ok(Faction::Fixed(Team::Blue)),
        "random" => Ok(Faction::Random),
        "observer" => Ok(Faction::Observer),
        _ => Err(format!(r#"Invalid faction: "{id}""#).into()),
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

fn reserve_piece_id(board_idx: DisplayBoard, force: Force, piece_kind: PieceKind) -> String {
    format!(
        "reserve-{}-{}-{}",
        board_id(board_idx),
        force_id(force),
        piece_kind.to_full_algebraic()
    )
}
fn parse_reserve_piece_id(id: &str) -> Option<(DisplayBoard, Force, PieceKind)> {
    let (reserve_literal, board_idx, force, piece_kind) = id.split('-').collect_tuple()?;
    if reserve_literal != "reserve" {
        return None;
    }
    let board_idx = parse_board_id(board_idx).ok()?;
    let force = parse_force_id(force).ok()?;
    let piece_kind = PieceKind::from_algebraic(piece_kind)?;
    Some((board_idx, force, piece_kind))
}

fn parse_location_id(id: &str) -> Option<(DisplayBoard, Location)> {
    if let Some((display_board_idx, force, piece)) = parse_reserve_piece_id(id) {
        Some((display_board_idx, Location::Reserve(force, piece)))
    } else if let Some((display_board_idx, coord)) = parse_square_id(id) {
        Some((display_board_idx, Location::Square(coord)))
    } else {
        None
    }
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

fn turn_record_node_id(index: TurnIndex, board: TurnRecordBoard) -> String {
    match board {
        TurnRecordBoard::Main => format!("turn-record-{}", index.to_string()),
        TurnRecordBoard::Auxiliary => format!("turn-record-aux-{}", index.to_string()),
    }
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
fn square_highlight_class_id(h: &SquareHighlight) -> String {
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
        TurnHighlightItem::DragStart => "dragstart",
        TurnHighlightItem::LegalDestination => "legaldestination",
    };
    let layer = match h.layer {
        TurnHighlightLayer::AboveFog => "-above",
        TurnHighlightLayer::BelowFog => "",
    };
    format!("{}-turn-{}{}-highlight", family, item, layer)
}

fn square_highlight_layer_id(layer: SquareHighlightLayer, board_idx: DisplayBoard) -> String {
    let layer_id = match layer {
        SquareHighlightLayer::Turn => "turn",
        SquareHighlightLayer::TurnAbove => "turn-above",
        SquareHighlightLayer::Ephemeral => "drag",
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

fn square_text_color_class(color: Force) -> &'static str {
    match color {
        Force::White => "on-sq-white",
        Force::Black => "on-sq-black",
    }
}

fn square_color_class(color: Force) -> &'static str {
    match color {
        Force::White => "sq-white",
        Force::Black => "sq-black",
    }
}

fn archive_game_row_id(game_id: i64) -> String { format!("archive-game-row-{}", game_id) }

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
        (Player(DoublePlayer(_)) | Observer(_), DisplayBoard::Primary) => Ok(-1.),
        (Player(DoublePlayer(_)) | Observer(_), DisplayBoard::Secondary) => Ok(1.),
    }
}
