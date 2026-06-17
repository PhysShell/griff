# ADR 0026: A minimal in-browser chunk.json capture tool (carve-out from "web curation — later")

Date: 2026-06-17
Status: Accepted

Narrows ADR-0024's out-of-scope line — *"corpus curation / persistence on web
(the `preview/design/` curation dock — later)"* (ADR-0024 §"out of scope") — to
carve out a **thin, download-only capture path**. The full web curation dock,
in-browser persistence, and editing actions remain deferred. ADR-0024's M2 egui
plan and S8's TUI curation are unchanged.

## Context

ADR-0025 made the browser load Guitar Pro tabs through the *same* Rust parser as
the CLI, explicitly to unblock **phone-side curation** (ADR-0025 §Consequences).
The maintainer works from a phone; the tabs live there.

The corpus schema (v7, `core/src/corpus.rs`) records one datum that **cannot be
derived from the notes** and must be captured at curation time: provenance —
`RightsInfo` (rights status, acquisition, redistributable, notes) — see S5's
pre-curation requirements. Everything else a chunk needs (boundaries, structure,
gesture, complexity) the engine measures deterministically from the score.

`griff curate` already does this on the desktop, and `griff manifest` already
folds a directory of `*.chunk.json` into a `CorpusManifest`. What is missing is a
*phone* path to produce a `chunk.json` at all. Building the full S8 web curation
dock (persistence, split/merge/rename, boundary editing, ensemble relations) to
get there is disproportionate.

## Decision

1. **Extend the existing M1 playground, not a new app.** Add two
   `#[wasm_bindgen]` exports to `griff-web` that reuse `griff-core` exactly as
   `griff curate` does:
   - `detect_boundaries_json(track)` — the S4 phrase detector with the same
     PPQN-scaled config the CLI uses, so the page can preview phrase cuts.
   - `build_chunk_json(track, id, title, filename, tuning, cohort, tags,
     quality, reviewer, rights…, created_at, updated_at)` — measures
     structure/gesture/complexity + boundaries for the track and assembles a
     real `griff_core::corpus::ChunkMeta`.

2. **Serialize the domain type, do not fork a schema.** The export builds a
   `corpus::ChunkMeta` and emits it with `serde_json`, so the bytes are exactly
   what `griff manifest` deserializes. The browser and CLI cannot drift, because
   they share one `ChunkMeta` definition. (Rejected: hand-rolling chunk JSON in
   Rust strings or JS — it would silently desync from the schema, the failure
   mode ADR-0025 already rejected for parsing.)

3. **Download-only; the CLI owns assembly.** The page downloads one
   `<id>.chunk.json` per captured track. There is **no** IndexedDB persistence
   and **no** in-browser manifest assembly — the user collects the files and runs
   `griff manifest`. This keeps the capture tool stateless and disposable.

4. **Determinism preserved (SPEC §6).** `created_at`/`updated_at` are supplied by
   the page, so the wasm output is a pure function of its inputs; the engine's
   seeded measurement code is untouched.

## Consequences

- Phone capture of the non-derivable rights datum (plus tags/boundaries) becomes
  possible; chunks flow into the corpus through the existing `griff manifest`.
  The gap S5 flags is closed without building the S8 dock.
- The web payload grows modestly (`serde_json`) on top of ADR-0025's ~830 KiB.
  Accepted — capture is the point of the web front.
- The playground gains a capture panel; it is still the throwaway M1 front
  (ADR-0024's egui M2 plan stands and will absorb this).
- The browser duplicates the CLI's small index→enum prompt mapping (tags,
  quality, rights codes). Mitigated by sharing the `ChunkMeta` type and the same
  core measurement/boundary functions — the schema has a single source of truth.
- Still **out of scope** (deferred to the S8 web dock): in-browser persistence,
  split/merge/rename/retag actions, boundary *editing*, ensemble/group capture,
  and multi-chunk manifest assembly.

## See also

- [`0025-guitar-pro-in-browser-needs-wasm-bindgen.md`](0025-guitar-pro-in-browser-needs-wasm-bindgen.md)
- [`0024-web-wasm-frontend-for-mobile.md`](0024-web-wasm-frontend-for-mobile.md)
- [`../stages/S5-corpus-and-schema.md`](../stages/S5-corpus-and-schema.md)
- [`../stages/S8-preview-app.md`](../stages/S8-preview-app.md)
