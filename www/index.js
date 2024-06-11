// TODO: Remove logging (or at least don't log heartbeat events).
// TODO: Check if ==/!= have to be replaced with ===/!== and other JS weirdness.
// TODO: Figure out if it's possible to enable strict mode with webpack.

import "./main.css";
import * as wasm from "bughouse-chess";

import transparent from "../assets/transparent.png";

import white_pawn from "../assets/pieces/white-pawn.png";
import white_knight from "../assets/pieces/white-knight.png";
import white_bishop from "../assets/pieces/white-bishop.png";
import white_rook from "../assets/pieces/white-rook.png";
import white_queen from "../assets/pieces/white-queen.png";
import white_cardinal from "../assets/pieces/white-cardinal.png";
import white_empress from "../assets/pieces/white-empress.png";
import white_amazon from "../assets/pieces/white-amazon.png";
import white_king from "../assets/pieces/white-king.png";
import white_king_broken from "../assets/pieces/white-king-broken.png";
import black_pawn from "../assets/pieces/black-pawn.png";
import black_knight from "../assets/pieces/black-knight.png";
import black_bishop from "../assets/pieces/black-bishop.png";
import black_rook from "../assets/pieces/black-rook.png";
import black_queen from "../assets/pieces/black-queen.png";
import black_cardinal from "../assets/pieces/black-cardinal.png";
import black_empress from "../assets/pieces/black-empress.png";
import black_amazon from "../assets/pieces/black-amazon.png";
import black_king from "../assets/pieces/black-king.png";
import black_king_broken from "../assets/pieces/black-king-broken.png";
import duck from "../assets/pieces/duck.png";

import fog_1 from "../assets/fog-of-war/fog-1.png";
import fog_2 from "../assets/fog-of-war/fog-2.png";
import fog_3 from "../assets/fog-of-war/fog-3.png";

import clack_sound from "../assets/sounds/clack.ogg";
import turn_sound from "../assets/sounds/turn.ogg";
import reserve_restocked_sound from "../assets/sounds/reserve-restocked.ogg";
import piece_stolen_sound from "../assets/sounds/piece-stolen.ogg";
import low_time_sound from "../assets/sounds/low-time.ogg";
import victory_sound from "../assets/sounds/victory.ogg";
import defeat_sound from "../assets/sounds/defeat.ogg";
import draw_sound from "../assets/sounds/draw.ogg";

class WasmClientDoesNotExist {}
class WasmClientPanicked {}
class InvalidCommand {
  constructor(msg) {
    this.msg = msg;
  }
}

class Timer {
  constructor() {
    this.t0 = performance.now();
  }
  finish() {
    const t1 = performance.now();
    const d = t1 - this.t0;
    this.t0 = t1;
    return d;
  }
  meter(m) {
    m.record(this.finish());
  }
}

class MyButton {
  static HIDE = Symbol(); // `Escape` button will hide the dialog iff `HIDE` button exists
  static DO = Symbol();
  static OPTION_1 = Symbol();
  static OPTION_2 = Symbol();
  constructor(label, action) {
    this.label = label;
    this.action = action;
  }
}

function log_time() {
  if (typeof log_time.start == "undefined") {
    log_time.start = performance.now();
  }
  const sec = (performance.now() - log_time.start) / 1000.0;
  return `[t=${sec.toFixed(2)}]`;
}
log_time(); // start the counter

// Improvement potential. Similarly group other global variables.
const Storage = {
  cookies_accepted: "cookies-accepted", // values: null, "essential", "all"
  chat_reference_tooltip: "chat-reference-tooltip", // values: "show" (default), "hide"
  player_name: "player-name",
};

const SearchParams = {
  match_id: "match-id",
  archive_game_id: "archive-game-id",
  server: "server",
};

const page_element = document.getElementById("page");
const chat_text_area = document.getElementById("chat-text-area");
const chat_input = document.getElementById("chat-input");
const chat_send_button = document.getElementById("chat-send-button");
const chat_reference_tooltip_container = document.getElementById(
  "chat-reference-tooltip-container"
);
const chat_reference_tooltip_hide = document.getElementById("chat-reference-tooltip-hide");
const connection_info = document.getElementById("connection-info");

const menu_backdrop = document.getElementById("menu-backdrop");
const menu_dialog = document.getElementById("menu-dialog");
const menu_start_page = document.getElementById("menu-start-page");
const menu_authorization_page = document.getElementById("menu-authorization-page");
const menu_login_page = document.getElementById("menu-login-page");
const menu_signup_page = document.getElementById("menu-signup-page");
const menu_signup_with_google_page = document.getElementById("menu-signup-with-google-page");
const menu_signup_with_lichess_page = document.getElementById("menu-signup-with-lichess-page");
const menu_view_account_page = document.getElementById("menu-view-account-page");
const menu_change_account_page = document.getElementById("menu-change-account-page");
const menu_delete_account_page = document.getElementById("menu-delete-account-page");
const menu_create_match_name_page = document.getElementById("menu-create-match-name-page");
const menu_create_match_page = document.getElementById("menu-create-match-page");
const menu_join_match_page = document.getElementById("menu-join-match-page");
const menu_lobby_page = document.getElementById("menu-lobby-page");
const menu_game_archive_game = document.getElementById("menu-game-archive-page");
const menu_pages = document.getElementsByClassName("menu-page");

const cookie_banner = document.getElementById("cookie-banner");
const accept_essential_cookies_button = document.getElementById("accept-essential-cookies-button");
const accept_all_cookies_button = document.getElementById("accept-all-cookies-button");

const registered_user_bar = document.getElementById("registered-user-bar");
const view_account_button = document.getElementById("view-account-button");
const guest_user_bar = document.getElementById("guest-user-bar");
const guest_user_tooltip = document.getElementById("guest-user-tooltip");
const authorization_button = document.getElementById("authorization-button");
const log_out_button = document.getElementById("log-out-button");
const sign_with_google_button = document.getElementById("sign-with-google-button");
const sign_with_lichess_button = document.getElementById("sign-with-lichess-button");
const begin_login_button = document.getElementById("begin-login-button");
const begin_signup_button = document.getElementById("begin-signup-button");
const view_account_change_button = document.getElementById("view-account-change-button");
const view_account_delete_button = document.getElementById("view-account-delete-button");
const change_account_email = document.getElementById("change-account-email");

const create_rated_match_button = document.getElementById("create-rated-match-button");
const create_unrated_match_button = document.getElementById("create-unrated-match-button");
const join_match_button = document.getElementById("join-match-button");
const jc_match_id = document.getElementById("jc-match-id");
const jc_confirm_button = document.getElementById("jc-confirm-button");
const lobby_leave_button = document.getElementById("lobby-leave-button");
const game_archive_button = document.getElementById("game-archive-button");

const leave_match_button = document.getElementById("leave-match-button");
const ready_button = document.getElementById("ready-button");
const ready_button_caption = document.getElementById("ready-button-caption");
const resign_button = document.getElementById("resign-button");
const toggle_faction_button = document.getElementById("toggle-faction-button");
const rules_button = document.getElementById("rules-button");
const export_button = document.getElementById("export-button");
const volume_button = document.getElementById("volume-button");
const shared_wayback_button = document.getElementById("shared-wayback-button");

const svg_defs = document.getElementById("svg-defs");

function board_svg(board_id) {
  return document.getElementById(`board-${board_id}`);
}
function reserve_svg(board_id, player_id) {
  return document.getElementById(`reserve-${board_id}-${player_id}`);
}

const menu_page_stack = [];

const loading_tracker = new (class {
  #resources_required = 0;
  #resources_loaded = 0;

  resource_required() {
    this.#resources_required += 1;
    this.#update();
  }
  resource_loaded() {
    this.#resources_loaded += 1;
    this.#update();
  }
  #update() {
    // TODO: Don't start the game until `ready`.
    console.assert(this.#resources_loaded <= this.#resources_required);
    const ready = this.#resources_loaded == this.#resources_required;
    if (ready) {
      console.log(`All resources loaded ${this.#resources_loaded}`);
    }
  }
})();

window.dataLayer = window.dataLayer || [];
function gtag() {
  window.dataLayer.push(arguments);
}
update_cookie_policy();

