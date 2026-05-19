# ADR 0004: Forbid unsafe_code at the workspace level

Date: 2026-05-19
Status: Accepted

## Context

`griff` is a symbolic music tool. No SIMD, FFI, or raw-pointer manipulation is
needed in the core or CLI. The future CLAP layer (S10) uses `nih-plug`, which
hides FFI behind a safe API.

## Decision

`[workspace.lints.rust] unsafe_code = "forbid"`. No exceptions.

## Consequences

- Removes a whole class of crashes / UB; forces safe wrappers.
- A future audio-rate hot path that genuinely needs `unsafe` requires a new
  superseding ADR.
