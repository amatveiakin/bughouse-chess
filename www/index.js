// TODO: Remove logging (or at least don't log heartbeat events).
// TODO: Check if ==/!= have to be replaced with ===/!== and other JS weirdness.
// TODO: Figure out if it's possible to enable strict mode with webpack.

import './main.css';
import * as wasm from 'bughouse-chess';

import transparent from '../assets/transparent.png';

import white_pawn from '../assets/pieces/white-pawn.png';
import white_knight from '../assets/pieces/white-knight.png';
import white_bishop from '../assets/pieces/white-bishop.png';
import white_rook from '../assets/pieces/white-rook.png';
import white_queen from '../assets/pieces/white-queen.png';
import white_cardinal from '../assets/pieces/white-cardinal.png';
import white_empress from '../assets/pieces/white-empress.png';
import white_amazon from '../assets/pieces/white-amazon.png';
import white_king from '../assets/pieces/white-king.png';
import white_king_broken from '../assets/pieces/white-king-broken.png';
import black_pawn from '../assets/pieces/black-pawn.png';
import black_knight from '../assets/pieces/black-knight.png';
import black_bishop from '../assets/pieces/black-bishop.png';
import black_rook from '../assets/pieces/black-rook.png';
import black_queen from '../assets/pieces/black-queen.png';
import black_cardinal from '../assets/pieces/black-cardinal.png';
import black_empress from '../assets/pieces/black-empress.png';
import black_amazon from '../assets/pieces/black-amazon.png';
import black_king from '../assets/pieces/black-king.png';
import black_king_broken from '../assets/pieces/black-king-broken.png';
import duck from '../assets/pieces/duck.png';

import fog_1 from '../assets/fog-of-war/fog-1.png';
import fog_2 from '../assets/fog-of-war/fog-2.png';
import fog_3 from '../assets/fog-of-war/fog-3.png';

import clack_sound from '../assets/sounds/clack.ogg';
import turn_sound from '../assets/sounds/turn.ogg';
import reserve_restocked_sound from '../assets/sounds/reserve-restocked.ogg';
import piece_stolen_sound from '../assets/sounds/piece-stolen.ogg';
import low_time_sound from '../assets/sounds/low-time.ogg';
import victory_sound from '../assets/sounds/victory.ogg';
import defeat_sound from '../assets/sounds/defeat.ogg';
import draw_sound from '../assets/sounds/draw.ogg';


class WasmClientDoesNotExist {}
class WasmClientPanicked {}
class InvalidCommand { constructor(msg) { this.msg = msg; } }

class Timer {
    constructor() { this.t0 = performance.now(); }
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
    static HIDE = Symbol();  // `Escape` button will hide the dialog iff `HIDE` button exists
    static DO = Symbol();
    constructor(label, action) {
        this.label = label;
        this.action = action;
    }
}

function log_time() {
    if (typeof log_time.start == 'undefined') {
        log_time.start = performance.now();
    }
    const sec = (performance.now() - log_time.start) / 1000.0;
    return `[t=${sec.toFixed(2)}]`;
}
log_time();  // start the counter

// Improvement potential. Similarly group other global variables.
const Storage = {
    cookies_accepted: 'cookies-accepted',  // values: null, "essential", "all"
    player_name: 'player-name',
};

const SearchParams = {
    match_id: 'match-id',
    server: 'server',
};

const page_element = document.getElementById('page');
const chat_input = document.getElementById('chat-input');
const connection_info = document.getElementById('connection-info');

const menu_backdrop = document.getElementById('menu-backdrop');
const menu_dialog = document.getElementById('menu-dialog');
const menu_start_page = document.getElementById('menu-start-page');
const menu_authorization_page = document.getElementById('menu-authorization-page');
const menu_login_page = document.getElementById('menu-login-page');
const menu_signup_page = document.getElementById('menu-signup-page');
const menu_signup_with_google_page = document.getElementById('menu-signup-with-google-page');
const menu_view_account_page = document.getElementById('menu-view-account-page');
const menu_change_account_page = document.getElementById('menu-change-account-page');
const menu_delete_account_page = document.getElementById('menu-delete-account-page');
const menu_create_match_name_page = document.getElementById('menu-create-match-name-page');
const menu_create_match_page = document.getElementById('menu-create-match-page');
const menu_join_match_page = document.getElementById('menu-join-match-page');
const menu_lobby_page = document.getElementById('menu-lobby-page');
const menu_pages = document.getElementsByClassName('menu-page');

const cookie_banner = document.getElementById('cookie-banner');
const accept_essential_cookies_button = document.getElementById('accept-essential-cookies-button');
const accept_all_cookies_button = document.getElementById('accept-all-cookies-button');

const registered_user_bar = document.getElementById('registered-user-bar');
const view_account_button = document.getElementById('view-account-button');
const guest_user_bar = document.getElementById('guest-user-bar');
const guest_user_tooltip = document.getElementById('guest-user-tooltip');
const authorization_button = document.getElementById('authorization-button');
const log_out_button = document.getElementById('log-out-button');
const sign_with_google_button = document.getElementById('sign-with-google-button');
const begin_login_button = document.getElementById('begin-login-button');
const begin_signup_button = document.getElementById('begin-signup-button');
const view_account_change_button = document.getElementById('view-account-change-button');
const view_account_delete_button = document.getElementById('view-account-delete-button');
const change_account_email = document.getElementById('change-account-email');

const create_rated_match_button = document.getElementById('create-rated-match-button');
const create_unrated_match_button = document.getElementById('create-unrated-match-button');
const join_match_button = document.getElementById('join-match-button');
const jc_match_id = document.getElementById('jc-match-id');

