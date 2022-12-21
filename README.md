# Bughouse chess platform

**This is the backend behind [bughouse.pro](https://bughouse.pro).**

It implements a client and a server for
[bughouse chess](https://en.wikipedia.org/wiki/Bughouse_chess) — the
best<sup>[citation not needed]</sup> kind of chess.

It exists because alternative bughouse implementations use a fixed set of rules
while the rules in fact vary.

Supported rule variations:

- Starting position: classic or Fischer random (a.k.a. Chess960).
- Limits on where pawns can be dropped.
- Limits on drop aggression, e.g. cannot drop piece if that leads to checkmate.

Folder structure:

- `/` — The core library (`bughouse_chess` Rust package).
- `/bughouse_console` — A binary that can run as a server or as console client.
  Note that this is only a game engine server. It does not serve HTML content.
- `/bughouse_wasm` — WASM (WebAssembly) bindings for the web client.
- `/bughouse_webserver` — Dynamic HTML content server.
- `/www` — Web client based on the abovementioned WASM bindings.


## Local setup

Run once:

```
cd www && npm install
```

Build & run server:

```
cargo run --package bughouse_console -- server
```

Run once in the beginning and every time after changing Rust code:

```
cd bughouse_wasm && wasm-pack build
```

Serve web client locally:

```
cd www && npm run start
```

Go to http://localhost:8080/. The client would automatically connect to the
local server.

Changes to css will apply immediately. Changes to html and js will
apply after a page refresh. Changes to Rust code must be recompiled via
`wasm-pack` (see above).


## Full Apache-based server setup

Serve static content:

```
cd bughouse_wasm && wasm-pack build && cd ../www && npm run build
sudo cp dist/* /var/www/<site>
```

Install Apache proxy modules:

```
sudo a2enmod proxy proxy_http proxy_wstunnel
```

Enable request redirection. Add this to `/etc/apache2/sites-available/<site>`:

```
<VirtualHost *:443>
    ProxyPreserveHost On
    ProxyRequests Off
    ProxyPass /dyn http://localhost:14362/dyn
    ProxyPassReverse /dyn http://localhost:14362/dyn
    ProxyPass /ws ws://localhost:14361 keepalive=On
    ProxyPassReverse /ws ws://localhost:14361
</VirtualHost>
```

Run the engine server:

```
export RUST_BACKTRACE=1
export RUST_LOG=INFO
cargo run -r --package bughouse_console -- server --sqlite_db <DB>
```

Run the webserver:

```
export RUST_BACKTRACE=1
export RUST_LOG=INFO
cargo run -r --package bughouse_webserver -- --database-address <DB>
```


## Local console client setup

> **Warning**
> Console client hasn't been updated to the latest protocol version and is
> currently broken.

Build & run server:

```
cargo run --package bughouse_console -- server
```

Build & run client:

```
cargo run --package bughouse_console -- client <server_address> <contest_id> <player_name>
```

Note. Client requires a modern terminal with raw mode support.
Windows 10+ cmd and most Linux terminals should work, while mingw, git bash,
cmd and powershell in older Windows versions may not.

Note. Unicode support in Windows cmd built-in fonts is very poor. You need to
install and activate custom fonts (e.g. DejaVu Sans Mono) in order for chess
pieces to render properly in Windows terminal.
