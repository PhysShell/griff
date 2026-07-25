# Proposal: Transactional editing, technique modules, candidate terrain

Cockpit-facing consequences of the 2026-07 research memos: how humans and
agents are allowed to *touch* the candidate space.

Status: proposal for discussion (v1)
Scope: docs-only until accepted; binds nothing. UX directions for S8/S16
surfaces; the editing contract would eventually deserve an ADR.

## 1. Technique modules (MOZLib model)

Instead of an army of sliders, named modules with semantic parameters:

```
Creep
  window: 5 notes
  shift: 1
  repetitions: 3
  apply boundary: phrase
  affected axes: pitch + fingering
```

Each module: semantic inputs, bounded parameters, visual explanation,
preview, provenance, apply/reject. Modules are thin UI over pattern
operators (see the reproducible-pattern-processes proposal) — the cockpit
never grows behaviour the typed layer doesn't have.

## 2. Candidate terrain (nn_terrain model, symbolic and deterministic)

A 2-D map over deterministic variants of a source:

```
X: rhythmic displacement    Y: register / contour change
color: playability or score    size: novelty    boundary: hard-constraint validity
```

Every point has a full identity — `source_hash + operator_program_hash +
seed bundle + policy version` — so clicking a point reproduces it exactly.
The candidate space is treated as axes around a source (rhythm / contour /
register / technique / complement variations), with per-axis freezing via
named seed streams, lineage display, A/B against the source, and visible
invalidity reasons. A learned latent terrain is explicitly out of scope
until the human similarity benchmark exists (see the preference-learning
proposal).

## 3. Agent editing API (MaxMSP-MCP lesson, hardened)

Operations an agent (or macro system) may perform:

```
inspect_score | inspect_selection | explain_candidate
preview_transform | validate_transform | apply_transform | undo_transform
```

Contract for every mutating operation — the editing transaction:

1. typed payload; 2. operates on an immutable snapshot; 3. returns a diff;
4. passes constraint validation *before* apply (see the constraint-contract
proposal); 5. declares scope; 6. applies at a musical boundary; 7. is
undoable; 8. records provenance.

Instead of a generic "are you sure?", confirmation states concrete
consequences:

```
preserve_favorites: true    replace_rejected: true
affected_axis: rhythm_only  apply_boundary: next_phrase
```

ML proposes; the architecture verifies. An invalid edit never destroys the
accepted snapshot (Glicol rule, shared with the pattern-processes proposal).

## 4. Timeline surface (Ossia model)

Long-term cockpit shape: processes with extent on a timeline — Guitar A,
Guitar B complement, variation processes, candidate-switch boundaries,
control curves, playback state — with process content separated from its
temporal placement. UX reference only; nothing is embedded.

## 5. Non-goals

Free-form agent access to internal state; host-code execution from the UI
(the embedded-Lisp anti-pattern); edits taking effect mid-note; any cockpit
behaviour that bypasses `ui-core` (ADR-0016) or invents styling outside the
theme tokens (ADR-0028); learned terrain before the benchmark gate.

## 6. Prior art surveyed (prior-art-first rule, AGENTS.md)

MOZLib / PWforMax (technique-as-module; embedded Lisp rejected); nn_terrain
(XY control space; made symbolic and deterministic here); Ossia Score
(timeline/process UX); MaxMSP MCP and its extended fork (agent
introspection, validation, feedback-loop detection — the validation posture
adopted, the free patching authority rejected); Glicol (invalid edits never
kill the running result); Neoscore (score as a free visual plane: phrase
bands, provenance overlays, violation overlays).
