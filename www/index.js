// TODO: Remove logging
// TODO: Scale down images
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


set_favicon();
const turn_audio = new Audio(turn_sound);

wasm.set_panic_hook();
wasm.init_page(
    white_pawn, white_knight, white_bishop, white_rook, white_queen, white_king,
    black_pawn, black_knight, black_bishop, black_rook, black_queen, black_king
);
set_up_drag_and_drop();

function WasmClientDoesNotExist() {}
function InvalidCommand(msg) { this.msg = msg; }

const coords = [];
for (const row of ['1', '2', '3', '4', '5', '6', '7', '8']) {
    for (const col of ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']) {
        coords.push(`${col}${row}`);
    }
}

let wasm_client = null;
let socket = null;
let socket_incoming_listener = null;

let drag_element = null;

const info_string = document.getElementById('info-string');
info_string.innerText = 'Type "/join name team" to start'

const command_input = document.getElementById('command');
command_input.addEventListener('change', on_command);

const next_button = document.getElementById('next-button');
next_button.addEventListener('click', function() { execute_command('/next') });

function wasm_client_or_throw() {
    if (wasm_client) {
        return wasm_client;
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
    wasm_client = null;
}

function on_server_event(event) {
    if (wasm_client) {
        console.log('server: ', event);
        try {
            const js_event = wasm_client.process_server_event(event);
            const js_event_type = js_event?.constructor?.name;
            if (js_event_type == 'JsEventOpponentTurnMade') {
                turn_audio.play();
            } else if (js_event_type == 'JsEventGameExportReady') {
                download(js_event.content(), 'game.pgn');
            } else if (js_event_type != null) {
                console.error('Something unexpected happened: ', js_event);
            }
        } catch (error) {
            console.warn('Error processing event from server: ', error);
            info_string.innerText = error;
        }
        update();
    } else {
        console.warn('WASM client missing; could not process server event: ', event);
    }
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

function on_command(event) {
    const input = String(event.target.value)
    event.target.value = '';
    execute_command(input);
}

function execute_command(input) {
    info_string.innerText = '';
    try {
        if (input.startsWith('/')) {
            const args = input.slice(1).split(/\s+/);
            switch (args[0]) {
                case 'local': {
                    const [name, team] = get_args(args, ['name', 'team']);
                    request_join('localhost', name, team);
                    break;
                }
                case 'join': {
                    const [name, team] = get_args(args, ['name', 'team']);
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
                    wasm_client_or_throw().cancel_preturn();
                    break;
                case 'resign':
                    get_args(args, []);
                    wasm_client_or_throw().resign();
                    break;
                case 'next':
                    get_args(args, []);
                    wasm_client_or_throw().next_game();
                    break;
                case 'leave':
                    get_args(args, []);
                    wasm_client_or_throw().leave();
                    break;
                case 'reset':
                    get_args(args, []);
                    wasm_client_or_throw().reset();
                    break;
                case 'save':
                    const [format] = get_args(args, ['bpgn:pgn-pair']);
                    wasm_client_or_throw().request_export(format);
                    break;
                default:
                    throw new InvalidCommand(`Command does not exist: /${args[0]}`)
            }
        } else {
            if (wasm_client_or_throw().make_turn_algebraic(input)) {
                turn_audio.play();
            }
        }
        update();
    } catch (e) {
        if (e instanceof WasmClientDoesNotExist) {
            info_string.innerText = 'Cannot execute command: not connected';
        } else if (e instanceof InvalidCommand) {
            info_string.innerText = e.msg;
        } else {
            info_string.innerText = `Unknown error: ${e}`;
            throw e;
        }
    }
}

function on_tick() {
    if (wasm_client) {
        wasm_client.update_clock();
    }
}

function update() {
    if (!wasm_client) {
        return;
    }
    while (true) {
        let event = wasm_client.next_outgoing_event();
        if (event == null) {
            break;
        } else {
            console.log('sending: ', event);
            socket.send(event);
        }
    }
    wasm_client.update_state();
    const drag_state = wasm_client.drag_state();
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
}

function on_socket_opened() {
    wasm_client.join();
    setInterval(on_tick, 100);  // TODO: Should the old `setInterval` be cancelled?
    update();
}

function request_join(address, my_name, my_team) {
    shutdown_wasm_client();
    socket = new WebSocket(`ws://${address}:38617`);  // TODO: get the port from Rust
    wasm_client = wasm.WebClient.new_client(my_name, my_team);
    info_string.innerText = 'Joining...';
    socket_incoming_listener = socket.addEventListener('message', function(event) {
        on_server_event(event.data);
    });
    socket.addEventListener('open', function(event) {
        on_socket_opened();
    });
}

function set_up_drag_and_drop() {
    // Improvement potential: Check which mouse button was pressed.
    document.addEventListener('mousedown', start_drag);
    document.addEventListener('mousemove', drag);
    document.addEventListener('mouseup', end_drag);
    document.addEventListener('mouseleave', end_drag);

    document.addEventListener('touchstart', start_drag);
    document.addEventListener('touchmove', drag);
    document.addEventListener('touchend', end_drag);
    document.addEventListener('touchleave', end_drag);
    document.addEventListener('touchcancel', end_drag);

    const svg = document.getElementById('board-primary');
    svg.addEventListener('contextmenu', cancel_preturn);

    function viewbox_mouse_position(event) {
        const ctm = svg.getScreenCTM();
        const src = event.touches ? event.touches[0] : event;
        return {
            x: (src.clientX - ctm.e) / ctm.a,
            y: (src.clientY - ctm.f) / ctm.d,
        };
    }

    function start_drag(event) {
        // Improvement potential. Highlight pieces outside of board area: add shadows separately
        //   and move them to the very back, behing boards.
        // Improvement potential: Choose the closest reserve piece rather then the one on top.
        console.assert(drag_element === null);
        if (event.target.classList.contains('draggable')) {
            event.preventDefault();
            drag_element = event.target;
            drag_element.classList.add('dragged');
            // Dissociate image from the board/reserve:
            drag_element.id = null;
            // Bring on top; (if reserve) remove shadow by extracting from reserve group:
            drag_element.remove();
            svg.appendChild(drag_element);

            const source = drag_element.getAttribute('data-bughouse-location');
            wasm_client.start_drag_piece(source);
            update();
        }
    }

    function drag(event) {
        if (drag_element) {
            event.preventDefault();
            const coord = viewbox_mouse_position(event);
            drag_element.setAttribute('x', coord.x - 0.5);
            drag_element.setAttribute('y', coord.y - 0.5);
            wasm_client.drag_piece(coord.x, coord.y);
        }
    }

    function end_drag(event) {
        if (drag_element) {
            event.preventDefault();
            const coord = viewbox_mouse_position(event);
            drag_element.remove();
            drag_element = null;
            if (wasm_client.drag_piece_drop(coord.x, coord.y, event.shiftKey)) {
                turn_audio.play();
            }
            update();
        }
    }

    function cancel_preturn(event) {
        event.preventDefault();
        wasm_client.cancel_preturn();
        update();
    }
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
        document.getElementsByTagName('head')[0].appendChild(link);
    }
    link.href = favicon;
}
