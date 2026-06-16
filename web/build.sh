#!/usr/bin/env bash
# Build the griff web playground into web/dist/ (static, deployable anywhere).
#
#   ./web/build.sh                              # release build → web/dist
#   python3 -m http.server -d web/dist 8080     # then open http://localhost:8080
#
# No wasm-bindgen / Trunk: griff-web is an import-free cdylib (ADR-0024), so the
# .wasm is copied next to the static files and loaded with WebAssembly.instantiate.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/dist"

rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
( cd "$here" && cargo build --release --target wasm32-unknown-unknown )

rm -rf "$out"
mkdir -p "$out"
cp "$here"/static/* "$out"/
cp "$here"/target/wasm32-unknown-unknown/release/griff_web.wasm "$out"/

size=$(wc -c < "$out/griff_web.wasm")
echo "built web/dist ($((size / 1024)) KiB wasm) — serve it with:"
echo "  python3 -m http.server -d \"$out\" 8080"
