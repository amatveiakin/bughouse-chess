// TODO: Remove logging
// TODO: Check if ==/!= have to be replaced with ===/!== and other JS weirdness.

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


class WasmClientDoesNotExist {}
class WasmClientPanicked {}
class InvalidCommand { constructor(msg) { this.msg = msg; } }


set_favicon();

// The additional logic for turn sounds allows to play distinct sounds for every turn
// in case of fast turns. This is mainly valuable for premoves.
const turn_audio = new Audio(turn_sound);
const turn_audio_min_interval_ms = 70;
const turn_audio_max_queue_size = 3;
let turn_audio_last_played = null;
let turn_audio_queue_size = 0;

wasm.set_panic_hook();
wasm.init_page(
    white_pawn, white_knight, white_bishop, white_rook, white_queen, white_king,
    black_pawn, black_knight, black_bishop, black_rook, black_queen, black_king
);
set_up_drag_and_drop();

const coords = [];
for (const row of ['1', '2', '3', '4', '5', '6', '7', '8']) {
    for (const col of ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']) {
        coords.push(`${col}${row}`);
    }
}

let wasm_client_object = null;
let wasm_client_panicked = false;
let socket = null;
let socket_incoming_listener = null;
let on_tick_interval_id = null;

let drag_element = null;

const info_string = document.getElementById('info-string');
info_string.innerText = 'Type "/join name team" to start'

const command_input = document.getElementById('command');
command_input.addEventListener('change', on_command);

