# Graph engineering in griff: deterministic generation and selection graphs

Status: design note, not an ADR or specification  
Verified: 2026-07-27 against `main` at `e094d9ab4b7633f4a1939b73c9fa06e24bab6dd0`

## Summary

“Loop engineering is dead; enter graph engineering” is a memorable label for a
real change of scale, but not a literal replacement. Loops still perform bounded
work. A graph makes explicit how loops and deterministic stages compose: which
node produces which typed value, what provenance crosses each edge, where
failure stops the pipeline, and which component has authority to choose.

For griff, the useful graph is primarily a **deterministic musical dataflow and
optimization graph**, not a team of conversational agents. The current tree
already contains:

- one pure corpus-to-generation compiler shared by CLI, cockpit, and experiments;
- deterministic strategy × seed fan-out;
- typed generation failures and canonical `Score` output;
- explainable multi-axis reranking;
- a domain-free layered-DAG dynamic-programming engine;
- a candidate-chain client that selects one candidate bar per output bar while
  preserving compatibility and provenance;
- and a human audition/history surface that can later become the measured
  feedback loop.

Graph engineering here means preserving those contracts and making future
feedback and evaluation paths explicit. It does not mean replacing rhythm,
harmony, ranking, or dynamic programming with agents that exchange prose.

## Loop and graph engineering

Loop engineering designs one bounded cycle:

```text
produce -> inspect -> adjust -> produce again
```

In griff, natural loops include human audition/regeneration, offline evaluation,
future preference learning, and deliberately bounded widening of candidate
search.

Graph engineering composes those loops with deterministic stages:

```text
input --typed contract--> transform --typed contract--> selection --> output
```

Its questions are relational:

- What exact musical object crosses the edge?
- Is the node generative, analytical, filtering, ranking, or selecting?
- Which facts survive as provenance?
- What remains deterministic under a fixed seed or policy?
- What mismatch causes refusal rather than silent normalization?
- Where may learned or agentic judgment enter without becoming the source of
  truth for the canonical musical model?

A graph may contain loops. The graph is the composition boundary around them.

## The current griff graph

The main generation path in the current tree is:

```text
 corpus records + imported Scores
              |
              v
 generation_input::prepare_chunk / corpus_material
              |
              | CorpusMaterial
              |   rhythms
              |   novelty references
              |   gesture
              |   reported skips
              v
 generation_input::ranked_candidates
              |
              | RuleGenerationRequest + precedence facts
              v
 rerank::generate_candidate_set
              |
              | fan-out: 5 strategies × seed variants
              v
 generate::generate / gesture::generate_gestured
              |
              | SetCandidate { Score, strategy, seed, gesture }
              v
 closure + novelty axes
              |
              v
 rerank::rerank_candidates
              |
              | RankedSet / Scored<SetCandidate>
              +----------------------------+
              |                            |
              v                            v
      select ranked winner       candidate_chain + layered_path
              |                            |
              |                            | one selected bar per layer
              +-------------+--------------+
                            |
                            v
                     canonical Score
                            |
                            v
                cockpit / CLI / audition history
```

Swang is a second authoring path that eventually rejoins the same canonical
`Score` and playback surface, but it has a deliberately different rhythm
contract. The current README states that a Swang program declaring a corpus is
refused until native corpus resolution exists; the implementation must not fake
that missing edge by borrowing Generate-panel behavior.

## Node inventory