const ready_button = document.getElementById('ready-button');
const resign_button = document.getElementById('resign-button');
const rules_button = document.getElementById('rules-button');
const export_button = document.getElementById('export-button');
const volume_button = document.getElementById('volume-button');

const svg_defs = document.getElementById('svg-defs');

function board_svg(board_id) { return document.getElementById(`board-${board_id}`); }

const menu_page_stack = [];

const loading_tracker = new class {
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
};

window.dataLayer = window.dataLayer || [];
function gtag() { window.dataLayer.push(arguments); }
update_cookie_policy();

const FOG_TILE_SIZE = 1.2;
load_svg_images([
    { path: transparent, symbol: 'transparent' },
    { path: white_pawn, symbol: 'white-pawn' },
    { path: white_knight, symbol: 'white-knight' },
    { path: white_bishop, symbol: 'white-bishop' },
    { path: white_rook, symbol: 'white-rook' },
    { path: white_queen, symbol: 'white-queen' },
    { path: white_cardinal, symbol: 'white-cardinal' },
    { path: white_empress, symbol: 'white-empress' },
    { path: white_amazon, symbol: 'white-amazon' },
    { path: white_king, symbol: 'white-king' },
    { path: white_king_broken, symbol: 'white-king-broken' },
    { path: black_pawn, symbol: 'black-pawn' },
    { path: black_knight, symbol: 'black-knight' },
    { path: black_bishop, symbol: 'black-bishop' },
    { path: black_rook, symbol: 'black-rook' },
    { path: black_queen, symbol: 'black-queen' },
    { path: black_cardinal, symbol: 'black-cardinal' },
    { path: black_empress, symbol: 'black-empress' },
    { path: black_amazon, symbol: 'black-amazon' },
    { path: black_king, symbol: 'black-king' },
    { path: black_king_broken, symbol: 'black-king-broken' },
    { path: duck, symbol: 'duck' },
    { path: fog_1, symbol: 'fog-1', size: FOG_TILE_SIZE },
    { path: fog_2, symbol: 'fog-2', size: FOG_TILE_SIZE },
    { path: fog_3, symbol: 'fog-3', size: FOG_TILE_SIZE },
]);

// Improvement potential. Establish priority on sounds; play more important sounds first
// in case of a clash.
const Sound = load_sounds({
    clack: clack_sound,  // similar to `turn` and roughly the same volume
    turn: turn_sound,
    reserve_restocked: reserve_restocked_sound,
    piece_stolen: piece_stolen_sound,
    low_time: low_time_sound,
    victory: victory_sound,
    defeat: defeat_sound,
    draw: draw_sound,
});

init_menu();

wasm.set_panic_hook();
wasm.init_page();
console.log('bughouse.pro client version:', wasm.git_version());

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

let audio_context = null;

// Parameters and data structures for the audio logic. Our goal is to make short and
// important sounds (like turn sound) as clear as possible when several events occur
// simultaneously. The main example is when you make a move and immediately get a
// premove back.
const audio_min_interval_ms = 70;
const audio_max_queue_size = 5;
const max_volume = 3;
const volume_to_js = {
    1: 0.25,
    2: 0.5,
    3: 1.0,
};
let audio_last_played = 0;
let audio_queue = [];
let audio_volume = 0;

let drag_source_board_id = null;
let drag_element = null;
function drag_source_board() { return board_svg(drag_source_board_id); }

const Meter = make_meters();

let is_registered_user = false;
update_session();

document.addEventListener('keydown', on_document_keydown);
document.addEventListener('paste', on_paste);

chat_input.addEventListener('keydown', on_command_keydown);

ready_button.addEventListener('click', () => execute_command('/ready'));
resign_button.addEventListener('click', request_resign);
rules_button.addEventListener('click', () => execute_command('/rules'));
export_button.addEventListener('click', () => execute_command('/save'));
volume_button.addEventListener('click', next_volume);

accept_essential_cookies_button.addEventListener('click', on_accept_essential_cookies);
accept_all_cookies_button.addEventListener('click', on_accept_all_cookies);
menu_dialog.addEventListener('cancel', (event) => event.preventDefault());
view_account_button.addEventListener('click', () => push_menu_page(menu_view_account_page));
authorization_button.addEventListener('click', () => push_menu_page(menu_authorization_page));
log_out_button.addEventListener('click', log_out);
sign_with_google_button.addEventListener('click',  sign_with_google);
begin_login_button.addEventListener('click',  () => push_menu_page(menu_login_page));
begin_signup_button.addEventListener('click',  () => push_menu_page(menu_signup_page));
view_account_change_button.addEventListener('click', () => push_menu_page(menu_change_account_page));
view_account_delete_button.addEventListener('click', () => push_menu_page(menu_delete_account_page));
menu_login_page.addEventListener('submit', log_in);
menu_signup_page.addEventListener('submit', sign_up);
menu_signup_with_google_page.addEventListener('submit', sign_up_with_google);
menu_change_account_page.addEventListener('submit', change_account);
menu_delete_account_page.addEventListener('submit', delete_account);
create_rated_match_button.addEventListener('click', (event) => on_create_match_request(event, true));
create_unrated_match_button.addEventListener('click', (event) => on_create_match_request(event, false));
join_match_button.addEventListener('click', on_join_match_submenu);
menu_create_match_name_page.addEventListener('submit', create_match_as_guest);
menu_create_match_page.addEventListener('submit', on_create_match_confirm);
menu_join_match_page.addEventListener('submit', on_join_match_confirm);

