#!/usr/bin/env bash
# Build the egui cockpit into cockpit/dist/ as a static wasm web app — the
# canonical M2 web front (ADR-0027 Slice 2). Same toolchain as web/build.sh
# (ADR-0025): cargo build for wasm32 with getrandom's wasm_js backend, then
# `wasm-bindgen --target web` to emit the ES module + wasm into a static dir.
#
#   ./cockpit/build-web.sh                          # release build → cockpit/dist
#   python3 -m http.server -d cockpit/dist 8080     # then open http://localhost:8080
#
# `wasm-bindgen-cli` must match the `wasm-bindgen` crate version pinned in
# cockpit/Cargo.toml; install it with:
#   cargo install wasm-bindgen-cli --version <that version> --locked
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
out="$here/dist"

rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
# getrandom's wasm_js backend (Web Crypto) needs this cfg alongside the feature;
# the GP reader's `zip` subtree pulls getrandom (ADR-0025 §3).
export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"wasm_js\""
# Build only the library: its `cdylib` emits the .wasm (the native bin is N/A on
# web). The cockpit lives in the root workspace, so build from there.
( cd "$root" && cargo build --release --lib -p griff-cockpit --target wasm32-unknown-unknown )

want=$(grep -m1 -E '^wasm-bindgen[[:space:]]*=' "$here/Cargo.toml" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
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
cp "$here"/web/* "$out"/
wasm-bindgen --target web --no-typescript \
  --out-dir "$out" \
  "$root/target/wasm32-unknown-unknown/release/griff_cockpit.wasm"

size=$(wc -c < "$out/griff_cockpit_bg.wasm")
echo "built cockpit/dist ($((size / 1024)) KiB wasm) — serve it with:"
echo "  python3 -m http.server -d \"$out\" 8080"
