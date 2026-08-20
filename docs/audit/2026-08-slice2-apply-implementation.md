# ADR-0033 Slice 2 — transactional Apply: implementation evidence

Status: **implementation review candidate — awaiting independent
implementation acceptance**. The contract itself was independently accepted
(normative reviewed artifact
`47e734cfbf1a6bd90c1bd2a035cdc68692378e96`, acceptance recorded in
[`../decisions.log.md`](../decisions.log.md) @ `bad7b44`); this document is
evidence for the *implementation*, which is a separate acceptance under the
contract's §16. Nothing here marks anything ACCEPTED / CLOSED / FROZEN.

Slice 3 (suggestions), the controlled pilot, and any real- / production- /
full-corpus labeling remain **BLOCKED** behind their own gates (ADR-0033
Decision 10). No real corpus file or label was read or modified by this
work: every test runs over synthetic fixtures in process-unique temp
directories.

## Implemented API

New public module `griff_song_curation::apply` in the isolated
non-workspace `song-curation/` crate (posture unchanged):

- `apply(&ApplyPaths) -> ApplyRun` — the §6 12-step transactional Apply
  over the serialized artifact boundary (plan file + corpus tree + index
  file → published output tree). `ApplyRun` is the §8.2 observable result
  shape: exactly one primary outcome (`AppliedReceipt` with the published
  `ApplicationReport`, or one typed `ApplyRefusal`) plus an optional
  orthogonal `LockReleaseWarning`.
- Wire contracts (§5, all `deny_unknown_fields`): `ApplicationIndex` /
  `ApplicationRecord` (`song-curation.applications.v1`),
  `ApplicationReport` / `Coverage` / `HoldoutRefusalRecord`
  (`song-curation.apply-report.v1`), `report_digest()` over the Slice-1
  shared canonical encoding (§5.4), and the constants
  `APPLICATIONS_SCHEMA`, `REPORT_SCHEMA`, `LOCK_MARKER`,
  `CURATED_MANIFEST_RELPATH`, `REPORT_RELPATH`, `RESERVED_DIR`.
- `ApplyRefusal` — the closed §12 surface: the 24 new typed refusals, plus
  `PlanVerification { refusals: Vec<CurationError> }` as *transport* for
  the Slice-1 refusals reused verbatim through step 5 (an encoding
  artifact, not a new refusal kind).
- `apply::fault` — the deterministic fault-injection registry
  (thread-local, one-shot named hooks; inert in any run that registers
  none). Test harness, not contract surface.

Frozen Slice 1: public API, semantics, and all 45 tests untouched and
green. The §9-permitted internal accommodation is exactly one: the shared
`replay` primitive now records, per source, the acting `event_id`
(`BatchEffect`), consumed by the §7.4 authority law; `canonical_json` was
made `pub(crate)` so `report_digest` uses the one shared encoding instead
of a near-copy. Both are crate-internal; no observable Slice-1 behaviour
changed.

## Commit sequence (RED→GREEN per commit, unsquashed)

| # | Commit | Kind | Content |
|---|---|---|---|
| 1 | `dfa7e06` | RED | 43 preregistered cases (A1–A8, C1–C10, L1–L9, K1–K9, K12, R1–R3, R5, F3, F5) + the fixture/builder module. Evidence in the commit message: both test binaries fail with `E0432: unresolved import griff_song_curation::apply` — as far as the absent API allows. |
| 2 | `4d311f4` | TEST-FIX | A8 drift fixtures clear the corpus tree before rewriting it (stale chunk files made the honest step-3 refusal fire before the preregistered step-5 one). |
| 3 | `f8c9f1c` | GREEN | Apply core: full §5 wire contracts, §6 ordering for the implemented checks, §7 laws, §8 happy path, §10 preservation with the round-trip guard, §11 single preflight. Adversarial hardening deliberately absent (kept RED). |
| 4 | `6ce1a69` | FIXTURE | `apply::fault` registry + eight inert named points (§14-exempt fixture work). |
| 5 | `bd2a339` | RED | 20 adversarial cases; evidence in the commit message: 12 FAILED (F1, F2, F6, F10–F13, C13, C15, K10, K11, R4), 8 declared as passing characterizations (F4, F7–F9, C11, C12, C14, C16). |
| 6 | `5cc34b3` | TEST-FIX | K10 split: serde's derived parse already refuses a duplicated *known* field at step 3, so the distinct §10.3 pass is exercised on its actual residual — a duplicate inside an unknown member (finding 1 below). |
| 7 | `c9489e1` | GREEN | The §8 filesystem protocol: full step-1 order, prefix-closed lock classification, under-lock temp inspection, `create_new` temp, reserved namespace, collisions, hardlink refusal, staged tree re-agreement, duplicate-key pass. |
| 8 | `5bb78fb` | WITNESS | Phase-8 falsification pass: 4 surviving mutations got dedicated killing witnesses (index schema check, null-prev against a non-empty head, record-level and report-level wire strictness). |

