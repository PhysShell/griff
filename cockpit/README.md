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

The web front boots on a baked demo score, with a toolbar over the canvas:
**Open** hands a picked MIDI/Guitar Pro file to the wasm `load_score` export (the
cockpit re-imports it through the shared parser and repaints), and **Capture**
builds a `chunk.json` for the focused track — through the shared
`griff_ui_core::capture::build_chunk`, byte-compatible with `griff manifest`.
It **persists** the chunk to the browser's OPFS corpus
(`corpus/<id>.chunk.json` — the same bytes the CLI reads, ADR-0027 §3) and
downloads an export copy. **Manifest** then folds the whole OPFS corpus into a
`manifest.json` in-wasm through the shared `griff_ui_core::corpus` (the in-wasm
`griff manifest`), so a phone-built corpus drops straight into the CLI
(ADR-0027 Slices 3–4). The `i` key opens the capture panel to edit the curator
inputs (id / title / rights / tags…) first.

## Web tests

Headless-browser tests (`web-test/`) serve the built `dist/` and boot the real
eframe app in Chromium — WebGL via SwiftShader, no GPU. They assert it **paints**
the cockpit (a non-blank canvas, the signature note / band / playhead fill
colours, a resize re-fit, and a coarse block-average match to the committed
`cockpit-reference.png` — deterministic because the font is baked into the wasm)
and that **interaction** works end-to-end: `Space` animates playback and the
playhead advances, `Space` again holds still, `←`/`→` scroll, `↑` shifts pitch,
`]` jumps a section, `=` zooms, `Home` resets the view, and an unmapped key is
inert; that **loading** a picked file repaints the chosen score (a multi-track
file even brings in lane colours the demo never shows); and that **capture**
works — toggling the inspector shows the panel, and Capture downloads a real
`chunk.json` for the loaded score *and* persists it to the OPFS corpus. This is
the pixel-truth `egui_kittest` can't
give headlessly (it rasterises through native wgpu, which finds no adapter in
CI); a browser ships its own software GL.

```sh
./cockpit/build-web.sh                       # produce cockpit/dist first
cd cockpit/web-test
npm ci && npx playwright install chromium chromium-headless-shell
npm test
```

CI runs build + smoke test on every `cockpit/`, `core/`, or `ui-core/` change
(`.github/workflows/cockpit-web-test.yml`).
