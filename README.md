# Bughouse chess client/server

Application that works both as a client and as a server for
[bughouse chess](https://en.wikipedia.org/wiki/Bughouse_chess) — the
best<sup>[citation not needed]</sup> kind of chess.

It exists because alternative bughouse implementations use a fixed set of rules
while the rules in fact vary.

Supported rule variations:

- Starting position: classic or Fischer random (a.k.a. Chess960).
- Limits on where pawns can be dropped.
- Limits on drop aggression, e.g. cannot drop piece if that leads to checkmate.

Folder structure:

- `/` — the core library (`bughouse_chess` Rust package).
- `/bughouse_console` — a binary that can run as a server or as console client.
- `/bughouse_wasm` — WASM (WebAssembly) bindings.
- `/www` — web client based on the abovementioned WASM bindings.


## Console how-to

Build & run server:

```
cargo run --package bughouse_console -- server
```

Build & run client:

```
cargo run --package bughouse_console -- client <server_address> <player_name> <team>
```

Note. Client requires a modern terminal with raw mode support.
Windows 10 cmd and most Linux terminals should work, while mingw, git bash,
cmd and powershell in older Windows versions may not.

Note. Unicode support in Windows cmd built-in fonts is very poor. You need to
install and activate custom fonts (e.g. DejaVu Sans Mono) in order for chess
pieces to render properly in Windows terminal.

Example. To play locally start 5 terminal instances and run:

```
cargo run --package bughouse_console -- server
cargo run --package bughouse_console -- client localhost p1 red
cargo run --package bughouse_console -- client localhost p2 red
cargo run --package bughouse_console -- client localhost p3 blue
cargo run --package bughouse_console -- client localhost p4 blue
```


## Web how-to

Run once in the beginning and every time after changing Rust code:

```
cd bughouse_wasm && wasm-pack build
```

Run once:

```
cd www && npm install
```

Test locally:

```
cd www && npm run start
```

Run on Apache:

```
cd www && npm run build
sudo cp dist/* /var/www/your-website-folder
```