## §14 matrix — 63/63

Test names are the case ids; files: `apply_core.rs` (A, C1–C10, L),
`apply_outputs.rs` (K1–K9, K12, R1–R3, R5, F3, F5), `apply_adversarial.rs`
(F1, F2, F4, F6–F13, C11–C16, K10, K11, R4). "RED commit" is where the
case was preregistered; ⊙ marks the eight cases that were declared passing
characterizations at preregistration time (pinning behaviour the core +
fixtures already provided).

| Case | Test | RED commit | Result |
|---|---|---|---|
| A1 | `a1_valid_serialized_plan_applies` | `dfa7e06` | green |
| A2 | `a2_plan_with_foreign_field_refuses` | `dfa7e06` | green |
| A3 | `a3_corrupted_plan_digest_refuses` | `dfa7e06` | green |
| A4 | `a4_corrupted_decisions_digest_refuses` | `dfa7e06` | green |
| A5 | `a5_corpus_drift_refuses` | `dfa7e06` | green |
| A6 | `a6_forged_projection_with_self_consistent_digests_refuses` | `dfa7e06` | green |
| A7 | `a7_invalid_embedded_batch_short_circuits_before_digests` | `dfa7e06` | green |
| A8 | `a8_reachable_slice1_refusals_surface_through_step5` | `dfa7e06` (+`4d311f4`) | green |
| C1 | `c1_valid_initial_application` | `dfa7e06` | green |
| C2 | `c2_valid_second_application_chained_to_first` | `dfa7e06` | green |
| C3 | `c3_duplicate_batch_id_refuses_as_already_applied_not_chain` | `dfa7e06` | green |
| C4 | `c4_wrong_previous_report_digest_refuses` | `dfa7e06` | green |
| C5 | `c5_wrong_chained_corpus_fingerprint_refuses` | `dfa7e06` | green |
| C6 | `c6_fingerprint_neutral_batch_applies_then_refuses_by_id` | `dfa7e06` | green |
| C7 | `c7_missing_index_and_foreign_field_refuse` | `dfa7e06` | green |
| C8 | `c8_duplicate_internal_batch_id_refuses` | `dfa7e06` | green |
| C9 | `c9_index_with_broken_internal_chain_refuses` | `dfa7e06` | green |
| C10 | `c10_initial_batch_on_independent_copy_with_own_index_applies` | `dfa7e06` | green |
| C11 | `c11_existing_lockfile_refuses_locked` | `bd2a339` ⊙ | green |
| C12 | `c12_stale_lock_blocks_until_validated_recovery` | `bd2a339` ⊙ | green |
| C13 | `c13_real_second_index_at_temp_name_is_never_unlinked` | `bd2a339` | green |
| C14 | `c14_second_applier_during_live_step12_temp_loses_at_the_lock` | `bd2a339` ⊙ | green |
| C15 | `c15_real_second_index_at_lock_name_is_occupied_not_locked` | `bd2a339` | green |
| C16 | `c16_partial_lock_marker_classifies_locked_and_recovery_is_gated` | `bd2a339` ⊙ | green |
| L1 | `l1_assign_unlabelled_source_updates_every_chunk` | `dfa7e06` | green |
| L2 | `l2_already_correct_label_counts_unchanged` | `dfa7e06` | green |
| L3 | `l3_authorized_correct_replaces_label` | `dfa7e06` | green |
| L4 | `l4_authorized_merge_replaces_labels` | `dfa7e06` | green |
| L5 | `l5_authorized_split_replaces_labels` | `dfa7e06` | green |
| L6 | `l6_unauthorized_replacement_refuses` | `dfa7e06` | green |
| L7 | `l7_on_disk_label_drift_refuses_as_fingerprint_mismatch` | `dfa7e06` | green |
| L8 | `l8_merge_and_split_supersession_mismatch_refuse` | `dfa7e06` | green |
| L9 | `l9_accept_with_nonempty_supersedes_refuses` | `dfa7e06` | green |
| F1 | `f1_output_inside_input_refuses_including_symlink_aliases` | `bd2a339` | green |
| F2 | `f2_pre_existing_output_or_staging_refuses` | `bd2a339` | green |
| F3 | `f3_pre_staging_refusal_leaves_no_trace` | `dfa7e06` | green |
| F4 | `f4_injected_failures_land_in_enumerated_states` | `bd2a339` ⊙ | green |
| F5 | `f5_repeated_execution_is_byte_identical` | `dfa7e06` | green |
| F6 | `f6_index_inside_corpus_tree_refuses` | `bd2a339` | green |
| F7 | `f7_path_like_batch_id_influences_no_filesystem_path` | `bd2a339` ⊙ | green |
| F8 | `f8_index_symlink_aliases_converge_on_the_canonical_file` | `bd2a339` ⊙ | green |
| F9 | `f9_lock_release_failure_is_a_warning_never_the_primary_outcome` | `bd2a339` ⊙ | green |
| F10 | `f10_output_colliding_with_index_artifacts_refuses_before_the_lock` | `bd2a339` | green |
| F11 | `f11_hardlinked_index_refuses_through_either_entry` | `bd2a339` | green |
| F12 | `f12_late_temp_occupant_makes_commit_refuse_and_stays_untouched` | `bd2a339` | green |
| F13 | `f13_output_in_reserved_staging_namespace_refuses` | `bd2a339` | green |
| K1 | `k1_every_chunk_of_one_sha_updated_together` | `dfa7e06` | green |
| K2 | `k2_untouched_files_are_raw_byte_copies` | `dfa7e06` | green |
| K3 | `k3_touched_file_with_unknown_member_refuses` | `dfa7e06` | green |
| K4 | `k4_root_manifest_with_songs_refuses` | `dfa7e06` | green |
| K5 | `k5_tree_disagreement_and_missing_manifest_refuse` | `dfa7e06` | green |
| K6 | `k6_curated_songs_map_matches_labels_exactly` | `dfa7e06` | green |
| K7 | `k7_curated_manifest_at_protected_path_root_manifest_stays_songless` | `dfa7e06` | green |
| K8 | `k8_partial_curation_reports_not_holdout_ready` | `dfa7e06` | green |
| K9 | `k9_fully_curated_fixture_is_holdout_ready` | `dfa7e06` | green |
| K10 | `k10_duplicate_json_keys_in_touched_file_refuse` | `bd2a339` (+`5cc34b3`) | green |
| K11 | `k11_staged_tree_corruption_aborts_before_publication` | `bd2a339` | green |
| K12 | `k12_foreign_reserved_area_entry_refuses` | `dfa7e06` | green |
| R1 | `r1_report_digest_recomputes_identically` | `dfa7e06` | green |
| R2 | `r2_index_record_matches_report` | `dfa7e06` | green |
| R3 | `r3_success_publishes_report_and_record_refusal_publishes_neither` | `dfa7e06` | green |
| R4 | `r4_commit_failure_is_never_success_and_orphan_blocks_retry` | `bd2a339` | green |
| R5 | `r5_curated_manifest_digest_is_sha256_of_published_bytes` | `dfa7e06` | green |

