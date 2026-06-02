# Decisions log

Append-only Y-statements for small, non-architectural decisions. Format:

> In the context of `<situation>`, facing `<concern>`, we decided for
> `<option>` and against `<alternatives>`, to achieve `<benefit>`, accepting
> `<downside>`.

Architectural decisions go to [`adr/`](adr/) instead.

---

- 2026-05-19 — In the context of bootstrapping the knowledge base, facing a
  Russian constitution but an English-only repo, we decided for a condensed
  English translation of the glossary (all terms preserved, prose tightened)
  and against a verbatim 1:1 translation or keeping Russian, to achieve a
  readable in-repo constitution, accepting that some authorial commentary is
  dropped.

- 2026-05-19 — In the context of mislabeled `feat(sN)` commits, facing a
  conflict with the canonical roadmap, we decided for keeping git history plus
  an audit doc and canonical numbering forward, and against rewriting history,
  to achieve honesty without destabilizing a published branch, accepting that
  old commit messages stay wrong (reconciled in `docs/audit/`).

- 2026-05-19 — In the context of S0 golden CLI tests, facing the `insta` vs
  hand-rolled snapshot-tooling open question, we decided for hand-rolled plain
  golden text files (compared in-process, re-blessed via `GRIFF_BLESS=1`) and
  against the `insta` crate, to keep the workspace dependency tree and the
  strict `cargo-deny`/clippy posture intact, accepting slightly less ergonomic
  snapshot review.

- 2026-05-19 — In the context of S0 `.mid` fixtures, facing the in-repo
  real-MIDI vs synthetic open question, we decided for fully synthetic minimal
  fixtures generated with `midly` (committed, byte-pinned by an in-sync guard
  test) and against any licensed real guitar MIDI, to avoid licensing concerns
  and keep the importer's golden inputs deterministic, accepting that the
  fixtures are not musically realistic.

- 2026-05-31 — In the context of adding ComplementArranger, facing where to slot
  it in the canonical roadmap, we decided for appending it as `S13` (next free
  number, `Depends on: S6`) and against renumbering `S7…S12`, to keep the
  project's append-only posture and avoid renaming six stage files, accepting
  that the integer no longer reflects logical position (captured by the
  dependency note and `docs/audit/2026-05-s13-complementary-arranger.md`).

- 2026-05-31 — In the context of the first ComplementArranger version, facing
  rule-derived vs corpus-mined complement relations, we decided for purely
  generative (derive part B from part A by rule) and against mining real
  two-guitar pairs now, to ship a deterministic baseline without a corpus-schema
  change, accepting that `ChunkMeta` carries no pair relations yet
  (`schema_version` stays 1) and that learning from real pairs is deferred to
  the graph layer.

- 2026-05-31 — In the context of the canon-lift needed for ComplementArranger,
  facing how much of the legacy linear model to retire now, we decided for
  porting only `feature` and `generate` to the canonical model and against a
  full legacy removal up front (ADR-0011), to unblock the new engine cheaply,
  accepting that `classify`/`slice`/the CLI import path stay on the legacy model
  until later characterization-gated ports.

- 2026-05-31 — In the context of S7 traversal, facing weighted-random-walk vs
  dynamic programming, we decided for DP/Viterbi as the primary mechanism (beam
  search only as a large-graph approximation) and against random walk
  (ADR-0013), to get deterministic, whole-sequence-optimal selection that fits
  SPEC §6 without an RNG, accepting that the DP state must stay small and that
  S7 now depends on a realised `EnergyState` and the fretboard model.

- 2026-05-31 — In the context of humanising guitar parts, facing pitch-only
  notes vs string/fret positions, we decided for making the canonical model
  fretboard-aware (`AtomNote` gains an optional `(string, fret)` under the
  score `Tuning`) and against staying pitch-only (ADR-0014), to enable position
  shifts / `fret_jump_penalty` / playability, accepting that position is
  optional (MIDI often can't recover it), inference is a deferred lossy
  sub-problem, and it adds scope to S7.

- 2026-06-01 — In the context of finishing ADR-0011 steps 2–3, facing how to
  move `classify` and the CLI off the legacy `Bar`/`Phrase` types and how
  to present per-bar output once bars are score-level (ADR-0003), we decided for
  a canonical `classify::bar_features_in_range(&Voice, TickRange)` and a
  score-level CLI summary (one `Bars:` line, per-track note counts) — and
  against preserving the old per-track `bars=` column — deliberately re-blessing
  the `import__`/`export__`/`roundtrip__` goldens behind characterization tests,
  accepting a one-off snapshot churn and a marginally smaller `export_score`
  byte stream that still round-trips. With this the legacy linear model is fully
  removed (single internal model).

- 2026-06-01 — In the context of starting S13 (ComplementArranger), facing
  where part-A's profile lives and how much to ship first, we decided for a
  dedicated `PartProfile` in a new `complement` module (over extending the
  feature layer) and a first vertical slice of `rhythm_lock` only — a constraint
  compiler that derives an S6 `RhythmCopyPitchSubstitute` request from A and
  appends B as a new `Track` on A's master bars — plus a minimal `validate_pair`
  and the P2 `complement_request` fuzz target (ADR-0012), accepting that the
  other five relation modes, per-part playability in the validator, and richer
  harmonic context in the profile are deferred to follow-up increments.
