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
piano-roll window. A **top toolbar** surfaces the controls so nothing hides
behind a hotkey: a **track selector** (the roll shows one part at a time, not
every track overlaid — the selector switches it, and capture targets it),
play/pause, and toggles for the capture form and the corpus dock. The same keys
still work: `space` play/pause, `←`/`→` scroll, `↑`/`↓` pitch, `+`/`−` zoom,
`[`/`]` section, `Home` reset, `i` inspector, `c` corpus dock, `g` generate
panel, `t` light/dark palette, `q`/`Esc` quit.

The palette is not this crate's: every cell — and egui's own chrome — resolves
through `griff_ui_core::theme` (ADR-0028), the same tokens the `ratatui` preview
paints from, with the WCAG contrast floors asserted in the core's tests.

## Generate (S8)

```sh
cargo run --release -p griff-cockpit -- --corpus path/to/corpus --out keeps
```

With `--corpus DIR` the cockpit loads the curated corpus — the chunks supply the
rhythm templates, novelty references and burst/rest gesture ask, their source
tabs become the **seed pick-list** — and opens on its first tab. The **Generate**
panel (toolbar, or `g`) then runs a whole session without the CLI:

- pick a seed tab, set **seed / bars / variants-per-strategy / gesture**, hit
  **generate** (or **next seed** to bump and re-roll);
- browse the **reranked candidate set** — rank, strategy, aggregate, note count,
  with the six rerank axes on hover. Clicking one paints it into the roll;
- **keep** writes `<out>/seed<N>_<Strategy>_<variant-seed>.mid` plus a `.json`
  sidecar naming the exact ask, so the candidate reproduces byte-for-byte;
- **open** hands that `.mid` to whatever the OS has registered for it. The
  cockpit **does not synthesise audio** — its playhead is visual.

Rank 1 is the candidate `griff generate` would have written: the panel enters the
same `griff_core::generation_input::ranked_candidates` the CLI does, so the two
cannot drift.

Without `--corpus` the panel still generates, seeding from the displayed score
alone (no corpus rhythms, no novelty references, no gesture) — the browser build
works this way today.

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
(ADR-0027 Slices 3–4). **Corpus** reads the OPFS tree back into an in-canvas
**dock** — browse and filter the captured chunks by class/tag, rights status, and
cohort, with an aggregate dashboard (totals, redistributable / near-duplicate /
rights-unset counts, top tags) over the shared `griff_ui_core::dock` (ADR-0027
Slice 5; the `c` key toggles it). Selecting a chunk opens the **curation
inspector** — approve / reject, rename, and retag — each edit applied through the
shared `griff_ui_core::curation` ops and persisted back to its OPFS `chunk.json`
(ADR-0027 Slice 6). The `i` key opens the capture panel to edit the curator
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
`chunk.json` for the loaded score *and* persists it to the OPFS corpus; and that
the **corpus dock** opens over the roll when 📚 Corpus reads the persisted chunks
back. This is the pixel-truth `egui_kittest` can't
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