const FOG_TILE_SIZE = 1.2;
load_svg_images([
  { path: transparent, symbol: "transparent" },
  { path: white_pawn, symbol: "white-pawn" },
  { path: white_knight, symbol: "white-knight" },
  { path: white_bishop, symbol: "white-bishop" },
  { path: white_rook, symbol: "white-rook" },
  { path: white_queen, symbol: "white-queen" },
  { path: white_cardinal, symbol: "white-cardinal" },
  { path: white_empress, symbol: "white-empress" },
  { path: white_amazon, symbol: "white-amazon" },
  { path: white_king, symbol: "white-king" },
  { path: white_king_broken, symbol: "white-king-broken" },
  { path: black_pawn, symbol: "black-pawn" },
  { path: black_knight, symbol: "black-knight" },
  { path: black_bishop, symbol: "black-bishop" },
  { path: black_rook, symbol: "black-rook" },
  { path: black_queen, symbol: "black-queen" },
  { path: black_cardinal, symbol: "black-cardinal" },
  { path: black_empress, symbol: "black-empress" },
  { path: black_amazon, symbol: "black-amazon" },
  { path: black_king, symbol: "black-king" },
  { path: black_king_broken, symbol: "black-king-broken" },
  { path: duck, symbol: "duck" },
  { path: fog_1, symbol: "fog-1", size: FOG_TILE_SIZE },
  { path: fog_2, symbol: "fog-2", size: FOG_TILE_SIZE },
  { path: fog_3, symbol: "fog-3", size: FOG_TILE_SIZE },
]);

// Ideally AudioContext should be created after the first user interaction. Otherwise
// it's be created in the "suspended" state, and a log warning will be shown (in Chrome):
// https://developer.chrome.com/blog/autoplay/#webaudio
// But we can't really create the context later, because we need it to load sounds.
let audio_context = new AudioContext();

// Improvement potential. Establish priority on sounds; play more important sounds first
// in case of a clash.
const Sound = load_sounds({
  clack: clack_sound, // similar to `turn` and roughly the same volume
  turn: turn_sound,
  reserve_restocked: reserve_restocked_sound,
  piece_stolen: piece_stolen_sound,
  low_time: low_time_sound,
  victory: victory_sound,
  defeat: defeat_sound,
  draw: draw_sound,
});

wasm.set_panic_hook();
wasm.init_page();
console.log("bughouse.pro client version:", wasm.git_version());

set_up_drag_and_drop();
set_up_chalk_drawing();
set_up_menu_pointers();
set_up_log_navigation();

let wasm_client_object = make_wasm_client();
let wasm_client_panicked = false;

let fatal_error_shown = false;

let last_socket_connection_attempt = null;
let consecutive_socket_connection_attempts = 0;
let socket = null;
open_socket();

// Parameters and data structures for the audio logic. Our goal is to make short and
// important sounds (like turn sound) as clear as possible when several events occur
// simultaneously. The main example is when you make a move and immediately get a
// premove back.
const audio_min_interval_ms = 70;
const audio_max_queue_size = 5;
const max_volume = 3;
const volume_to_js = {
  0: 0.0,
  1: 0.25,
  2: 0.5,
  3: 1.0,
};
let audio_last_played = 0;
let audio_queue = [];
let audio_volume = 0;

const gain_node = audio_context.createGain();
gain_node.connect(audio_context.destination);
const pan_nodes = new Map();
const drag_start_threshold = 8;
let pointer_down_position = null;
let pointer_down_element = null;
let pointer_down_is_main_pointer = null;
let drag_element = null;
let drag_source_board_id = null;
function drag_source_board() {
  return board_svg(drag_source_board_id);
}

const Meter = make_meters();

init_menu();
auto_open_menu();

let is_registered_user = false;
update_session();

document.addEventListener("click", on_document_click);
document.addEventListener("mouseover", on_document_mouseover);
document.addEventListener("keydown", on_document_keydown);
document.addEventListener("paste", on_document_paste);

chat_text_area.addEventListener("wheel", chat_wheel_scroll);
chat_input.addEventListener("input", () => update_chat_input());
chat_input.addEventListener("keydown", on_chat_input_keydown);
chat_input.addEventListener("focusin", () => update_chat_reference_tooltip());
chat_input.addEventListener("focusout", () => update_chat_reference_tooltip());
chat_send_button.addEventListener("click", () => execute_chat_input());
chat_reference_tooltip_hide.addEventListener("click", () =>
  set_show_chat_reference_tooltip("hide")
);

leave_match_button.addEventListener("click", leave_match);
ready_button.addEventListener("click", () => execute_input("/ready"));
resign_button.addEventListener("click", request_resign);
toggle_faction_button.addEventListener("click", toggle_faction_ingame);
rules_button.addEventListener("click", () => execute_input("/rules"));
export_button.addEventListener("click", () => execute_input("/save"));
volume_button.addEventListener("click", next_volume);
shared_wayback_button.addEventListener("click", toggle_shared_wayback);

accept_essential_cookies_button.addEventListener("click", on_accept_essential_cookies);
accept_all_cookies_button.addEventListener("click", on_accept_all_cookies);
menu_dialog.addEventListener("cancel", (event) => event.preventDefault());
view_account_button.addEventListener("click", () => push_menu_page(menu_view_account_page));
authorization_button.addEventListener("click", () => push_menu_page(menu_authorization_page));
log_out_button.addEventListener("click", log_out);
sign_with_google_button.addEventListener("click", sign_with_google);
sign_with_lichess_button.addEventListener("click", sign_with_lichess);
begin_login_button.addEventListener("click", () => push_menu_page(menu_login_page));
begin_signup_button.addEventListener("click", () => push_menu_page(menu_signup_page));
view_account_change_button.addEventListener("click", () =>
  push_menu_page(menu_change_account_page)
);
view_account_delete_button.addEventListener("click", () =>
  push_menu_page(menu_delete_account_page)
);
menu_login_page.addEventListener("submit", log_in);
menu_signup_page.addEventListener("submit", sign_up);
menu_signup_with_google_page.addEventListener("submit", sign_up_with_google);
menu_signup_with_lichess_page.addEventListener("submit", sign_up_with_lichess);
menu_change_account_page.addEventListener("submit", change_account);
menu_delete_account_page.addEventListener("submit", delete_account);
create_rated_match_button.addEventListener("click", (event) =>
  on_create_match_request(event, true)
);
create_unrated_match_button.addEventListener("click", (event) =>
  on_create_match_request(event, false)
);
join_match_button.addEventListener("click", on_join_match_submenu);
menu_create_match_name_page.addEventListener("submit", create_match_as_guest);
menu_create_match_page.addEventListener("submit", on_create_match_confirm);
menu_join_match_page.addEventListener("submit", on_join_match_confirm);
lobby_leave_button.addEventListener("click", leave_match_lobby);
game_archive_button.addEventListener("click", view_archive_game_list);

for (const button of document.querySelectorAll(".back-button")) {
  button.addEventListener("click", pop_menu_page);
}
for (const button of document.querySelectorAll("[data-suburl]")) {
  button.addEventListener("click", go_to_suburl);
}

// TODO: Make sounds louder and set volume to 2 by default.
set_volume(max_volume);

setInterval(on_tick, 50);

// TODO: Support async function and use in `toggle_faction_ingame`.
function with_error_handling(f) {
  // Note. Re-throw all unexpected errors to get a stacktrace.
  try {
    f();
  } catch (e) {
    if (e instanceof WasmClientDoesNotExist) {
      fatal_error_dialog("Internal error! WASM object does not exist.");
      throw e;
    } else if (e instanceof WasmClientPanicked) {
      // Error dialog should already be shown.
    } else if (e instanceof InvalidCommand) {
      wasm_client().show_command_error(e.msg);
      update();
    } else if (e?.constructor?.name == "IgnorableError") {
      ignorable_error_dialog(e.message);
    } else if (e?.constructor?.name == "KickedFromMatch") {
      ignorable_error_dialog(e.message);
      // Need to recreate the socket because server aborts the connection here.
      // If this turns out to be buggy, could do
      //   ignorable_error_dialog(e.message).then(() => location.reload());
      // instead.
      open_socket("kicked");
      open_menu();
      push_menu_page(menu_join_match_page);
    } else if (e?.constructor?.name == "FatalError") {
      fatal_error_dialog(e.message);
    } else if (e?.constructor?.name == "RustError") {
      ignorable_error_dialog(`Internal error: ${e.message}`);
      if (socket.readyState == WebSocket.OPEN) {
        socket.send(wasm.make_rust_error_event(e));
      }
      throw e;
    } else {
      const rust_panic = wasm.last_panic();
      if (rust_panic) {
        wasm_client_panicked = true;
        let report = "";
        if (socket.readyState == WebSocket.OPEN) {
          socket.send(rust_panic);
        } else {
          report = "Please consider reporting the error to contact.bughousepro@gmail.com";
        }
        fatal_error_dialog(
          "Internal error! This client is now dead 💀 " +
            "Only refreshing the page may help you. We are very sorry. " +
            report
        );
      } else {
        console.log(log_time(), "Unknown error: ", e);
        ignorable_error_dialog(`Unknown error: ${e}`);
        if (socket.readyState == WebSocket.OPEN) {
          // Improvement potential. Include stack trace.
          socket.send(wasm.make_unknown_error_event(e.toString()));
        }
        throw e;
      }
    }
  }
}

