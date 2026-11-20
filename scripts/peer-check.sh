#!/bin/sh
# Runs the core's socket tests as an ordinary user, and checks another user cannot talk to
# that user's core. For a throwaway Linux container only: it adds users and changes owners.
#   docker run --rm --init -v "$PWD:/src:ro" -v dusk-target:/target \
#     -e CARGO_TARGET_DIR=/target <rust image> sh /src/scripts/peer-check.sh
# User a (uid 1000) runs the socket tests, once without XDG_RUNTIME_DIR and once with a
# private one. Then a serves a core and user b (uid 1001) connects: the 0700 run folder
# must refuse b, and with the folder opened for the check the core must drop b by its uid,
# without a byte, and log it once.
set -eu

: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR to a folder the users can write}"
useradd -m -u 1000 a
useradd -m -u 1001 b
chown -R a "$CARGO_TARGET_DIR" "${CARGO_HOME:-/usr/local/cargo}"
mkdir -p -m 1777 /shared
install -d -m 0700 -o a /run/user/1000

# Runs a command as user a, from the source folder.
as_a() {
    runuser -u a -- env -u XDG_RUNTIME_DIR HOME=/home/a PATH="$PATH" \
        CARGO_TARGET_DIR="$CARGO_TARGET_DIR" "$@"
}

cd /src
echo "== user a, no XDG_RUNTIME_DIR"
as_a cargo test -p dusk_server --locked --test local
as_a cargo test -p dusk_server --locked --lib local::
echo "== user a, XDG_RUNTIME_DIR=/run/user/1000"
as_a env XDG_RUNTIME_DIR=/run/user/1000 cargo test -p dusk_server --locked --test local
as_a env XDG_RUNTIME_DIR=/run/user/1000 cargo test -p dusk_server --locked --lib local::

echo "== user b against the core of user a"
tests=$(as_a cargo test -p dusk_server --locked --test local --no-run --message-format=json \
    | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | tail -n 1)
as_a env DUSK_PEER_DIR=/shared "$tests" --ignored --exact peer_check_serve_as_owner \
    --nocapture &
owner=$!
runuser -u b -- env HOME=/home/b DUSK_PEER_DIR=/shared "$tests" --ignored --exact \
    peer_check_connect_as_stranger --nocapture
wait "$owner"
echo "== peer check passed"
