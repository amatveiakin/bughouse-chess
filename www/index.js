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
import white_king from '../assets/pieces/white-king.png';
import black_pawn from '../assets/pieces/black-pawn.png';
import black_knight from '../assets/pieces/black-knight.png';
import black_bishop from '../assets/pieces/black-bishop.png';
import black_rook from '../assets/pieces/black-rook.png';
import black_queen from '../assets/pieces/black-queen.png';
import black_king from '../assets/pieces/black-king.png';

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
    player_name: 'player-name',
};

const SearchParams = {
    contest_id: 'contest-id',
    server: 'server',
};

const git_version = document.getElementById('git-version');
const info_string = document.getElementById('info-string');

const menu_dialog = document.getElementById('menu-dialog');
const menu_start_page = document.getElementById('menu-start-page');
const menu_create_contest_page = document.getElementById('menu-create-contest-page');
const menu_join_contest_page = document.getElementById('menu-join-contest-page');
const menu_pages = document.getElementsByClassName('menu-page');

const create_contest_button = document.getElementById('create-contest-button');
const join_contest_button = document.getElementById('join-contest-button');
const cc_back_button = document.getElementById('cc-back-button');
const cc_player_name = document.getElementById('cc-player-name');
const jc_back_button = document.getElementById('jc-back-button');
const jc_player_name = document.getElementById('jc-player-name');
const jc_contest_id = document.getElementById('jc-contest-id');

const ready_button = document.getElementById('ready-button');

const svg_defs = document.getElementById('svg-defs');

const loading_status = new class {
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
            info_string.innerText = '';
        } else {
            const connection_string = this.#connected ? 'Connected' : 'Connecting...';
            const resource_string = resources_ready
                ? 'Resources loaded'
                : `Loading resources... ${this.#resources_loaded} / ${this.#resources_required}`;
            info_string.innerText = `${connection_string}\n${resource_string}`;
        }
    }
};

set_favicon();

