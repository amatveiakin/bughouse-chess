// TODO: Remove logging
// TODO: Scale down images
// TODO: Check if ==/!= have to be replaced with ===/!== and other JS weirdness.

import './main.css'
import * as wasm from 'bughouse-chess';

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


wasm.set_panic_hook();
wasm.init_page(
    white_pawn, white_knight, white_bishop, white_rook, white_queen, white_king,
    black_pawn, black_knight, black_bishop, black_rook, black_queen, black_king
);

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

// request_join('localhost', 'p1', 'red');  // uncomment to speed up debugging; TODO: Delete


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
    if (wasm_client != null) {
        console.log('server: ', event);
        try {
            const what_happened = wasm_client.process_server_event(event);
            if (what_happened == "game_started") {
                setup_drag_and_drop();
            } else if (what_happened != null) {
                console.error('Something unexpected happened: ', what_happened);
            }
        } catch (error) {
            console.warn('Error processing event from server: ', error);
            info_string.innerText = error;
            shutdown_wasm_client();
        }
        update();
    } else {
        console.warn('WASM client missing; could not process server event: ', event);
    }
}

function on_command(event) {
    const input = String(event.target.value)
    console.log('user: ', input);
    if (input.startsWith('/')) {
        const args = input.slice(1).split(/\s+/);
        switch (args[0]) {
            case 'local':
                console.assert(args.length == 3);
                request_join('localhost', args[1], args[2]);
                break;
            case 'join':
                console.assert(args.length == 3);
                request_join('51.250.84.85', args[1], args[2]);
                break;
            case 'resign':
                // TODO: Consistent policy for checking when wasm_client exists.
                wasm_client.resign();
                break;
            case 'leave':
                wasm_client.leave();
                break;
        }
    } else {
        if (wasm_client) {
            wasm_client.make_turn(input);
        }
    }
    event.target.value = '';
    update();
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
    setInterval(on_tick, 100);
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
            const from = `${piece_kind}@`;
            e.dataTransfer.setData('application/bughouse-move-from', from);
            e.dataTransfer.effectAllowed = 'move';
        });
    }
}

function setup_drag_and_drop() {
    for (const coord of coords) {
        const element = document.getElementById(`primary-${coord}`);
        element.addEventListener('dragstart', function(e) {
            const piece_kind = this.getAttribute('data-piece-kind');
            const from = `${piece_kind}${coord}`;
            e.dataTransfer.setData('application/bughouse-move-from', from);
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
            // TODO: Proper API
            // TODO: Promotions, castling, drops
            if (wasm_client) {
                wasm_client.make_turn(`${from}${to}`);
            }
            update();
        });
    }
}
