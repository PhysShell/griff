//! The level-2 input budget (SWG-INF-06, spec §5.11).
//!
//! # Level 2 only, and the type is named for it
//!
//! Level 1's acceptance set is frozen (§5.5). A parser-wide bound that
//! rejected a source level 1 accepts would be an observable change to a
//! frozen level, so §5.11 puts every declared bound at level 2 alone. A
//! level-1 run may still die of exhaustion — that is a runtime outcome, and
//! it never becomes a typed refusal. Nothing in this module may be reached
//! from a level-1 path, and the type carries `Level2` in its name so that a
//! call site which forgot is visible at a glance rather than after a
//! bisect.
//!
//! # A live counter, not an audit
//!
//! Every axis is admitted *before* the thing it counts is built. Checking
//! `tokens.len()` after lexing four million tokens is not a resource gate;
//! it is an obituary written after the allocation. The enforcement order the
//! level-2 parser owes this module:
//!
//! ```text
//! frozen header pre-parser
//!   -> resolve the supported level
//!   -> level 1: never consult this module
//!   -> level 2: admit_source        before lexing
//!               admit_token         while lexing, before storing the token
//!               enter_block         on entering a structural `{ ... }`
//!               admit_diagnostic    before appending another diagnostic
//! ```
//!
//! # What the counters count
//!
//! - **source bytes** — UTF-8 bytes of the complete source, header included;
//! - **tokens** — tokens emitted by the level-2 lexer after the frozen
//!   header pre-parser. End of input is not a token;
//! - **nesting depth** — simultaneously open structural `{ ... }`
//!   constructs. The `score` root is depth 1. A `[ ... ]` scalar list is one
//!   value carried by one word, so it adds no structural depth;
//! - **diagnostics** — the most one level-2 parse attempt may return. A
//!   terminal budget diagnostic counts toward that total, so the last slot
//!   is reserved for it rather than spent and then exceeded.
//!
//! # Two of the four are forward reservations
//!
//! The exact-score grammar Phase 4A admits has no recursive production —
//! `score`, `track`, `voice`, `group`, `note`, `position`, `evidence`
//! bottoms out — so it cannot approach depth 64, and today's parser maps
//! each error into a one-element vector, so it cannot approach 256
//! diagnostics. Both limits are declared anyway, because §5.11 requires
//! declaration *before* level 2's first accepted program. That deadline is
//! §5.11's own admission rule, and it is **stricter than the freeze**: by
//! §5.3 level 2 stays provisional until Phase 4A is accepted, so a bound
//! added after the first accepted program would still predate the freeze —
//! §5.11 forbids it anyway, because programs written against a provisional
//! level are already running. They are reservations, not evidence of a
//! stack-overflow hazard in today's grammar.
//!
//! # No caller yet, on purpose
//!
//! Level 2 is unreachable on this build — `header_level` refuses `swang 2`
//! — so this module has no live caller. SWG-4A-06 owes it one: level-2
//! dispatch must construct and consult this budget before its first
//! successful level-2 parse. Wiring a gate into a parser that does not exist
//! would be the fake half of the work, so the mechanism ships here and the
//! wiring ships there.
#![allow(dead_code)]

use super::diagnostic::Diagnostic;
use super::span::Span;

/// UTF-8 bytes of source a level-2 parse may read — exactly `16 MiB`.
pub(crate) const MAX_SOURCE_BYTES: u64 = 16_777_216;

/// Tokens a level-2 lex may emit.
pub(crate) const MAX_TOKENS: u64 = 4_000_000;

/// Simultaneously open structural blocks a level-2 parse may hold.
pub(crate) const MAX_NESTING_DEPTH: u32 = 64;

/// Diagnostics one level-2 parse attempt may return, terminal budget
/// diagnostic included.
pub(crate) const MAX_DIAGNOSTICS: u32 = 256;