| Node | Current implementation | Input | Output / authority |
|---|---|---|---|
| Corpus loading boundary | caller-specific filesystem or OPFS code | files/records | parsed `ChunkMeta` and `Score`; I/O stays outside the pure compiler |
| Chunk preparation | `generation_input::prepare_chunk` | metadata + source score | provenance-respecting slice and exact sounding track, or reported skip |
| Corpus compiler | `generation_input::corpus_material` | prepared chunks | `CorpusMaterial`: rhythms, references, gesture, skips |
| Request compiler | `generation_input::ranked_candidates` | source score, optional corpus, ask, optional rhythm override | reproducible request and material actually used |
| Candidate fan-out | `rerank::generate_candidate_set` | deterministic set request | every applicable strategy × variant seed |
| Rule generator | `generate::generate` | `RuleGenerationRequest` | canonical `Score` with strategy/seed provenance, or typed refusal |
| Gesture compiler | `gesture::generate_gestured` | rule request + gesture | carved candidate under the same seed identity |
| Musical measurement | closure and novelty modules | candidate + pitch/reference material | named axes, never only an opaque scalar |
| Reranker | `rerank::rerank_candidates` | candidates, axes, `WeightPolicy` | deterministic rank order in `Scored` envelopes |
| Layered optimizer | `layered_path::solve` | local/transition axes over layers | exact path with deterministic tie-break and retained rationale |
| Candidate-chain client | `candidate_chain` | compatible ranked candidates | multi-bar `Score`, one supplying candidate per bar |
| Human surface | cockpit history/A-B/keep | scores and provenance | audition decisions; marks are currently session-local and non-steering |

This is already stronger than a generic “generator agent.” Each node has a
narrow mathematical or musical contract, and `Score` remains the shared
internal model.

## Two graph meanings

### Execution and dataflow graph

```text
CorpusMaterial -> RuleGenerationRequest -> SetCandidate -> Scored candidate -> Score
```

This graph is about ownership, provenance, and reproducibility.

### Optimization graph

`layered_path` models an actual layered DAG. The solver chooses one state per
layer using local and transition axes. `candidate_chain` interprets a layer as
an output bar and a state as “bar `b` supplied by ranked candidate `c`.”

The pipeline graph orchestrates computation. The layered graph is the musical
search problem. They may share vocabulary, but not contracts.

## Current edge contracts

### Corpus records to prepared material

The edge carries source provenance, including exact `track_index` and optional
`bar_range`. A named track that is absent or silent is not replaced by another
sounding track. `CorpusMaterial.skipped` reports missing, unreadable, or silent
records rather than deleting them from the run's explanation.

### Corpus material to generation request

Rhythm authority is frozen:

```text
explicit rhythm palette > corpus rhythms > source first bar
```

An explicit palette is preserved verbatim, including silent templates, and uses
a separate scheduler with no quarter-note fallback. Novelty references and
gesture remain corpus-based even when an explicit rhythm palette wins; one edge
does not silently seize authority over unrelated inputs.

### Request to candidate set

Fan-out follows the declaration order of the five strategies and
`variants_per_strategy`. Variant seeds derive deterministically from
`(base seed, strategy index, variant index)` with a SplitMix64 finalizer. Gesture
on/off does not change those identities, enabling paired comparisons.

`RhythmCopyPitchSubstitute` is skipped only when its required template is absent.
Other branches do not fail because that one strategy is inapplicable.

### Candidate to ranked candidate

A `SetCandidate` retains its canonical `Score`, strategy, derived seed, and
optional gesture control. Reranking adds six named axes, rationale, aggregate,
and versioned policy provenance. The scalar is derived convenience, not the only
surviving fact.

### Ranked set to candidate chain

The chain consumes an already-ranked set. It does not regenerate, reseed, or
rerank. Each selected bar can still name its candidate, strategy, variant seed,
and original rank.

Before optimization it rejects incompatible candidates: bar-count, PPQ,
master-timeline, track/voice metadata, source metadata, loss report, cross-bar
material, empty groups, outside-timeline material, and missing material. Refusal
is preferable to assembling a plausible score under false metadata.

### Layered problem to path solution

The domain-free engine rejects empty layers, malformed transition dimensions,
non-finite local or edge facts, and non-finite accumulation. Selection is exact
dynamic programming, not greedy or beam search. Exact ties resolve to the
lexicographically smallest state-ordinal vector.

