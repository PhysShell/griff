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
- [`docs/stages/`](docs/stages/) — canonical roadmap S0…S14
- [`docs/adr/`](docs/adr/README.md) — architecture decisions
- [`AGENTS.md`](AGENTS.md) — guide for AI agents

## Build

```
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```
