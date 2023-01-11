#!/bin/bash

export RUST_BACKTRACE="1"
export RUST_LOG="INFO"

./bin/bughouse_console server --sqlite-db bughouse.db &
./bin/bughouse_webserver --sqlite-db bughouse.db --static-content-url-prefix=http://localhost:8080 &
cd www
npm run start &

# Background processes should not finish. If any of them does, exit with its
# status code.
wait -n
exit $?
