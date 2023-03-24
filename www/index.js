// TODO: Remove logging (or at least don't log heartbeat events).
// TODO: Check if ==/!= have to be replaced with ===/!== and other JS weirdness.
// TODO: Figure out if it's possible to enable strict mode with webpack.

import './main.css';
import * as wasm from 'bughouse-chess';

import favicon from '../assets/favicon.png';

import white_pawn from '../assets/pieces/white-pawn.png';
import white_knight from '../assets/pieces/white-knight.png';
import white_bishop from '../assets/pieces/white-bishop.png';
import white_rook from '../assets/pieces/white-rook.png';
import white_queen from '../assets/pieces/white-queen.png';
import white_cardinal from '../assets/pieces/white-cardinal.png';
import white_empress from '../assets/pieces/white-empress.png';
import white_amazon from '../assets/pieces/white-amazon.png';
import white_king from '../assets/pieces/white-king.png';
import black_pawn from '../assets/pieces/black-pawn.png';
import black_knight from '../assets/pieces/black-knight.png';
import black_bishop from '../assets/pieces/black-bishop.png';
import black_rook from '../assets/pieces/black-rook.png';
import black_queen from '../assets/pieces/black-queen.png';
import black_cardinal from '../assets/pieces/black-cardinal.png';
import black_empress from '../assets/pieces/black-empress.png';
import black_amazon from '../assets/pieces/black-amazon.png';
import black_king from '../assets/pieces/black-king.png';

import fog_1 from '../assets/fog-of-war/fog-1.png';
import fog_2 from '../assets/fog-of-war/fog-2.png';
import fog_3 from '../assets/fog-of-war/fog-3.png';

import turn_sound from '../assets/sounds/turn.ogg';
import reserve_restocked_sound from '../assets/sounds/reserve-restocked.ogg';
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
    const t = performance.now() - log_time.start;
    return `[t=${t.toFixed(1)}]`;
}
log_time();  // start the counter

// Improvement potential. Similarly group other global variables.
const Storage = {
    cookies_accepted: 'cookies-accepted',  // values: null, "essential", "all"
    player_name: 'player-name',
};

const SearchParams = {
    contest_id: 'contest-id',
    server: 'server',
};

const page_element = document.getElementById('page');
const git_version = document.getElementById('git-version');
const command_input = document.getElementById('command');
const command_result = document.getElementById('command-result');
const loading_status = document.getElementById('loading-status');
const connection_info = document.getElementById('connection-info');

const menu_backdrop = document.getElementById('menu-backdrop');
const menu_dialog = document.getElementById('menu-dialog');
const menu_start_page = document.getElementById('menu-start-page');
const menu_authorization_page = document.getElementById('menu-authorization-page');
const menu_login_page = document.getElementById('menu-login-page');
const menu_signup_page = document.getElementById('menu-signup-page');
const menu_signup_with_google_page = document.getElementById('menu-signup-with-google-page');
const menu_create_contest_page = document.getElementById('menu-create-contest-page');
const menu_join_contest_page = document.getElementById('menu-join-contest-page');
const menu_about_page = document.getElementById('menu-about-page');
const menu_lobby_page = document.getElementById('menu-lobby-page');
const menu_pages = document.getElementsByClassName('menu-page');

const cookie_banner = document.getElementById('cookie-banner');
const accept_essential_cookies_button = document.getElementById('accept-essential-cookies-button');
const accept_all_cookies_button = document.getElementById('accept-all-cookies-button');

const logged_in_user_bar = document.getElementById('logged-in-user-bar');
const guest_user_bar = document.getElementById('guest-user-bar');
const guest_user_tooltip = document.getElementById('guest-user-tooltip');
const signup_with_google_email = document.getElementById('signup-with-google-email');
const authorization_button = document.getElementById('authorization-button');
const log_out_button = document.getElementById('log-out-button');
const sign_with_google_button = document.getElementById('sign-with-google-button');
const begin_login_button = document.getElementById('begin-login-button');
const begin_signup_button = document.getElementById('begin-signup-button');