function wasm_client() {
  if (wasm_client_panicked) {
    throw new WasmClientPanicked();
  } else if (wasm_client_object) {
    return wasm_client_object;
  } else {
    throw new WasmClientDoesNotExist();
  }
}

function make_wasm_client() {
  const user_agent = window.navigator.userAgent;
  const time_zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  return wasm.WebClient.new_client(user_agent, time_zone);
}

function make_meters() {
  return {
    process_outgoing_events: wasm_client().meter("process_outgoing_events"),
    process_notable_events: wasm_client().meter("process_notable_events"),
    refresh: wasm_client().meter("refresh"),
    update_state: wasm_client().meter("update_state"),
    update_clock: wasm_client().meter("update_clock"),
    update_drag_state: wasm_client().meter("update_drag_state"),
  };
}

function on_socket_message(event) {
  with_error_handling(function () {
    // console.log(log_time(), 'server: ', event.data);
    const update_needed = wasm_client().process_server_event(event.data);
    if (update_needed) {
      update();
    }
  });
}
function on_socket_open(event) {
  with_error_handling(function () {
    console.info(log_time(), "WebSocket connection opened");
    consecutive_socket_connection_attempts = 0;
    wasm_client().hot_reconnect();
  });
}
function on_socket_close(event) {
  open_socket("closed");
}
function on_socker_error(event) {
  // TODO: Report socket errors.
  console.warn(log_time(), "WebSocket error: ", event);
}

// Closes WebSocket and ignores all further messages and other events.
function cut_off_socket() {
  if (socket !== null) {
    socket.removeEventListener("message", on_socket_message);
    socket.removeEventListener("open", on_socket_open);
    socket.removeEventListener("error", on_socker_error);
    socket.removeEventListener("close", on_socket_close);
    socket.close();
  }
}

function open_socket(reason) {
  let now = performance.now();
  if (last_socket_connection_attempt !== null && now - last_socket_connection_attempt <= 7000) {
    return;
  }
  last_socket_connection_attempt = now;
  consecutive_socket_connection_attempts += 1;
  if (consecutive_socket_connection_attempts > 3) {
    fatal_error_dialog("Cannot connect to the server. Sorry! Please come again later.");
  }
  if (reason) {
    console.warn(log_time(), `WebSocket: ${reason}. Reconnecting...`);
  }

  // Ignore all further events from WebSocket. They could mess up with the client state
  // if they arrive in parallel with the events in the new socket. Plus, we don't want
  // to receive `JoinedInAnotherClient` error on reconnect.
  cut_off_socket();

  socket = new WebSocket(server_websocket_address());
  socket.addEventListener("message", on_socket_message);
  socket.addEventListener("open", on_socket_open);
  socket.addEventListener("error", on_socker_error);
  socket.addEventListener("close", on_socket_close);
}

function page_redirect(href) {
  cut_off_socket();
  location.href = href;
}

function usage_error(args_array, expected_args) {
  return new InvalidCommand(`Usage: /${args_array[0]} ${expected_args.join(" ")}`);
}

function get_args(args_array, expected_args) {
  const args_without_command_name = args_array.slice(1);
  if (args_without_command_name.length === expected_args.length) {
    return args_without_command_name;
  } else {
    throw usage_error(args_array, expected_args);
  }
}

function get_displayed(element) {
  return element.style.display !== "none";
}
// TODO: Update all code to use this.
function set_displayed(element, value) {
  element.style.display = value ? null : "none";
}

function on_document_click(event) {
  with_error_handling(function () {
    const archive_game_id = event.target.getAttribute("archive-game-id");
    if (archive_game_id) {
      const url = new URL(window.location);
      url.search = "";
      url.searchParams.set(SearchParams.archive_game_id, archive_game_id);
      window.history.pushState({}, "", url);
      wasm_client().view_archive_game_content(archive_game_id);
      update_events();
      close_menu();
    }
  });
}

function on_document_mouseover(event) {
  with_error_handling(function () {
    const archive_game_id = event.target.getAttribute("archive-game-id");
    document.body.classList.toggle("game-archive-preview", archive_game_id);
    if (archive_game_id) {
      wasm_client().view_archive_game_content(archive_game_id);
      update_events();
      const rect = menu_dialog.getBoundingClientRect();
      const x = ((event.clientX - rect.left) / rect.width) * 100.0;
      const y = ((event.clientY - rect.top) / rect.height) * 100.0;
      document.documentElement.style.setProperty("--menu-dialog-mouse-x", `${x}%`);
      document.documentElement.style.setProperty("--menu-dialog-mouse-y", `${y}%`);
    }
  });
}

function on_document_keydown(event) {
  with_error_handling(function () {
    if (menu_dialog.open) {
      if (event.key === "Escape") {
        pop_menu_page();
      }
    } else {
      let isPrintableKey = event.key?.length === 1; // https://stackoverflow.com/a/38802011/3092679
      if (isPrintableKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
        chat_input.focus();
      } else if (["ArrowDown", "ArrowUp"].includes(event.key)) {
        // Make sure log is not scrolled by arrow keys: we are scrolling it
        // programmatically to make sure the current turn is visible.
        event.preventDefault();
        wasm_client().on_vertical_arrow_key_down(
          event.key,
          event.ctrlKey,
          event.shiftKey,
          event.altKey
        );
        update();
      }
    }
  });
}

function on_document_paste(event) {
  if (!menu_dialog.open) {
    chat_input.focus();
  }
}

// Chat window is quite narrow, so the default behavior might scroll more than a page.
function chat_wheel_scroll(event) {
  event.preventDefault();
  // TODO: Find a way to make the scroll smooth. Note that simply using `behavior: "smooth"` is bad.
  // It works well for exactly one turn of the wheel, but gets stuck it you keep rotating the wheel.
  this.scrollBy({ top: event.deltaY * 0.5, behavior: "instant" });
}

function update_chat_input() {
  chat_send_button.classList.toggle("display-none", chat_input.value === "");
}

function execute_chat_input() {
  const input = String(chat_input.value);
  if (input.startsWith("<")) {
    chat_input.value = "<";
  } else if (input.startsWith(">")) {
    chat_input.value = ">";
  } else {
    chat_input.value = "";
  }
  chat_input.focus();
  update_chat_input();
  execute_input(input);
}

function on_chat_input_keydown(event) {
  if (!event.repeat && event.key == "Enter") {
    execute_chat_input();
  } else if (!event.repeat && event.key == "Escape") {
    // Remove focus thus hiding the chat reference tooltip.
    chat_input.blur();
  } else if (["<", ">", "/"].includes(chat_input.value) && [">", "<", "/"].includes(event.key)) {
    chat_input.value = "";
  } else if (["<", ">", "/", ""].includes(chat_input.value) && event.key === " ") {
    chat_input.value = "";
    event.preventDefault();
  }
}

function execute_input(input) {
  with_error_handling(function () {
    wasm_client().clear_ephemeral_chat_items();
    // TODO: Move all command handling to WASM.
    let known_command = false;
    if (input.startsWith("/")) {
      known_command = true;
      const args = input.slice(1).split(/\s+/);
      switch (args[0]) {
        case "h":
        case "help":
          get_args(args, []);
          show_chat_reference_dialog();
          break;
        case "tooltip":
          get_args(args, []);
          toggle_chat_reference_tooltip();
          break;
        case "sound": {
          const expected_args = ["0:1:2:3"];
          const [value] = get_args(args, expected_args);
          let volume = parseInt(value);
          if (isNaN(volume) || volume < 0 || volume > max_volume) {
            throw usage_error(args, expected_args);
          }
          set_volume(volume);
          wasm_client().show_command_result(`Applied: sound volume ${volume}.`);
          break;
        }
        case "resign":
          get_args(args, []);
          wasm_client().resign();
          break;
        case "ready":
          get_args(args, []);
          wasm_client().toggle_ready();
          break;
        case "rules":
          get_args(args, []);
          show_match_rules();
          break;
        case "save": {
          get_args(args, []);
          const content = wasm_client().get_game_bpgn();
          if (content) {
            download(content, "game.pgn");
          }
          break;
        }
        // Internal.
        case "perf": {
          get_args(args, []);
          const stats = wasm_client().meter_stats();
          console.log(stats);
          wasm_client().show_command_result(stats);
          break;
        }
        // Internal. For testing WebSocket re-connection.
        case "reconnect":
          socket.close();
          break;
        default:
          known_command = false;
      }
    }
    if (!known_command) {
      wasm_client().execute_input(input);
    }
    update();
  });
}

