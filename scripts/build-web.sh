#!/usr/bin/env bash
# Builds the web client into projects/clients/browser/dist, which the next `cargo build` embeds in
# the binary. Needs the wasm32 target and trunk; both are installed here when missing.
set -euo pipefail

TRUNK_VERSION=0.21.14
# The version the lockfile resolves; trunk takes this one from PATH instead of fetching a
# prebuilt binary, which needs a newer glibc than some Linux runners have.
WASM_BINDGEN_VERSION=0.2.130
here="$(cd "$(dirname "$0")/.." && pwd)"

rustup target add wasm32-unknown-unknown
if ! command -v trunk >/dev/null 2>&1 || [[ "$(trunk --version)" != "trunk $TRUNK_VERSION" ]]; then
  cargo install trunk --locked --version "$TRUNK_VERSION"
fi
if ! command -v wasm-bindgen >/dev/null 2>&1 \
  || [[ "$(wasm-bindgen --version)" != "wasm-bindgen $WASM_BINDGEN_VERSION" ]]; then
  cargo install wasm-bindgen-cli --locked --version "$WASM_BINDGEN_VERSION"
fi
cd "$here/projects/clients/browser"
trunk build --release --locked