Float addition order is normative: search, reported total, and client baseline
use the same right-associated recurrence. A differently folded scalar would
explain a path the engine did not choose.

## Determinism laws to preserve

1. **Stable input order.** Corpus first-seen order affects rhythm-palette order.
2. **Stable fan-out order.** Strategy order and variant index are candidate identity.
3. **Stable seed derivation.** New features must not casually reseed old candidates.
4. **Stable precedence.** Explicit, corpus, and source rhythm are distinct authorities.
5. **Stable scoring vocabulary.** Axes and policy versions travel with results.
6. **Stable tie-breaking.** Equivalent optima still require one canonical winner.
7. **Stable arithmetic association.** Floating-point grouping is algorithm semantics.
8. **No silent repair.** Invalid dimensions, timelines, or facts cause typed refusal.
9. **Canonical model at joins.** MIDI and UI are boundaries; branches rejoin as `Score`.

## Where loops should enter

### Human audition and feedback

The cockpit already records audition history and favorite/rejected marks. S9 can
turn that into a measured loop:

```text
candidate set -> audition -> explicit feedback -> versioned policy update
       ^                                             |
       +---------------------------------------------+
```

The feedback edge needs candidate identity, strategy/seed, policy version,
corpus/source identity, and exact user action. A vague statement that the user
liked “something similar” is not a training record.

### Offline evaluation and policy calibration

Policy changes should replay fixed, leakage-safe evaluation sets and compare
preference agreement, constraint violations, diversity, novelty, reproducibility,
search cost, and regressions by source/song identity. Evaluation stays outside
the production path and gates changes to learned or hand-tuned policies.

### Bounded candidate search

`variants_per_strategy` is already an explicit search-width control. Future
adaptive widening may loop, but budget and stop conditions remain outside the
learned scorer:

```text
width N -> evaluate coverage/confidence -> widen or stop
```

Every pass must retain candidate identities and explain why another round ran.

### Reachability and holdout

Generator reachability and ADR-0031's `song_id` imply another graph:

```text
identity preflight -> held-out material -> generation -> evaluation
```

Song-level holdout refuses incomplete identity coverage. It must not patch the
edge with title similarity and call that fail-closed.

## Why multi-agent music generation is the wrong default

A tempting topology is:

```text
rhythm agent -> harmony agent -> technique agent -> critic agent
```

It would usually be worse than the current code.

- **Determinism weakens.** Seed is no longer sufficient; prompt, model version,
  sampling, provider behavior, and context become undeclared inputs.
- **Authority blurs.** Rhythm precedence, pitch constraints, timeline, and axes
  currently have explicit owners; prose negotiation hides which law won.
- **Failure types disappear.** `GenerationError`, `SetError`, `PathError`, and
  `ChainError` name exact invalid facts. “The agent could not do it” does not.
- **Deterministic work becomes expensive.** Placement, scale selection,
  measurement, compatibility checks, and DP are ordinary algorithms.

Use a model where work is genuinely interpretive: translating a musical request
into typed constraints, proposing a Swang program, explaining retained rationale,
suggesting axes from failure clusters, or assisting curation. The model proposes
typed input; Rust validates, generates, scores, and records.

## Useful graph-engineering increments

### Candidate lineage record

Define one serializable envelope for source/corpus snapshot, generation ask,
rhythm authority and fingerprints, strategy/seed, gesture, scoring axes/policy,
chain selections, final score digest, and export loss report. Most facts already
exist; the useful work is making their end-to-end edge durable.

### Evaluation graph

```text
identity preflight
    -> held-out corpus material
    -> fixed generation matrix
    -> deterministic metrics + human judgments
    -> versioned comparison report
```

No learned component should promote itself on its training examples.

### Feedback graph

Persist immutable audition event, user verdict, feature snapshot, training
example, trained policy artifact, and acceptance evaluation as separate stages.
Relabeling a later stage must not rewrite what the user originally did.