function update_events() {
  with_error_handling(function () {
    const timer = new Timer();
    process_outgoing_events();
    timer.meter(Meter.process_outgoing_events);
    process_notable_events();
    timer.meter(Meter.process_notable_events);
  });
}

function on_tick() {
  with_error_handling(function () {
    const timer = new Timer();
    wasm_client().refresh();
    timer.meter(Meter.refresh);
    process_outgoing_events();
    timer.meter(Meter.process_outgoing_events);
    wasm_client().update_clock();
    timer.meter(Meter.update_clock);
    process_notable_events();
    timer.meter(Meter.process_notable_events);
    update_lobby_countdown();
    update_connection_status();
  });
}

function update() {
  with_error_handling(function () {
    const timer = new Timer();
    wasm_client().refresh();
    timer.meter(Meter.refresh);
    process_outgoing_events();
    timer.meter(Meter.process_outgoing_events);
    wasm_client().update_state();
    timer.meter(Meter.update_state);
    process_notable_events();
    timer.meter(Meter.process_notable_events);
    update_drag_state();
    timer.meter(Meter.update_drag_state);
    update_lobby_countdown();
    update_connection_status();
    update_buttons();
  });
}

function process_outgoing_events() {
  if (socket.readyState !== WebSocket.OPEN) {
    // Try again later when the socket is open.
    return;
  }
  let event;
  while ((event = wasm_client().next_outgoing_event())) {
    // console.log(log_time(), "sending: ", event);
    socket.send(event);
  }
}

function process_notable_events() {
  let js_event;
  while ((js_event = wasm_client().next_notable_event())) {
    const js_event_type = js_event?.constructor?.name;
    if (js_event_type == "JsEventNoop") {
      // Noop, but other events might be coming.
    } else if (js_event_type == "JsEventSessionUpdated") {
      update_session();
    } else if (js_event_type == "JsEventMatchStarted") {
      const url = new URL(window.location);
      url.search = "";
      url.searchParams.set(SearchParams.match_id, js_event.match_id);
      window.history.pushState({}, "", url);
      push_menu_page(menu_lobby_page);
    } else if (js_event_type == "JsEventGameStarted") {
      close_menu();
    } else if (js_event_type == "JsEventGameOver") {
      play_audio(Sound[js_event.result]);
    } else if (js_event_type == "JsEventPlaySound") {
      play_audio(Sound[js_event.audio], js_event.pan);
    } else if (js_event_type == "JsEventArchiveGameLoaded") {
      update();
    } else if (js_event_type != null) {
      throw "Unexpected notable event: " + js_event_type;
    }
  }
}

function update_drag_state() {
  const drag_state = wasm_client().drag_state();
  switch (drag_state) {
    case "no":
      if (drag_element) {
        drag_element.remove();
        drag_element = null;
        drag_source_board_id = null;
      }
      wasm_client().reset_drag_highlights();
      break;
    case "yes":
      console.assert(drag_element != null);
      break;
    case "defunct":
      // Improvement potential: Better image (broken piece / add red cross).
      drag_element.setAttribute("opacity", 0.5);
      wasm_client().reset_drag_highlights();
      break;
    default:
      console.error(`Unknown drag_state: ${drag_state}`);
  }
}

function update_lobby_countdown() {
  const lobby_footer = document.getElementById("lobby-footer");
  const lobby_waiting = document.getElementById("lobby-waiting");
  const lobby_countdown_seconds = document.getElementById("lobby-countdown-seconds");
  const s = wasm_client().lobby_countdown_seconds_left();
  lobby_footer.classList.toggle("countdown", s != null);
  lobby_waiting.textContent = wasm_client().lobby_waiting_explanation();
  lobby_countdown_seconds.textContent = s;
}

function update_connection_status() {
  const FIGURE_SPACE = " "; // &numsp;
  const s = wasm_client().current_turnaround_time();
  const ms = Math.round(s * 1000);
  const ms_str = ms.toString().padStart(4, FIGURE_SPACE);
  if (s < 3.0) {
    connection_info.textContent = `Ping: ${ms_str} ms`;
    connection_info.classList.toggle("bad-connection", false);
  } else {
    // Set the content once to avoid breaking dots animation.
    if (!connection_info.classList.contains("bad-connection")) {
      connection_info.textContent = "Reconnecting ";
      connection_info.appendChild(make_animated_dots());
      connection_info.classList.toggle("bad-connection", true);
    }
    open_socket("irresponsive");
  }
}

function update_ready_button() {
  set_displayed(ready_button_caption, get_displayed(ready_button));
  const is_ready = wasm_client().is_ready();
  set_displayed(document.getElementById("ready-yes"), is_ready);
  set_displayed(document.getElementById("ready-no"), !is_ready);
  ready_button_caption.innerText = is_ready ? "Go!" : "Go?";
}

function update_toggle_faction_button() {
  const faction = wasm_client().my_desired_faction();
  const is_observer = faction == "observer";
  set_displayed(document.getElementById("toggle-faction-observer"), is_observer);
  set_displayed(document.getElementById("toggle-faction-player"), !is_observer);
  toggle_faction_button.title = is_observer
    ? "You are observing. Click to participate starting from the next game (note that you will still seat out sometimes if there are more that four players)"
    : "You are playing. Click to observe starting from the next game";
}

function update_shared_wayback_button() {
  const shared_wayback = wasm_client().shared_wayback_enabled();
  set_displayed(document.getElementById("wayback-together"), shared_wayback);
  set_displayed(document.getElementById("wayback-alone"), !shared_wayback);
  shared_wayback_button.title = shared_wayback
    ? "Viewing the same moment together with other players when navigating game log via mouse or ↑↓ arrow keys. Click to toggle"
    : "Viewing game history independently from other players when navigating game log via mouse or ↑↓ arrow keys. Click to toggle";
}

function update_buttons() {
  const observer_status = wasm_client().observer_status();
  const game_status = wasm_client().game_status();
  // TODO: Allow leaving active matches.
  switch (game_status) {
    case "active":
      set_displayed(leave_match_button, false);
      set_displayed(resign_button, observer_status == "no");
      set_displayed(ready_button, false);
      set_displayed(toggle_faction_button, true);
      set_displayed(export_button, false);
      set_displayed(shared_wayback_button, false);
      break;
    case "over":
      set_displayed(leave_match_button, false);
      set_displayed(resign_button, false);
      set_displayed(ready_button, observer_status != "permanently");
      set_displayed(toggle_faction_button, true);
      // TODO: Add "get game permalink" button.
      set_displayed(export_button, false);
      set_displayed(shared_wayback_button, true);
      break;
    case "archive":
      set_displayed(leave_match_button, true);
      set_displayed(resign_button, false);
      set_displayed(ready_button, false);
      set_displayed(toggle_faction_button, false);
      set_displayed(export_button, true);
      set_displayed(shared_wayback_button, false); // TODO: allow watching archive games together and set to `true`
      break;
    case "none":
      set_displayed(leave_match_button, false);
      set_displayed(resign_button, false);
      set_displayed(ready_button, false);
      set_displayed(toggle_faction_button, false);
      set_displayed(export_button, false);
      set_displayed(shared_wayback_button, false);
      break;
    default:
      throw new Error(`Unknown game status: ${game_status}`);
  }
  update_ready_button();
  update_toggle_faction_button();
  update_shared_wayback_button();
}

function leave_match() {
  with_error_handling(function () {
    if (wasm_client().game_status() == "archive") {
      const url = new URL(window.location);
      url.search = "";
      window.history.pushState({}, "", url);
      open_menu();
      view_archive_game_list();
    }
  });
}

async function request_resign() {
  const ret = await text_dialog("Are you sure you want to resign?", [
    new MyButton("Keep playing", MyButton.HIDE),
    new MyButton("🏳 Resign", MyButton.DO),
  ]);
  if (ret == MyButton.DO) {
    execute_input("/resign");
  }
}