load_piece_images([
    [ white_pawn, 'white-pawn' ],
    [ white_knight, 'white-knight' ],
    [ white_bishop, 'white-bishop' ],
    [ white_rook, 'white-rook' ],
    [ white_queen, 'white-queen' ],
    [ white_king, 'white-king' ],
    [ black_pawn, 'black-pawn' ],
    [ black_knight, 'black-knight' ],
    [ black_bishop, 'black-bishop' ],
    [ black_rook, 'black-rook' ],
    [ black_queen, 'black-queen' ],
    [ black_king, 'black-king' ],
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

let wasm_client_object = make_wasm_client();
let wasm_client_panicked = false;
let socket = make_socket();

// Parameters and data structures for the audio logic. Our goal is to make short and
// important sounds (like turn sound) as clear as possible when several events occur
// simultaneously. The main example is when you make a move and immediately get a
// premove back.
const audio_min_interval_ms = 70;
const audio_max_queue_size = 5;
let audio_last_played = 0;
let audio_queue = [];
let audio_volume = 1.0;
let audio_muted = false;

let drag_element = null;

const Meter = make_meters();

document.addEventListener('keydown', on_document_keydown);
document.addEventListener('paste', on_paste);

const command_input = document.getElementById('command');
command_input.addEventListener('keydown', on_command_keydown);

ready_button.addEventListener('click', function() { execute_command('/ready'); });
menu_dialog.addEventListener('cancel', function(event) { event.preventDefault(); });
create_contest_button.addEventListener('click', on_create_contest_submenu);
join_contest_button.addEventListener('click', on_join_contest_submenu);
cc_back_button.addEventListener('click', show_start_page);
jc_back_button.addEventListener('click', show_start_page);
menu_create_contest_page.addEventListener('submit', on_create_contest_confirm);
menu_join_contest_page.addEventListener('submit', on_join_contest_confirm);

let on_tick_interval_id = setInterval(on_tick, 100);


function with_error_handling(f) {
    // Note. Re-throw all unexpected errors to get a stacktrace.
    try {
        f();
    } catch (e) {
        if (e instanceof WasmClientDoesNotExist) {
            const msg = 'Not connected';
            info_string.innerText = msg;
            throw msg;
        } else if (e instanceof WasmClientPanicked) {
            const msg = 'The client is dead. Please reload the page.';
            info_string.innerText = msg;
            throw msg;
        } else if (e instanceof InvalidCommand) {
            info_string.innerText = e.msg;
        } else if (e?.constructor?.name == 'RustError') {
            const msg = `Internal Rust error: ${e.message()}`;
            info_string.innerText = msg;
            if (socket.readyState == WebSocket.OPEN) {
                socket.send(wasm.make_rust_error_event(e));
            }
            throw msg;
        } else {
            const rust_panic = wasm.last_panic();
            if (rust_panic) {
                wasm_client_panicked = true;
                let reported = '';
                if (socket.readyState == WebSocket.OPEN) {
                    socket.send(rust_panic);
                    reported = 'The error has been reported (unless that failed too).';
                } else {
                    reported = 'The error has NOT been reported: not connected to server.';
                }
                info_string.innerText =
                    'Internal error! This client is now dead 💀 ' +
                    'Only refreshing the page may help you. We are very sorry. ' +
                    reported;
                if (on_tick_interval_id != null) {
                    clearInterval(on_tick_interval_id);
                    on_tick_interval_id = null;
                }
            } else if (e.name === 'InvalidStateError' && socket.readyState == WebSocket.CONNECTING) {
                info_string.innerText = 'Still connecting to the server... Please try again later.';
            } else {
                console.log(log_time(), 'Unknown error: ', e);
                const msg = `Unknown error: ${e}`;
                info_string.innerText = msg;
                if (socket.readyState == WebSocket.OPEN) {
                    // Improvement potential. Include stack trace.
                    socket.send(wasm.make_unknown_error_event(e.toString()));
                }
                throw msg;
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
        loading_status.connected();
    });
    // addEventListener('error', (event) => { })  // TODO: report socket errors
    return socket;
}

function on_server_event(event) {
    with_error_handling(function() {
        console.log(log_time(), 'server: ', event);
        wasm_client().process_server_event(event);
        // TODO: Avoid full update on heartbeat.
        update();
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
            show_start_page();
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
        info_string.innerText = '';
        if (input.startsWith('/')) {
            const args = input.slice(1).split(/\s+/);
            switch (args[0]) {
                case 'team': {
                    const [team] = get_args(args, ['blue:red']);
                    wasm_client().set_team(team);
                    break;
                }
                case 'sound': {
                    // TODO: Save settings to a local storage.
                    const expected_args = ['on:off:0:1:...:100'];
                    const [value] = get_args(args, expected_args);
                    switch (value) {
                        case 'on': { audio_muted = false; break; }
                        case 'off': { audio_muted = true; break; }
                        default: {
                            // Improvement potential: Stricter integer parse.
                            let volume = parseInt(value);
                            if (isNaN(volume) || volume < 0 || volume > 100) {
                                throw usage_error(args, expected_args);
                            }
                            audio_muted = false;
                            audio_volume = volume / 100.0;
                            break;
                        }
                    }
                    info_string.innerText = 'Applied';
                    break;
                }
                case 'undo':
                    get_args(args, []);
                    wasm_client().cancel_preturn();
                    break;
                case 'resign':
                    get_args(args, []);
                    wasm_client().resign();
                    break;
                case 'ready':
                    get_args(args, []);
                    wasm_client().toggle_ready();
                    break;
                case 'leave':
                    get_args(args, []);
                    wasm_client().leave();
                    break;
                case 'save':
                    get_args(args, []);
                    wasm_client().request_export();
                    break;
                case 'perf':
                    get_args(args, []);
                    info_string.innerText = wasm_client().meter_stats();
                    break;
                default:
                    throw new InvalidCommand(`Command does not exist: /${args[0]}`);
            }
        } else {
            wasm_client().make_turn_algebraic(input);
        }
        update();
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
    });
}

function process_outgoing_events() {
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
        if (js_event_type == 'JsEventMyNoop') {
            // noop, but are events might be coming
        } else if (js_event_type == 'JsEventContestStarted') {
            const url = new URL(window.location);
            url.searchParams.set(SearchParams.contest_id, js_event.contest_id());
            window.history.pushState({}, '', url);
        } else if (js_event_type == 'JsEventVictory') {
            play_audio(Sound.victory);
        } else if (js_event_type == 'JsEventDefeat') {
            play_audio(Sound.defeat);
        } else if (js_event_type == 'JsEventDraw') {
            play_audio(Sound.draw);
        } else if (js_event_type == 'JsEventTurnMade') {
            play_audio(Sound.turn);
        } else if (js_event_type == 'JsEventMyReserveRestocked') {
            play_audio(Sound.reserve_restocked);
        } else if (js_event_type == 'JsEventLowTime') {
            play_audio(Sound.low_time);
        } else if (js_event_type == 'JsEventGameExportReady') {
            download(js_event.content(), 'game.pgn');
        } else if (js_event_type != null) {
            throw 'Unexpected notable event: ' + js_event.toString();
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
    // Note. One would think that the new and shiny pointer events
    // (https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events) are the
    // answer to supporting both mouse and touch events in a uniform fashion.
    // Unfortunately pointer events don't work here for two reasons:
    //   - It seems impossible to implement drag cancellation with a right-click,
    //     because pointer API does not report nested mouse events.
    //   - The `drag_element.id = null; svg.appendChild(drag_element);` trick
    ///    starts to break `touch-action: none`, i.e. the drag gets cancelled and
    //     the page is panned instead. This is really quite bizarre: the same code
    //     works differently depending on whether it was called from 'touchstart'
    //     or from 'pointerdown'. But what can you do?

    document.addEventListener('mousedown', start_drag);
    document.addEventListener('mousemove', drag);
    document.addEventListener('mouseup', end_drag);
    document.addEventListener('mouseleave', end_drag);

    document.addEventListener('touchstart', start_drag);
    document.addEventListener('touchmove', drag);
    document.addEventListener('touchend', end_drag);
    document.addEventListener('touchcancel', end_drag);

    const svg = document.getElementById('board-primary');
    svg.addEventListener('contextmenu', cancel_preturn);
    document.addEventListener('contextmenu', cancel_drag);

    function is_main_pointer(event) {
        return event.button == 0 || event.changedTouches?.length >= 1;
    }

    function mouse_position_relative_to_board(event) {
        const ctm = svg.getScreenCTM();
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
                // Bring on top; (if reserve) remove shadow by extracting from reserve group:
                svg.appendChild(drag_element);

                const source = drag_element.getAttribute('data-bughouse-location');
                wasm_client().start_drag_piece(source);
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
                wasm_client().drag_piece_drop(coord.x, coord.y, event.shiftKey);
                update();
            }
        });
    }

    function cancel_preturn(event) {
        with_error_handling(function() {
            event.preventDefault();
            if (!drag_element) {
                wasm_client().cancel_preturn();
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
                wasm_client().chalk_down(event.currentTarget.id, coord.x, coord.y, event.shiftKey);
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
        svg.addEventListener('contextmenu', function(event) { event.preventDefault(); });
    }
}

function on_hide_menu_page(page) {
    if (page === menu_create_contest_page) {
        window.localStorage.setItem(Storage.player_name, cc_player_name.value);
    } else if (page === menu_join_contest_page) {
        window.localStorage.setItem(Storage.player_name, jc_player_name.value);
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

function show_menu_page(page) {
    hide_menu_pages();
    page.style.display = 'block';
}

function close_menu() {
    hide_menu_pages();  // hide the pages to execute "on hide" handlers
    menu_dialog.close();
}

function show_start_page() {
    show_menu_page(menu_start_page);
}

function init_menu() {
    const search_params = new URLSearchParams(window.location.search);
    const contest_id = search_params.get(SearchParams.contest_id);
    hide_menu_pages(false);
    menu_dialog.showModal();
    if (contest_id) {
        show_menu_page(menu_join_contest_page);
        jc_contest_id.value = contest_id;
        jc_player_name.value = window.localStorage.getItem(Storage.player_name);
        jc_player_name.focus();
    } else {
        show_menu_page(menu_start_page);
    }
}

function make_piece_image(symbol_id) {
    const SVG_NS = 'http://www.w3.org/2000/svg';
    const symbol = document.createElementNS(SVG_NS, 'symbol');
    symbol.id = symbol_id;
    const image = document.createElementNS(SVG_NS, 'image');
    image.id = `${symbol_id}-image`;
    image.setAttribute('width', '1');
    image.setAttribute('height', '1');
    symbol.appendChild(image);
    svg_defs.appendChild(symbol);
}

async function load_image(filepath, target_id) {
    const reader = new FileReader();
    reader.addEventListener('load', () => {
        const image = document.getElementById(target_id);
        image.setAttribute('href', reader.result);
        loading_status.resource_loaded();
    }, false);
    reader.addEventListener('error', () => {
        console.error(`Cannot load image ${filepath}`);
    });
    const response = await fetch(filepath);
    const blob = await response.blob();
    reader.readAsDataURL(blob);
}

function load_piece_images(image_records) {
    for (const record of image_records) {
        const [filepath, symbol_id] = record;
        const image_id = `${symbol_id}-image`;
        make_piece_image(symbol_id);
        load_image(filepath, image_id);
        loading_status.resource_required();
    }
}

async function load_sound(filepath, key) {
    const reader = new FileReader();
    reader.addEventListener('load', () => {
        const audio = Sound[key];
        audio.setAttribute('src', reader.result);
        loading_status.resource_loaded();
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
        loading_status.resource_required();
    }
    return ret;
}

function on_create_contest_submenu(event) {
    show_menu_page(menu_create_contest_page);
    cc_player_name.value = window.localStorage.getItem(Storage.player_name);
    cc_player_name.focus();
}

function on_join_contest_submenu(event) {
    show_menu_page(menu_join_contest_page);
    jc_player_name.value = window.localStorage.getItem(Storage.player_name);
    jc_contest_id.focus();
}

function on_create_contest_confirm(event) {
    with_error_handling(function() {
        const data = new FormData(event.target);
        wasm_client().new_contest(
            data.get('player-name'),
            data.get('teaming'),
            data.get('starting-position'),
            data.get('starting-time'),
            data.get('drop-aggression'),
            data.get('pawn-drop-rows'),
            data.get('rated') == "on",
        );
        update();
        close_menu();
    });
}

function on_join_contest_confirm(event) {
    with_error_handling(function() {
        const data = new FormData(event.target);
        wasm_client().join(
            data.get('contest-id').toUpperCase(),
            data.get('player-name'),
        );
        update();
        close_menu();
    });
}

function play_audio(audio) {
    if (audio_queue.length >= audio_max_queue_size) {
        return;
    }
    audio_queue.push(audio);
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
    let audio = audio_queue.shift();
    if (!audio_muted) {
        // Clone node to allow playing overlapping instances of the same sound.
        let auto_clone = audio.cloneNode();
        auto_clone.volume = audio_volume;
        auto_clone.play();
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
