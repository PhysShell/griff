# Swang exact canonical score text — census and representability

Language level: **2** (allocated by [`spec.md`](spec.md) §5.7, not yet
frozen). Stage: [S16](../stages/S16-swang-language-and-verified-lifting.md)
Phase 4A. Task: SWG-4A-01 in
[`foundation-backlog.md`](foundation-backlog.md).

This document is **normative for level 2** once Phase 4A is accepted. Until
then it is the accepted inventory and the accepted representability
decisions; the concrete grammar follows in this same document and freezes
with the phase.

It deliberately begins with an exhaustive field census rather than an
attractive `.swg` example. An example shows what *is* representable; only
the census shows what is *not*, and Phase 4A breaks on the second.

## Scope

In scope: what every canonical fact looks like as text, whether it is
required, how collections order, its one canonical spelling, its empty
form, and its malformed cases — then the grammar that follows from those.

Out of scope, and not present anywhere in this task's commits: parser code,
`SourceMap`, `ScoreBuilder`, `dump` / `verify`, patches, authoring
operators, and **any change to the canonical `Score` model**. Where the
census finds an ugly canonical fact, the text is honestly ugly with it. A
grammar task that quietly edits `griff-core` leaves a history in which the
only way to learn why the model changed is carbon-dating the git log.

## Method — two exhaustiveness obligations

> Exact comparison coverage and textual decomposition coverage are separate
> exhaustiveness obligations.

This forecloses a very natural error: "`ExactSemanticDiff` is exhaustive,
therefore the grammar is exhaustive." It is not. The comparator settles
`Tuning` with a single `==`; the serializer must still pronounce every open
string, in order.

So the task builds two inventories:

```text
A. Exact comparison inventory      what ExactSemanticDiff distinguishes
B. Textual decomposition inventory what must be physically written to
                                   reconstruct each fact in A
```

B is strictly larger than A, and the derivation runs **B from the model, A
as the proof that B covers every compared fact** — never the reverse.
Deriving B from A alone would leave the internals of `Repeat`, `Tuning`,
`Marks`, `Tempo`, `TickRange`, `TechniqueEvidence`, and `NotePosition`
unspecified, because each is one field to the comparator.

## 1. Inventory A — the exact comparison surface

`SemanticField` (`core/src/semantic_diff.rs`) has **45** variants. They
partition exactly, with none unused:

| Partition | Count | Variants |
| --- | --- | --- |
| Exact-only | 23 | `AbsoluteStart` `Atoms` `BarIndex` `Duration` `EventGroups` `Evidence` `Format` `Kind` `LossWarnings` `MasterBars` `Message` `NearestMicros` `Position` `Repeat` `SourceMeta` `Technique` `TechniqueSpans` `TickRange` `TicksPerQuarter` `TimeSignature` `TrackIndex` `TupletDen` `TupletNum` |
| Walked by both | 11 | `Channel` `Id` `Index` `Marks` `Name` `Pitch` `Tempo` `Tracks` `Tuning` `Velocity` `Voices` |
| Normalized-only | 11 | `Bars` `DurTick` `EndTick` `NoteFret` `NoteString` `Notes` `OnsetTick` `Ppqn` `Spans` `StartTick` `TimeSig` |

**The exact walker uses 34 of the 45.** The eleven normalized-only variants
are facts of the `NormalizedMusicalDiff` projection, not of the canonical
tree: `Ppqn` and `TimeSig` are the projection's own spellings of facts the
exact walker already covers as `TicksPerQuarter` and `TimeSignature`, and
`OnsetTick` / `DurTick` / `NoteString` / `NoteFret` are flattened
projection leaves with no canonical field behind them.

Reading all 45 as canonical fields would import projection-only facts into
exact text. **Inventory B is checked against the 34, never the 45.**

## 2. Inventory B — textual decomposition census

Every field of the canonical tree, from `core/src/score.rs` and
`core/src/event.rs`. "Req" is whether the text must carry it; "Empty" is
the spelling when a collection or option is absent.

### 2.1 `Score`

| Field | Type | Req | Order | Empty | Malformed |
| --- | --- | --- | --- | --- | --- |
| `ticks_per_quarter` | `u16` | yes | — | — | `0` violates `InvalidTicksPerQuarter` (see H5) |
| `master_bars` | `Vec<MasterBar>` | yes | **positional, load-bearing** | omitted when empty (§6.2) | — |
| `tracks` | `Vec<Track>` | yes | **positional, load-bearing** | omitted when empty (§6.2) | — |
| `source_meta` | `Option<SourceMeta>` | optional | — | omitted when `None` | — |
| `loss` | `LossReport` | yes | warning order **load-bearing** | omitted when clean (§6.2) | — |

