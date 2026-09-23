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