async function toggle_faction_ingame() {
  const current_faction = wasm_client().my_desired_faction();
  if (current_faction == "none") {
    // unavailable
  } else if (current_faction == "observer") {
    if (wasm_client().fixed_teams()) {
      // TODO: Popup menu instead of a dialog.
      // TODO: List team players. Team color is hidden from the UI.
      const ret = await text_dialog("Which team do you want to join?", [
        new MyButton("Cancel", MyButton.HIDE),
        new MyButton("Team Red", MyButton.OPTION_1),
        new MyButton("Team Blue", MyButton.OPTION_2),
      ]);
      if (ret == MyButton.OPTION_1) {
        wasm_client().change_faction_ingame("team_red");
      } else if (ret == MyButton.OPTION_2) {
        wasm_client().change_faction_ingame("team_blue");
      }
    } else {
      // Improvement potential. Allow joining as `Faction::Fixed` in `DynamicTeams` mode.
      wasm_client().change_faction_ingame("random");
    }
  } else {
    wasm_client().change_faction_ingame("observer");
  }
}

function server_websocket_address() {
  const search_params = new URLSearchParams(window.location.search);
  let address = search_params.get(SearchParams.server);
  if (address === "local" || (!address && window.location.hostname === "localhost")) {
    address = "ws://localhost:14361";
  }
  address ??= `${window.location.origin}/ws`;
  if (!address.includes("://")) {
    address = `wss://${address}`;
  }
  const url = new URL(address);
  if (url.protocol !== "ws:") {
    url.protocol = "wss:";
  }
  return url;
}

function set_up_drag_and_drop() {
  // Note. Need to process mouse and touch screens separately. Cannot use pointer events
  // (https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events) here: it seems impossible
  // to implement drag cancellation with a right-click, because pointer API does not report
  // nested mouse events.

  document.addEventListener("mousedown", pointer_down);
  document.addEventListener("mousemove", pointer_move);
  document.addEventListener("mouseup", pointer_stop);
  document.addEventListener("mouseleave", pointer_stop);

  document.addEventListener("touchstart", pointer_down);
  document.addEventListener("touchmove", pointer_move);
  document.addEventListener("touchend", pointer_stop);
  document.addEventListener("touchcancel", pointer_stop);

  for (const board_id of ["primary", "secondary"]) {
    // Note the difference: drag is cancelled while dragging, no matter the mouse position. Other
    // partial turn inputs and preturns are cancelled by right-click on the corresponding board.
    const svg = board_svg(board_id);
    svg.addEventListener("contextmenu", (event) => cancel_preturn(event, board_id));
    for (const player_id of ["top", "bottom"]) {
      const reserve = reserve_svg(board_id, player_id);
      reserve.addEventListener("contextmenu", (event) => cancel_preturn(event, board_id));
    }
  }
  document.addEventListener("contextmenu", cancel_drag);

  function distance(a, b) {
    return Math.sqrt((a.x - b.x) ** 2 + (a.y - b.y) ** 2);
  }

  function is_main_pointer(event) {
    return event.button == 0 || event.changedTouches?.length >= 1;
  }

  function pointer_position(event) {
    const src = event.changedTouches ? event.changedTouches[0] : event;
    return { x: src.clientX, y: src.clientY };
  }

  function position_relative_to_board(pos, board_svg) {
    const ctm = board_svg.getScreenCTM();
    return {
      x: (pos.x - ctm.e) / ctm.a,
      y: (pos.y - ctm.f) / ctm.d,
    };
  }

  function pointer_down(event) {
    pointer_down_position = pointer_position(event);
    pointer_down_element = event.target;
    pointer_down_is_main_pointer = is_main_pointer(event);
  }

  function pointer_move(event) {
    const pos = pointer_position(event);
    if (
      pointer_down_position &&
      distance(pointer_down_position, pos) > drag_start_threshold &&
      pointer_down_is_main_pointer
    ) {
      start_drag(pos, pointer_down_element);
      pointer_down_position = null;
      pointer_down_element = null;
      pointer_down_is_main_pointer = null;
    } else if (drag_element) {
      drag(pos);
    } else {
      board_hover(pos, event.target);
    }
  }

  function pointer_stop(event) {
    const pos = pointer_position(event);
    if (drag_element) {
      if (is_main_pointer(event)) {
        end_drag(pos);
      }
    } else if (pointer_down_position) {
      if (is_main_pointer(event)) {
        click(pointer_down_position, pointer_down_element);
        board_hover(pos, event.target);
      }
    }
    pointer_down_position = null;
    pointer_down_element = null;
    pointer_down_is_main_pointer = null;
  }

  function click(pos, element) {
    with_error_handling(function () {
      const promotion_target = element.getAttribute("data-promotion-target");
      if (promotion_target) {
        wasm_client().choose_promotion_upgrade(promotion_target);
        update();
      } else {
        const source = element.getAttribute("data-bughouse-location");
        if (source) {
          wasm_client().click_element(source);
          update();
        } else {
          const board_id = element.closest("[data-board-id]")?.getAttribute("data-board-id");
          if (board_id) {
            const coord = position_relative_to_board(pos, board_svg(board_id));
            wasm_client().click_board(board_id, coord.x, coord.y);
            update();
          }
        }
      }
    });
  }

  function board_hover(pos, element) {
    with_error_handling(function () {
      const board_id = element.closest("[data-board-id]")?.getAttribute("data-board-id");
      if (board_id) {
        const coord = position_relative_to_board(pos, board_svg(board_id));
        wasm_client().board_hover(board_id, coord.x, coord.y);
      }
    });
  }

  function start_drag(pos, element) {
    with_error_handling(function () {
      // Improvement potential. Highlight pieces outside of board area: add shadows separately
      //   and move them to the very back, behing boards.
      // Improvement potential: Choose the closest reserve piece rather then the one on top.
      // Note. For a mouse we can simple assume that drag_element is null here. For multi-touch
      //   screens however this is not always the case.
      if (!drag_element && element.classList.contains("draggable")) {
        document.getSelection().removeAllRanges();

        const source = element.getAttribute("data-bughouse-location");
        const board_idx = wasm_client().start_drag_piece(source);

        if (board_idx == "abort") {
          return;
        }

        drag_source_board_id = board_idx;
        drag_element = element;
        drag_element.classList.add("dragged");
        // Dissociate image from the board/reserve:
        drag_element.id = null;

        // Reparent: bring on top; (if reserve) remove shadow by extracting from reserve group.
        //
        // TODO: Fix: Reparenting breaks touch drag. According to
        //   https://stackoverflow.com/questions/33298828/touch-move-event-dont-fire-after-touch-start-target-is-removed
        // this should've helped:
        //   drag_element.addEventListener('touchmove', drag);
        // but it didn't work for me.
        drag_source_board().appendChild(drag_element);

        update();

        // Properly position reserve piece after re-parenting.
        drag(pos);
      }
    });
  }

  function drag(pos) {
    with_error_handling(function () {
      if (drag_element) {
        const coord = position_relative_to_board(pos, drag_source_board());
        wasm_client().drag_piece(drag_source_board_id, coord.x, coord.y);
        drag_element.setAttribute("x", coord.x - 0.5);
        drag_element.setAttribute("y", coord.y - 0.5);
      }
    });
  }

  function end_drag(pos) {
    with_error_handling(function () {
      console.assert(drag_element != null);
      const coord = position_relative_to_board(pos, drag_source_board());
      wasm_client().drag_piece_drop(drag_source_board_id, coord.x, coord.y);
      drag_element.remove();
      drag_element = null;
      drag_source_board_id = null;
      update();
    });
  }

  function cancel_preturn(event, board_id) {
    with_error_handling(function () {
      event.preventDefault();
      if (!drag_element) {
        wasm_client().cancel_preturn(board_id);
        update();
      }
    });
  }

  function cancel_drag(event) {
    with_error_handling(function () {
      if (drag_element) {
        event.preventDefault();
        wasm_client().abort_drag_piece();
        update();
      }
    });
  }
}