/// The four declared level-2 bounds.
///
/// The fields are private and there is no production constructor but
/// [`Level2ResourceLimits::declared`]. A declared bound that a caller may
/// replace is not a declared bound: `Level2Budget::new(Level2ResourceLimits
/// { tokens: u64::MAX, .. })` would satisfy every word of the contract while
/// meaning none of it. Tests reach the scaled constructor below, which is
/// `#[cfg(test)]` and therefore cannot appear in a shipped call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Level2ResourceLimits {
    /// UTF-8 bytes of the complete source, header included.
    source_bytes: u64,
    /// Tokens after the frozen header pre-parser; end of input is not one.
    tokens: u64,
    /// Simultaneously open structural `{ ... }` constructs.
    nesting_depth: u32,
    /// Diagnostics one parse attempt may return.
    diagnostics: u32,
}

impl Level2ResourceLimits {
    /// The bounds spec §5.11 declares. The only way to build these outside
    /// a test.
    pub(crate) const fn declared() -> Self {
        Self {
            source_bytes: MAX_SOURCE_BYTES,
            tokens: MAX_TOKENS,
            nesting_depth: MAX_NESTING_DEPTH,
            diagnostics: MAX_DIAGNOSTICS,
        }
    }

    /// Scaled-down bounds, so a test can prove an exact off-by-one without
    /// allocating the declared caps. Test-only on purpose: see the type's
    /// documentation.
    #[cfg(test)]
    pub(crate) const fn scaled(
        source_bytes: u64,
        tokens: u64,
        nesting_depth: u32,
        diagnostics: u32,
    ) -> Self {
        Self {
            source_bytes,
            tokens,
            nesting_depth,
            diagnostics,
        }
    }
}

/// Which budget a refusal is about. One code, four axes — they share a
/// meaning, so they share `SWG0509`, and the message names the axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    SourceBytes,
    Tokens,
    NestingDepth,
    Diagnostics,
}

impl Axis {
    const fn word(self) -> &'static str {
        match self {
            Self::SourceBytes => "source bytes",
            Self::Tokens => "tokens",
            Self::NestingDepth => "nesting depth",
            Self::Diagnostics => "diagnostics",
        }
    }
}

/// Builds the one budget refusal, naming the axis, the declared limit, and
/// what the parse would have needed.
fn breach(axis: Axis, limit: u64, needed: u64, at: Span) -> Diagnostic {
    Diagnostic {
        code: "SWG0509",
        span: at,
        message: format!(
            "level-2 {} budget exceeded: the declared limit is {limit}, this parse needed {needed}",
            axis.word()
        ),
    }
}

/// A level-2 parse's running resource state.
///
/// Deliberately neither `Copy` nor `Clone`: it is a counter, and a
/// duplicated counter lets a caller spend the same budget twice by
/// advancing the copy. The documentation already promised that; the derives
/// used to contradict it.
#[allow(
    missing_copy_implementations,
    reason = "a running counter must not be silently duplicated"
)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Level2Budget {
    limits: Level2ResourceLimits,
    tokens: u64,
    depth: u32,
    diagnostics: u32,
}

impl Level2Budget {
    /// The budget every level-2 parse runs under: the bounds §5.11 declares,
    /// with every counter at zero. This is the only constructor a shipped
    /// call site can reach.
    pub(crate) const fn declared() -> Self {
        Self::over(Level2ResourceLimits::declared())
    }

    /// A fresh budget over the given bounds. Private, so the declared bounds
    /// cannot be swapped out at a call site; `declared()` is the production
    /// door and `#[cfg(test)]` code reaches this through [`Self::scaled`].
    const fn over(limits: Level2ResourceLimits) -> Self {
        Self {
            limits,
            tokens: 0,
            depth: 0,
            diagnostics: 0,
        }
    }

    /// A budget over scaled-down bounds, for boundary tests.
    #[cfg(test)]
    pub(crate) const fn scaled(limits: Level2ResourceLimits) -> Self {
        Self::over(limits)
    }

