# Graph engineering in griff: deterministic generation and selection graphs

Status: design note, not an ADR or specification  
Verified: 2026-07-27 against `main` at `e094d9ab4b7633f4a1939b73c9fa06e24bab6dd0`

## Summary

“Loop engineering is dead; enter graph engineering” is a memorable label for a
real change of scale, but not a literal replacement. Loops still perform bounded
work. A graph makes explicit how those loops and deterministic stages compose:
which node produces which typed value, which edges carry provenance, where
failure stops the pipeline, and which component has authority to choose.

For griff, the important conclusion is slightly unfashionable: the useful graph
is primarily a **deterministic musical dataflow and optimization graph**, not a
team of conversational agents.

The current repository already contains the core pieces:

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
feedback/evaluation paths explicit. It does not mean replacing rhythm, harmony,
ranking, or dynamic programming with agents that send each other paragraphs.

## Terminology

### Loop engineering

A loop repeatedly performs bounded work:

```text
produce -> inspect -> adjust -> produce again
```

In griff, the most natural loops are:

- human audition and regeneration;
- offline evaluation and policy calibration;
- future preference learning from explicit feedback;
- bounded search that deliberately changes candidate width or constraints.

### Graph engineering

A graph composes stages and loops through typed edges:

```text
input --contract--> transform --contract--> selection --contract--> output
```

Its questions are:

- What exact musical object crosses the edge?
- Is the node generative, analytical, filtering, ranking, or selecting?
- Which facts are retained as provenance?
- What is deterministic under a fixed seed or policy?
- What mismatches cause refusal rather than silent normalization?
- Where may learned or agentic judgment enter without becoming the source of
  truth for the canonical musical model?

A graph may contain loops. The phrase therefore names composition, not the death
of iteration.

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

A second authoring path, Swang, eventually rejoins the same canonical `Score`
and playback surface, but it has a deliberately different rhythm contract. The
current README states that a Swang program declaring a corpus is refused until
native corpus resolution exists; the implementation must not fake that edge by
quietly borrowing Generate-panel behavior.

## Node inventory

| Node | Current implementation | Input | Output / authority |
|---|---|---|---|
| Corpus loading boundary | caller-specific filesystem or OPFS code | files/records | parsed `ChunkMeta` and `Score`; I/O remains outside the pure compiler |
| Chunk preparation | `generation_input::prepare_chunk` | metadata + source score | provenance-respecting sliced score and exact sounding track, or reported skip |
| Corpus compiler | `generation_input::corpus_material` | prepared chunks | `CorpusMaterial`: deduped rhythms, novelty references, gesture, skips |
| Request compiler | `generation_input::ranked_candidates` | source score, optional corpus material, ask, optional rhythm override | one reproducible base request and the material actually used |
| Candidate fan-out | `rerank::generate_candidate_set` | deterministic set request | candidates over every strategy × variant seed |
| Rule generator | `generate::generate` | one `RuleGenerationRequest` | canonical `Score` plus strategy/seed provenance, or typed refusal |
| Gesture compiler | `gesture::generate_gestured` | rule request + gesture control | carved candidate under the same seed identity |
| Musical measurement | closure and novelty modules | candidate + pitch/reference material | named axes, never only an opaque scalar |
| Reranker | `rerank::rerank_candidates` | candidates, axes, versioned `WeightPolicy` | deterministic rank order in `Scored` envelopes |
| Layered optimizer | `layered_path::solve` | local/transition axes over ordered layers | exact minimum-cost path with deterministic tie-break and retained rationale |
| Candidate-chain client | `candidate_chain` | already-ranked compatible candidates | assembled multi-bar `Score` selecting one source candidate per bar |
| Human surface | cockpit history/A-B/keep flow | generated scores and provenance | audition decisions; current marks are session-local and do not yet steer generation |

This separation is already stronger than a generic “generator agent” abstraction.
Each node has a narrow mathematical or musical contract, and the canonical
`Score` remains the shared internal model.

## The graph has two different meanings

### 1. Execution and dataflow graph

This graph says which transformation consumes which value:

```text
CorpusMaterial -> RuleGenerationRequest -> SetCandidate -> Scored candidate -> Score
```