Plus 4 Phase-8 witnesses (`apply_witnesses.rs`, `5bb78fb`) beyond the
preregistered 63.

## Refusal coverage

**24 new typed refusals** — 23 exercised by at least one case:
`OutputAlreadyExists` (F2, R4), `OutputWouldModifyInput` (F1),
`ApplicationIndexInsideTree` (F6), `OutputCollidesWithIndexArtifacts`
(F10), `OutputNameReserved` (F13), `ApplicationIndexHardLinked` (F11),
`ApplicationIndexLocked` (C11, C12, C14, C16, F8),
`ApplicationIndexLockPathOccupied` (C15), `ApplicationIndexTempExists`
(C13), `MalformedPlanArtifact` (A2), `MalformedApplicationIndex` (C7,
witness), `CorpusTreeDisagreement` (K5, K10ii, K12),
`OrdinaryManifestCarriesSongs` (K4), `UnsupportedApplicationIndexSchema`
(witness), `DuplicateAppliedBatchId` (C8), `ApplicationIndexChainInvalid`
(C9), `DecisionBatchAlreadyApplied` (C3, C6), `ApplicationChainMismatch`
(C4, C5, witness), `SupersessionEvidenceContradiction` (L8, L9),
`ExistingLabelReplacementNotAuthorized` (L6, F9b),
`NonCanonicalCorpusFile` (K3, K10i), `ApplyIoError` (F4, F12),
`OutputPreflightInconsistent` (K11). The 24th —
`CuratedManifestPathNotDistinct` — is implemented as the §12 hard guard
and is, exactly as §12 itself states, structurally unreachable under the
fixed v1 curated path; no behavioural case can reach it (finding 3).