const create_contest_button = document.getElementById('create-contest-button');
const join_contest_button = document.getElementById('join-contest-button');
const about_button = document.getElementById('about-button');
const jc_contest_id = document.getElementById('jc-contest-id');

const ready_button = document.getElementById('ready-button');
const resign_button = document.getElementById('resign-button');
const export_button = document.getElementById('export-button');
const volume_button = document.getElementById('volume-button');

const svg_defs = document.getElementById('svg-defs');

const menu_page_stack = [];

const loading_tracker = new class {
    #resources_required = 0;
    #resources_loaded = 0;
    #connected = false;

    constructor() {
        this.#update();
    }
    resource_required() {
        this.#resources_required += 1;
        this.#update();
    }
    resource_loaded() {
        this.#resources_loaded += 1;
        this.#update();
    }
    connected() {
        this.#connected = true;
        this.#update();
    }
    #update() {
        // TODO: Don't start the game until everything is ready.
        console.assert(this.#resources_loaded <= this.#resources_required);
        const resources_ready = this.#resources_loaded == this.#resources_required;
        if (resources_ready && this.#connected) {
            loading_status.innerText = '';
        } else {
            const connection_string = this.#connected ? 'Connected' : 'Connecting...';
            const resource_string = resources_ready
                ? 'Resources loaded'
                : `Loading resources... ${this.#resources_loaded} / ${this.#resources_required}`;
            loading_status.innerText = `${connection_string}\n${resource_string}`;
        }
    }
};

set_favicon();

window.dataLayer = window.dataLayer || [];
function gtag() { window.dataLayer.push(arguments); }
update_cookie_policy();

const FOG_TILE_SIZE = 1.2;
load_svg_images([
    { path: white_pawn, symbol: 'white-pawn' },
    { path: white_knight, symbol: 'white-knight' },
    { path: white_bishop, symbol: 'white-bishop' },
    { path: white_rook, symbol: 'white-rook' },
    { path: white_queen, symbol: 'white-queen' },
    { path: white_cardinal, symbol: 'white-cardinal' },
    { path: white_empress, symbol: 'white-empress' },
    { path: white_amazon, symbol: 'white-amazon' },
    { path: white_king, symbol: 'white-king' },
    { path: black_pawn, symbol: 'black-pawn' },
    { path: black_knight, symbol: 'black-knight' },
    { path: black_bishop, symbol: 'black-bishop' },
    { path: black_rook, symbol: 'black-rook' },
    { path: black_queen, symbol: 'black-queen' },
    { path: black_cardinal, symbol: 'black-cardinal' },
    { path: black_empress, symbol: 'black-empress' },
    { path: black_amazon, symbol: 'black-amazon' },
    { path: black_king, symbol: 'black-king' },
    { path: fog_1, symbol: 'fog-1', size: FOG_TILE_SIZE },
    { path: fog_2, symbol: 'fog-2', size: FOG_TILE_SIZE },
    { path: fog_3, symbol: 'fog-3', size: FOG_TILE_SIZE },
]);

// Improvement potential. Establish priority on sounds; play more important sounds first
// in case of a clash.
const Sound = load_sounds({
    victory: victory_sound,
    defeat: defeat_sound,
    draw: draw_sound,
    turn: turn_sound,
    reserve_restocked: reserve_restocked_sound,
    low_time: low_time_sound,
});

init_menu();

wasm.set_panic_hook();
wasm.init_page();
git_version.innerText = wasm.git_version();

set_up_drag_and_drop();
set_up_chalk_drawing();
set_up_menu_pointers();

let wasm_client_object = make_wasm_client();
let wasm_client_panicked = false;
let socket = make_socket();

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

let drag_source_board = null;
let drag_element = null;

const Meter = make_meters();

