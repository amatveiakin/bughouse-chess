FROM ubuntu:latest
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN apt-get update && apt-get install -y \
    curl \
    npm \
    # as recommended by https://docs.rs/openssl/latest/openssl/:
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin:$PATH"
RUN curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

WORKDIR bughouse-chess
# The set of NPM packages changes very seldom, so install them first.
COPY www/package.json www/package-lock.json www/
RUN cd www && npm install
# Copy and build non-test Rust code.
COPY Cargo.toml Cargo.lock ./
COPY src src/
COPY bughouse_webserver bughouse_webserver/
COPY bughouse_console bughouse_console/
COPY bughouse_wasm bughouse_wasm/
RUN \
    # Save Cargo cache with `--mount=type=cache` to speed up incremental builds.
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/bughouse-chess/target \
    # Create a DB stub. An empty file isn't a valid SQLite DB, but it's enough
    # to make the webserver happy.
    touch bughouse.db && chmod +w bughouse.db \
    # Build everything. Subsequent runs of `cargo build` will be fast thanks to
    # the caches above, but `wasm-pack` is always slow.
    && cargo build -r --package bughouse_console \
    && cd bughouse_wasm && wasm-pack build && cd - \
    # Copy produced binaries to a separate folder, because cache folders are not
    # available when running the container.
    && mkdir bin \
    && cp target/release/bughouse_console bin/
# Copy everything else last. Changes to "www/" shouldn't trigger Rust builds.
COPY . .

CMD bash ./docker_start.sh
EXPOSE 8080 14361
