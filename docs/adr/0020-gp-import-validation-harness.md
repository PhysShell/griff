# ADR 0020: Validate Guitar Pro import against a reference oracle

Date: 2026-06-12
Status: Proposed

## Context

S3 made Guitar Pro a first-class input (`core/src/gp.rs::import_gp_score`
→ `Score`), and the `guitar_pro_import` fuzz target (ADR-0010) proves the
parser does not panic, hang, or allocate without bound on garbage. Neither
proves the parser reads the *right* notes: that fret 3 on string 2 became the
correct `(string, fret, pitch)`, that a palm mute survived, that durations and
onsets land on the master timeline. Fuzzing covers robustness; unit and golden
tests cover narrow hand-checked cases. There is no systematic check that a real
`.gp5` is imported *correctly and without silent loss*.

The canonical model is deliberately **not** a copy of the Guitar Pro format
(ADR-0002): GP's object tree is `Song → measures → voices → beats → notes`
mirroring the binary layout, while griff's `Score` is normalized. So a direct
field-for-field diff of griff's IR against any GP parser's object tree is
brittle and conceptually wrong — the schemas differ by design. SPEC mandates a
`LossReport` for every adapter, but nothing today asserts the report is
*complete* (that everything dropped is actually reported).

A mature independent reference parser exists — PyGuitarPro (Python, read/write
GP3/4/5; glossary §4). It can also author `.gp5` programmatically, so it can
generate fixtures whose content is known from the constructing code. But it is
Python, it has its own bugs and format gaps, and using it both to *generate* a
fixture and to *parse the reference* would let it bless its own output (a
closed loop). It is therefore an oracle, not ground truth.

## Decision

We add an isolated **GP import validation harness** that compares griff's IR
against a reference projection at a normalized semantic level, across three
oracle tiers.

1. **Normalized comparison form.** Both sides project into one canonical schema
   (`tools/gp-validate/schema/normalized.schema.json`): per track a `tuning`
   and `bars`; per bar `index / time_sig / tempo` and `voices`; per voice
   `notes` as `{onset_tick, dur_tick, string, fret, pitch, marks[], spans[]}`.
   Comparison is over this form only, never raw parser output. The form is
   canonical per SPEC §6 (sorted keys; notes ordered by
   `(bar, voice, onset, string)`; no paths or map-iteration order).

2. **In-product emitter.** griff exposes the IR via a new `griff gp-dump
   --json` CLI subcommand backed by a `core` serializer (`Score` → normalized
   form + its `LossReport`). The harness shells out to it and stays
   language-agnostic; the dump is also a debugging surface in its own right.

3. **Isolation.** `tools/gp-validate/` is a Python package, **not** a cargo
   workspace member (the `fuzz/` precedent, ADR-0010). It owns fixture
   authoring, reference dumping, comparison, and reporting. The main workspace,
   its pinned stable toolchain, and its lint policy are untouched.

4. **Three oracle tiers.**
   - **A — Golden (exact).** Tiny fixtures authored in code via PyGuitarPro
     (one technique per file). griff's IR dump is diffed against a committed
     golden `*.norm.json`, re-blessed on intent change via the `insta`
     snapshot crate (`INSTA_UPDATE=always`), adopted for the golden tier per
     decisions.log 2026-06-12 (reversing the 2026-05-19 `insta` rejection).
     Ground truth is the authoring code's intent, *not* a reparse.
   - **B — Loss-oracle (subset).** Real `.gp5` files. PyGuitarPro enumerates
     the file; the assertion is `reference ⊆ (IR ∪ LossReport)` — every
     semantic element is either represented in the IR **or** explicitly
     reported as loss. This is robust to the model mismatch and directly
     enforces the SPEC loss-report rule. It is *not* an equality check.
   - **C — Properties (no oracle).** Invariants on every IR dump: onsets
     monotonic per voice, `fret` in range, `string` within the track tuning,
     per-voice durations consistent with the bar, half-open ranges.

5. **No closed loop.** Tier-A fixtures may be PyGuitarPro-authored (content
   known from code). Tier-B fixtures MUST be real `.gp5` not produced by
   PyGuitarPro, so the generator cannot bless the parser. An optional tier D
   triangulates a subset against a second reference (the `guitarpro` Rust crate
   or TuxGuitar).

6. **Determinism and CI.** Reference `*.norm.json` is generated offline and
   committed, so CI needs no GP parsing in Python — it runs `griff gp-dump`
   (Rust) and the comparator over committed reference. PyGuitarPro version is
   pinned; regenerating reference is a manual, reviewed step. The harness runs
   as a non-blocking check initially; promotion to a blocking gate follows once
   the fixture set stabilises.

7. **Oracle, not ground truth.** A tier-B disagreement is an *investigation*,
   not an automatic griff failure: PyGuitarPro gaps are recorded and excluded
   by explicit, documented allowlist entries rather than by loosening the
   check.

## Consequences

- GP import gains a correctness/completeness check distinct from the ADR-0010
  robustness layer; the two are complementary, not overlapping.
- The SPEC loss-report rule becomes *testable*: tier B fails when an element is
  neither represented nor reported, turning "emits a LossReport" into "emits a
  *complete* LossReport".
- `griff gp-dump --json` is a small new product surface (one subcommand + a
  `core` serializer) — the only main-app change; everything else is the
  isolated sidecar.
- Accepted: Python enters the tooling, but only inside `tools/gp-validate/`;
  committed reference JSON keeps GP parsing out of CI and off the determinism
  path.
- Accepted: PyGuitarPro is an oracle with its own gaps, so tier B carries a
  documented allowlist; it never silently weakens to absorb a disagreement.
- Accepted: the normalized schema is a second projection of `Score` and must
  track the model; it is intentionally lossy (comparison-relevant fields only)
  and versioned alongside the dump.
- Tier B is bounded by available real `.gp5` material; licensing-sensitive
  fixtures are git-ignored (corpus policy, ADR-0005), so coverage grows with
  the private corpus.
- Migrating the comparator to Rust later, or swapping PyGuitarPro for the
  `guitarpro` crate, does not invalidate the schema or the tiers — only the
  reference-dumper changes.