    /// Tokens admitted so far.
    pub(crate) const fn tokens(&self) -> u64 {
        self.tokens
    }

    /// Structural blocks currently open.
    pub(crate) const fn depth(&self) -> u32 {
        self.depth
    }

    /// Diagnostics accounted for, terminal one included.
    pub(crate) const fn diagnostics(&self) -> u32 {
        self.diagnostics
    }

    /// Admits the whole source, **before lexing**.
    ///
    /// `at` is the caller's location for a refusal: no body token has been
    /// admitted yet, so the level/header span is the only honest place to
    /// point.
    ///
    /// # Errors
    /// `SWG0509` when the source is longer than the declared byte budget.
    pub(crate) fn admit_source(&self, source: &str, at: Span) -> Result<(), Diagnostic> {
        let bytes = u64::try_from(source.len()).unwrap_or(u64::MAX);
        if bytes > self.limits.source_bytes {
            return Err(breach(
                Axis::SourceBytes,
                self.limits.source_bytes,
                bytes,
                at,
            ));
        }
        Ok(())
    }

    /// Admits one token, **before the lexer stores it**. A refused token is
    /// not counted: the budget records what it granted, never what it
    /// turned away.
    ///
    /// # Errors
    /// `SWG0509` at the token that crosses the budget.
    pub(crate) fn admit_token(&mut self, at: Span) -> Result<(), Diagnostic> {
        let needed = self.tokens.saturating_add(1);
        if needed > self.limits.tokens {
            return Err(breach(Axis::Tokens, self.limits.tokens, needed, at));
        }
        self.tokens = needed;
        Ok(())
    }

    /// Opens one structural block. The `score` root is depth 1; a scalar
    /// list never calls this.
    ///
    /// # Errors
    /// `SWG0509` at the opening token of the block that would exceed the
    /// depth. The refused block is not entered.
    pub(crate) fn enter_block(&mut self, at: Span) -> Result<(), Diagnostic> {
        let needed = self.depth.saturating_add(1);
        if needed > self.limits.nesting_depth {
            return Err(breach(
                Axis::NestingDepth,
                u64::from(self.limits.nesting_depth),
                u64::from(needed),
                at,
            ));
        }
        self.depth = needed;
        Ok(())
    }

    /// Closes the innermost structural block. Saturating, so an unbalanced
    /// close cannot wrap the counter into a budget it never earned.
    pub(crate) const fn leave_block(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Accounts for one diagnostic, **before it is appended**.
    ///
    /// The cap is on what one parse attempt returns, and the terminal budget
    /// diagnostic counts toward it, so the last slot is reserved for that
    /// refusal instead of being spent on an ordinary diagnostic and then
    /// exceeded.
    ///
    /// Exhaustion is **terminal and idempotent**: the last slot is consumed
    /// once, and every later admission is refused identically without
    /// advancing admitted state. `admit_token` states the same law — a
    /// refused thing does not move the counter — and the only difference
    /// here is that the terminal refusal itself genuinely occupies a slot.
    /// A budget that kept counting after termination would report a parse
    /// that never happened.
    ///
    /// # Errors
    /// `SWG0509` — itself the last diagnostic the attempt may return.
    pub(crate) fn admit_diagnostic(&mut self, at: Span) -> Result<(), Diagnostic> {
        let needed = self.diagnostics.saturating_add(1);
        if needed >= self.limits.diagnostics {
            // Saturate rather than increment: `needed` is what a parse would
            // have required to keep this diagnostic *and* still carry the
            // terminal refusal, which is one past the cap however many times
            // an ignored `Err` is retried.
            self.diagnostics = self.limits.diagnostics;
            return Err(breach(
                Axis::Diagnostics,
                u64::from(self.limits.diagnostics),
                u64::from(self.limits.diagnostics.saturating_add(1)),
                at,
            ));
        }
        self.diagnostics = needed;
        Ok(())
    }
}
