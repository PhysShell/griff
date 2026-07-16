//! Session history, favorite/reject verdicts, and candidate provenance (S8
//! Slice 3): the append-only record a curator browses, and the honest, typed
//! origin each candidate carries — the ground S9's human-in-the-loop will
//! stand on, without any ranking, learning, or adaptation of its own.
//!
//! Backend-neutral and wasm-safe: pure data and pure transitions, so the
//! cockpit shell and any future frontend share one model. Provenance is a
//! **typed** value, never a pre-baked UI string — a renderer builds its own
//! description from it.

/// A curator's verdict on a candidate.
///
/// Modelled as an `Option<Verdict>` on a history entry: `None` is undecided,
/// and because a single slot holds it, favorite and rejected are mutually
/// exclusive by construction — setting one clears the other (see [`toggle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The curator likes this candidate.
    Favorite,
    /// The curator rejects this candidate.
    Rejected,
}

/// The next verdict after the curator presses `action` on a slot currently
/// holding `current`. Pressing the same verdict again clears it (an undo);
/// pressing the other switches to it — so favorite and rejected can never both
/// hold. The one place this transition lives, so no UI handler re-implements it.
#[must_use]
pub fn toggle(current: Option<Verdict>, action: Verdict) -> Option<Verdict> {
    let _ = (current, action);
    unimplemented!("history::toggle")
}

#[cfg(test)]
#[allow(clippy::missing_assert_message)]
mod tests {
    use super::{toggle, Verdict};

    #[test]
    fn pressing_a_verdict_on_an_undecided_slot_sets_it() {
        assert_eq!(toggle(None, Verdict::Favorite), Some(Verdict::Favorite));
        assert_eq!(toggle(None, Verdict::Rejected), Some(Verdict::Rejected));
    }

    #[test]
    fn pressing_the_same_verdict_again_clears_it() {
        assert_eq!(toggle(Some(Verdict::Favorite), Verdict::Favorite), None);
        assert_eq!(toggle(Some(Verdict::Rejected), Verdict::Rejected), None);
    }

    #[test]
    fn favorite_clears_rejected_and_reject_clears_favorite() {
        assert_eq!(
            toggle(Some(Verdict::Rejected), Verdict::Favorite),
            Some(Verdict::Favorite),
            "favorite supplants rejected — never both",
        );
        assert_eq!(
            toggle(Some(Verdict::Favorite), Verdict::Rejected),
            Some(Verdict::Rejected),
            "reject supplants favorite — never both",
        );
    }
}