// TODO: Merge into event handlers from `set_up_drag_and_drop`.
function set_up_chalk_drawing() {
  let chalk_target = null;
  let ignore_next_cancellation = false;
  let ignore_next_context_menu = false;

  function is_draw_button(event) {
    return event.button == 2;
  }
  function is_cancel_button(event) {
    return event.button == 0;
  }

  function viewbox_mouse_position(node, event) {
    const ctm = node.getScreenCTM();
    return {
      x: (event.clientX - ctm.e) / ctm.a,
      y: (event.clientY - ctm.f) / ctm.d,
    };
  }

  function board_mouse_down(event) {
    with_error_handling(function () {
      if (drag_element) {
        // Do not draw while a turn is being made.
      } else if (!wasm_client().is_chalk_active() && is_draw_button(event)) {
        chalk_target = event.currentTarget;
        const coord = viewbox_mouse_position(chalk_target, event);
        wasm_client().chalk_down(chalk_target.id, coord.x, coord.y, event.ctrlKey);
      } else if (wasm_client().is_chalk_active() && is_cancel_button(event)) {
        chalk_target = null;
        ignore_next_cancellation = true;
        wasm_client().chalk_abort();
      }
    });
  }

  function mouse_move(event) {
    with_error_handling(function () {
      if (wasm_client().is_chalk_active()) {
        console.assert(chalk_target != null);
        const coord = viewbox_mouse_position(chalk_target, event);
        wasm_client().chalk_move(coord.x, coord.y);
      }
    });
  }

  function mouse_up(event) {
    with_error_handling(function () {
      if (wasm_client().is_chalk_active() && is_draw_button(event)) {
        console.assert(chalk_target != null);
        const coord = viewbox_mouse_position(chalk_target, event);
        wasm_client().chalk_up(coord.x, coord.y);
        chalk_target = null;
        ignore_next_context_menu = true;
      }
    });
  }

  function board_mouse_up(event) {
    with_error_handling(function () {
      if (is_cancel_button(event)) {
        if (ignore_next_cancellation) {
          ignore_next_cancellation = false;
          return;
        }
        if (event.shiftKey) {
          wasm_client().chalk_clear(event.currentTarget.id);
        } else {
          wasm_client().chalk_remove_last(event.currentTarget.id);
        }
      }
    });
  }

  function on_context_menu(event) {
    // Note. This relies on `contextmenu` arriving after `mouseup`. I'm not
    // sure if the order is specified anywhere, but it seems to be the case.
    if (ignore_next_context_menu) {
      event.preventDefault();
      ignore_next_context_menu = false;
    }
  }

  for (const board_id of ["primary", "secondary"]) {
    // Improvement potential. Support chalk on touch screens.
    const svg = board_svg(board_id);
    svg.addEventListener("mousedown", board_mouse_down);
    svg.addEventListener("mouseup", board_mouse_up);
  }
  document.addEventListener("mousemove", mouse_move);
  document.addEventListener("mouseup", mouse_up);
  document.addEventListener("contextmenu", on_context_menu);
}

function update_cookie_policy() {
  const is_analytics_ok = window.localStorage.getItem(Storage.cookies_accepted) == "all";
  const show_banner = window.localStorage.getItem(Storage.cookies_accepted) == null;
  gtag("consent", "update", {
    analytics_storage: is_analytics_ok ? "granted" : "denied",
  });
  cookie_banner.style.display = show_banner ? null : "None";
}

function on_accept_essential_cookies() {
  window.localStorage.setItem(Storage.cookies_accepted, "essential");
  update_cookie_policy();
}

function on_accept_all_cookies() {
  window.localStorage.setItem(Storage.cookies_accepted, "all");
  update_cookie_policy();
}

function set_up_menu_pointers() {
  function is_cycle_forward(event) {
    return event.button == 0 || event.changedTouches?.length >= 1;
  }
  function is_cycle_backward(event) {
    return event.button == 2;
  }

  function mouse_down(event) {
    with_error_handling(function () {
      const my_readiness = document.getElementById("my-readiness");
      const my_faction = document.getElementById("my-faction");
      if (my_readiness?.contains(event.target)) {
        if (is_cycle_forward(event) || is_cycle_backward(event)) {
          wasm_client().toggle_ready();
        }
      } else if (my_faction?.contains(event.target)) {
        if (is_cycle_forward(event)) {
          wasm_client().next_faction();
          update();
        } else if (is_cycle_backward(event)) {
          wasm_client().previous_faction();
          update();
        }
      }
    });
  }

  function context_menu(event) {
    const lobby_participants = document.getElementById("lobby-participants");
    if (lobby_participants?.contains(event.target)) {
      event.preventDefault();
    }
  }

  menu_dialog.addEventListener("mousedown", mouse_down);
  menu_dialog.addEventListener("contextmenu", context_menu);
}

function set_up_log_navigation() {
  for (const board_id of ["primary", "secondary"]) {
    const area_node = document.getElementById(`turn-log-scroll-area-${board_id}`);
    area_node.addEventListener("click", (event) => {
      with_error_handling(function () {
        const turn_node = event.target.closest("[data-turn-index]");
        const turn_index = turn_node?.getAttribute("data-turn-index");
        wasm_client().wayback_to_turn(turn_index);
        update();
      });
    });
  }
}

function find_player_name_input(page) {
  for (const input of page.getElementsByTagName("input")) {
    if (input.name == "player_name" || input.name == "user_name") {
      return input;
    }
  }
  return null;
}

function on_hide_menu_page(page) {
  const player_name_input = find_player_name_input(page);
  if (player_name_input) {
    window.localStorage.setItem(Storage.player_name, player_name_input.value);
  }
  for (const input of page.getElementsByTagName("input")) {
    if (input.type == "password") {
      input.value = "";
    }
  }
}

function hide_menu_pages(execute_on_hide = true) {
  for (const page of menu_pages) {
    if (page.style.display !== "none") {
      if (execute_on_hide) {
        on_hide_menu_page(page);
      }
      page.style.display = "none";
    }
  }
}

function reset_menu() {
  menu_page_stack.length = 0;
  hide_menu_pages();

  const search_params = new URLSearchParams(window.location.search);
  const match_id = search_params.get(SearchParams.match_id);
  if (match_id) {
    jc_match_id.value = match_id;
    push_menu_page(menu_join_match_page);
  } else {
    menu_start_page.style.display = "block";
  }
}

function current_menu_page() {
  return menu_page_stack.at(-1) || menu_start_page;
}

function menu_page_auto_focus() {
  const page = current_menu_page();
  for (const input of page.getElementsByTagName("input")) {
    if (!input.disabled != "none" && !input.value) {
      input.focus();
      return;
    }
  }
  if (page === menu_join_match_page) {
    // Make sure "Enter" joines the match when joining via link.
    jc_confirm_button.focus();
    return;
  }
  document.activeElement.blur();
}

function push_menu_page(page) {
  menu_page_stack.push(page);
  hide_menu_pages();
  page.style.display = "block";

  const player_name_input = find_player_name_input(page);
  if (player_name_input) {
    player_name_input.value = window.localStorage.getItem(Storage.player_name);
  }
  menu_page_auto_focus();
}

function pop_menu_page() {
  menu_page_stack.pop();
  const page = current_menu_page();
  hide_menu_pages();
  page.style.display = "block";
}

function close_menu() {
  hide_menu_pages(); // hide the pages to execute "on hide" handlers
  menu_dialog.close();
  menu_backdrop.style.display = "None";
  for (const element of page_element.getElementsByTagName("*")) {
    if ("disabled" in element) {
      element.disabled = false;
    }
  }
}

function open_menu() {
  reset_menu();
  // The "`show` + manual backdrop + disable the rest of the page" combo emulates `showModal`.
  // We cannot use `showModal` because of the cookie banner.
  menu_dialog.show();
  menu_backdrop.style.display = null;
  for (const element of page_element.getElementsByTagName("*")) {
    if ("disabled" in element) {
      element.disabled = true;
    }
  }
}

function auto_open_menu() {
  with_error_handling(function () {
    const search_params = new URLSearchParams(window.location.search);
    const archive_game_id = search_params.get(SearchParams.archive_game_id);
    if (archive_game_id) {
      close_menu();
      wasm_client().view_archive_game_content(archive_game_id);
      update_events();
    } else {
      open_menu();
    }
  });
}

function init_menu() {
  hide_menu_pages(false);
}

// Shows a dialog with a message and buttons.
// If there is a button with `MyButton.HIDE` action, then `Escape` will close the dialog and
// also return `MyButton.HIDE`. If there are no buttons with `MyButton.HIDE` action, then
// `Escape` key will be ignored.
function html_dialog(dialog_classes, body, buttons) {
  if (fatal_error_shown) {
    return;
  }
  return new Promise((resolve) => {
    const dialog = document.createElement("dialog");
    if (dialog_classes.length > 0) {
      dialog.classList.add(dialog_classes);
    }
    document.body.appendChild(dialog);
    const button_box = document.createElement("div");
    button_box.className = "simple-dialog-button-box";
    let can_hide = false;
    for (const button of buttons || []) {
      const button_node = document.createElement("button");
      button_node.type = "button";
      button_node.role = "button";
      button_node.textContent = button.label;
      const action = button.action;
      can_hide ||= action == MyButton.HIDE;
      button_node.addEventListener("click", (event) => {
        dialog.close();
        resolve(action);
      });
      button_box.appendChild(button_node);
    }
    dialog.addEventListener("cancel", (event) => {
      // TODO: Prevent `Escape` key from reaching the menu in addition to this dialog.
      if (can_hide) {
        resolve(MyButton.HIDE);
      } else {
        event.preventDefault();
      }
    });
    dialog.appendChild(body);
    dialog.appendChild(button_box);
    // Delay `showModal`. If it's called directly, then the dialog gets `Enter` key press if it
    // was the trigger, e.g. if the dialog displays an error processing command line instruction.
    setTimeout(() => dialog.showModal());
  });
}

