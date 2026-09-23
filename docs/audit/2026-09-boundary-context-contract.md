# Boundary context ownership contract — preregistered design experiment

Date: 2026-09-23
Status: preregistered before implementation and outcome

## Purpose

#212 established that a target-line solver can consume preceding hand state and
incoming technique obligations. It did not establish who produces that state,
how long it lives, or whether replay works without reading imported future
positions. This Lab-only design experiment tests that missing contract. It does
not propose production adoption.

## Typed contract

```text
BoundaryContext {
    hand: Known(HandState) | Unknown,
    pending_techniques: [TechniqueObligation],
}

HandState {
    fret,
    source_note_id,
    source_onset,
    provenance: SolvedPartition,
}

TechniqueObligation {
    origin_note_id,
    origin_onset,
    target_note_id,
    required_string,
    kind,
    provenance: ProjectedImportedRelation,
}
```

Stable imported note ids are transport identity. Unknown hand state remains
explicit and never becomes fret zero. Obligations are ordered and deduplicated
by stable identity; they remain alive across any number of partitions until
their target is consumed or the voice ends.

## Ownership and lifetime

- **Producer:** after solving a partition, derives hand state only from that
  partition's chosen positions, note onsets and tap labels. Its outgoing
  technique obligation uses the chosen origin string plus the already-projected
  target stable id. It receives no target imported position or target-line
  human path.
- **Transport:** owns the state between partitions of one
  `source + track + voice`. Serialization must round-trip byte-deterministically.
  Passing unrelated partitions may update hand state but must not drop or
  retarget a pending obligation.
- **Consumer:** selects obligations by target stable id, restricts only those
  target domains, and uses known hand state only as the frozen lexicographic
  `anchor_distance` secondary. Consumed obligations disappear exactly once.

The import/projector may establish relation identity before solving; it may not
expose the target's imported string/fret to producer or consumer. Required
string comes from the producer's realized origin, not from future target data.

## Fixed population and regimes

The population remains #212's eight kept-line cross-boundary relations on
corpus fingerprint `9e53e55a19cddf29`. The runner fails closed on any identity,
count, same-voice or census drift.

For each target line compare:

1. **Independent L0** — unchanged `v1-fit`.
2. **Oracle replay O** — #212's imported-past context, retained only as the
   compatibility reference.
3. **Causal replay C** — context produced by sequentially solving preceding
   partitions under `v1-fit`, transported and consumed through the typed API.

Report context equality O↔C by field, target feasibility/agreement, whole and
non-target deterministic agreement, primary optimum membership, obligation
age in partitions, and every Unknown/expired/unconsumed case. No weights or
thresholds are selected after outcome.

## Gates and falsifiers

The transport design passes only if:

- producer construction is structurally unable to receive target imported
  positions;
- encode → decode → encode is deterministic and lossless;
- all eight projected obligations reach and are consumed by the exact stable
  target once, with no leak to another voice or line;
- feeding an equivalent serialized context reproduces #212 bit-for-bit;
- causal replay remains feasible and every difference from oracle replay is
  attributed to a producer field mismatch, not hidden future access.

Oracle/causal equality is measured, not required: disagreement would show that
solver-derived outgoing state differs from imported past state. If causal
technique strings fail to match the observed targets, the relation cannot yet
be a production hard constraint. If hand mismatches dominate, a single fret is
not a sufficient endogenous state. Either result blocks an ADR recommendation
but does not invalidate #211/#212's measurement findings.

Scope excludes production crates, public APIs, global graph solving, joined
lines, fitted objectives and changes to `TabLine` slicing or projection.

## Methodological amendment registered before rerun

Review found that the first hand-fidelity comparison joined two effects.
`TabLine::anchor_fret` is derived from the full imported voice, including
onsets excluded from retained monophonic partitions, while causal production
sees only solved retained partitions. Their tap-update semantics also differ.
The first full-oracle `1/8` remains an observed end-to-end mismatch but is
withdrawn as a pure objective-fidelity attribution.