It is about ownership, provenance, and reproducibility.

### 2. Optimization graph

`layered_path` models an actual layered DAG:

```text
layer 0        layer 1        layer 2        layer 3
  s0  -------->  s0  -------->  s0  -------->  s0
  s1  -------->  s1  -------->  s1  -------->  s1
  s2  -------->  s2  -------->  s2  -------->  s2
```

The solver chooses one state per layer using local and transition axes. The
candidate-chain client interprets a layer as an output bar and a state as “bar
`b` supplied by ranked candidate `c`.”

These two meanings should not be blurred. The pipeline graph orchestrates
computation. The layered graph is the musical search problem being solved.
Using one fashionable word for both is tolerable only while the contracts stay
explicit.

## Edge contracts in the current code

### Corpus records to prepared material

The edge carries source provenance, including exact `track_index` and optional
`bar_range`. A named track that is absent or silent is not replaced by another
sounding track. That refusal prevents a quiet wrong-part substitution.

`CorpusMaterial.skipped` reports unreadable, absent, or silent records. Missing
material does not simply vanish from the explanation of a run.

### Corpus material to generation request

The rhythm authority is frozen:

```text
explicit rhythm palette > corpus rhythms > source first bar
```

An explicit palette is preserved verbatim, including silent templates, and uses
a separate scheduler with no quarter-note fallback. Corpus-derived templates
are deduplicated in first-seen order and unusable templates may fall back to the
source path according to the documented contract.

Novelty references and gesture remain corpus-based even when an explicit rhythm
palette wins. One edge does not silently seize authority over unrelated inputs.

### Request to candidate set

The fan-out axis is fixed by the declaration order of the five generation
strategies and `variants_per_strategy`. Variant seeds derive deterministically
from `(base seed, strategy index, variant index)` using a SplitMix64 finalizer.
Gesture on/off does not change those identities, allowing paired comparisons.

`RhythmCopyPitchSubstitute` is skipped only when its required template does not
exist. Other strategies do not fail merely because that one branch is
inapplicable.

### Candidate to reranked candidate

A `SetCandidate` retains:

- canonical `Score`;
- strategy;
- derived seed;
- optional gesture control.

Reranking adds six named axes, rationale, aggregate, and versioned policy
provenance. The scalar is derived convenience, not the only surviving fact. A
future learned policy may change weights, but it should consume the same named
facts rather than turning the graph into an uninspectable score oracle.

### Ranked set to candidate chain

The chain consumes an already-ranked set. It does not regenerate, reseed, or
rerank. Every selected bar can still answer which candidate, strategy, variant
seed, and original rank supplied it.

Before optimization, the client rejects incompatible candidates:

- bar-count or PPQ mismatch;
- master-timeline mismatch;
- track/voice metadata mismatch;
- source metadata or loss-report mismatch;
- cross-bar material;
- empty groups or material outside the timeline;
- missing material after validation.

Refusal is preferable to assembling a plausible score under false metadata.

### Layered problem to path solution

The domain-free engine validates every layer and transition shape. It rejects
empty layers, malformed transition tables, non-finite local/edge facts, and
non-finite accumulation.

Selection is exact dynamic programming, not greedy search or beam search. Exact
ties resolve to the lexicographically smallest state-ordinal vector. Float
addition order is itself normative: search, reported total, and client baseline
use the same right-associated recurrence. A differently folded scalar would
explain a path the engine did not actually choose.

## Determinism laws to preserve

Graph extensions must not weaken the existing SPEC-level guarantee that a fixed
input and seed produce the same result.

1. **Stable input order.** Corpus first-seen order affects rhythm-palette order;
   callers must provide deterministic record order.
2. **Stable fan-out order.** Strategy declaration order and variant indices are
   part of candidate identity.
3. **Stable seed derivation.** Adding a feature must not casually reseed all
   existing candidates.
4. **Stable precedence.** Explicit pattern, corpus, and source fallback are
   different authorities, not interchangeable suggestions.
5. **Stable scoring vocabulary.** Axes and policy versions travel with results.
6. **Stable tie-breaking.** Equivalent optima still require one canonical winner.
7. **Stable arithmetic association.** Floating-point grouping is part of the
   algorithm's semantics.
