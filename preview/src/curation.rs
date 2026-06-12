//! Curation persistence: a viewport decision lands in a chunk record (S8).
//!
//! The bridge between the UI-level [`CurationDecision`] (interaction core,
//! ADR-0016 — no domain types there) and the S5 corpus schema.
//!
//! [`decide_record`] parses a `ChunkMeta` JSON record, sets its `reviewer`
//! field (`Approve` → `Accepted`, `Reject` → `Rejected`), and re-serializes.
//! Every other field passes through untouched — the schema's lossless
//! round-trip guarantee carries them — and an earlier decision may be
//! overwritten (re-curation is the established healing path). Pure; the
//! frontend shell owns the file I/O.

use griff_core::corpus::{BoundaryEntry, ChunkId, ChunkMeta, ReviewerDecision, SwancoreTag};

use crate::viewport::CurationDecision;

/// A UI-level digest of a chunk record's curation state.
///
/// Plain strings in the schema's `snake_case` wire names, so renderers show
/// exactly what the corpus stores and no domain type crosses into the UI
/// (ADR-0016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSummary {
    /// The record's `title`.
    pub title: String,
    /// The prior reviewer decision in wire casing, if any.
    pub reviewer: Option<String>,
    /// The record's tags in wire casing, in stored order.
    pub tags: Vec<String>,
}

/// Errors the curation persistence seam can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurationError {
    /// The input is not a parseable `ChunkMeta` record.
    ParseFailed,
    /// A tag name does not match any schema variant.
    UnknownTag,
    /// A rename would leave the record without a title.
    EmptyTitle,
    /// A split/merge needs the record's `source.bar_range`, which is absent.
    MissingBarRange,
    /// The split bar does not fall strictly inside the record's bar range.
    SplitOutOfRange,
    /// The merged records' bar ranges are not consecutive in the source.
    NotAdjacent,
    /// The merged records disagree on source or timing (filename, format,
    /// tick grid, time signature, or tuning).
    MergeMismatch,
}

/// The full tag palette in wire casing: one entry per [`SwancoreTag`]
/// variant, in `all_variants` order. Renderers cycle this list; the names
/// are derived via serde, so they cannot drift from the schema.
#[must_use]
pub fn tag_palette() -> Vec<String> {
    SwancoreTag::all_variants()
        .iter()
        .filter_map(|t| wire_name(serde_json::to_value(t)))
        .collect()
}

/// Rewrites the record's `tags` to exactly `names` (wire casing, in order);
/// every other field passes through untouched.
///
/// # Errors
/// [`CurationError::ParseFailed`] when `json` is not a `ChunkMeta` record;
/// [`CurationError::UnknownTag`] when a name matches no schema variant.
pub fn set_tags(json: &str, names: &[String]) -> Result<String, CurationError> {
    let mut meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    meta.tags = names
        .iter()
        .map(|n| {
            serde_json::from_value(serde_json::Value::String(n.clone()))
                .map_err(|_| CurationError::UnknownTag)
        })
        .collect::<Result<Vec<SwancoreTag>, CurationError>>()?;
    serde_json::to_string(&meta).map_err(|_| CurationError::ParseFailed)
}

/// Rewrites the record's `title` to `title` (trimmed); every other field
/// passes through untouched.
///
/// # Errors
/// [`CurationError::ParseFailed`] when `json` is not a `ChunkMeta` record;
/// [`CurationError::EmptyTitle`] when the trimmed title is empty.
pub fn rename_record(json: &str, title: &str) -> Result<String, CurationError> {
    let mut meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(CurationError::EmptyTitle);
    }
    trimmed.clone_into(&mut meta.title);
    serde_json::to_string(&meta).map_err(|_| CurationError::ParseFailed)
}

