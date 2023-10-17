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
  The server part is responsible for the game engine and dynamic HTML content.
  It does not serve static HTML content.
- `/bughouse_wasm` — WASM (WebAssembly) bindings for the web client.
- `/www` — Web client based on the abovementioned WASM bindings.

Note on `rust-toolchain`: using the obsolete format instead of
`rust-toolchain.toml`, because GitHub `actions-rs` does not understand the
latter. The version needs to be bumped manually, because CI build fails on
warnings.


## Docker setup

Build the container:

```
DOCKER_BUILDKIT=1 docker build -t bughouse-chess .
```

Run the container:

```
docker run -d -p 8080:8080 -p14361:14361 bughouse-chess
```

Go to http://localhost:8080 for the game. Use links in the start menu or go to
http://localhost:14361/dyn/stats or http://localhost:14361/dyn/games for stats.

> **Note** Docker is the easiest way to set up the entire environment. It's
> configured to make `docker build` / `docker run` development workflow somewhat
> non-miserable (Cargo caches are kept and changes to things like HTML don't
> trigger Rust builds at all). Still, [Local setup](#local-setup) below provides
> way better speed and flexibility: you can keep the server running while
> relaunching the client; update HTML, CSS and JS on the fly; or keep game
> history across launches in a local SQLite DB.


## Local setup

Install Rust, npm, OpenSSL and pkg-config (if Linux). See `Dockerfile` for
Ubuntu setup steps.

Run once:

```
cd www && npm install
```

Build & run game server:

```
cargo run --package bughouse_console -- server bughouse_console/test-config.yaml
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
local server. For stats, use links in the main menu or go to
http://localhost:14361/dyn/stats or http://localhost:14361/dyn/games.

Changes to CSS will apply immediately. Changes to HTML and JS will
apply after a page refresh. Changes to Rust code must be recompiled via
`wasm-pack` (see above).


## Full Ubuntu/Apache server setup

Install tools and libraries:

```
apt update
apt install curl npm pkg-config libssl-dev apache2
curl https://sh.rustup.rs -sSf | sh -s -- -y
cargo install wasm-pack
```

Move Certbot data. On the old server:

```
tar zpcvf backup-letsencrypt.tar.gz /etc/letsencrypt/
```

Copy data to the new server. On the new server:

```
tar zxvf backup-letsencrypt.tar.gz -C /
```

Configure Cerbot as usual:
https://certbot.eff.org/instructions?ws=apache&os=ubuntufocal

Install Apache modules:

```
a2enmod proxy proxy_http proxy_wstunnel headers brotli
systemctl restart apache2
```

Configure Apache:
- Enable request redirection to make game server available.
- Set `Cache-Control` to `no-cache` and enable ETag to make sure that the
  clients are always up-to-date.
- Enable Brotli compression. It's more efficient that Gzip and it's supported by
  all browsers that support WASM. Note that compression breaks ETags in Apache,
  so we need to work this around:
  https://bz.apache.org/bugzilla/show_bug.cgi?id=45023#c30.

Add this to `/etc/apache2/sites-available/<site>`:

```
<VirtualHost *:443>
    ProxyPreserveHost On
    ProxyRequests Off
    ProxyPass /dyn http://localhost:14361/dyn
    ProxyPassReverse /dyn http://localhost:14361/dyn
    ProxyPass /auth http://localhost:14361/auth
    ProxyPassReverse /auth http://localhost:14361/auth
    ProxyPass /ws ws://localhost:14361 keepalive=On
    ProxyPassReverse /ws ws://localhost:14361

    Header set Cache-Control "public, no-cache, must-revalidate"
    FileETag All

    SetEnvIf If-None-Match '^"((.*)-(gzip))"$' gzip
    SetEnvIf If-None-Match '^"((.*)-(br))"$' br
    RequestHeader edit "If-None-Match" '^"((.*)-(gzip|br))"$' '"$1", "$2"'
    Header edit "ETag" '^"(.*)"$' '"$1-gzip"' env=gzip
    Header edit "ETag" '^"(.*)"$' '"$1-br"' env=br

    SetEnv no-gzip 1
    AddOutputFilterByType BROTLI_COMPRESS application/javascript
    AddOutputFilterByType BROTLI_COMPRESS application/wasm
    AddOutputFilterByType BROTLI_COMPRESS application/xhtml+xml
    AddOutputFilterByType BROTLI_COMPRESS application/xml
    AddOutputFilterByType BROTLI_COMPRESS font/opentype
    AddOutputFilterByType BROTLI_COMPRESS font/otf
    AddOutputFilterByType BROTLI_COMPRESS font/ttf
    AddOutputFilterByType BROTLI_COMPRESS font/woff
    AddOutputFilterByType BROTLI_COMPRESS font/woff2
    AddOutputFilterByType BROTLI_COMPRESS image/svg+xml
    AddOutputFilterByType BROTLI_COMPRESS text/css
    AddOutputFilterByType BROTLI_COMPRESS text/html
    AddOutputFilterByType BROTLI_COMPRESS text/xml
    AddOutputFilterByType BROTLI_COMPRESS text/javascript
    AddOutputFilterByType BROTLI_COMPRESS text/plain
</VirtualHost>
```

And apply Apache config changes:

```
sudo service apache2 reload
```

Clone the repo:

```
git clone https://github.com/amatveiakin/bughouse-chess.git
```

Add to `.bashrc`:
```
export BUGHOUSE_ROOT=<path-to-bughouse-chess>
export PATH="$BUGHOUSE_ROOT/prod:$PATH"
```

Install npm packages:

```
cd "$BUGHOUSE_ROOT/www" && npm install
```

Serve static content:

```
bh_deploy_web
```

Copy `prod-config-template.yaml` to the path specified in `bh_run_server` and
fill in the blanks.

Put Google client secret (from https://console.cloud.google.com/apis/credentials)
to the path pointed by `client_secret_source`. Generate a random session secret
using `util/gen_session_secret.py`.

Run game server (e.g. in `screen`):

```
bh_run_server
```


## Local console client setup

> **Warning**
> Console client hasn't been updated to the latest protocol version and is
> currently broken.

Build & run server:

```
cargo run --package bughouse_console -- server bughouse_console/test-config.yaml
```

Build & run client:

```
cargo run --package bughouse_console -- client <server_address> <match_id> <player_name>
```

Note. Client requires a modern terminal with raw mode support.
Windows 10+ cmd and most Linux terminals should work, while mingw, git bash,
cmd and powershell in older Windows versions may not.

Note. Unicode support in Windows cmd built-in fonts is very poor. You need to
install and activate custom fonts (e.g. DejaVu Sans Mono) in order for chess
pieces to render properly in Windows terminal.