**11 Apply-reachable Slice-1 refusals** — all exercised through the reused
`verify_plan`: `UnidentifiedSource` (A8a), `ConflictingExistingSongIds`
(A8b), `UnknownDecisionSource` (A8c), `SourceAssignedToMultipleSongs`
(A8d), `InvalidDecisionBatchOrder` (A7), `DuplicateDecisionEventId` (A8e),
`PlanCorpusFingerprintMismatch` (A5, L7),
`DecisionBatchFingerprintMismatch` (A5 — corpus drift breaks both
bindings), `DecisionDigestMismatch` (A4), `PlanDigestMismatch` (A3),
`DecisionProjectionMismatch` (A6).

**3 ledger-side Slice-1 members stay intentionally unreachable**
(`UnsupportedDecisionsLedgerSchema`, `DuplicateDecisionBatchId`,
`BatchNotInLedger`): Apply consumes a plan, never a ledger — `grep` proof:
`apply.rs` calls `verify_plan` and `replay` only; `validate_ledger` /
`build_plan` are not referenced anywhere in the module.

## Adversarial / fault-injection results

Concurrency is deterministic: the second applier runs *inline inside a
one-shot fault hook on the same thread* (C14 inside the live step-12 temp
window; C16(a) inside the `create_new`→marker window) — no scheduler
timing anywhere. Injected failures land in exactly the enumerated §8.2
states (F4 a–d), a commit failure can never surface as success and its
orphan blocks a retry until the mandated recovery (R4), and a lock-release
failure never changes the primary outcome in either direction (F9).

The Phase-8 falsification pass probed 14 mutation targets; 10 already had
killers in the matrix, 4 got dedicated witnesses (`5bb78fb`). Three
mutations survive **by unreachability**, documented rather than tested:
§7.2 relation (3) (entailed by relation (2) + the step-5 fingerprint
proof; the contract states it for attribution), the
`CuratedManifestPathNotDistinct` guard (see above), and corpus-inside-
output containment (such an output necessarily exists and refuses
`OutputAlreadyExists` first under the §6 order).

## Findings (implementation-time, no contract law changed)

1. **serde's derived parse already refuses duplicated *known* struct
   fields** ("duplicate field" at step-3 tree agreement), i.e. earlier and
   stricter than the §10.3 pass. The distinct duplicate-rejecting pass
   therefore guards its actual residual: duplicates inside *unknown*
   members, which the tolerant derive skips wholesale. K10 was split to
   prove both branches (`5cc34b3`). Fail-closed both ways; no law changed.
2. **Unresolvable non-index input paths** (missing corpus dir, missing
   output parent) surface as `ApplyIoError { op: "canonicalize…" }` — the
   contract's single typed I/O boundary; §6 enumerates no dedicated
   refusal for them and none was invented.
3. **§14's coverage sentence vs the `CuratedManifestPathNotDistinct`
   guard**: the contract simultaneously declares the guard structurally
   unreachable (§12) and claims every Apply-reachable refusal appears in a
   case (§14). Read together, the guard belongs with the documented-
   unreachable set; recorded here so the implementation reviewer can
   confirm that reading rather than inherit it silently.

## Validation matrix

| Check | Result |
|---|---|
| `cargo test --manifest-path song-curation/Cargo.toml` (isolated crate) | 112 passed / 0 failed (45 frozen Slice-1 + 27 + 16 + 20 + 4) |
| frozen Slice-1 suite | 45/45 green, untouched |
| `cargo test --workspace` | 1535 passed / 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo clippy --manifest-path song-curation/Cargo.toml --all-targets` (crate `deny(all)`) | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo doc --no-deps --workspace` | 1 pre-existing swang warning (ambiguous `format` link), untouched by this work |
| `cargo doc --no-deps --manifest-path song-curation/Cargo.toml` | 1 pre-existing warning in a frozen Slice-1 doc comment (private-item link), untouched |
| MSRV: `cargo +1.92 check --manifest-path song-curation/Cargo.toml --all-targets` | clean |

The isolated crate remains a non-workspace member (root `Cargo.toml`
`exclude` unchanged) and is verified by the dedicated commands above, per
the ADR-0010 isolation posture.