for (const button of document.querySelectorAll('.back-button')) {
    button.addEventListener('click', pop_menu_page);
}
for (const button of document.querySelectorAll('[data-suburl]')) {
    button.addEventListener('click', go_to_suburl);
}

// TODO: Make sounds louder and set volume to 2 by default.
set_volume(max_volume);

setInterval(on_tick, 50);


function with_error_handling(f) {
    // Note. Re-throw all unexpected errors to get a stacktrace.
    try {
        f();
    } catch (e) {
        if (e instanceof WasmClientDoesNotExist) {
            fatal_error_dialog('Internal error! WASM object does not exist.');
            throw e;
        } else if (e instanceof WasmClientPanicked) {
            // Error dialog should already be shown.
        } else if (e instanceof InvalidCommand) {
            wasm_client().add_command_error(e.msg);
        } else if (e?.constructor?.name == 'IgnorableError') {
            ignorable_error_dialog(e.message);
        } else if (e?.constructor?.name == 'KickedFromMatch') {
            ignorable_error_dialog(e.message);
            // Need to recreate the socket because server aborts the connection here.
            // If this turns out to be buggy, could do
            //   ignorable_error_dialog(e.message).then(() => location.reload());
            // instead.
            open_socket('kicked');
            open_menu();
            push_menu_page(menu_join_match_page);
        } else if (e?.constructor?.name == 'FatalError') {
            fatal_error_dialog(e.message);
        } else if (e?.constructor?.name == 'RustError') {
            ignorable_error_dialog(`Internal error: ${e.message}`);
            if (socket.readyState == WebSocket.OPEN) {
                socket.send(wasm.make_rust_error_event(e));
            }
            throw e;
        } else {
            const rust_panic = wasm.last_panic();
            if (rust_panic) {
                wasm_client_panicked = true;
                let report = '';
                if (socket.readyState == WebSocket.OPEN) {
                    socket.send(rust_panic);
                } else {
                    report = 'Please consider reporting the error to contact.bughousepro@gmail.com';
                }
                fatal_error_dialog(
                    'Internal error! This client is now dead 💀 ' +
                    'Only refreshing the page may help you. We are very sorry. ' +
                    report
                );
            } else {
                console.log(log_time(), 'Unknown error: ', e);
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
    with_error_handling(function() {
        console.log(log_time(), 'server: ', event.data);
        const update_needed = wasm_client().process_server_event(event.data);
        if (update_needed) {
            update();
        }
    });
}
function on_socket_open(event) {
    with_error_handling(function() {
        console.info(log_time(), 'WebSocket connection opened');
        consecutive_socket_connection_attempts = 0;
        wasm_client().hot_reconnect();
    });
}
function on_socket_close(event) {
    open_socket('closed');
}
function on_socker_error(event) {
    // TODO: Report socket errors.
    console.warn(log_time(), 'WebSocket error: ', event);
}

// Closes WebSocket and ignores all further messages and other events.
function cut_off_socket() {
    if (socket !== null) {
        socket.removeEventListener('message', on_socket_message);
        socket.removeEventListener('open', on_socket_open);
        socket.removeEventListener('error', on_socker_error);
        socket.removeEventListener('close', on_socket_close);
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
        fatal_error_dialog('Cannot connect to the server. Sorry! Please come again later.');
    }
    if (reason) {
        console.warn(log_time(), `WebSocket: ${reason}. Reconnecting...`);
    }

    // Ignore all further events from WebSocket. They could mess up with the client state
    // if they arrive in parallel with the events in the new socket. Plus, we don't want
    // to receive `JoinedInAnotherClient` error on reconnect.
    cut_off_socket();

    socket = new WebSocket(server_websocket_address());
    socket.addEventListener('message', on_socket_message);
    socket.addEventListener('open', on_socket_open);
    socket.addEventListener('error', on_socker_error);
    socket.addEventListener('close', on_socket_close);
}

function page_redirect(href) {
    cut_off_socket();
    location.href = href;
}

function usage_error(args_array, expected_args) {
    return new InvalidCommand(`Usage: /${args_array[0]} ${expected_args.join(' ')}`);
}

function get_args(args_array, expected_args) {
    const args_without_command_name = args_array.slice(1);
    if (args_without_command_name.length === expected_args.length) {
        return args_without_command_name;
    } else {
        throw usage_error(args_array, expected_args);
    }
}

function on_document_keydown(event) {
    with_error_handling(function() {
        if (menu_dialog.open) {
            if (event.key === 'Escape') {
                pop_menu_page();
            }
        } else {
            let isPrintableKey = event.key.length === 1;  // https://stackoverflow.com/a/38802011/3092679
            if (isPrintableKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
                chat_input.focus();
            } else if (['ArrowDown', 'ArrowUp'].includes(event.key)) {
                // Make sure log is not scrolled by arrow keys: we are scrolling it
                // programmatically to make sure the current turn is visible.
                event.preventDefault();
                wasm_client().on_vertical_arrow_key_down(event.key, event.ctrlKey, event.altKey);
                update();
            }
        }
    });
}

function on_paste(event) {
    if (!menu_dialog.open) {
        chat_input.focus();
    }
}

function on_command_keydown(event) {
    if (!event.repeat && event.key == 'Enter') {
        const input = String(chat_input.value);
        chat_input.value = '';
        execute_command(input);
    }
}

function execute_command(input) {
    with_error_handling(function() {
        let command_result_message = '';
        if (input.startsWith('/')) {
            const args = input.slice(1).split(/\s+/);
            switch (args[0]) {
                case 'sound': {
                    const expected_args = ['0:1:2:3'];
                    const [value] = get_args(args, expected_args);
                    let volume = parseInt(value);
                    if (isNaN(volume) || volume < 0 || volume > max_volume) {
                        throw usage_error(args, expected_args);
                    }
                    set_volume(volume);
                    command_result_message = 'Applied';
                    break;
                }
                case 'resign':
                    get_args(args, []);
                    wasm_client().resign();
                    break;
                case 'ready':
                    get_args(args, []);
                    wasm_client().toggle_ready();
                    break;
                case 'rules':
                    get_args(args, []);
                    show_match_rules();
                    break;
                case 'save':
                    get_args(args, []);
                    wasm_client().request_export();
                    break;
                // Internal.
                case 'perf': {
                    get_args(args, []);
                    const stats = wasm_client().meter_stats();
                    console.log(stats);
                    command_result_message = stats;
                    break;
                }
                // Internal. For testing WebSocket re-connection.
                case 'reconnect':
                    socket.close();
                    break;
                default:
                    throw new InvalidCommand(`Command does not exist: /${args[0]}`);
            }
        } else {
            wasm_client().execute_turn_command(input);
        }
        update();
        if (command_result_message) {
            wasm_client().add_command_result(command_result_message);
        }
    });
}

function on_tick() {
    with_error_handling(function() {
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
    with_error_handling(function() {
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
        console.log(log_time(), 'sending: ', event);
        socket.send(event);
    }
}

function process_notable_events() {
    let js_event;
    while ((js_event = wasm_client().next_notable_event())) {
        const js_event_type = js_event?.constructor?.name;
        if (js_event_type == 'JsEventNoop') {
            // Noop, but other events might be coming.
        } else if (js_event_type == 'JsEventSessionUpdated') {
            update_session();
        } else if (js_event_type == 'JsEventMatchStarted') {
            const url = new URL(window.location);
            url.searchParams.set(SearchParams.match_id, js_event.match_id);
            window.history.pushState({}, '', url);
            push_menu_page(menu_lobby_page);
        } else if (js_event_type == 'JsEventGameStarted') {
            close_menu();
        } else if (js_event_type == 'JsEventGameOver') {
            play_audio(Sound[js_event.result]);
        } else if (js_event_type == 'JsEventPlaySound') {
            play_audio(Sound[js_event.audio], js_event.pan);
        } else if (js_event_type == 'JsEventGameExportReady') {
            download(js_event.content, 'game.pgn');
        } else if (js_event_type != null) {
            throw 'Unexpected notable event: ' + js_event_type;
        }
    }
}

function update_drag_state() {
    const drag_state = wasm_client().drag_state();
    switch (drag_state) {
        case 'no':
            if (drag_element) {
                drag_element.remove();
                drag_element = null;
                drag_source_board_id = null;
            }
            wasm_client().reset_drag_highlights();
            break;
        case 'yes':
            console.assert(drag_element != null);
            break;
        case 'defunct':
            // Improvement potential: Better image (broken piece / add red cross).
            drag_element.setAttribute('opacity', 0.5);
            wasm_client().reset_drag_highlights();
            break;
        default:
            console.error(`Unknown drag_state: ${drag_state}`);
    }
}

function update_lobby_countdown() {
    const lobby_footer = document.getElementById('lobby-footer');
    const lobby_waiting = document.getElementById('lobby-waiting');
    const lobby_countdown_seconds = document.getElementById('lobby-countdown-seconds');
    const s = wasm_client().lobby_countdown_seconds_left();
    lobby_footer.classList.toggle('countdown', s != null);
    lobby_waiting.textContent = wasm_client().lobby_waiting_explanation();
    lobby_countdown_seconds.textContent = s;
}

function update_connection_status() {
    const FIGURE_SPACE = ' ';  // &numsp;
    const s = wasm_client().current_turnaround_time();
    const ms = Math.round(s * 1000);
    const ms_str = ms.toString().padStart(4, FIGURE_SPACE);
    if (s < 3.0) {
        connection_info.textContent = `Ping: ${ms_str} ms`;
        connection_info.classList.toggle('bad-connection', false);
    } else {
        // Set the content once to avoid breaking dots animation.
        if (!connection_info.classList.contains('bad-connection')) {
            connection_info.textContent = "Reconnecting ";
            connection_info.appendChild(make_animated_dots());
            connection_info.classList.toggle('bad-connection', true);
        }
        open_socket('irresponsive');
    }
}

function update_buttons() {
    const SHOW = null;
    const HIDE = 'none';
    const observer_status = wasm_client().observer_status();
    const game_status = wasm_client().game_status();
    switch (game_status) {
        case 'active':
            resign_button.style.display = (observer_status == 'no') ? SHOW : HIDE;
            ready_button.style.display = HIDE;
            break;
        case 'over':
            resign_button.style.display = HIDE;
            ready_button.style.display = (observer_status == 'permanently') ? HIDE : SHOW;
            break;
        case 'none':
            resign_button.style.display = HIDE;
            ready_button.style.display = HIDE;
            break;
        default:
            throw new Error(`Unknown game status: ${game_status}`);
    }
}

async function request_resign() {
    const ret = await text_dialog('Are you sure you want to resign?', [
        new MyButton('Keep playing', MyButton.HIDE),
        new MyButton('🏳 Resign', MyButton.DO),
    ]);
    if (ret == MyButton.DO) {
        execute_command('/resign');
    }
}

function server_websocket_address() {
    const search_params = new URLSearchParams(window.location.search);
    let address = search_params.get(SearchParams.server);
    if (address === 'local' || (!address && window.location.hostname === 'localhost')) {
        address = 'ws://localhost:14361';
    }
    address ??= `${window.location.origin}/ws`;
    if (!address.includes('://')) {
        address = `wss://${address}`;
    }
    const url = new URL(address);
    if (url.protocol !== 'ws:') {
        url.protocol = 'wss:';
    }
    return url;
}

function set_up_drag_and_drop() {
    // Note. Need to process mouse and touch screens separately. Cannot use pointer events
    // (https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events) here: it seems impossible
    // to implement drag cancellation with a right-click, because pointer API does not report
    // nested mouse events.

    document.addEventListener('mousedown', start_drag);
    document.addEventListener('mousemove', drag);
    document.addEventListener('mouseup', end_drag);
    document.addEventListener('mouseleave', end_drag);

    document.addEventListener('touchstart', start_drag);
    document.addEventListener('touchmove', drag);
    document.addEventListener('touchend', end_drag);
    document.addEventListener('touchcancel', end_drag);

    for (const board_id of ['primary', 'secondary']) {
        const svg = board_svg(board_id);
        svg.addEventListener('click', (event) => click(event, board_id));
        // Note the difference: drag is cancelled while dragging, no matter the mouse position. Other
        // partial turn inputs and preturns are cancelled by right-click on the corresponding board.
        svg.addEventListener('contextmenu', (event) => cancel_preturn(event, board_id));
    }
    document.addEventListener('contextmenu', cancel_drag);

    function is_main_pointer(event) { return event.button == 0 || event.changedTouches?.length >= 1; }

    function mouse_position_relative_to_board(event, board_svg) {
        const ctm = board_svg.getScreenCTM();
        const src = event.changedTouches ? event.changedTouches[0] : event;
        return {
            x: (src.clientX - ctm.e) / ctm.a,
            y: (src.clientY - ctm.f) / ctm.d,
        };
    }

    function click(event, board_id) {
        with_error_handling(function() {
            if (is_main_pointer(event)) {
              const promotion_target = event.target.getAttribute('data-promotion-target');
              if (promotion_target) {
                wasm_client().choose_promotion_upgrade(board_id, promotion_target);
                update();
              } else {
                const coord = mouse_position_relative_to_board(event, board_svg(board_id));
                wasm_client().click_board(board_id, coord.x, coord.y);
                update();
              }
            }
        });
    }

    function start_drag(event) {
        with_error_handling(function() {
            // Improvement potential. Highlight pieces outside of board area: add shadows separately
            //   and move them to the very back, behing boards.
            // Improvement potential: Choose the closest reserve piece rather then the one on top.
            // Note. For a mouse we can simple assume that drag_element is null here. For multi-touch
            //   screens however this is not always the case.
            if (!drag_element && event.target.classList.contains('draggable') && is_main_pointer(event)) {
                const source = event.target.getAttribute('data-bughouse-location');
                const board_idx = wasm_client().start_drag_piece(source);

                if (board_idx == 'abort') {
                    return;
                }

                drag_source_board_id = board_idx;
                drag_element = event.target;
                drag_element.classList.add('dragged');
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
                drag(event);
            }
        });
    }

    function drag(event) {
        with_error_handling(function() {
            if (drag_element) {
                const coord = mouse_position_relative_to_board(event, drag_source_board());
                wasm_client().drag_piece(drag_source_board_id, coord.x, coord.y);
                drag_element.setAttribute('x', coord.x - 0.5);
                drag_element.setAttribute('y', coord.y - 0.5);
            }
        });
    }

    function end_drag(event) {
        with_error_handling(function() {
            if (drag_element && is_main_pointer(event)) {
                const coord = mouse_position_relative_to_board(event, drag_source_board());
                wasm_client().drag_piece_drop(drag_source_board_id, coord.x, coord.y);
                drag_element.remove();
                drag_element = null;
                drag_source_board_id = null;
                update();
            }
        });
    }

    function cancel_preturn(event, board_id) {
        with_error_handling(function() {
            event.preventDefault();
            if (!drag_element) {
                wasm_client().cancel_preturn(board_id);
                update();
            }
        });
    }

    function cancel_drag(event) {
        with_error_handling(function() {
            if (drag_element) {
                event.preventDefault();
                wasm_client().abort_drag_piece();
                update();
            }
        });
    }
}

function set_up_chalk_drawing() {
    let chalk_target = null;
    let ignore_next_cancellation = false;
    let ignore_next_context_menu = false;

    function is_draw_button(event) { return event.button == 2; }
    function is_cancel_button(event) { return event.button == 0; }

    function viewbox_mouse_position(node, event) {
        const ctm = node.getScreenCTM();
        return {
            x: (event.clientX - ctm.e) / ctm.a,
            y: (event.clientY - ctm.f) / ctm.d,
        };
    }

    function mouse_down(event) {
        with_error_handling(function() {
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
        with_error_handling(function() {
            if (wasm_client().is_chalk_active()) {
                console.assert(chalk_target != null);
                const coord = viewbox_mouse_position(chalk_target, event);
                wasm_client().chalk_move(coord.x, coord.y);
            }
        });
    }

    function mouse_up(event) {
        with_error_handling(function() {
            if (wasm_client().is_chalk_active() && is_draw_button(event)) {
                console.assert(chalk_target != null);
                const coord = viewbox_mouse_position(chalk_target, event);
                wasm_client().chalk_up(coord.x, coord.y);
                chalk_target = null;
                ignore_next_context_menu = true;
            }
        });
    }

    function mouse_click(event) {
        with_error_handling(function() {
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

    for (const board_id of ['primary', 'secondary']) {
        // Improvement potential. Support chalk on touch screens.
        const svg = board_svg(board_id);
        svg.addEventListener('mousedown', mouse_down);
        svg.addEventListener('click', mouse_click);
    }
    document.addEventListener('mousemove', mouse_move);
    document.addEventListener('mouseup', mouse_up);
    document.addEventListener('contextmenu', on_context_menu);
}

function update_cookie_policy() {
    const is_analytics_ok = window.localStorage.getItem(Storage.cookies_accepted) == 'all';
    const show_banner = window.localStorage.getItem(Storage.cookies_accepted) == null;
    gtag('consent', 'update', {
        'analytics_storage': is_analytics_ok ? 'granted' : 'denied'
    });
    cookie_banner.style.display = show_banner ? null : 'None';
}

function on_accept_essential_cookies() {
    window.localStorage.setItem(Storage.cookies_accepted, 'essential');
    update_cookie_policy();
}

function on_accept_all_cookies() {
    window.localStorage.setItem(Storage.cookies_accepted, 'all');
    update_cookie_policy();
}

function set_up_menu_pointers() {
    function is_cycle_forward(event) { return event.button == 0 || event.changedTouches?.length >= 1; }
    function is_cycle_backward(event) { return event.button == 2; }

    function mouse_down(event) {
        with_error_handling(function() {
            const my_readiness = document.getElementById('my-readiness');
            const my_faction = document.getElementById('my-faction');
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
        const lobby_participants = document.getElementById('lobby-participants');
        if (lobby_participants?.contains(event.target)) {
            event.preventDefault();
        }
    }

    menu_dialog.addEventListener('mousedown', mouse_down);
    menu_dialog.addEventListener('contextmenu', context_menu);
}

function set_up_log_navigation() {
    for (const board_id of ['primary', 'secondary']) {
        const area_node = document.getElementById(`turn-log-scroll-area-${board_id}`);
        area_node.addEventListener('click', (event) => {
            with_error_handling(function() {
                // TODO: Convenient ways to navigate (including keyboard) and to reset.
                const turn_node = event.target.closest('[data-turn-index]');
                const turn_index = turn_node?.getAttribute('data-turn-index');
                wasm_client().wayback_to_turn(board_id, turn_index);
                update();
            });
        });
    }
}

function find_player_name_input(page) {
    for (const input of page.getElementsByTagName('input')) {
        if (input.name == 'player_name' || input.name == 'user_name') {
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
    for (const input of page.getElementsByTagName('input')) {
        if (input.type == 'password') {
            input.value = '';
        }
    }
}

function hide_menu_pages(execute_on_hide = true) {
    for (const page of menu_pages) {
        if (page.style.display !== 'none') {
            if (execute_on_hide) {
                on_hide_menu_page(page);
            }
            page.style.display = 'none';
        }
    }
}

function reset_menu(page) {
    menu_page_stack.length = 0;
    hide_menu_pages();

    const search_params = new URLSearchParams(window.location.search);
    const match_id = search_params.get(SearchParams.match_id);
    if (match_id) {
        jc_match_id.value = match_id;
        push_menu_page(menu_join_match_page);
    } else {
        menu_start_page.style.display = 'block';
    }
}

function push_menu_page(page) {
    menu_page_stack.push(page);
    hide_menu_pages();
    page.style.display = 'block';

    // Auto fill player name:
    const player_name_input = find_player_name_input(page);
    if (player_name_input) {
        player_name_input.value = window.localStorage.getItem(Storage.player_name);
    }
    // Focus first empty input, if any:
    for (const input of page.getElementsByTagName('input')) {
        if (!input.disabled != 'none' && !input.value) {
            input.focus();
            break;
        }
    }
}

function pop_menu_page() {
    menu_page_stack.pop();
    const page = menu_page_stack.at(-1) || menu_start_page;
    hide_menu_pages();
    page.style.display = 'block';
}

function close_menu() {
    hide_menu_pages();  // hide the pages to execute "on hide" handlers
    menu_dialog.close();
    menu_backdrop.style.display = 'None';
    for (const element of page_element.getElementsByTagName('*')) {
        if ('disabled' in element) {
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
    for (const element of page_element.getElementsByTagName('*')) {
        if ('disabled' in element) {
            element.disabled = true;
        }
    }
}

function init_menu() {
    hide_menu_pages(false);
    open_menu();
}

// Shows a dialog with a message and buttons.
// If there is a button with `MyButton.HIDE` action, then `Escape` will close the dialog and
// also return `MyButton.HIDE`. If there are no buttons with `MyButton.HIDE` action, then
// `Escape` key will be ignored.
function html_dialog(body, buttons) {
    if (fatal_error_shown) {
        return;
    }
    return new Promise(resolve => {
        const dialog = document.createElement('dialog');
        document.body.appendChild(dialog);
        const button_box = document.createElement('div');
        button_box.className = 'simple-dialog-button-box';
        let can_hide = false;
        for (const button of (buttons || [])) {
            const button_node = document.createElement('button');
            button_node.type = 'button';
            button_node.role = 'button';
            button_node.textContent = button.label;
            const action = button.action;
            can_hide ||= (action == MyButton.HIDE);
            button_node.addEventListener('click', (event) => {
                dialog.close();
                resolve(action);
            });
            button_box.appendChild(button_node);
        }
        dialog.addEventListener('cancel', (event) => {
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
  const message_node = document.createElement('div');
  message_node.className = 'simple-dialog-message';
  message_node.innerText = message;
  return html_dialog(message_node, buttons);
}

function info_dialog(message) {
    return text_dialog(message, [new MyButton('Ok', MyButton.HIDE)]);
}

function ignorable_error_dialog(message) {
    return info_dialog(message);
}

function fatal_error_dialog(message) {
    text_dialog(message);
    fatal_error_shown = true;
}

function make_svg_image(symbol_id, size) {
    const SVG_NS = 'http://www.w3.org/2000/svg';
    const symbol = document.createElementNS(SVG_NS, 'symbol');
    symbol.id = symbol_id;
    const image = document.createElementNS(SVG_NS, 'image');
    image.id = `${symbol_id}-image`;
    image.setAttribute('width', size);
    image.setAttribute('height', size);
    symbol.appendChild(image);
    svg_defs.appendChild(symbol);
}

async function load_image(filepath, target_id) {
    const reader = new FileReader();
    reader.addEventListener('load', () => {
        const image = document.getElementById(target_id);
        image.setAttribute('href', reader.result);
        loading_tracker.resource_loaded();
    }, false);
    reader.addEventListener('error', () => {
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
    const reader = new FileReader();
    reader.addEventListener('load', () => {
        const audio = Sound[key];
        audio.setAttribute('src', reader.result);
        loading_tracker.resource_loaded();
    }, false);
    reader.addEventListener('error', () => {
        console.error(`Cannot load sound ${filepath}`);
    });
    const response = await fetch(filepath);
    const blob = await response.blob();
    reader.readAsDataURL(blob);
}

function load_sounds(sound_map) {
    const ret = {};
    for (const [key, filepath] of Object.entries(sound_map)) {
        ret[key] = new Audio();
        load_sound(filepath, key);
        loading_tracker.resource_required();
    }
    return ret;
}

function go_to_suburl(event) {
    const suburl = event.target.getAttribute('data-suburl');
    const url = new URL(window.location);
    url.pathname = suburl;
    window.open(url, '_blank')?.focus();
}

function update_session() {
    with_error_handling(function() {
        reset_menu();
        const session = wasm_client().session();
        const using_password_auth = session.registration_method == 'Password';
        let is_guest = null;
        let user_name = null;
        switch (session.status) {
            case 'unknown':
            case 'google_oauth_registering': {
                is_registered_user = false;
                is_guest = false;
                user_name = null;
                break;
            }
            case 'logged_out': {
                is_registered_user = false;
                is_guest = true;
                user_name = 'Guest';
                break;
            }
            case 'logged_in': {
                is_registered_user = true;
                is_guest = false;
                user_name = session.user_name;
                break;
            }
        }
        const session_info_loaded = is_registered_user || is_guest;
        const ready_to_play = session_info_loaded && wasm_client().got_server_welcome();
        registered_user_bar.style.display = is_registered_user ? null : 'None';
        guest_user_bar.style.display = !is_registered_user ? null : 'None';
        guest_user_tooltip.style.display = is_guest ? null : 'None';
        if (ready_to_play) {
            if (is_registered_user) {
                create_rated_match_button.disabled = false;
                set_tooltip(create_rated_match_button, null);
            } else {
                create_rated_match_button.disabled = true;
                set_tooltip(create_rated_match_button, 'Please sign in to play rated games');
            }
            create_unrated_match_button.disabled = false;
            join_match_button.disabled = false;
            authorization_button.disabled = false;
        } else {
            create_rated_match_button.disabled = true;
            create_unrated_match_button.disabled = true;
            join_match_button.disabled = true;
            authorization_button.disabled = true;
        }
        for (const node of document.querySelectorAll('.logged-in-as-account')) {
            node.classList.toggle('account-user', is_registered_user);
            node.classList.toggle('account-guest', is_guest);
            if (user_name === null) {
                node.textContent = '';
                node.appendChild(make_animated_dots());
            } else {
                node.textContent = user_name;
            }
        }
        change_account_email.value = session.email;
        for (const node of document.querySelectorAll('.logged-in-as-email')) {
            node.textContent = session.email || '—';
        }
        for (const node of document.querySelectorAll('.logged-in-with-password')) {
            node.style.display = using_password_auth ? null : 'None';
            node.disabled = !using_password_auth;
        }
        for (const node of document.querySelectorAll('.guest-player-name')) {
            node.style.display = is_guest ? null : 'None';
            node.disabled = !is_guest;
        }
        if (session.status == 'google_oauth_registering') {
            push_menu_page(menu_signup_with_google_page);
        }
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
        window.history.replaceState({}, '');
        // Now wait for `UpdateSession` socket event...
    } else {
        await ignorable_error_dialog(await response.text());
    }
}

async function sign_up(event) {
    const data = new FormData(event.target);
    if (data.get('confirm_password') != data.get('password')) {
        ignorable_error_dialog('Passwords do not match!');
        return;
    }
    data.delete('confirm_password');
    if (!data.get('email')) {
        const ret = await text_dialog(
            'Without an email you will not be able to restore your account ' +
            'if you forget your password. Continue?',
            [
                new MyButton('Go back', MyButton.HIDE),
                new MyButton('Proceed without email', MyButton.DO),
            ]
        );
        if (ret != MyButton.DO) {
            return;
        }
    }
    process_authentification_request(new Request('auth/signup', {
        method: 'POST',
        body: as_x_www_form_urlencoded(data),
    }));
}

function sign_up_with_google(event) {
    const data = new FormData(event.target);
    process_authentification_request(new Request('auth/finish-signup-with-google', {
        method: 'POST',
        body: as_x_www_form_urlencoded(data),
    }));
}

async function sign_with_google(event) {
    page_redirect('/auth/sign-with-google');
}

async function log_in(event) {
    const data = new FormData(event.target);
    process_authentification_request(new Request('auth/login', {
        method: 'POST',
        body: as_x_www_form_urlencoded(data),
    }));
}

function log_out(event) {
    process_authentification_request(new Request('auth/logout', {
        method: 'POST',
    }));
}

async function change_account(event) {
    const data = new FormData(event.target);
    if (data.get('confirm_new_password') != data.get('new_password')) {
        ignorable_error_dialog('Passwords do not match!');
        return;
    }
    data.delete('confirm_new_password');
    process_authentification_request(new Request('auth/change-account', {
        method: 'POST',
        body: as_x_www_form_urlencoded(data),
    }), 'Account changes applied.');
}

async function delete_account(event) {
    const data = new FormData(event.target);
    process_authentification_request(new Request('auth/delete-account', {
        method: 'POST',
        body: as_x_www_form_urlencoded(data),
    }), 'Account deleted.');
}

function show_create_match_page() {
    with_error_handling(function() {
        wasm_client().init_new_match_rules_body();
    });
    let rules_variants = document.getElementById('cc-rule-variants');
    for (const node of rules_variants.querySelectorAll('button')) {
        node.addEventListener('click', rule_variant_button_clicked);
    }
    push_menu_page(menu_create_match_page);
}

function create_match_as_guest(event) {
    with_error_handling(function() {
        const player_name = document.getElementById('ccn-player-name').value;
        wasm_client().set_guest_player_name(player_name);
        show_create_match_page();
    });
}

function on_create_match_request(event, rated) {
    const cc_rating = document.getElementById('cc-rating');
    const cc_confirm_button = document.getElementById('cc-confirm-button');
    cc_rating.value = rated ? 'rated' : 'unrated';
    cc_confirm_button.innerText = rated ? 'Create rated match!' : 'Create unrated match!';
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
    const next_state = node.getAttribute('data-next-state');
    const next_state_node = document.getElementById(next_state);
    node.classList.add('display-none');
    next_state_node.classList.remove('display-none');
    with_error_handling(function() {
        wasm.update_new_match_rules_body();
    });
}

function on_create_match_confirm(event) {
    with_error_handling(function() {
        wasm_client().new_match();
        update();
    });
}

function on_join_match_confirm(event) {
    with_error_handling(function() {
        const data = new FormData(event.target);
        wasm_client().set_guest_player_name(data.get('player_name'));
        wasm_client().join(data.get('match_id').toUpperCase());
        update();
    });
}

function show_match_rules() {
  with_error_handling(function() {
    const rules_node = document.createElement('div');
    rules_node.className = 'match-rules-body';
    rules_node.innerHTML = wasm_client().readonly_rules_body();
    html_dialog(rules_node, [new MyButton('Ok', MyButton.HIDE)]);
  });
}

function set_volume(volume) {
    // TODO: Save settings to a local storage.
    audio_volume = volume;
    if (volume == 0) {
        document.getElementById('volume-mute').style.display = null;
        for (let v = 1; v <= max_volume; ++v) {
            document.getElementById(`volume-${v}`).style.display = 'none';
        }
    } else {
        document.getElementById('volume-mute').style.display = 'none';
        for (let v = 1; v <= max_volume; ++v) {
            document.getElementById(`volume-${v}`).style.display = (v > volume) ? 'none' : null;
        }
    }
}

function next_volume() {
    set_volume((audio_volume + 1) % (max_volume + 1));
    play_audio(Sound.clack);
}

function ensure_audio_context() {
    // Ideally this should be called after the first user interaction.
    // If an AudioContext is created before the document receives a user gesture, it will be
    // created in the "suspended" state, and a log warning will be shown (in Chrome):
    // https://developer.chrome.com/blog/autoplay/#webaudio
    audio_context ||= new AudioContext();
    // Ensure that the context is active in case it was created too early.
    audio_context.resume();
}

function play_audio(audio, pan) {
    ensure_audio_context();
    pan = pan || 0;
    if (audio_queue.length >= audio_max_queue_size) {
        return;
    }
    audio_queue.push({ audio, pan });
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
    const { audio, pan } = audio_queue.shift();
    if (audio_volume > 0) {
        // Clone node to allow playing overlapping instances of the same sound.
        // TODO: Should `audio_clone`, `track` and/or `panner` be manually GCed?
        let audio_clone = audio.cloneNode();
        const panner = new StereoPannerNode(audio_context, { pan });
        const track = audio_context.createMediaElementSource(audio_clone);
        track.connect(panner).connect(audio_context.destination);
        audio_clone.volume = volume_to_js[audio_volume];
        audio_clone.play();
    }
    audio_last_played = performance.now();
}

function download(text, filename) {
    var element = document.createElement('a');
    element.setAttribute('href', 'data:text/plain;charset=utf-8,' + encodeURIComponent(text));
    element.setAttribute('download', filename);
    element.style.display = 'none';
    document.body.appendChild(element);
    element.click();
    document.body.removeChild(element);
}

// Must have corresponding tooltip class (`tooltip-right` or `tooltip-below`).
function set_tooltip(node, text) {
    for (const tooltip_node of node.querySelectorAll('.tooltip-text')) {
        tooltip_node.remove();
    }
    if (text !== null) {
        const tooltip_node = document.createElement('div');
        tooltip_node.className = 'tooltip-text';
        const p = document.createElement('p');
        p.innerText = text;
        tooltip_node.appendChild(p);
        node.appendChild(tooltip_node);
    }
}

// TODO: Dedup against `index.html`.
function make_animated_dots() {
    const parent = document.createElement('span');
    for (let i = 0; i < 3; ++i) {
        const dot = document.createElement('span');
        dot.className = 'dot';
        dot.innerText = '.';
        parent.appendChild(dot);
    }
    return parent;
}
