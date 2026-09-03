//! The level-2 canonical formatter.
//!
//! Spec §5.8: for `swang 2` the formatter preserves `swang 2`. It never
//! downgrades a document to an older level and never promotes one to a newer
//! level — changing a level is an authoring act, never a formatting act.
//!
//! The canonical layout is `exact-score-text.md` §6.1's, restricted to what
//! SWG-4A-06 parses: the header, one blank line, and the `score` block with
//! a four-space field indent. The blocks that fill a fuller score are
//! SWG-4A-08's, and this slice cannot construct one that holds them.

use crate::syntax::ast::v2::ExactScoreDocument;
use crate::syntax::parser::v2::ExactScore;

/// Emits the one canonical text for `score`.
///
/// `format_exact(parse(t))` is idempotent and `parse(format_exact(s))`
/// recovers the same score, for the level-2 parse.
pub(crate) fn format_exact(score: &ExactScore) -> String {
    // Destructured with no `..` on purpose: when SWG-4A-08 starts filling
    // the structural slots, this function stops compiling until it learns to
    // emit them. A formatter that silently drops what it does not recognise
    // is the one defect a canonical writer must not have.
    let ExactScoreDocument {
        ppqn,
        master_bars,
        tracks,
        source,
        loss,
    } = score.document();
    debug_assert!(
        master_bars.is_empty() && tracks.is_empty() && source.is_none() && loss.is_empty(),
        "SWG-4A-06 parses only the minimal score; a document holding more \
         than that has arrived from somewhere this formatter cannot serve"
    );
    format!("swang 2\n\nscore {{\n    ppqn {ppqn}\n}}\n")
}
