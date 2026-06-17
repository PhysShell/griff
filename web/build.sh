#!/usr/bin/env bash
# Build the griff web playground into web/dist/ (static, deployable anywhere).
#
#   ./web/build.sh                              # release build → web/dist
#   python3 -m http.server -d web/dist 8080     # then open http://localhost:8080
#
# Guitar Pro support needs the Rust GP reader, which pulls wasm-bindgen
# (ADR-0025 supersedes ADR-0024's import-free cdylib). Pipeline: cargo build
# (with getrandom's wasm_js backend) -> wasm-bindgen --target web -> static files.
# `wasm-bindgen-cli` must match the `wasm-bindgen` crate version pinned in
# Cargo.toml; install it with:
#   cargo install wasm-bindgen-cli --version <that version> --locked
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/dist"

rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
# getrandom's wasm_js backend (Web Crypto) needs this cfg alongside the feature.
export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"wasm_js\""
( cd "$here" && cargo build --release --target wasm32-unknown-unknown )

want=$(grep -m1 'wasm-bindgen = ' "$here/Cargo.toml" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "error: wasm-bindgen CLI not found. Install the matching version:" >&2
  echo "  cargo install wasm-bindgen-cli --version ${want:-<see Cargo.toml>} --locked" >&2
  exit 1
fi
# The CLI must match the wasm-bindgen crate exactly, or the generated glue and
# the .wasm disagree at runtime. Fail loudly on a mismatch instead of shipping it.
have=$(wasm-bindgen --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true)
if [ -n "${want:-}" ] && [ "$have" != "$want" ]; then
  echo "error: wasm-bindgen CLI version mismatch (have ${have:-none}, want $want)." >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked --force" >&2
  exit 1
fi

rm -rf "$out"
mkdir -p "$out"
cp "$here"/static/* "$out"/
wasm-bindgen --target web --no-typescript \
  --out-dir "$out" \
  "$here/target/wasm32-unknown-unknown/release/griff_web.wasm"

size=$(wc -c < "$out/griff_web_bg.wasm")
echo "built web/dist ($((size / 1024)) KiB wasm) — serve it with:"
echo "  python3 -m http.server -d \"$out\" 8080"
