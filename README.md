# griff

A swancore-first guitar riff engine. It analyzes, slices, generates, and
regenerates guitar parts as a structured symbolic model (MIDI in → MIDI out,
no audio synthesis). Long-term target: a MIDI-oriented CLAP plugin with
human-in-the-loop curation.

## Workspace

- `core/` — library: musical model, MIDI I/O, slicing, features, generation
- `cli/` — binary `griff` (`import` / `inspect` / `export` / `classify` / `curate`)
- `preview/` — headless-testable ratatui preview: piano-roll view + section /
  structure analysis
- `plugin/` — CLAP plugin (S10+, not yet)

## Documentation

- [`docs/SPEC.md`](docs/SPEC.md) — what griff is/isn't, hard rules
- [`docs/glossary.md`](docs/glossary.md) — the constitution (terms)
- [`docs/preview-guide.md`](docs/preview-guide.md) — using the interactive
  preview TUI (keys, inspector, curation, complexity)
- [`docs/stages/`](docs/stages/) — canonical roadmap S0…S16
- [`docs/adr/`](docs/adr/README.md) — architecture decisions
- [`AGENTS.md`](AGENTS.md) — guide for AI agents

## Build

```
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

## Corpus quickstart (the cockpit, S8)

The cockpit generates from an open score alone, or — with a **corpus** — from
curated material: rhythm templates, novelty references, and burst/rest
gesture statistics, plus the corpus's tabs as the seed pick-list.

Run the native cockpit over an existing corpus:

```
cargo run --release -p griff-cockpit -- --corpus /path/to/corpus --out /path/to/keeps
```

Then: open **Generate** (`g`) → pick a seed tab → set seed / bars / candidates
/ gesture → **Generate** → click the ranked candidates → **▶** to hear them,
drag the tempo, **loop** a bar range, **A/B** (`b`) two of them → **Keep**
writes the MIDI and a provenance sidecar into `--out`.

Build a test corpus from a couple of tabs (the generation loader reads the
`*.chunk.json` records directly; `manifest` is a coverage check, not required
at runtime):

```
mkdir -p corpus
cp tabs/song1.gp5 tabs/song2.gp5 corpus/
cargo run --release -p griff-cli -- split corpus/song1.gp5 -o corpus/song1
cargo run --release -p griff-cli -- split corpus/song2.gp5 -o corpus/song2
cargo run --release -p griff-cli -- manifest corpus
cargo run --release -p griff-cockpit -- --corpus corpus --out keeps
```

Without `--corpus`, generation uses only the open score — no corpus rhythms,
references, or gesture — so it reads as an honest early rule generator handed
one file.

### Generate vs Swang, with a corpus — they differ, on purpose

- **Generate panel + corpus:** the corpus supplies the **rhythm**, novelty
  references, and gesture.
- **Swang + corpus:** the current native cockpit does **not resolve** a
  corpus for Swang. A program that *declares* a `corpus` is therefore
  **refused outright** — the run reports the refusal and produces **no
  candidates** at all (remove the `corpus` line, or build it with `griff
  swang build`). Only a program with **no** `corpus` line runs, on the
  kernel's own rhythm. By the frozen precedence **explicit pattern > corpus
  > source first bar** (ADR-0029 §7), once corpus resolution is wired a
  declared corpus will contribute novelty and gesture but will **never**
  replace the kernel's rhythm; the grid always comes from the program.

So to *play with a real corpus's rhythms* today, use the **Generate** panel,
not Swang. That precedence is frozen and does not change here.

Playback (native MIDI or the browser's Web Audio) is identical for a Generate
candidate and a Swang candidate — both are just a `Score`.
