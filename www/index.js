// TODO: Remove logging
// TODO: Check if ==/!= have to be replaced with ===/!== and other JS weirdness.
// TODO: Figure out if it's possible to enable strict mode with webpack.

import './main.css'
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

set_favicon();

// Improvement potential. Establish priority on sounds; play more important sounds first
// in case of a clash.
const victory_audio = new Audio(victory_sound);
const defeat_audio = new Audio(defeat_sound);
const draw_audio = new Audio(draw_sound);
const turn_audio = new Audio(turn_sound);
const reserve_restocked_audio = new Audio(reserve_restocked_sound);
const low_time_audio = new Audio(low_time_sound);

const my_search_params = new URLSearchParams(window.location.search);

const info_string = document.getElementById('info-string');

wasm.set_panic_hook();
wasm.init_page(
    white_pawn, white_knight, white_bishop, white_rook, white_queen, white_king,
    black_pawn, black_knight, black_bishop, black_rook, black_queen, black_king
);
set_up_drag_and_drop();

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

let process_outgoing_events_meter = null;
let process_notable_events_meter = null;
let refresh_meter = null;
let update_state_meter = null;
let update_clock_meter = null;
let update_drag_state_meter = null;
init_meters();

document.addEventListener('keydown', on_document_keydown);
document.addEventListener('paste', function(event) { command_input.focus(); });

const command_input = document.getElementById('command');
command_input.addEventListener('keydown', on_command_keydown);

const ready_button = document.getElementById('ready-button');
ready_button.addEventListener('click', function() { execute_command('/ready') });

setInterval(on_tick, 100);


function with_error_handling(f) {
    // Note. Re-throw all unexpected errors to get a stacktrace.
    try {
        f()
    } catch (e) {
        if (e instanceof WasmClientDoesNotExist) {
            const msg = 'Not connected'
            info_string.innerText = msg;
            throw msg;
        } else if (e instanceof WasmClientPanicked) {
            const msg = 'The client is dead. Please reload the page.'
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

function init_meters() {
    process_outgoing_events_meter = wasm_client().meter("process_outgoing_events");
    process_notable_events_meter = wasm_client().meter("process_notable_events");
    refresh_meter = wasm_client().meter("refresh");
    update_state_meter = wasm_client().meter("update_state");
    update_clock_meter = wasm_client().meter("update_clock");
    update_drag_state_meter = wasm_client().meter("update_drag_state");
}

function make_socket() {
    const socket = new WebSocket(server_websocket_address());
    socket.addEventListener('message', function(event) {
        on_server_event(event.data);
    });
    socket.addEventListener('open', function(event) {
        info_string.innerText = 'Use /new to create contest or /join to join';
    });
    // addEventListener('error', (event) => { })  // TODO: report socket errors
    info_string.innerText = 'Connecting...';
    return socket
}

function on_server_event(event) {
    with_error_handling(function() {
        console.log(log_time(), 'server: ', event);
        wasm_client().process_server_event(event);
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
    let isPrintableKey = event.key.length === 1;  // https://stackoverflow.com/a/38802011/3092679
    if (isPrintableKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
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
                case 'new': {
                    const [name] = get_args(args, ['name']);
                    wasm_client().new_contest(name);
                    break;
                }
                case 'join': {
                    const [contest_id, name] = get_args(args, ['contest_id', 'name']);
                    wasm_client().join(contest_id, name);
                    break;
                }
                case 'sound': {
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
                    throw new InvalidCommand(`Command does not exist: /${args[0]}`)
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
        timer.meter(refresh_meter);
        wasm_client().update_clock();
        timer.meter(update_clock_meter);
        process_notable_events();
        timer.meter(process_notable_events_meter);
    });
}

function update() {
    with_error_handling(function() {
        const timer = new Timer();
        process_outgoing_events();
        timer.meter(process_outgoing_events_meter);
        wasm_client().refresh();
        timer.meter(refresh_meter);
        wasm_client().update_state();
        timer.meter(update_state_meter);
        process_notable_events();
        timer.meter(process_notable_events_meter);
        update_drag_state();
        timer.meter(update_drag_state_meter);
    });
}

function process_outgoing_events() {
    let event;
    while (event = wasm_client().next_outgoing_event()) {
        console.log(log_time(), 'sending: ', event);
        socket.send(event);
    }
}

function process_notable_events() {
    let js_event;
    while (js_event = wasm_client().next_notable_event()) {
        const js_event_type = js_event?.constructor?.name;
        if (js_event_type == 'JsEventMyNoop') {
            // noop, but are events might be coming
        } else if (js_event_type == 'JsEventGotContestId') {
            // TODO: Modify search param (via URLSearchParams.set + window.history.pushState)
            //   and use it to autoconnect.
        } else if (js_event_type == 'JsEventVictory') {
            play_audio(victory_audio);
        } else if (js_event_type == 'JsEventDefeat') {
            play_audio(defeat_audio);
        } else if (js_event_type == 'JsEventDraw') {
            play_audio(draw_audio);
        } else if (js_event_type == 'JsEventTurnMade') {
            play_audio(turn_audio);
        } else if (js_event_type == 'JsEventMyReserveRestocked') {
            play_audio(reserve_restocked_audio);
        } else if (js_event_type == 'JsEventLowTime') {
            play_audio(low_time_audio);
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
    // TODO: Get the port from Rust.
    const DEFAULT_PORT = 38617;
    const DEFAULT_ADDRESS = 'bughouse.pro';
    let address = my_search_params.get('server') ?? DEFAULT_ADDRESS;
    if (!address.includes('://')) {
        address = `ws://${address}`;
    }
    const url = new URL(address);
    url.protocol = 'ws:';
    // TODO: Fix: `URL` automatically strips port 80, so it is ignored.
    if (!url.port) {
        url.port = DEFAULT_PORT;
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

    function is_main_pointer(event) {
        return event.button == 0 || event.changedTouches?.length >= 1;
    }

    function viewbox_mouse_position(event) {
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
                const coord = viewbox_mouse_position(event);
                drag_element.setAttribute('x', coord.x - 0.5);
                drag_element.setAttribute('y', coord.y - 0.5);
                wasm_client().drag_piece(coord.x, coord.y);
            }
        });
    }

    function end_drag(event) {
        with_error_handling(function() {
            if (drag_element && is_main_pointer(event)) {
                const coord = viewbox_mouse_position(event);
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
            if (drag_element) {
                wasm_client().abort_drag_piece();
            } else {
                wasm_client().cancel_preturn();
            }
            update();
        });
    }
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