update_session();

document.addEventListener('keydown', on_document_keydown);
document.addEventListener('paste', on_paste);

command_input.addEventListener('keydown', on_command_keydown);

ready_button.addEventListener('click', () => execute_command('/ready'));
resign_button.addEventListener('click', request_resign);
export_button.addEventListener('click', () => execute_command('/save'));
volume_button.addEventListener('click', next_volume);

accept_essential_cookies_button.addEventListener('click', on_accept_essential_cookies);
accept_all_cookies_button.addEventListener('click', on_accept_all_cookies);
menu_dialog.addEventListener('cancel', (event) => event.preventDefault());
authorization_button.addEventListener('click', () => push_menu_page(menu_authorization_page));
log_out_button.addEventListener('click', log_out);
sign_with_google_button.addEventListener('click',  sign_with_google);
begin_login_button.addEventListener('click',  () => push_menu_page(menu_login_page));
begin_signup_button.addEventListener('click',  () => push_menu_page(menu_signup_page));
menu_login_page.addEventListener('submit', log_in);
menu_signup_page.addEventListener('submit', sign_up);
menu_signup_with_google_page.addEventListener('submit', sign_up_with_google);
create_contest_button.addEventListener('click', on_create_contest_submenu);
join_contest_button.addEventListener('click', on_join_contest_submenu);
about_button.addEventListener('click', () => push_menu_page(menu_about_page));
menu_create_contest_page.addEventListener('submit', on_create_contest_confirm);
menu_join_contest_page.addEventListener('submit', on_join_contest_confirm);

for (const button of document.querySelectorAll('.back-button')) {
    button.addEventListener('click', pop_menu_page);
}
for (const button of document.querySelectorAll('[data-suburl]')) {
    button.addEventListener('click', go_to_suburl);
}

// TODO: Make sounds louder and set volume to 2 by default.
set_volume(max_volume);

let on_tick_interval_id = setInterval(on_tick, 50);


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
            throw e;
        } else if (e instanceof InvalidCommand) {
            command_result.innerText = e.msg;
        } else if (e?.constructor?.name == 'IgnorableError') {
            ignorable_error_dialog(e.message);
        } else if (e?.constructor?.name == 'KickedFromContest') {
            ignorable_error_dialog(e.message);
            // Need to recreate the socket because server aborts the connection here.
            // If this turns out to be buggy, could do
            //   ignorable_error_dialog(e.message).then(() => location.reload());
            // instead.
            socket = make_socket();
            open_menu();
            push_menu_page(menu_join_contest_page);
        } else if (e?.constructor?.name == 'FatalError') {
            fatal_error_dialog(e.message);
        } else if (e?.constructor?.name == 'RustError') {
            ignorable_error_dialog(`Internal Rust error: ${e.message}`);
            if (socket.readyState == WebSocket.OPEN) {
                socket.send(wasm.make_rust_error_event(e));
            }
            throw e;
        } else {
            const rust_panic = wasm.last_panic();
            if (rust_panic) {
                wasm_client_panicked = true;
                let reported = '';
                if (socket.readyState == WebSocket.OPEN) {
                    socket.send(rust_panic);
                    reported = 'The error has been reported (unless that failed too).';
                } else {
                    reported =
                        'The error has NOT been reported: not connected to server. ' +
                        'Please consider reporting it to contact.bughousepro@gmail.com'
                    ;
                }
                fatal_error_dialog(
                    'Internal error! This client is now dead 💀 ' +
                    'Only refreshing the page may help you. We are very sorry. ' +
                    reported
                );
                if (on_tick_interval_id != null) {
                    clearInterval(on_tick_interval_id);
                    on_tick_interval_id = null;
                }
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

function make_socket() {
    const socket = new WebSocket(server_websocket_address());
    socket.addEventListener('message', function(event) {
        on_server_event(event.data);
    });
    socket.addEventListener('open', function(event) {
        loading_tracker.connected();
    });
    socket.addEventListener('close', (event) => {
        // TODO: Report socket errors.
        // TODO: Reconnect automatically.
        console.error('WebSocket closed: ', event);
        fatal_error_dialog('Connection lost. Please reload the page.');
    });
    return socket;
}

function on_server_event(event) {
    with_error_handling(function() {
        console.log(log_time(), 'server: ', event);
        const update_needed = wasm_client().process_server_event(event);
        if (update_needed) {
            update();
        }
    });
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
    if (menu_dialog.open) {
        if (event.key === 'Escape') {
            pop_menu_page();
        }
    } else {
        let isPrintableKey = event.key.length === 1;  // https://stackoverflow.com/a/38802011/3092679
        if (isPrintableKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
            command_input.focus();
        }
    }
}

function on_paste(event) {
    if (!menu_dialog.open) {
        command_input.focus();
    }
}

function on_command_keydown(event) {
    if (!event.repeat && event.key == 'Enter') {
        const input = String(command_input.value);
        command_input.value = '';
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
                case 'save':
                    get_args(args, []);
                    wasm_client().request_export();
                    break;
                case 'perf': {
                    get_args(args, []);
                    const stats = wasm_client().meter_stats();
                    console.log(stats);
                    command_result_message = stats;
                    break;
                }
                default:
                    throw new InvalidCommand(`Command does not exist: /${args[0]}`);
            }
        } else {
            wasm_client().execute_turn_command(input);
        }
        update();
        command_result.innerText = command_result_message;
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
        command_result.innerText = '';
    });
}