const ready_button = document.getElementById('ready-button');
ready_button.addEventListener('click', function() { execute_command('/ready') });

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
            if (socket) {
                socket.send(wasm.make_rust_error_event(e));
            }
            throw msg;
        } else {
            const rust_panic = wasm.last_panic();
            if (rust_panic) {
                wasm_client_panicked = true;
                let reported = '';
                if (socket) {
                    socket.send(rust_panic);
                    reported = 'The error has been reported (unless that failed too).';
                } else {
                    reported = 'The error has NOT been reported: not connected to server.';
                }
                info_string.innerText =
                    'Internal error! This client is now dead 💀 ' +
                    'Only refreshing the page may help you. We are very sorry. ' +
                    reported;
                shutdown_wasm_client();
            } else {
                console.log('Unknown error: ', e);
                const msg = `Unknown error: ${e}`;
                info_string.innerText = msg;
                if (socket) {
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

function shutdown_wasm_client() {
    if (socket != null) {
        socket.removeEventListener('message', socket_incoming_listener);
        socket.close();
    }
    socket = null;
    socket_incoming_listener = null;
    wasm_client_object = null;
    if (on_tick_interval_id != null) {
        clearInterval(on_tick_interval_id);
        on_tick_interval_id = null;
    }
}

function on_server_event(event) {
    with_error_handling(function() {
        console.log('server: ', event);
        const js_event = wasm_client().process_server_event(event);
        const js_event_type = js_event?.constructor?.name;
        if (js_event_type == 'JsEventOpponentTurnMade') {
            play_turn_audio();
        } else if (js_event_type == 'JsEventGameExportReady') {
            download(js_event.content(), 'game.pgn');
        } else if (js_event_type != null) {
            throw 'Unexpected reaction to a server event: ' + js_event.toString();
        }
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

function get_join_args(args_array) {
    const args_without_command_name = args_array.slice(1);
    if (args_without_command_name.length === 1) {
        return args_without_command_name.concat([""]);
    } else if (args_without_command_name.length === 2) {
        return args_without_command_name;
    } else {
        throw new InvalidCommand(`Usage: /join name [team]`);
    }
}

function on_command(event) {
    const input = String(event.target.value)
    event.target.value = '';
    execute_command(input);
}

function execute_command(input) {
    with_error_handling(function() {
        info_string.innerText = '';
        if (input.startsWith('/')) {
            const args = input.slice(1).split(/\s+/);
            switch (args[0]) {
                case 'local': {
                    const [name, team] = get_join_args(args);
                    request_join('localhost', name, team);
                    break;
                }
                case 'join': {
                    const [name, team] = get_join_args(args);
                    request_join('51.250.84.85', name, team);
                    break;
                }
                case 'sound': {
                    const expected_args = ['on:off:0:1:...:100'];
                    const [value] = get_args(args, expected_args);
                    switch (value) {
                        case 'on': turn_audio.muted = false; break;
                        case 'off': turn_audio.muted = true; break;
                        default: {
                            // Improvement potential: Stricter integer parse.
                            let volume = parseInt(value);
                            if (isNaN(volume) || volume < 0 || volume > 100) {
                                throw usage_error(args, expected_args);
                            }
                            turn_audio.muted = false;
                            turn_audio.volume = volume / 100.0;
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
                case 'reset':
                    get_args(args, []);
                    wasm_client().reset();
                    break;
                case 'save':
                    get_args(args, []);
                    wasm_client().request_export();
                    break;
                default:
                    throw new InvalidCommand(`Command does not exist: /${args[0]}`)
            }
        } else {
            if (wasm_client().make_turn_algebraic(input)) {
                play_turn_audio();
            }
        }
        update();
    });
}

function on_tick() {
    with_error_handling(function() {
        wasm_client().update_clock();
    });
}

function update() {
    with_error_handling(function() {
        while (true) {
            let event = wasm_client().next_outgoing_event();
            if (event == null) {
                break;
            } else {
                console.log('sending: ', event);
                socket.send(event);
            }
        }
        wasm_client().update_state();
        const drag_state = wasm_client().drag_state();
        switch (drag_state) {
            case 'no':
                if (drag_element) {
                    drag_element.remove();
                    drag_element = null;
                }
                break;
            case 'yes':
                console.assert(drag_element != null);
                break;
            case 'defunct':
                // Improvement potential: Better image (broken piece / add red cross).
                drag_element.setAttribute('opacity', 0.5);
                break;
            default:
                console.error(`Unknown drag_state: ${drag_state}`);
        }
    });
}

function on_socket_opened() {
    with_error_handling(function() {
        wasm_client().join();
        on_tick_interval_id = setInterval(on_tick, 100);
        update();
    });
}

function request_join(address, my_name, my_team) {
    with_error_handling(function() {
        shutdown_wasm_client();
        socket = new WebSocket(`ws://${address}:38617`);  // TODO: get the port from Rust
        wasm_client_object = wasm.WebClient.new_client(my_name, my_team);
        info_string.innerText = 'Joining...';
        socket_incoming_listener = socket.addEventListener('message', function(event) {
            on_server_event(event.data);
        });
        socket.addEventListener('open', function(event) {
            on_socket_opened();
        });
    });
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
                if (wasm_client().drag_piece_drop(coord.x, coord.y, event.shiftKey)) {
                    play_turn_audio();
                }
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

function play_turn_audio() {
    const now = performance.now();
    const turn_audio_next_avaiable = turn_audio_last_played + turn_audio_min_interval_ms;
    if (turn_audio_queue_size > 0) {
        turn_audio_queue_size = Math.min(turn_audio_queue_size + 1, turn_audio_max_queue_size);
    } else if (now < turn_audio_next_avaiable) {
        turn_audio_queue_size = 1;
        setTimeout(play_turn_audio_delayed, turn_audio_next_avaiable - now);
    } else {
        play_turn_audio_impl();
    }
}

function play_turn_audio_delayed() {
    play_turn_audio_impl();
    turn_audio_queue_size -= 1;
    if (turn_audio_queue_size > 0) {
        setTimeout(play_turn_audio_delayed, turn_audio_min_interval_ms);
    }
}

function play_turn_audio_impl() {
    turn_audio.cloneNode().play();  // clone node to allow two overlapping sounds
    turn_audio_last_played = performance.now();
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

// TODO: Is it possible to set a static favicon in a way that is recognized  by webpack?
function set_favicon() {
    var link = document.querySelector("link[rel~='icon']");
    if (!link) {
        link = document.createElement('link');
        link.rel = 'icon';
        document.head.appendChild(link);
    }
    link.href = favicon;
}