`source_meta: None` and `Some(SourceMeta { format: None })` are **different
values** and must have different spellings. The exact walker compares
`SourceMeta` and `Format` as separate fields, so collapsing them would be a
loss the diff can see.

### 2.2 `MasterBar`

| Field | Type | Req | Order | Empty | Malformed |
| --- | --- | --- | --- | --- | --- |
| `index` | `usize` | yes | — | — | may disagree with position (H4); platform-sized (H3) |
| `tick_range` | `TickRange { start, end }` | yes | — | `start == end` legal | `start > end` violates `InvalidTickRange` (H5) |
| `time_signature` | `{ numerator: u8, denominator: u8 }` | yes | — | — | numerator `0`, non-power-of-two denominator (H5) |
| `tempo` | `Tempo` (private rational) | yes | — | — | see H1 |
| `repeat` | `RepeatMarker { start: bool, play_count: u8 }` | yes | — | `{ false, 0 }` = no barline | `play_count == 1` is degenerate but legal |

`index` is written as its **stored value**, never as the vector position —
see H4. `repeat` decomposes into both fields: the comparator treats
`RepeatMarker` as one `Repeat` field, but `{ true, 0 }` and `{ false, 2 }`
are distinct and both must be spellable.

### 2.3 `Track`

| Field | Type | Req | Order | Empty | Malformed |
| --- | --- | --- | --- | --- | --- |
| `name` | `Option<String>` | optional | — | omitted when `None`; `Some("")` is distinct | arbitrary UTF-8 (H2) |
| `channel` | `u8` | yes | — | — | `>15` is undocumented but type-legal |
| `tuning` | `Tuning(Vec<Pitch>)` | yes | **positional — string 1 first** | empty vector is legal | — |
| `voices` | `Vec<Voice>` | yes | **positional, load-bearing** | omitted when empty (§6.2) | — |

`Tuning`'s vector is index 0 = string 1 (highest). Order is semantics, not
presentation: reversing it retunes the instrument. The comparator settles
`Tuning` with one `==`, which is exactly why B must decompose it.