### Reachability graph

Map each UI control to the generation input, strategy, metric, and rendered
output it actually reaches. A UI-only control must not be described as steering
generation; a generator field with no caller edge is unreachable functionality.

### Shared frontend graph

Keep CLI, native cockpit, and web cockpit on the same pure `generation_input`
compiler and renderer-agnostic `ui-core`. Frontends own I/O and presentation,
not separate musical semantics.

### Agentic proposal boundary

An LLM-assisted composer should emit a versioned typed object, such as
constraints or a Swang program. Record model/prompt provenance, then run the
normal deterministic graph. Do not accept an opaque MIDI blob that bypasses the
canonical model and loss accounting.

## Comparison with 007

| Concern | 007 | griff |
|---|---|---|
| Primary graph | execution, trust, evidence, control | musical dataflow, fan-out, scoring, optimization |
| Canonical edge values | events, observations, attestations, verifier evidence, ledger records | `Score`, corpus material, requests, candidates, axes, ranked sets, paths |
| Authority question | who may execute, verify, persist, and decide | which source supplies material, which policy scores, which solver selects |
| Failure posture | blocked/error/refusal never becomes green | incompatible or unmeasurable facts are never silently normalized |
| Determinism | protocols, digest chains, pure reducer/replay | fixed seeds/order/precedence, versioned weights, exact DP |
| Natural loops | retry, recovery, verification under hard budgets | audition, feedback, evaluation, bounded search |
| Agent role | untrusted worker/reviewer behind external gates | optional interpreter/proposer before deterministic core |
| Immediate risk | orchestration outrunning evidence wiring | fashionable agents replacing transparent algorithms |

The shared lesson is not merely that everything can be drawn as a graph. Every
boundary must name its data, authority, provenance, failure semantics, and
reproduction story.

## Review checklist

Before adding a node or edge, answer:

- Which current requirement or run proves it is needed?
- Is it a pipeline, optimization, or feedback edge?
- What canonical type crosses it?
- Which source, corpus, seed, strategy, policy, and schema identities travel?
- Is the node pure? If not, which side effect does it own?
- What is the deterministic tie-break?
- What invalid input causes typed refusal?
- Does any branch silently drop a candidate or corpus record?
- Can a frontend bypass the shared compiler?
- Can a learned component alter the facts used to evaluate itself?
- Does a model propose typed input, or become an unversioned source of truth?
- Can the final score explain where every selected part came from?

If the last answer is “the agents discussed it,” the graph has lost information
the current Rust code already knows how to preserve.

## Sources and nearby project documents

- [`hardness1020/awesome-agent-architecture`](https://github.com/hardness1020/awesome-agent-architecture) — harness vocabulary, not a reason to agentize deterministic music code.
- [`docs/SPEC.md`](SPEC.md) — canonical model and determinism constraints.
- [`docs/glossary.md`](glossary.md) — authoritative terminology.
- [`docs/adr/0013-dp-viterbi-traversal.md`](adr/0013-dp-viterbi-traversal.md) and [`docs/adr/0030-reduced-state-layered-dp-clients.md`](adr/0030-reduced-state-layered-dp-clients.md) — layered optimization decisions.
- [`docs/adr/0017-explainable-scoring-contract.md`](adr/0017-explainable-scoring-contract.md) — axes, policy provenance, and the anti-scalar rule.
- [`docs/adr/0029-swang-authoring-and-verified-lifting.md`](adr/0029-swang-authoring-and-verified-lifting.md) — rhythm precedence and verified authoring.
- [`docs/adr/0031-canonical-song-identity.md`](adr/0031-canonical-song-identity.md) — identity for leakage-safe song holdout.
- `core/src/generation_input.rs`, `generate.rs`, `rerank.rs`, `layered_path.rs`, and `candidate_chain.rs` — current implementation mapped here.