Before rerun, hand fidelity is decomposed into three frozen producers:

- **F full-import oracle:** existing `target_line.tab.anchor_fret`;
- **P partition-compatible imported replay:** the same retained partitions and
  the same `produce_context`, supplied with imported human positions;
- **C causal solver replay:** the same retained partitions and producer,
  supplied with frozen `v1-fit` positions.

`F == P` measures representation/partition loss. `P == C` is the registered
solver-objective fidelity gate. F/P/C must use identical tap filtering,
lifetime, serialization and consumer code after their position source enters
the producer. Report string and hand comparisons separately; no threshold is
chosen after outcome.

The serialized-state gate is also strengthened. Producer and decoder must use
one shared pending-obligation canonicalizer. Exact duplicates are deduplicated;
multiple relations requiring the same string for one target are compatible;
more than one distinct required string for one `target_note_id` is rejected by
`decode_context` as `ConflictingObligation(target)` before consumer solving.
A crafted-payload regression test is required.

## Corpus outcome

The typed contract and adversarial tests were implemented before the corpus
outcome. Tests pin prefix-only production, stable-id matching after reindexing,
one-shot obligation consumption, persistent hand state, distinct
`Unknown`/`Absent`/known-zero semantics, canonical deterministic bytes,
voice/ambiguity refusal, and direct-versus-serialized fresh-consumer LHT.

The sequential runner then solved every retained partition under frozen
`v1-fit`. Each partition consumed a freshly decoded context and produced the
next context from its chosen positions. Initial `Unknown` is handled by an
explicit independent bootstrap branch; it is never converted by the contract
API to absent or fret zero. The fixed corpus and census remained unchanged:
410 files, fingerprint `9e53e55a19cddf29`, 29,758 within-line and 46
cross-line relations.

### Transport and lifetime

All eight obligations reached their unique stable target and were consumed
exactly once. All eight causal target domains remained feasible. For every
case, direct in-memory consumption and
`encode -> fresh decode -> consume -> LHT solve` produced identical context
and deterministic positions (`8/8`). Canonical ordering was independent of
input obligation order. No obligation crossed a voice or leaked beyond its
matched target.

The producer/transport/consumer mechanics therefore pass. The semantic values
produced by the frozen solver do not:

```text
solver-derived origin string == imported/oracle origin string: 1 / 8
solver-derived hand anchor   == imported/oracle anchor:        1 / 8
complete causal context      == oracle context:                1 / 8
causal deterministic target agreement:                         1 / 8
```

The mismatch is not serialization loss: transport replay equality is `8/8`.
It is upstream objective mismatch. The producer faithfully serializes the
state selected by `v1-fit`, but that state differs from the imported past in
seven cases. Consequently, deriving a hard same-string obligation from the
solver-chosen origin does not reproduce the observed target string.

Against independent L0, causal whole-line and non-target agreement each split
`3 better / 2 equal / 3 worse`. This balanced result supplies no preference
evidence and does not rescue the target failure. All imported whole paths also
remain outside the primary optimum, as in #212.

## Verdict

The experiment separates mechanism from semantics:

1. **Ownership/lifetime/serialization contract: supported in the Lab.** A
   causal producer, canonical transport and fresh consumer are partition-safe
   and deterministic for all eight obligations.
2. **Endogenous hand-state fidelity: not supported.** The single-fret state is
   transported correctly, but frozen `v1-fit` produces the imported anchor in
   only one case.
3. **Endogenous hard technique obligation: rejected for production.** The
   solver-derived origin string matches the observed relation in only one case;
   using it as a hard future constraint would faithfully propagate the wrong
   state in seven cases.
4. **ADR promotion: blocked by the registered falsifier.** #211/#212 still
   establish that imported semantic context is informative and consumable.
   This experiment shows that the current line solver cannot yet *produce* an
   equivalent context from its own path.

No alternative objective, fitted weight, exception rule or oracle fallback is
introduced. The next research question, if pursued, belongs to sequence-state
estimation/objective adequacy, not to boundary transport engineering.