function text_dialog(message, buttons) {
  const message_node = document.createElement("div");
  message_node.className = "simple-dialog-message";
  message_node.innerText = message;
  return html_dialog([], message_node, buttons);
}

function info_dialog(message) {
  return text_dialog(message, [new MyButton("Ok", MyButton.HIDE)]);
}

function ignorable_error_dialog(message) {
  return info_dialog(message);
}

function fatal_error_dialog(message) {
  text_dialog(message);
  fatal_error_shown = true;
}

function make_svg_image(symbol_id, size) {
  const SVG_NS = "http://www.w3.org/2000/svg";
  const symbol = document.createElementNS(SVG_NS, "symbol");
  symbol.id = symbol_id;
  const image = document.createElementNS(SVG_NS, "image");
  image.id = `${symbol_id}-image`;
  image.setAttribute("width", size);
  image.setAttribute("height", size);
  symbol.appendChild(image);
  svg_defs.appendChild(symbol);
}

async function load_image(filepath, target_id) {
  const reader = new FileReader();
  reader.addEventListener(
    "load",
    () => {
      const image = document.getElementById(target_id);
      image.setAttribute("href", reader.result);
      loading_tracker.resource_loaded();
    },
    false
  );
  reader.addEventListener("error", () => {
    console.error(`Cannot load image ${filepath}`);
  });
  const response = await fetch(filepath);
  const blob = await response.blob();
  reader.readAsDataURL(blob);
}

function load_svg_images(image_records) {
  for (const record of image_records) {
    const symbol_id = record.symbol;
    const size = record.size || 1;
    const image_id = `${symbol_id}-image`;
    make_svg_image(symbol_id, size);
    load_image(record.path, image_id, size);
    loading_tracker.resource_required();
  }
}

async function load_sound(filepath, key) {
  const response = await fetch(filepath);
  const array_buffer = await response.arrayBuffer();
  const audio_buffer = await audio_context.decodeAudioData(array_buffer);
  Sound[key] = audio_buffer;
  loading_tracker.resource_loaded();
}

function load_sounds(sound_map) {
  ensure_audio_context_running();
  const ret = {};
  for (const [key, filepath] of Object.entries(sound_map)) {
    ret[key] = null;
    load_sound(filepath, key);
    loading_tracker.resource_required();
  }
  return ret;
}

function go_to_suburl(event) {
  const suburl = event.target.getAttribute("data-suburl");
  const url = new URL(window.location);
  url.pathname = suburl;
  window.open(url, "_blank")?.focus();
}

function update_session() {
  with_error_handling(function () {
    reset_menu();
    const session = wasm_client().session();
    const using_password_auth = session.registration_method == "Password";
    let is_guest = null;
    let user_name = null;
    switch (session.status) {
      case "unknown":
      case "google_oauth_registering":
      case "lichess_oauth_registering": {
        is_registered_user = false;
        is_guest = false;
        user_name = null;
        break;
      }
      case "logged_out": {
        is_registered_user = false;
        is_guest = true;
        user_name = "Guest";
        break;
      }
      case "logged_in": {
        is_registered_user = true;
        is_guest = false;
        user_name = session.user_name;
        break;
      }
    }
    const session_info_loaded = is_registered_user || is_guest;
    const ready_to_play = session_info_loaded && wasm_client().got_server_welcome();
    registered_user_bar.style.display = is_registered_user ? null : "None";
    guest_user_bar.style.display = !is_registered_user ? null : "None";
    guest_user_tooltip.style.display = is_guest ? null : "None";
    if (ready_to_play) {
      if (is_registered_user) {
        create_rated_match_button.disabled = false;
        game_archive_button.disabled = false;
        set_tooltip(create_rated_match_button, null);
        set_tooltip(game_archive_button, null);
      } else {
        create_rated_match_button.disabled = true;
        game_archive_button.disabled = true;
        set_tooltip(create_rated_match_button, "Please sign in to play rated games");
        set_tooltip(game_archive_button, "Please sign in to view your game history");
      }
      create_unrated_match_button.disabled = false;
      join_match_button.disabled = false;
      authorization_button.disabled = false;
      jc_confirm_button.disabled = false;
    } else {
      create_rated_match_button.disabled = true;
      create_unrated_match_button.disabled = true;
      join_match_button.disabled = true;
      authorization_button.disabled = true;
      jc_confirm_button.disabled = true;
      game_archive_button.disabled = true;
    }
    for (const node of document.querySelectorAll(".logged-in-as-account")) {
      node.classList.toggle("account-user", is_registered_user);
      node.classList.toggle("account-guest", is_guest);
      if (user_name === null) {
        node.textContent = "";
        node.appendChild(make_animated_dots());
      } else {
        node.textContent = user_name;
      }
    }
    change_account_email.value = session.email;
    for (const node of document.querySelectorAll(".logged-in-as-email")) {
      node.textContent = session.email || "—";
    }
    for (const node of document.querySelectorAll(".logged-in-as-lichess-user-id")) {
      node.textContent = session.lichess_user_id || "—";
    }
    for (const node of document.querySelectorAll(".logged-in-with-password")) {
      node.style.display = using_password_auth ? null : "None";
      node.disabled = !using_password_auth;
    }
    for (const node of document.querySelectorAll(".guest-player-name")) {
      node.style.display = is_guest ? null : "None";
      node.disabled = !is_guest;
    }
    if (session.status == "google_oauth_registering") {
      push_menu_page(menu_signup_with_google_page);
    }
    if (session.status == "lichess_oauth_registering") {
      push_menu_page(menu_signup_with_lichess_page);
    }
    menu_page_auto_focus();
  });
}

// Encodes `FormData` as application/x-www-form-urlencoded (the default is multipart/form-data).
function as_x_www_form_urlencoded(form_data) {
  return new URLSearchParams(form_data);
}

async function process_authentification_request(request, success_message) {
  // TODO: Loading animation.
  let response;
  try {
    response = await fetch(request);
  } catch (e) {
    await ignorable_error_dialog(`Network error: ${e}`);
    return;
  }
  if (response.ok) {
    if (success_message) {
      await info_dialog(success_message);
    }
    // Emulate a navigation to indicate that the form has been submitted to password managers:
    // https://www.chromium.org/developers/design-documents/create-amazing-password-forms/#make-sure-form-submission-is-clear
    window.history.replaceState({}, "");
    // Now wait for `UpdateSession` socket event...
  } else {
    await ignorable_error_dialog(await response.text());
  }
}

async function sign_up(event) {
  const data = new FormData(event.target);
  if (data.get("confirm_password") != data.get("password")) {
    ignorable_error_dialog("Passwords do not match!");
    return;
  }
  data.delete("confirm_password");
  if (!data.get("email")) {
    const ret = await text_dialog(
      "Without an email you will not be able to restore your account " +
        "if you forget your password. Continue?",
      [new MyButton("Go back", MyButton.HIDE), new MyButton("Proceed without email", MyButton.DO)]
    );
    if (ret != MyButton.DO) {
      return;
    }
  }
  process_authentification_request(
    new Request("auth/signup", {
      method: "POST",
      body: as_x_www_form_urlencoded(data),
    })
  );
}

function sign_up_with_google(event) {
  const data = new FormData(event.target);
  process_authentification_request(
    new Request("auth/finish-signup-with-google", {
      method: "POST",
      body: as_x_www_form_urlencoded(data),
    })
  );
}

function sign_up_with_lichess(event) {
  const data = new FormData(event.target);
  process_authentification_request(
    new Request("auth/finish-signup-with-lichess", {
      method: "POST",
      body: as_x_www_form_urlencoded(data),
    })
  );
}


async function sign_with_google(event) {
  page_redirect("/auth/sign-with-google");
}

async function sign_with_lichess(event) {
  page_redirect("/auth/sign-with-lichess");
}

async function log_in(event) {
  const data = new FormData(event.target);
  process_authentification_request(
    new Request("auth/login", {
      method: "POST",
      body: as_x_www_form_urlencoded(data),
    })
  );
}