8. **No silent repair.** Invalid dimensions, timelines, or non-finite facts cause
   typed refusal.
9. **Canonical model at every join.** MIDI and UI representations remain
   boundaries; graph branches rejoin through `Score`, not frontend-specific
   structures.

## Where loops should enter

### Human audition and feedback

The cockpit already records audition history and favorite/rejected marks during
a session. S9 can turn that into a measured loop:

```text
candidate set -> audition -> explicit feedback -> versioned policy update
       ^                                             |
       +---------------------------------------------+
```

The feedback edge needs candidate identity, strategy/seed, policy version,
corpus/source identity, and the exact user action. “The user liked something
roughly like this” is not a training record.

### Offline evaluation and policy calibration

Policy changes should replay fixed, leakage-safe evaluation sets and compare:

- preference agreement;
- musical constraint violations;
- diversity and novelty;
- deterministic reproducibility;
- cost/latency of wider search;
- and regressions by source/song identity.

Evaluation runs outside the production generation path and gates changes to
weights or learned components.

### Bounded candidate search

The existing graph already supports a controlled search-width parameter through
`variants_per_strategy`. Future adaptive widening may be a loop, but its budget
and stop condition must remain external to the model or learned scorer:

```text
width N -> evaluate coverage/confidence -> widen or stop
```

A widening pass must retain all candidate identities and explain why another
round was required.

### Reachability and holdout work

The current generator-reachability audit and ADR-0031's canonical `song_id`
create another explicit graph:

```text
curated source identities -> holdout preflight -> material construction -> generation -> evaluation
```

A song-level holdout cannot proceed when identity coverage is incomplete. The
preflight refusal is a graph gate, not missing-data inconvenience to be patched
with title similarity.

## Why a multi-agent musical pipeline is the wrong default

A tempting redesign is:

```text
rhythm agent -> harmony agent -> technique agent -> critic agent
```

That would be worse than the current code unless one of those stages genuinely
requires open-ended judgment.

### It weakens determinism

Conversational output makes the same seed insufficient to reproduce the result.
Prompt, model version, sampling settings, hidden provider changes, and context
all become additional undeclared inputs.

### It obscures authority

The current rhythm precedence, pitch constraints, timeline, and scoring axes
have explicit owners. Agents negotiating these in prose make it unclear which
rule won and why.

### It destroys useful failure types

`GenerationError`, `SetError`, `PathError`, and `ChainError` name exact invalid
facts. An agent saying “I could not make this work” is a severe downgrade in
observability.

### It spends tokens on deterministic work

Rhythm placement, scale-ladder selection, closure/novelty measurement, DP, and
compatibility checking are ordinary algorithms. Replacing them with model calls
would be slower, more expensive, and less reliable, a rare architectural
hat-trick.

### The right boundary

Use a model where the task is genuinely interpretive:

- translating a user's musical description into typed constraints;
- proposing candidate edits or a Swang program;
- explaining retained scoring rationale in user-facing language;
- suggesting new axes from failure clusters;
- or assisting curation while leaving the final provenance assertion explicit.

The model proposes typed inputs. The Rust core validates, generates, scores, and
records the result.

## Useful graph-engineering increments

### 1. Candidate lineage record

Define a serializable lineage envelope covering:

- source/corpus snapshot identity;
- generation ask;
- rhythm authority and fingerprints;
- strategy and derived seed;
- gesture control;
- scoring axes and policy version;
- chain state/edge selections when used;
- final score digest and export loss report.

The code carries most of these facts already. The work is to make the end-to-end
edge durable rather than inventing another scoring system.

### 2. Evaluation graph

Make the evaluation path explicit and leakage-safe:

```text
identity preflight
    -> held-out corpus material
    -> fixed generation matrix
    -> deterministic metrics + human judgments
    -> versioned comparison report
```

No learned reranker or generator change should promote itself using the same
examples it trained on.

### 3. Feedback graph

When S9 persists feedback, separate:

- immutable audition event;
- user verdict;
- feature snapshot;
- policy-training example;
- trained policy artifact;
- acceptance evaluation.

Deleting or relabeling one stage must not rewrite what the user originally did.

### 4. Reachability graph

