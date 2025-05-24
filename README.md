# Bughouse chess platform

**This is the backend behind [bughouse.pro](https://bughouse.pro).**

It implements a client and a server for
[bughouse chess](https://en.wikipedia.org/wiki/Bughouse_chess) — the
best<sup>[citation not needed]</sup> kind of chess.

Our platform supports a range of bughouse configurations as well as other chess
variants:

- We allow to tune bughouse-related aspects: allowed pawn drop ranks; limits on
  drop aggression (e.g. whether drops can checkmate); whether pawn promotion
  follows chess rules or is allows to steal a piece for the other board.
- On top of that, we support a range of chess variants: Fischer random
  (Chess960), Accolade, Duck chess, Fog of war (Dark chess) and Koedem.

All abovementioned options can be combined arbitrarily.

Folder structure:

- `/src` — The core library used by the server and the client.
- `/bughouse_console` — A binary that can run as a server or as console client.
  The server part is responsible for the game engine and dynamic HTML content.
  Static HTML content is served separately.
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

#### Install tools and libraries:

```bash
sudo apt update

# Set up Node.js (v18.x)
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt install -y nodejs  # npm is included

# Install required packages
sudo apt install -y curl pkg-config libssl-dev apache2 python3-certbot-apache

# Install Rust and wasm-pack
curl https://sh.rustup.rs -sSf | sh -s -- -y
source $HOME/.cargo/env
cargo install wasm-pack
```

---
**Using Certbot with Apache** *(Reccomended but Optional)*:

If this is the first time using Certbot, run the following, replacing yourdomain.com with your domain:

Edit the Apache Config using the following:
```bash
sudo nano /etc/apache2/sites-available/tardbug.duckdns.org.conf
```

Add in:
```apache
ServerName tardbug.duckdns.org
DocumentRoot /var/www/html
```
Your config should look something like this:
```apache
<VirtualHost *:80>
    ServerName tardbug.duckdns.org
    DocumentRoot /var/www/html

    <Directory /var/www/html>
        Options Indexes FollowSymLinks
        AllowOverride All
        Require all granted
    </Directory>
</VirtualHost>
```

Then enable the site, reload Apache, and run Certbot:
```bash
sudo a2ensite tardbug.duckdns.org.conf
sudo systemctl reload apache2
sudo certbot --apache -d yourdomain.com
```

For example, your command should look like the following:
```bash
sudo certbot --apache -d bughouse.pro
```

Note: if you need www.yourdomain.com, include `-d www.yourdomain.com` to the end of the command.

If you have Certbot on another server, move the data. On the old server:

```bash
tar zpcvf backup-letsencrypt.tar.gz /etc/letsencrypt/
```

Copy data to the new server. On the new server:

```bash
tar zxvf backup-letsencrypt.tar.gz -C /
```

Configure Cerbot as usual:
https://certbot.eff.org/instructions?ws=apache&os=ubuntufocal

---
**Apache** *(Required)*:

```bash
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

To do the above changes, add this to `/etc/apache2/sites-available/yourdomain.com.conf`:

```apache
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
    Header set Cross-Origin-Opener-Policy "same-origin"
    Header set Cross-Origin-Embedder-Policy "require-corp"
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

Go to `/etc/apache2/mods-available/mpm_event.conf` and increase
`MaxRequestWorkers`. It determines the number of connections that Apache is able
to maintain. With WebSockets connections stay alive the entire time, so this
should be set to 4 times the number of expected concurrent games. I set it to
1000 on a very weak VM and it worked fine.

Apply Apache config changes:

```bash
sudo service apache2 reload
```
---
**Build & Configure Repository** *(Required)*:
Clone the repo:

```bash
git clone https://github.com/amatveiakin/bughouse-chess.git
```

Add to `.bashrc` (in your root folder):
```bash
export BUGHOUSE_ROOT=<path-to-bughouse-chess>
export PATH="$BUGHOUSE_ROOT/prod/bin:$PATH"
```
For example, if you have cloned the repository in your root folder, then add:
```bash
export BUGHOUSE_ROOT=<path-to-bughouse-chess>
export PATH="$BUGHOUSE_ROOT/prod/bin:$PATH"
```

Copy `prod-config-template.yaml` to `~/bughouse-config.yaml` and fill in the
blanks.

Put Google client secret (from https://console.cloud.google.com/apis/credentials)
to the path pointed by `client_secret_source`.

Generate a random session secret using `tools/gen_session_secret.py`.

Here is an example of the `~/bughouse-config.yaml` assuming the repository was cloned in root and keeping secrets in `bughouse-chess/secrets`.
```yaml
database_options: !Sqlite /home/user_name/bughouse-chess/db/bughouse.db
secret_database_options: !Sqlite /home/user_name/bughouse-chess/db/bughouse-secret.db
auth_options:
  callback_is_https: true
  google: !Some
	client_id_source: !Literal "1234567890-abc.apps.googleusercontent.com"
	client_secret_source: !File /home/user_name/bughouse-chess/secrets/google-client-secret
  lichess: !Some
	client_id_source: !Literal "bughouse.pro"
session_options: !WithSessions
  secret: !File /home/user_name/bughouse-chess/secrets/session-secret
  expire_in: 30d
static_content_url_prefix: ""
allowed_origin: !ThisSite https://bughouse.pro
check_git_version: true
max_starting_time: 1h
```

In order to get notification on bughouse server failures, create and fill
`~/secrets/telegram-bot-token` and `~/secrets/telegram-chat-id`.
You could get the token from BotFather and the chat ID from the URL of the
“Saved Messages” chat in Telegram web.

---
**Alternative: Build via GitHub Actions**

Setup:

* Install Python packages: `pip install requests`
* Generate a GitHub token with read access to Actions and save it to
  `~/secrets/github_token`.

Getting and deploying new changes:

* Get latest artifact: `bh_artifact_get`
* Deploy artifact: `bh_artifact_deploy`

---

**Alternative: Build locally**

Setup:

* Install Rust and wasm-pack.
* Install npm packages: `cd "$BUGHOUSE_ROOT/www" && npm install`

Getting and deploying new changes:

* Get latest version: `bh_pull`
* Deploy web client: `bh_build_and_deploy`

---

After doing one of the two alternatives, register bughouse server as a systemd service.
Copy `prod/configs/bughouse-handle-failure.service` and
`prod/configs/bughouse-server.service` to `/etc/systemd/system/`.
Enable and start the service:

```bash
systemctl enable bughouse-server
systemctl start bughouse-server
```


## Local console client setup

> **Warning**
> Console client hasn't been updated to the latest protocol version and is
> currently broken.

Build & run server:

```bash
cargo run --package bughouse_console -- server bughouse_console/test-config.yaml
```

Build & run client:

```bash
cargo run --package bughouse_console -- client <server_address> <match_id> <player_name>
```

Note. Client requires a modern terminal with raw mode support.
Windows 10+ cmd and most Linux terminals should work, while mingw, git bash,
cmd and powershell in older Windows versions may not.

Note. Unicode support in Windows cmd built-in fonts is very poor. You need to
install and activate custom fonts (e.g. DejaVu Sans Mono) in order for chess
pieces to render properly in Windows terminal.