function log_out(event) {
  process_authentification_request(
    new Request("auth/logout", {
      method: "POST",
    })
  );
}

async function change_account(event) {
  const data = new FormData(event.target);
  if (data.get("confirm_new_password") != data.get("new_password")) {
    ignorable_error_dialog("Passwords do not match!");
    return;
  }
  data.delete("confirm_new_password");
  process_authentification_request(
    new Request("auth/change-account", {
      method: "POST",
      body: as_x_www_form_urlencoded(data),
    }),
    "Account changes applied."
  );
}

async function delete_account(event) {
  const data = new FormData(event.target);
  process_authentification_request(
    new Request("auth/delete-account", {
      method: "POST",
      body: as_x_www_form_urlencoded(data),
    }),
    "Account deleted."
  );
}

function show_create_match_page() {
  with_error_handling(function () {
    wasm_client().init_new_match_rules_body();
  });
  let rules_variants = document.getElementById("cc-rule-variants");
  for (const node of rules_variants.querySelectorAll("button")) {
    node.addEventListener("click", rule_variant_button_clicked);
  }
  push_menu_page(menu_create_match_page);
}

function create_match_as_guest(event) {
  with_error_handling(function () {
    const player_name = document.getElementById("ccn-player-name").value;
    wasm_client().set_guest_player_name(player_name);
    show_create_match_page();
  });
}

function on_create_match_request(event, rated) {
  const cc_rating = document.getElementById("cc-rating");
  const cc_confirm_button = document.getElementById("cc-confirm-button");
  cc_rating.value = rated ? "rated" : "unrated";
  cc_confirm_button.innerText = rated ? "Create rated match!" : "Create unrated match!";
  if (is_registered_user) {
    show_create_match_page();
  } else {
    push_menu_page(menu_create_match_name_page);
  }
}

function on_join_match_submenu(event) {
  push_menu_page(menu_join_match_page);
}

export function rule_variant_button_clicked(event) {
  const node = event.currentTarget;
  const next_state = node.getAttribute("data-next-state");
  const next_state_node = document.getElementById(next_state);
  node.classList.add("display-none");
  next_state_node.classList.remove("display-none");
  with_error_handling(function () {
    wasm.update_new_match_rules_body();
  });
}

function on_create_match_confirm(event) {
  with_error_handling(function () {
    wasm_client().new_match();
    update();
  });
}

function on_join_match_confirm(event) {
  with_error_handling(function () {
    const data = new FormData(event.target);
    wasm_client().set_guest_player_name(data.get("player_name"));
    wasm_client().join(data.get("match_id").toUpperCase());
    update();
  });
}

function leave_match_lobby() {
  with_error_handling(function () {
    wasm_client().leave_match();
    update();
    const url = new URL(window.location);
    url.searchParams.delete(SearchParams.match_id);
    window.history.pushState({}, "", url);
    reset_menu();
  });
}

function view_archive_game_list() {
  with_error_handling(function () {
    wasm_client().view_archive_game_list();
    update_events();
    push_menu_page(menu_game_archive_game);
  });
}

function show_match_rules() {
  with_error_handling(function () {
    const rules_node = document.createElement("div");
    rules_node.className = "match-rules-body";
    rules_node.appendChild(wasm_client().readonly_rules_body());
    html_dialog(["overflow-visible"], rules_node, [new MyButton("Ok", MyButton.HIDE)]);
  });
}

function show_chat_reference_dialog() {
  const reference_node = document.getElementById("chat-reference-dialog-body");
  reference_node.classList.remove("display-none");
  html_dialog([], reference_node, [new MyButton("Ok", MyButton.HIDE)]);
}

function toggle_chat_reference_tooltip() {
  const old_value = window.localStorage.getItem(Storage.chat_reference_tooltip) || "show";
  const new_value = old_value === "show" ? "hide" : "show";
  set_show_chat_reference_tooltip(new_value);
}
function set_show_chat_reference_tooltip(value) {
  with_error_handling(function () {
    window.localStorage.setItem(Storage.chat_reference_tooltip, value);
    update_chat_reference_tooltip();
    if (value === "hide") {
      wasm_client().show_command_result("Type /tooltip to show the tooltip again.");
      update();
    }
  });
}

function update_chat_reference_tooltip() {
  // TODO: Don't hide while clicking the send button.
  const enabled = window.localStorage.getItem(Storage.chat_reference_tooltip) !== "hide";
  const input_focused = document.activeElement === chat_input;
  const show = enabled && input_focused;
  if (show) {
    if (update_chat_reference_tooltip.hide_timeout_id !== undefined) {
      clearTimeout(update_chat_reference_tooltip.hide_timeout_id);
      update_chat_reference_tooltip.hide_timeout_id = undefined;
    }
    chat_reference_tooltip_container.classList.remove("display-none");
    chat_reference_tooltip_container.classList.remove("fading");
  } else {
    // Besides being so marvelously beautiful, the fading animation plays an important role:
    // it allows to click on "Hide" before the tooltip disappears.
    chat_reference_tooltip_container.classList.add("fading");
    update_chat_reference_tooltip.hide_timeout_id = setTimeout(
      () => chat_reference_tooltip_container.classList.add("display-none"),
      200
    );
  }
}

function toggle_shared_wayback() {
  with_error_handling(function () {
    wasm_client().toggle_shared_wayback();
    update();
  });
}

function set_volume(volume) {
  // TODO: Save settings to a local storage.
  audio_volume = volume;
  gain_node.gain.value = volume_to_js[volume];
  if (volume == 0) {
    document.getElementById("volume-mute").style.display = null;
    for (let v = 1; v <= max_volume; ++v) {
      document.getElementById(`volume-${v}`).style.display = "none";
    }
  } else {
    document.getElementById("volume-mute").style.display = "none";
    for (let v = 1; v <= max_volume; ++v) {
      document.getElementById(`volume-${v}`).style.display = v > volume ? "none" : null;
    }
  }
}

function next_volume() {
  set_volume((audio_volume + 1) % (max_volume + 1));
  play_audio(Sound.clack);
}

function ensure_audio_context_running() {
  // Need to activate the context, because we create it before the first user interaction.
  audio_context.resume();
}

function play_audio(audio_buffer, pan) {
  ensure_audio_context_running();
  pan = pan || 0;
  if (audio_queue.length >= audio_max_queue_size) {
    return;
  }
  audio_queue.push({ audio_buffer, pan });
  const now = performance.now();
  const audio_next_avaiable = audio_last_played + audio_min_interval_ms;
  if (audio_queue.length > 1) {
    // play_audio_delayed already scheduled
  } else if (now < audio_next_avaiable) {
    setTimeout(play_audio_delayed, audio_next_avaiable - now);
  } else {
    play_audio_impl();
  }
}

function play_audio_delayed() {
  play_audio_impl();
  if (audio_queue.length > 0) {
    setTimeout(play_audio_delayed, audio_min_interval_ms);
  }
}

function play_audio_impl() {
  console.assert(audio_queue.length > 0);
  const { audio_buffer, pan } = audio_queue.shift();
  if (audio_volume > 0) {
    if (!pan_nodes.has(pan)) {
      const panner_node = audio_context.createStereoPanner();
      panner_node.pan.value = pan;
      panner_node.connect(gain_node);
      pan_nodes.set(pan, panner_node);
    }
    const source = audio_context.createBufferSource();
    source.buffer = audio_buffer;
    source.connect(pan_nodes.get(pan));
    source.start();
  }
  audio_last_played = performance.now();
}

function download(text, filename) {
  var element = document.createElement("a");
  element.setAttribute("href", "data:text/plain;charset=utf-8," + encodeURIComponent(text));
  element.setAttribute("download", filename);
  element.style.display = "none";
  document.body.appendChild(element);
  element.click();
  document.body.removeChild(element);
}

// Must have corresponding tooltip class (`tooltip-right` or `tooltip-below`).
function set_tooltip(node, text) {
  for (const tooltip_node of node.querySelectorAll(".tooltip-text")) {
    tooltip_node.remove();
  }
  if (text !== null) {
    const tooltip_node = document.createElement("div");
    tooltip_node.className = "tooltip-text";
    const p = document.createElement("p");
    p.innerText = text;
    tooltip_node.appendChild(p);
    node.appendChild(tooltip_node);
  }
}

// TODO: Dedup against `index.html`.
function make_animated_dots() {
  const parent = document.createElement("span");
  for (let i = 0; i < 3; ++i) {
    const dot = document.createElement("span");
    dot.className = "dot";
    dot.innerText = ".";
    parent.appendChild(dot);
  }
  return parent;
}