Use the Phase-0 inventory to map which UI control reaches which generation input,
strategy, metric, and rendered output. A control that changes only UI state must
not be described as steering generation. A generator field with no caller edge
is unreachable functionality, not a hidden advanced feature.

### 5. Shared frontend graph

Continue routing CLI, native cockpit, and web cockpit through the same pure
`generation_input` compiler and renderer-agnostic `ui-core`. Frontends may own
I/O and presentation, but not fork musical semantics.

### 6. Agentic proposal boundary

If an LLM-assisted composer is added, make it emit a versioned, typed object such
as constraints or a Swang program. Record the model/prompt provenance, then run
the normal deterministic graph. Do not let the model return an opaque MIDI blob
that bypasses the canonical model and loss accounting.

## Comparison with 007

The same vocabulary applies to both repositories, but the graphs solve different
problems.

| Concern | 007 | griff |
|---|---|---|
| Primary graph | execution, trust, evidence, control | musical dataflow, fan-out, scoring, optimization |
| Canonical edge values | events, observations, attestations, verifier evidence, ledger records | `Score`, corpus material, requests, candidates, axes, ranked sets, paths |
| Main authority question | who may execute, verify, persist, and decide a verdict | which source supplies rhythm/pitch/gesture, which policy scores, which solver selects |
| Failure posture | blocked/error/refusal must never become green | incompatible or unmeasurable musical facts must never be silently normalized |
| Determinism mechanism | versioned protocols, digest chains, pure reducer/replay | fixed seeds/order/precedence, versioned weights, exact DP and tie-breaks |
| Natural loops | retry/recovery/verification under hard budgets | audition/feedback, evaluation/calibration, bounded search widening |
| Agent role | untrusted worker or reviewer behind external gates | optional interpreter/proposer ahead of deterministic musical core |
| Immediate risk | orchestration outrunning trust and evidence wiring | fashionable agents replacing transparent algorithms |

The shared lesson is not “everything is a graph.” It is that every boundary
should name its data, authority, provenance, failure semantics, and replay story.

## Review checklist for graph changes

Before adding a node or edge, answer:

- Which present requirement or run proves it is needed?
- Is this a pipeline edge, an optimization edge, or a human-feedback edge?
- What canonical type crosses it?
- What exact source, corpus, seed, strategy, policy, and schema identities travel
  with the value?
- Is the node pure? If not, what side effect does it own?
- What is the deterministic tie-break?
- What invalid input causes typed refusal?
- Does any branch silently drop a candidate or corpus record?
- Can a frontend bypass the shared compiler?
- Can a learned component change the facts used to evaluate itself?
- Does a model propose typed input, or has it become an unversioned source of
  musical truth?
- Can the final score explain where every selected part came from?

If the answer to the last question is “the agents discussed it,” the graph has
lost information the current Rust code already knows how to preserve.

## Sources and nearby project documents

- [`hardness1020/awesome-agent-architecture`](https://github.com/hardness1020/awesome-agent-architecture) — useful vocabulary for harness loops, tasks, protocols, observability, and verification; not a reason to agentize deterministic music code.
- [`docs/SPEC.md`](SPEC.md) — hard project constraints, including canonical model and determinism.
- [`docs/glossary.md`](glossary.md) — authoritative terminology.
- [`docs/adr/0013-layered-dp-generation.md`](adr/0013-layered-dp-generation.md) and [`docs/adr/0030-reduced-state-layered-dp-clients.md`](adr/0030-reduced-state-layered-dp-clients.md) — layered optimization decisions.
- [`docs/adr/0017-explainable-scoring.md`](adr/0017-explainable-scoring.md) — named axes, policy provenance, and the anti-scalar rule.
- [`docs/adr/0029-swang-authoring-and-verified-lifting.md`](adr/0029-swang-authoring-and-verified-lifting.md) — explicit rhythm precedence and verified authoring boundary.
- [`docs/adr/0031-canonical-song-identity.md`](adr/0031-canonical-song-identity.md) — Work-level identity required for leakage-safe song holdout.
- `core/src/generation_input.rs`, `generate.rs`, `rerank.rs`, `layered_path.rs`, and `candidate_chain.rs` — the current implementation mapped by this note.