function process_outgoing_events() {
    if (socket.readyState == WebSocket.CONNECTING) {
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
        } else if (js_event_type == 'JsEventContestStarted') {
            const url = new URL(window.location);
            url.searchParams.set(SearchParams.contest_id, js_event.contest_id);
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
                drag_source_board = null;
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
    const ms = (s == null) ? '–––' : Math.round(s * 1000);
    const ms_str = ms.toString().padStart(4, FIGURE_SPACE);
    connection_info.textContent = `Ping: ${ms_str} ms`;
    connection_info.classList.toggle('bad-connection', s >= 3.0);
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
    const ret = await simple_dialog('Are you sure you want to resign?', [
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

    for (const board of ['primary', 'secondary']) {
        const svg = document.getElementById(`board-${board}`);
        svg.addEventListener('contextmenu', (event) => cancel_preturn(event, board));
    }
    document.addEventListener('contextmenu', cancel_drag);

    function is_main_pointer(event) { return event.button == 0 || event.changedTouches?.length >= 1; }

    function mouse_position_relative_to_board(event) {
        const ctm = drag_source_board.getScreenCTM();
        const src = event.changedTouches ? event.changedTouches[0] : event;
        return {
            x: (src.clientX - ctm.e) / ctm.a,
            y: (src.clientY - ctm.f) / ctm.d,
        };
    }

    function start_drag(event) {
        with_error_handling(function() {
            // Improvement potential. Highlight pieces outside of board area: add shadows separately
            //   and move them to the very back, behing boards.
            // Improvement potential: Choose the closest reserve piece rather then the one on top.
            // Note. For a mouse we can simple assume that drag_element is null here. For multi-touch
            //   screens however this is not always the case.
            if (!drag_element && event.target.classList.contains('draggable') && is_main_pointer(event)) {
                drag_element = event.target;
                drag_element.classList.add('dragged');
                // Dissociate image from the board/reserve:
                drag_element.id = null;

                const source = drag_element.getAttribute('data-bughouse-location');
                const drag_source_board_idx = wasm_client().start_drag_piece(source);
                drag_source_board = document.getElementById(`board-${drag_source_board_idx}`);

                // Reparent: bring on top; (if reserve) remove shadow by extracting from reserve group.
                //
                // TODO: Fix: Reparenting breaks touch drag. According to
                //   https://stackoverflow.com/questions/33298828/touch-move-event-dont-fire-after-touch-start-target-is-removed
                // this should've helped:
                //   drag_element.addEventListener('touchmove', drag);
                // but it didn't work for me.
                drag_source_board.appendChild(drag_element);

                update();

                // Properly position reserve piece after re-parenting.
                drag(event);
            }
        });
    }

    function drag(event) {
        with_error_handling(function() {
            if (drag_element) {
                const coord = mouse_position_relative_to_board(event);
                drag_element.setAttribute('x', coord.x - 0.5);
                drag_element.setAttribute('y', coord.y - 0.5);
                wasm_client().drag_piece(coord.x, coord.y);
            }
        });
    }

    function end_drag(event) {
        with_error_handling(function() {
            if (drag_element && is_main_pointer(event)) {
                const coord = mouse_position_relative_to_board(event);
                drag_element.remove();
                drag_element = null;
                drag_source_board = null;
                wasm_client().drag_piece_drop(coord.x, coord.y, event.shiftKey);
                update();
            }
        });
    }

    function cancel_preturn(event, board) {
        with_error_handling(function() {
            event.preventDefault();
            if (!drag_element) {
                wasm_client().cancel_preturn(board);
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
    function is_draw_button(event) { return event.button == 2; }
    function is_cancel_button(event) { return event.button == 0; }

    function viewbox_mouse_position(event) {
        const ctm = event.currentTarget.getScreenCTM();
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
                const coord = viewbox_mouse_position(event);
                wasm_client().chalk_down(event.currentTarget.id, coord.x, coord.y, event.ctrlKey);
            } else if (wasm_client().is_chalk_active() && is_cancel_button(event)) {
                wasm_client().chalk_abort();
            }
        });
    }

    function mouse_move(event) {
        with_error_handling(function() {
            if (wasm_client().is_chalk_active()) {
                const coord = viewbox_mouse_position(event);
                wasm_client().chalk_move(coord.x, coord.y, event.shiftKey);
            }
        });
    }

    function mouse_up(event) {
        with_error_handling(function() {
            if (wasm_client().is_chalk_active() && is_draw_button(event)) {
                const coord = viewbox_mouse_position(event);
                wasm_client().chalk_up(coord.x, coord.y, event.shiftKey);
            }
        });
    }

    function mouse_leave(event) {
        // Improvement potential: Don't abort drawing if the user temporarily moved the
        //   mouse outside the board.
        with_error_handling(function() {
            if (wasm_client().is_chalk_active()) {
                wasm_client().chalk_abort();
            }
        });
    }

    function mouse_click(event) {
        with_error_handling(function() {
            if (is_cancel_button(event)) {
                if (event.shiftKey) {
                    wasm_client().chalk_clear(event.currentTarget.id);
                } else {
                    wasm_client().chalk_remove_last(event.currentTarget.id);
                }
            }
        });
    }

    for (const board of ['primary', 'secondary']) {
        // Improvement potential. Support chalk on touch screens.
        const svg = document.getElementById(`board-${board}`);
        svg.addEventListener('mousedown', mouse_down);
        svg.addEventListener('mousemove', mouse_move);
        svg.addEventListener('mouseup', mouse_up);
        svg.addEventListener('mouseleave', mouse_leave);
        svg.addEventListener('click', mouse_click);
        svg.addEventListener('contextmenu', (event) => event.preventDefault());
    }
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

    const menu = document.getElementById('menu-dialog');
    menu.addEventListener('mousedown', mouse_down);
    menu.addEventListener('contextmenu', context_menu);
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
    const contest_id = search_params.get(SearchParams.contest_id);
    if (contest_id) {
        jc_contest_id.value = contest_id;
        push_menu_page(menu_join_contest_page);
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
function simple_dialog(message, buttons) {
    return new Promise(resolve => {
        const dialog = document.createElement('dialog');
        document.body.appendChild(dialog);
        const message_node = document.createElement('div');
        message_node.className = 'simple-dialog-message';
        message_node.textContent = message;
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
            if (can_hide) {
                resolve(MyButton.HIDE);
            } else {
                event.preventDefault();
            }
        });
        dialog.appendChild(message_node);
        dialog.appendChild(button_box);
        // Delay `showModal`. If it's called directly, then the dialog gets `Enter` key press if it
        // was the trigger, e.g. if the dialog displays an error processing command line instruction.
        setTimeout(() => dialog.showModal());
    });
}

function ignorable_error_dialog(message) {
    return simple_dialog(message, [new MyButton('Ok', MyButton.HIDE)]);
}

function fatal_error_dialog(message) {
    return simple_dialog(message);
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
    window.open(url, '_blank').focus();
}

function update_session() {
    reset_menu();
    const session = wasm_client().session();
    let is_registered_user = null;
    let is_guest = null;
    let user_name = null;
    switch (session.status) {
        case 'unknown':
        case 'google_oauth_registering': {
            is_registered_user = false;
            is_guest = false;
            user_name = '...';
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
    logged_in_user_bar.style.display = is_registered_user ? null : 'None';
    guest_user_bar.style.display = !is_registered_user ? null : 'None';
    guest_user_tooltip.style.display = is_guest ? null : 'None';
    for (const node of document.querySelectorAll('.logged-in-as-account')) {
        node.classList.toggle('account-user', is_registered_user);
        node.classList.toggle('account-guest', is_guest);
        node.textContent = user_name;
    }
    for (const node of document.querySelectorAll('.guest-player-name')) {
        node.style.display = is_guest ? null : 'None';
        node.disabled = !is_guest;
    }
    if (session.status == 'google_oauth_registering') {
        signup_with_google_email.textContent = session.email;
        push_menu_page(menu_signup_with_google_page);
    }
}

// Encodes `FormData` as application/x-www-form-urlencoded (the default is multipart/form-data).
function as_x_www_form_urlencoded(form_data) {
    return new URLSearchParams(form_data);
}

async function process_authentification_request(request) {
    // TODO: Loading animation.
    let response;
    try {
        response = await fetch(request);
    } catch (e) {
        await ignorable_error_dialog(`Network error: ${e}`);
        return;
    }
    if (response.ok) {
        // Emulate a navigation to indicate that the form has been submitted to password managers:
        // https://www.chromium.org/developers/design-documents/create-amazing-password-forms/#make-sure-form-submission-is-clear
        window.history.replaceState({});
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
        const ret = await simple_dialog(
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

function sign_with_google(event) {
    location.href = '/auth/sign-with-google';
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

function on_create_contest_submenu(event) {
    push_menu_page(menu_create_contest_page);
}

function on_join_contest_submenu(event) {
    push_menu_page(menu_join_contest_page);
}

function on_create_contest_confirm(event) {
    with_error_handling(function() {
        const data = new FormData(event.target);
        wasm_client().new_contest(
            data.get('player_name'),
            data.get('teaming'),
            data.get('starting_position'),
            data.get('chess_variant'),
            data.get('fairy_pieces'),
            data.get('starting_time'),
            data.get('drop_aggression'),
            data.get('pawn_drop_ranks'),
            data.get('rating'),
        );
        update();
    });
}

function on_join_contest_confirm(event) {
    with_error_handling(function() {
        const data = new FormData(event.target);
        wasm_client().join(
            data.get('contest_id').toUpperCase(),
            data.get('player_name'),
        );
        update();
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

// TODO: Is it possible to set a static favicon in a way that is recognized by webpack?
function set_favicon() {
    var link = document.querySelector("link[rel~='icon']");
    if (!link) {
        link = document.createElement('link');
        link.rel = 'icon';
        document.head.appendChild(link);
    }
    link.href = favicon;
}
