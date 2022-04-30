// TODO: Remove logging

import * as wasm from "bughouse-chess";


wasm.set_panic_hook();

let wasm_client = null;
let socket = null;

const loading_status = document.getElementById('loading_status');
loading_status.innerText = 'Idle'

const command_input = document.getElementById('command');
command_input.addEventListener('change', on_command);

function on_server_event(event) {
    console.log('server: ', event);
    wasm_client.process_server_event(event);
    update();
}

function on_command(event) {
    const input = String(event.target.value)
    console.log('user: ', input);
    if (input.startsWith('/')) {
        const args = input.slice(1).split(/\s+/);
        switch (args[0]) {
            case 'join':
                console.assert(args.length == 3);
                request_join(args[1], args[2]);
                break;
            case 'resign':
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
    update();
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
    const board = document.getElementById('board');
    board.innerHTML = wasm_client.get_state();
}

function on_socket_opened() {
    loading_status.textContent = 'Ready';
    wasm_client.join();
    setInterval(on_tick, 100);
    update();
}

function request_join(my_name, my_team) {
    loading_status.innerText = 'Joining...';
    socket = new WebSocket('ws://localhost:38617');  // TODO: get the port from Rust
    wasm_client = wasm.WebClient.new_client(my_name, my_team);
    socket.addEventListener('message', function(event) {
        on_server_event(event.data);
    });
    socket.addEventListener('open', function(event) {
        on_socket_opened();
    });
}
