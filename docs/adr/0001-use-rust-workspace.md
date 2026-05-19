# ADR 0001: Use a Rust workspace with core / cli / plugin crates

Date: 2026-05-19
Status: Accepted

## Context

`griff` comprises a reusable library, a CLI front-end, and (later) a CLAP
plugin and possibly a preview app. These share musical types without circular
dependencies and need independent release cadences and build targets.

## Decision

We use a Cargo workspace with members `core`, `cli`, `plugin`. Common
dependencies (`midly`, `thiserror`, `clap`) are pinned in
`[workspace.dependencies]`. Lints are inherited via `[workspace.lints]`.
Resolver 2.

## Consequences

- One `cargo test --workspace`; shared, strict lint config; the plugin builds
  independently of the CLI.
- More boilerplate per new crate; a workspace-wide MSRV bump affects all
  crates.