/// Splits the record's extent at `at_bar` (absolute source bar): the first
/// half keeps bars `[start, at_bar - 1]`, the second `[at_bar, end]`.
///
/// The halves derive their ids from the original — `.1` for the first,
/// `.{second_slot}` for the second, so the id always mirrors the sibling
/// slot the caller stores the file in (Codex P2, PR #45) — and their titles
/// (`(1/2)`/`(2/2)`); tags, techniques, and quality flags carry over.
/// Boundaries are partitioned at the split tick — a straddler stays in the
/// first half, clamped — and the second half's are rebased to its new
/// start. The reviewer decision and the whole-extent measurements
/// (structure, gesture, complexity) reset: each half is a new record the
/// curator reviews afresh.
///
/// # Errors
/// [`CurationError::ParseFailed`] when `json` is not a `ChunkMeta` record;
/// [`CurationError::MissingBarRange`] when the record has no
/// `source.bar_range`; [`CurationError::SplitOutOfRange`] unless
/// `start < at_bar <= end` and `second_slot >= 2` (a lower slot would
/// collide with the first half's `.1`).
pub fn split_record(
    json: &str,
    at_bar: u32,
    second_slot: u32,
) -> Result<(String, String), CurationError> {
    let meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    let (start, end) = meta
        .source
        .bar_range
        .ok_or(CurationError::MissingBarRange)?;
    if at_bar <= start || at_bar > end || second_slot < 2 {
        return Err(CurationError::SplitOutOfRange);
    }
    let split_tick = at_bar
        .saturating_sub(start)
        .saturating_mul(ticks_per_bar(&meta));

    let mut first = fresh_extent(meta.clone());
    first.id = ChunkId(format!("{}.1", meta.id.0));
    first.title = format!("{} (1/2)", meta.title);
    first.source.bar_range = Some((start, at_bar.saturating_sub(1)));
    first.boundaries = meta
        .boundaries
        .iter()
        .filter(|b| b.start_tick < split_tick)
        .map(|b| BoundaryEntry {
            end_tick: b.end_tick.min(split_tick),
            ..*b
        })
        .collect();

    let mut second = fresh_extent(meta.clone());
    second.id = ChunkId(format!("{}.{second_slot}", meta.id.0));
    second.title = format!("{} (2/2)", meta.title);
    second.source.bar_range = Some((at_bar, end));
    second.boundaries = meta
        .boundaries
        .iter()
        .filter(|b| b.start_tick >= split_tick)
        .map(|b| BoundaryEntry {
            start_tick: b.start_tick.saturating_sub(split_tick),
            end_tick: b.end_tick.saturating_sub(split_tick),
            ..*b
        })
        .collect();

    match (
        serde_json::to_string(&first),
        serde_json::to_string(&second),
    ) {
        (Ok(a), Ok(b)) => Ok((a, b)),
        _ => Err(CurationError::ParseFailed),
    }
}

/// Splits the record at the bar containing the chunk-relative `tick`.
///
/// The shell's mapping from a TUI playhead to [`split_record`]'s absolute
/// source bar: the split lands on the bar boundary at or before the tick.
///
/// # Errors
/// Everything [`split_record`] emits; a tick inside the first bar floors to
/// the range start and is therefore [`CurationError::SplitOutOfRange`].
pub fn split_record_at_tick(
    json: &str,
    tick: u32,
    second_slot: u32,
) -> Result<(String, String), CurationError> {
    let meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    let (start, _) = meta
        .source
        .bar_range
        .ok_or(CurationError::MissingBarRange)?;
    let offset = tick
        .checked_div(ticks_per_bar(&meta))
        .ok_or(CurationError::SplitOutOfRange)?;
    split_record(json, start.saturating_add(offset), second_slot)
}