`channel` carries no documented range (`Track.channel` is "dominant MIDI
channel (0–15)" in prose, with no `ValidationError` variant), so values
above 15 are model-valid and must be spellable.

### 2.4 `Voice` and `EventGroup`

| Field | Type | Req | Order | Empty | Malformed |
| --- | --- | --- | --- | --- | --- |
| `Voice.id` | `u8` | yes | — | — | duplicates within a track are legal |
| `Voice.event_groups` | `Vec<EventGroup>` | yes | **positional, load-bearing** | omitted when empty (§6.2) | — |
| `EventGroup.kind` | `EventGroupKind` | yes | — | — | `Tuplet { 0, 0 }` is legal |
| `EventGroup.atoms` | `Vec<AtomEvent>` | yes | **positional, load-bearing** | omitted when empty (§6.2) | — |
| `EventGroup.technique_spans` | `Vec<TechniqueSpan>` | yes | **positional, load-bearing** | omitted when empty (§6.2) | range may fall outside the group's atoms |

`EventGroupKind` has six variants and exactly one carries a payload:

```text
Single | Chord | Arpeggio | Strum | Tuplet { num: u8, den: u8 } | Grace
```

All six need distinct spellings, and `Tuplet` opens its payload — the
comparator has separate `TupletNum` and `TupletDen` fields, so a text that
wrote only "tuplet" would lose a fact the diff reports.

### 2.5 `AtomEvent`

Two variants, distinguished in text; a rest is never an absence.

| Field | Type | Req | Empty | Malformed |
| --- | --- | --- | --- | --- |
| `Note.absolute_start` | `Ticks(u32)` | yes | — | — |
| `Note.duration` | `Ticks(u32)` | yes | — | `0` is legal |
| `Note.pitch` | `Pitch(u8)` | yes | — | `>127` violates `PitchOutOfRange` (H5) |
| `Note.velocity` | `Velocity(u8)` | yes | — | `>127` violates `VelocityOutOfRange` (H5) |
| `Note.marks` | `NoteMarks` (bitset) | yes | empty set written | — |
| `Note.position` | `Option<NotePosition>` | optional | omitted when `None` | string may exceed the tuning |
| `Rest.absolute_start` | `Ticks(u32)` | yes | — | — |
| `Rest.duration` | `Ticks(u32)` | yes | — | `0` is legal |

`absolute_start` plus `duration` is the whole story: S16 requires that the
writer **never synthesizes ties**, and a note crossing a barline stays one
note whose duration crosses the barline.

`NoteMarks` decomposes into a list of the seven declared marks —

```text
Accent | Ghost | Staccato | DeadNote | HarmonicNatural | HarmonicPinch | Tap
```

— in `NoteMark::ALL` declaration order, which is already what
`NoteMarks::iter` yields. Canonical order is that order, not the order an
author typed. This list is *exhaustive by construction*: see §4, "verified
not holes".

`NotePosition` decomposes into `FretboardPosition { string: u8, fret: u8 }`
plus its own `TechniqueEvidence` — a note's position evidence is
independent of any span's.

### 2.6 `TechniqueSpan` and `TechniqueEvidence`

| Field | Type | Req | Malformed |
| --- | --- | --- | --- |
| `technique` | `SpanTechnique` (8 variants) | yes | — |
| `tick_range` | `TickRange` | yes | `start > end` (H5); may exceed the group |
| `evidence.source` | `Explicit \| InferredFromMidi` | yes | — |
| `evidence.confidence` | `ConfidenceBps(u16)` | yes | any `0..=10_000`, unpaired from `source` |

`SpanTechnique`: `Slide` `Bend` `Legato` `PalmMute` `HammerOn` `PullOff`
`Vibrato` `LetRing`. Eight spellings, no wildcard.

`TechniqueEvidence` is one `Evidence` field to the comparator and two facts
in text. The pairing is **not** enforced by the type — `explicit()` and
`inferred()` are conveniences, and the struct's fields are public — so
`Explicit` at `0` bps and `InferredFromMidi` at `10_000` bps are both
model-valid and both must be spellable. S16 required control 7 ("no
inferred MIDI technique is emitted as explicit Guitar Pro evidence") is a
constraint on *importers*, not a licence for the text to drop the
combination.

### 2.7 `SourceMeta` and `LossReport`

| Field | Type | Req | Empty | Malformed |
| --- | --- | --- | --- | --- |
| `SourceMeta.format` | `Option<String>` | optional | omitted when `None`; `Some("")` distinct | arbitrary UTF-8 (H2) |
| `LossReport.warnings` | `Vec<ImportWarning>` | yes | omitted when clean (§6.2) | — |

`ImportWarning` has four variants, three with payloads:

```text
TrackNameInvalidUtf8 { track_index: usize }          ← H3
SmpteTimingUnsupported
TempoApproximated { bar_index: usize, nearest_micros: u32 }   ← H3
Other(String)                                         ← H2
```

Warning **order is load-bearing** — `LossReport::add` appends and `absorb`
concatenates, and the comparator walks `LossWarnings` positionally. The
exact walker distinguishes `TrackIndex`, `BarIndex`, `NearestMicros`, and
`Message`, so every payload opens in text.

Losses do not sound. They are still exact semantics, and the writer keeps
them for the same reason a receipt keeps the tax line.

## 3. Writer domain — the decision

> Writer domain is the inhabited canonical model, not the subset considered
> musically well-formed by the author of the text grammar.

Phase 4A requires `parse(format(score)) ~= score` under `ExactSemanticDiff`.
For every state the writer can emit, exactly one of these must therefore
hold:

1. the writer prints it **and** the builder accepts it back; or
2. `format` is explicitly **partial**, over a normative admissibility
   predicate stated here.

A third arrangement — writer prints, parser reads, builder refuses on taste
— is forbidden: the writer's own output would fail to round-trip and the
phase's central law would be false by construction.

**Decision.** Arrangement 2, with the predicate defined as *the conjunction
of the model's own existing invariants* — those that already have a
`ValidationError` variant in `core/src/event.rs`:

```text
ticks_per_quarter > 0                    InvalidTicksPerQuarter
every Pitch <= 127                       PitchOutOfRange
every Velocity <= 127                    VelocityOutOfRange
every TimeSignature numerator > 0        InvalidTimeSignatureNumerator
every TimeSignature denominator is a
  non-zero power of two                  InvalidTimeSignatureDenominator
every TickRange start <= end             InvalidTickRange
```

`Tempo` positivity and `ConfidenceBps` range need no clause: their fields
are private and the types enforce them.

This predicate **invents nothing**. Every clause is a rule `griff-core`
already states, with an error variant already named. Anything the model
permits and this list does not forbid **must be spellable**, however ugly:

- empty `master_bars`, `tracks`, `voices`, `event_groups`, `atoms`,
  `technique_spans`, and `Tuning`;
- `EventGroupKind::Tuplet { num: 0, den: 0 }`;
- zero-duration notes and rests;
- duplicate `Voice.id` within one track;
- `MasterBar.index` disagreeing with its position; overlapping or unordered
  master-bar ranges;
- a `TechniqueSpan` whose range falls outside its group's atoms;
- `NotePosition.string` beyond the track's tuning length;
- `RepeatMarker { start: false, play_count: 1 }`;
- `Explicit` evidence at `0` bps, `InferredFromMidi` at `10_000` bps;
- `Track.channel` above 15.

SWG-4A-09's builder may enforce the six clauses above and any other
invariant `griff-core` already states. It may **not** invent new musical
correctness. Otherwise Swang quietly becomes a `Score` validator, which is
emphatically not the job it was given.

## 4. Representability holes

### H1 — `Tempo` can be read as a rational but not built as one

`Tempo`'s fields are private. Public constructors: `from_bpm_integer(u32)`,
`from_micros_per_quarter(u32)`, `FALLBACK_120`. Public readers:
`bpm_numerator()`, `bpm_denominator()`. No rational constructor exists
anywhere in `core/`. So a writer can emit `num/den` that a parser cannot
feed back.

Inhabited set — and since the fields are private, inhabited is exactly
constructible:

```text
den == 1:  any num in 1..=u32::MAX
           (from_bpm_integer; no divisibility condition, so 7/1 exists
            even though 7 does not divide 60_000_000)

den > 1:   gcd(num, den) == 1
           (60_000_000 * den) % num == 0
           micros = (60_000_000 * den) / num, and micros <= u32::MAX
```

So `100/7` BPM exists (`micros = 4_200_000`); `7/2` does not — `7` does not
divide `60e6`, and `den != 1` closes the integer branch.

**Resolution, with no `griff-core` change.** Text spells the reduced
rational. The builder reconstructs with three checks:

1. `gcd(num, den) == 1` — the written fraction is already reduced;
2. `(60e6 * den) % num == 0` — the division is exact;
3. the quotient fits `u32`;

then `den == 1 → from_bpm_integer(num)`, otherwise
`from_micros_per_quarter(micros)`, otherwise a typed diagnostic.

**Plus a readback assertion**: after `from_micros_per_quarter`, verify
`bpm_numerator() == num && bpm_denominator() == den`. Check 1 already
rejects an unreduced `200/14`, but the readback proves the different and
stronger property — that the builder holds the tempo the *document
asserted*, not merely some equivalent value reached through the API.

**Rejected alternative:** spelling the provenance form, `tempo micros
4200000`. Reduction is not injective, so the originating `micros` is
unrecoverable from a `Tempo`; writing one would be inventing a fact the
model does not hold. Three notions stay separate here:

```text
stored semantic value  ≠  constructor provenance  ≠  textual admissibility
```

`200/14` is the negative witness for the third: mathematically the same
value, and still not canonical exact text, because canonical text must
choose exactly one spelling.

### H2 — arbitrary `String` needs escapes level 1 deliberately lacks

`Track.name`, `SourceMeta.format`, and `ImportWarning::Other` hold
arbitrary UTF-8 — quotes, backslashes, LF, CR, control characters. Level
1's `StringLiteral` *rejects* quotes and newlines by construction, and its
lexer has no escape sequences at all. Level 2 must therefore add a string
lexeme with real escapes: the first place level 2 needs lexical machinery
level 1 does not have.

Because the escape set decides canonical spelling, it is a **single
canonical encoding policy**, not merely "strings have escapes" — exactly
one spelling per string value, or `fmt` idempotence dies.

**`char::is_control()` must not decide it.** Spec §1.2 bans
Unicode-table-dependent classification from observable semantics: the same
byte would canonicalize differently under two Unicode versions, and the
formatter's fixed point would depend on which table the build linked
against. The escaped set is an explicit, frozen range list. Humanity has
managed to make even string escaping a reproducibility question.

The policy itself is fixed in the grammar section of this document.

### H3 — three `usize` fields in a hashed tree — **prerequisite blocker**

`MasterBar.index`, `ImportWarning::TrackNameInvalidUtf8.track_index`, and
`ImportWarning::TempoApproximated.bar_index` are `usize`, inside a graph
that derives `Hash`. Spec §1.2 forbids platform-sized integers in hashed or
serialized state. A document written on a 64-bit host with an index above
`u32::MAX` cannot exact-round-trip on a 32-bit target, so the portable
exact format is, today, not portable.

```text
discovered pre-existing model violation (spec §1.2)
→ out of scope to repair in SWG-4A-01
→ BLOCKS the round-trip gate and the level-2 freeze
→ separate fixed-width migration, its own scope and commit chain
```

Dependency shape:

```text
SWG-4A-01 exact grammar
   │ discovers and pins the requirement
   ↓
fixed-width core migration          (SWG-CORE-01, its own scope)
   ├──────────────┐
   ↓              ↓
4A writer       4A builder
   └──────┬───────┘
          ↓
   round-trip gate (SWG-4A-12)
          ↓
   level 2 freeze
```

Scope of the block, precisely: SWG-4A-02 (`ExactScoreDocument`, a transient
syntax form) need **not** wait, provided it models these integer fields
abstractly instead of baking a width in. The migration must close before
the production writer/parser-builder round trip, and certainly before
SWG-4A-12 and Phase 4A acceptance — until then the portability claim is
false.

Whether the replacement is `u32` or `u64` is that task's decision on
evidence, never the grammar picking a width because it needed one.

Level 2 remains *allocated* while this stands. It cannot honestly be
*frozen*, which is precisely the distinction spec §5.3 exists to keep
available.

### H4 — `MasterBar.index` is not the vector position

`master_bars[i].index` need not equal `i`, and the exact walker compares
the stored `Index` as its own fact while walking the vector positionally.
Exact text writes the **stored value**. Deriving it from the position would
make a disagreeing import unrepresentable — a silent normalization wearing
the costume of a convenient shorthand.

### H5 — checked constructors are bypassable, so "valid" is weaker than it looks

`Pitch(pub u8)`, `Velocity(pub u8)`, `Ticks(pub u32)`,
`TimeSignature { pub numerator, pub denominator }`,
`TickRange { pub start, pub end }`, and `Score.ticks_per_quarter: u16` all
expose their fields publicly. `Pitch::new`, `Velocity::new`,
`TimeSignature::new`, and `TickRange::new` check their ranges; `Pitch(200)`
and `TickRange { start: Ticks(9), end: Ticks(4) }` compile and skip the
check entirely.

So the type-inhabited set is strictly larger than the invariant-valid set.
§3 resolves this for the text — the writer domain is the invariant-valid
set, because those invariants are the model's own — but the underlying gap
is a `griff-core` encapsulation issue, not a text issue:

```text
discovered pre-existing encapsulation gap
→ out of scope to repair in SWG-4A-01
→ does NOT block the grammar (§3's predicate covers the text)
→ filed as SWG-CORE-02; a later decision on sealing these newtypes
```

Unlike H3 this is not a freeze blocker: exact text is well defined over the
invariant-valid domain either way. It is recorded because a reader who sees
`Pitch::new` and concludes pitches are always in range will be wrong, and
because a future builder that trusts the type instead of checking will be
wrong in a more expensive way.

## 5. Prerequisites this task pins and does not perform

| ID | What | Blocks |
| --- | --- | --- |
| SWG-CORE-01 | Replace the three `usize` fields with a fixed width (H3) | round-trip gate, level-2 freeze |
| SWG-CORE-02 | Decide whether the canonical newtypes seal their fields (H5) | nothing; recorded for a later decision |

Both are `griff-core` scopes with their own commit chains. Neither is
touched by SWG-4A-01.

## 6. The grammar

Derived from §2's census and §3's domain decision, and not before them. It
inherits level 1's shape — `word value` pairs, canonical order fixed by the
formatter, no invented defaults — because the two levels are read by one
build and should not feel like two languages.

### 6.1 The reference document

```text
swang 2

score {
    ppqn 960

    master_bar {
        index 0
        ticks 0..3840
        meter 4/4
        tempo 120/1
    }

    master_bar {
        index 1
        ticks 3840..7680
        meter 7/8
        tempo 100/7
        repeat { start true play_count 2 }
    }

    track {
        name "Guitar"
        channel 0
        tuning [64 59 55 50 45 40]

        voice {
            id 0

            group chord {
                note { at 0 duration 480 pitch 40 velocity 96 }
                note { at 0 duration 480 pitch 47 velocity 96 marks [accent] }
                span palm_mute {
                    ticks 0..480
                    evidence { source explicit confidence 10000 }
                }
            }

            group single {
                rest { at 480 duration 240 }
            }

            group tuplet 3/2 {
                note {
                    at 720
                    duration 160
                    pitch 52
                    velocity 80
                    position {
                        string 4
                        fret 2
                        evidence { source inferred_from_midi confidence 5000 }
                    }
                }
            }
        }
    }

    source { format "GP5" }

    loss {
        tempo_approximated { bar_index 1 nearest_micros 4200000 }
    }
}
```

### 6.2 Canonical order and omission

Field order inside every block is fixed by the formatter, in the order the
census lists it; an author may write words in any order and `fmt` normalizes
them. Collections are written in their vector order, which is semantics
(§2.3, §2.7), never sorted.

Exactly three things are omitted, and none of them is an invented default:

| Omitted when | Spells | Why it is not a default |
| --- | --- | --- |
| an `Option` is `None` | absence | `Some("")` is written as `""`, so absence is unambiguous |
| a collection is empty | absence | a collection has no `None`, so absence can only mean empty |
| `repeat` equals `RepeatMarker::default()` | absence | the model itself documents `default()` as "no repeat barline" |

Everything else is written every time. `ppqn`, `index`, `ticks`, `meter`,
`tempo`, `channel`, `tuning`, `id`, `at`, `duration`, `pitch`, and
`velocity` are required words: level 1 refuses to invent defaults over
frozen semantics (spec §3.5 law 7) and level 2 does not get a discount.

### 6.3 Scalars and their one spelling

| Form | Spelling | Non-canonical → |
| --- | --- | --- |
| integer | decimal, no leading zeros, no separators, no sign | `SWG0505` |
| tick range | `<start>..<end>` | `SWG0505` |
| meter | `<numerator>/<denominator>` | `SWG0505` |
| tempo | reduced `<num>/<den>`, `den` written even when `1` | `SWG0505` |
| confidence | basis points, `0..=10000` | `SWG0506` |
| boolean | `true` / `false` | `SWG0402` |
| pitch list | `[p1 p2 …]`, space-separated, vector order | `SWG0505` |
| mark list | `[m1 m2 …]` in `NoteMark::ALL` order | `SWG0505` |

`tempo 120/1` keeps its denominator so that one production covers every
tempo and the reduced-fraction law has a single shape to check.

Mark order is the declaration order of `NoteMark::ALL` — `accent`, `ghost`,
`staccato`, `dead_note`, `harmonic_natural`, `harmonic_pinch`, `tap` — which
is already what `NoteMarks::iter` yields. A set has no author order to
preserve, so canonical order is the only order.

### 6.4 Closed word sets

No wildcards; an unknown name is `SWG0402` listing the set.

```text
group kind    single | chord | arpeggio | strum | tuplet <num>/<den> | grace
span          slide | bend | legato | palm_mute | hammer_on | pull_off
              | vibrato | let_ring
mark          accent | ghost | staccato | dead_note | harmonic_natural
              | harmonic_pinch | tap
evidence src  explicit | inferred_from_midi
loss          track_name_invalid_utf8 | smpte_timing_unsupported
              | tempo_approximated | other
atom          note | rest
```

`tuplet` is the only group kind carrying a payload, and it is written
inline — `group tuplet 3/2` — because the payload is part of the kind, not
a field of the group.

### 6.5 Strings — the canonical encoding policy

Level 2's string lexeme is the one piece of lexical machinery level 1 does
not have (H2). Exactly one spelling per string value:

- every code point **outside** the escaped set is written through as UTF-8,
  unescaped;
- `"` is `\"` and `\` is `\\`, always;
- U+000A is `\n`, U+000D is `\r`, U+0009 is `\t`;
- every code point in the enumerated set

  ```text
  U+0000..=U+0008   U+000B..=U+000C   U+000E..=U+001F   U+007F..=U+009F
  ```

  is written `\u{h}` with **lowercase** hex digits and no leading zeros.

The escaped set is this frozen range list and nothing else. It is not
`char::is_control()`, not `char::is_whitespace()`, and not any other
Unicode-table predicate: §1.2 keeps table-dependent classification out of
observable semantics, and a formatter whose fixed point moved when the
build relinked a newer Unicode table would violate it. A malformed or
non-canonical escape is `SWG0508`.

### 6.6 Diagnostics

Level 2 reuses the `04xx` syntax class wherever the meaning is identical to
level 1's — §5.10 forbids one number meaning two things, and these mean the
same thing:

| Code | Reused meaning |
| --- | --- |
| `SWG0401` | malformed syntax, unexpected token, structural violation |
| `SWG0402` | unknown name in a closed word set |
| `SWG0403` | required word missing from a construct |
| `SWG0404` | word repeated within a construct |

Four codes are new, because these failures do not exist at level 1:

| Code | Meaning |
| --- | --- |
| `SWG0505` | non-canonical spelling: leading zeros, an unreduced tempo, a mark list out of canonical order, or a written-out omissible default |
| `SWG0506` | a value violates a canonical-model invariant named in §3 (pitch or velocity above 127, zero `ppqn`, zero meter numerator, non-power-of-two meter denominator, inverted tick range) |
| `SWG0507` | the tempo is a reduced fraction the canonical model cannot construct (H1's third branch) |
| `SWG0508` | malformed or non-canonical string escape |

No block is reserved beyond these four. Errors that do not exist yet get
numbers when they do.

## 7. Negative matrix

Each row needs a positive fixture, a malformed negative fixture, and a
recorded round-trip result. The list is the alphaTab #1484 expressibility
checklist as S16 requires, plus the rows this census added.

| Feature | Canonical representation | Swang representation | Negative fixture |
| --- | --- | --- | --- |
| note crossing a barline | one `AtomNote`, duration spans the line | one `note`, no tie synthesized | a text that splits it into two notes must not compare equal |
| tuplet | `Tuplet { num, den }` | `group tuplet 3/2` | `group tuplet 3` (missing den) |
| degenerate tuplet | `Tuplet { 0, 0 }` | `group tuplet 0/0` | — (legal; must round-trip) |
| multiple voices | `Vec<Voice>` | repeated `voice` blocks | duplicate `id` must round-trip, not be rejected |
| rests | `AtomRest` | `rest { … }` | a text omitting the rest must not compare equal |
| repeats | `RepeatMarker` | `repeat { start … play_count … }` | `repeat { start false play_count 0 }` written out → `SWG0505` |
| alternate endings | *not in the model* | unrepresentable, by design | — |
| bends and techniques | `TechniqueSpan` | `span bend { … }` | span range outside the group must round-trip |
| fretboard positions | `NotePosition` | `position { … }` | `string` beyond the tuning must round-trip |
| evidence | `TechniqueEvidence` | `evidence { source … confidence … }` | `explicit` at `0` bps must round-trip |
| tempo changes | per-bar `Tempo` | `tempo num/den` | `tempo 200/14` → `SWG0505`; `tempo 7/2` → `SWG0507` |
| unusual meters | `TimeSignature` | `meter 7/8` | `meter 4/3` → `SWG0506` |
| loss metadata | `LossReport` | `loss { … }` | reordered warnings must not compare equal |
| source metadata | `Option<SourceMeta>` | `source { … }` | `source { }` and an absent `source` are different |
| strings | arbitrary UTF-8 | escaped literal | `"\u{0A}"` for LF → `SWG0505` (canonical is `\n`) |

Alternate endings and jump directions are absent from the canonical model
(ADR-0022; `RepeatMarker`'s own documentation says an importer meeting them
records a loss). Exact text cannot represent what the model does not hold,
and inventing syntax for it here would promise a fact the round trip could
never deliver.

## 8. Acceptance

SWG-4A-01 is complete when:

- every field in §2 has a written form, an optionality, an order rule, an
  empty spelling, and its malformed cases — with no entry left as
  "obvious";
- every enum variant appears in §6.4 with no wildcard arm;
- §1's 34 exact-walker variants each map to at least one §2 row, and no
  §2 row rests on a normalized-only variant;
- the writer domain predicate in §3 cites only invariants `griff-core`
  already states, and every model-permitted state outside it is listed as
  spellable;
- each representability hole has either a resolution requiring no core
  change, or a filed prerequisite that this task does not perform;
- §7 names a positive and a negative fixture per row.

Level 2 freezes when Phase 4A is accepted — not with this document, and not
before SWG-CORE-01 closes.
