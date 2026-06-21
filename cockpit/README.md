# griff-cockpit

The egui **cockpit** — griff's `Scene` renderer (ADR-0027). One `eframe`/`egui`
codebase paints the shared `griff-ui-core` piano-roll on two targets: a
**native** desktop window and the **browser** (wasm). Both drive the identical
`resolve → paint` and `input → intent` path through the shared core; this crate
only maps placed cells to pixels and key presses to intents (ADR-0016).

## Native

```sh
cargo run -p griff-cockpit -- path/to/score.mid     # or .gp3/.gp4/.gp5/.gpx
```

Reads a MIDI or Guitar Pro file through the shared importer and opens the
piano-roll window. Keys: `space` play/pause, `←`/`→` scroll, `↑`/`↓` pitch,
`+`/`−` zoom, `[`/`]` section, `Home` reset, `i` inspector, `q`/`Esc` quit.

## Web (wasm) — ADR-0027 Slice 2

```sh
./cockpit/build-web.sh                            # → cockpit/dist/
python3 -m http.server -d cockpit/dist 8080       # open http://localhost:8080
```

`build-web.sh` mirrors the ADR-0025 web toolchain: `cargo build` for
`wasm32-unknown-unknown` (with getrandom's `wasm_js` backend), then
`wasm-bindgen --target web`. It needs a `wasm-bindgen-cli` matching the
`wasm-bindgen` version pinned in `Cargo.toml`:

```sh
cargo install wasm-bindgen-cli --version <pinned> --locked
```

Slice 2 paints a built-in demo score (`assets/demo.mid`) to prove the web render
path end-to-end; interactive file loading and capture arrive in Slice 3.

## Web tests

Headless-browser tests (`web-test/`) serve the built `dist/` and boot the real
eframe app in Chromium — WebGL via SwiftShader, no GPU. They assert it **paints**
the cockpit (a non-blank canvas, the signature note / band / playhead fill
colours, a resize re-fit, and a coarse block-average match to the committed
`cockpit-reference.png` — deterministic because the font is baked into the wasm)
and that **interaction** works end-to-end: `Space` animates playback and the
playhead advances, `Space` again holds still, `←`/`→` scroll, `↑` shifts pitch,
`]` jumps a section, `=` zooms, `Home` resets the view, and an unmapped key is
inert. This is the pixel-truth `egui_kittest` can't give headlessly (it
rasterises through native wgpu, which finds no adapter in CI); a browser ships
its own software GL.

```sh
./cockpit/build-web.sh                       # produce cockpit/dist first
cd cockpit/web-test
npm ci && npx playwright install chromium chromium-headless-shell
npm test
```

CI runs build + smoke test on every `cockpit/`, `core/`, or `ui-core/` change
(`.github/workflows/cockpit-web-test.yml`).
