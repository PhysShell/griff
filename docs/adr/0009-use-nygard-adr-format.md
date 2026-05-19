# ADR 0009: Use the Nygard ADR format, stored in-repo, append-only

Date: 2026-05-19
Status: Accepted

## Context

`griff` has one contributor plus AI agents. Decisions must be cheap to write
and useful to read. Heavier formats (MADR) add audit overhead a small project
does not need yet. Decisions must stay in sync with the code.

## Decision

ADRs use the Nygard format (Context / Decision / Consequences), live in
`docs/adr/NNNN-verb-phrase.md` with 4-digit padded ids, and are append-only:
after `Accepted` they are immutable; a change is a new ADR with `Superseded by
ADR-NNNN`. Small, non-architectural decisions go to `docs/decisions.log.md` as
Y-statements.

## Consequences

- Low-friction, source-controlled decision history.
- Migrating to MADR later (team > ~15) does not require rewriting old ADRs.