/// Merges two same-source records whose bar ranges are consecutive (`first`
/// ends on the bar before `second` starts).
///
/// The first record's identity — id, title, tempo, timestamps — wins; tags,
/// techniques, and quality flags union in order; the second record's
/// boundaries rebase past the first's span. The reviewer decision and the
/// whole-extent measurements reset, and a cohort/ensemble label survives
/// only when both records agree on it: the join is a new record the curator
/// reviews afresh.
///
/// # Errors
/// [`CurationError::ParseFailed`] when either input is not a `ChunkMeta`
/// record; [`CurationError::MissingBarRange`] when either record has no
/// `source.bar_range`; [`CurationError::MergeMismatch`] when they disagree
/// on source or timing; [`CurationError::NotAdjacent`] when the ranges are
/// not consecutive in source order.
pub fn merge_records(first: &str, second: &str) -> Result<String, CurationError> {
    let a: ChunkMeta = serde_json::from_str(first).map_err(|_| CurationError::ParseFailed)?;
    let b: ChunkMeta = serde_json::from_str(second).map_err(|_| CurationError::ParseFailed)?;
    let (a_start, a_end) = a.source.bar_range.ok_or(CurationError::MissingBarRange)?;
    let (b_start, b_end) = b.source.bar_range.ok_or(CurationError::MissingBarRange)?;
    if a.source.filename != b.source.filename
        || a.source.format != b.source.format
        || a.ticks_per_quarter != b.ticks_per_quarter
        || a.time_signature != b.time_signature
        || a.tuning != b.tuning
    {
        return Err(CurationError::MergeMismatch);
    }
    if a_end.checked_add(1) != Some(b_start) {
        return Err(CurationError::NotAdjacent);
    }
    let span = a_end
        .saturating_sub(a_start)
        .saturating_add(1)
        .saturating_mul(ticks_per_bar(&a));

    let mut merged = fresh_extent(a);
    merged.source.bar_range = Some((a_start, b_end));
    for tag in b.tags {
        if !merged.tags.contains(&tag) {
            merged.tags.push(tag);
        }
    }
    for technique in b.techniques {
        if !merged.techniques.contains(&technique) {
            merged.techniques.push(technique);
        }
    }
    for flag in b.quality_flags {
        if !merged.quality_flags.contains(&flag) {
            merged.quality_flags.push(flag);
        }
    }
    merged
        .boundaries
        .extend(b.boundaries.iter().map(|e| BoundaryEntry {
            start_tick: e.start_tick.saturating_add(span),
            end_tick: e.end_tick.saturating_add(span),
            ..*e
        }));
    if merged.style_cohort != b.style_cohort {
        merged.style_cohort = None;
    }
    if merged.ensemble != b.ensemble {
        merged.ensemble = None;
    }
    serde_json::to_string(&merged).map_err(|_| CurationError::ParseFailed)
}

/// One bar of the record's grid in ticks (`tpq × 4 × num / den`); a corrupt
/// zero denominator yields a zero-tick bar instead of dividing by zero.
fn ticks_per_bar(meta: &ChunkMeta) -> u32 {
    let (num, den) = meta.time_signature;
    u32::from(meta.ticks_per_quarter)
        .saturating_mul(4)
        .saturating_mul(u32::from(num))
        .checked_div(u32::from(den))
        .unwrap_or(0)
}

/// Resets what a changed extent invalidates: the reviewer decision and the
/// whole-extent measurements (structure, gesture, complexity).
const fn fresh_extent(mut meta: ChunkMeta) -> ChunkMeta {
    meta.reviewer = None;
    meta.structure = None;
    meta.gesture = None;
    meta.complexity = None;
    meta
}

/// Digests a serialized `ChunkMeta` record into a [`RecordSummary`].
///
/// The inspector shows the digest: title, prior reviewer decision, and tags,
/// all in the schema's wire casing (via serde, so the names cannot drift).
///
/// # Errors
/// [`CurationError::ParseFailed`] when `json` is not a `ChunkMeta` record.
pub fn summarize_record(json: &str) -> Result<RecordSummary, CurationError> {
    let meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    Ok(RecordSummary {
        reviewer: meta
            .reviewer
            .and_then(|d| wire_name(serde_json::to_value(d))),
        tags: meta
            .tags
            .iter()
            .filter_map(|t| wire_name(serde_json::to_value(t)))
            .collect(),
        title: meta.title,
    })
}

/// A serialized value's wire name (`snake_case` per the schema), if it is a
/// plain string on the wire.
fn wire_name(value: serde_json::Result<serde_json::Value>) -> Option<String> {
    match value {
        Ok(serde_json::Value::String(s)) => Some(s),
        _ => None,
    }
}

/// Applies `decision` to a serialized `ChunkMeta` record and returns the
/// updated JSON; everything except `reviewer` is untouched.
///
/// # Errors
/// [`CurationError::ParseFailed`] when `json` is not a `ChunkMeta` record.
pub fn decide_record(json: &str, decision: CurationDecision) -> Result<String, CurationError> {
    let mut meta: ChunkMeta = serde_json::from_str(json).map_err(|_| CurationError::ParseFailed)?;
    meta.reviewer = Some(match decision {
        CurationDecision::Approve => ReviewerDecision::Accepted,
        CurationDecision::Reject => ReviewerDecision::Rejected,
    });
    serde_json::to_string(&meta).map_err(|_| CurationError::ParseFailed)
}
