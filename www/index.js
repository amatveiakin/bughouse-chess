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

const info_string = document.getElementById('info-string');
info_string.innerText = 'Type "/join name team" to start'

const command_input = document.getElementById('command');
command_input.addEventListener('change', on_command);

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
            const what_happened = wasm_client.process_server_event(event);
            if (what_happened == 'game_started') {
                setup_drag_and_drop();
            } else if (what_happened == 'opponent_turn_made') {
                turn_audio.play();
            } else if (what_happened != null) {
                console.error('Something unexpected happened: ', what_happened);
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
                default:
                    throw new InvalidCommand(`Command does not exist: /${args[0]}`)
            }
        } else {
            wasm_client_or_throw().make_turn_algebraic(input);
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
    setup_drag_for_reserve();
}

function on_socket_opened() {
    wasm_client.join();
    setInterval(on_tick, 100);  // TODO: Should the old `setInterval` be cancelled?
    update();
}

function request_join(address, my_name, my_team) {
    shutdown_wasm_client();
    info_string.innerText = 'Joining...';
    socket = new WebSocket(`ws://${address}:38617`);  // TODO: get the port from Rust
    wasm_client = wasm.WebClient.new_client(my_name, my_team);
    socket_incoming_listener = socket.addEventListener('message', function(event) {
        on_server_event(event.data);
    });
    socket.addEventListener('open', function(event) {
        on_socket_opened();
    });
}

function setup_drag_for_reserve() {
    for (const element of document.getElementsByClassName('reserve-piece-primary')) {
        element.addEventListener('dragstart', function(e) {
            const piece_kind = this.getAttribute('data-piece-kind');
            const from = `reserve-${piece_kind}`;
            e.dataTransfer.setData('application/bughouse-move-from', from);
            e.dataTransfer.effectAllowed = 'move';
        });
    }
}

function setup_drag_and_drop() {
    for (const coord of coords) {
        const element = document.getElementById(`primary-${coord}`);
        element.addEventListener('dragstart', function(e) {
            e.dataTransfer.setData('application/bughouse-move-from', coord);
            e.dataTransfer.effectAllowed = 'move';
            // const img = new Image();
            // img.src = this.getAttribute('data-piece-image');
            // var canvas = document.createElement('canvas');
            // canvas.width = this.clientWidth;
            // canvas.height = this.clientHeight;
            // var ctx = canvas.getContext('2d');
            // ctx.drawImage(img, 0, 0, this.clientWidth, this.clientHeight);
            // img.src = canvas.toDataURL();
            // e.dataTransfer.setDragImage(img, 0, 0);
        });
        element.addEventListener('dragenter', function (e) {
            e.preventDefault();
            e.target.classList.add('dragover');
        });
        element.addEventListener('dragover', function (e) {
            e.preventDefault();
        });
        element.addEventListener('dragleave', function (e) {
            e.preventDefault();
            e.target.classList.remove('dragover');
        });
        element.addEventListener('drop', function (e) {
            e.preventDefault();
            e.target.classList.remove('dragover');
            const from = e.dataTransfer.getData('application/bughouse-move-from');
            const to = coord;
            if (wasm_client) {
                wasm_client.make_turn_drag_drop(from, to, e.shiftKey);
            }
            update();
        });
    }
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
