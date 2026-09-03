//! Source locations as a side table (SWG-INF-04).
//!
//! The AST is span-free and stays that way: `Program` equality, the
//! `parse(format(ast)) == ast` law, and a lifter that builds programs with
//! no source text in sight all depend on it. Locations therefore live
//! beside the tree rather than inside it:
//!
//! ```text
//! source -> parse_with_source_map -> Parsed<Program> { value, source_map }
//! ```
//!
//! # Prior art, and where this departs from it
//!
//! `rustc` keeps byte offsets and resolves line/column only when it renders;
//! this module does the same, so nothing here stores a line number as
//! semantic state. `rust-analyzer`'s `AstIdMap` separates *structural
//! identity* from *position*, which is the architecture borrowed here.
//! `rowan`'s `SyntaxNodePtr` is the same idea again, but a lossless CST is
//! not adopted — SWG-UI-07 owns that admission gate and has not opened it.
//!
//! The departure from `rust-analyzer` is the one worth stating out loud: its
//! `AstId`s are engineered to survive edits, because an IDE needs them to.
//! [`AstId`] here is **parse-local**. It is deterministic for a given AST
//! topology and blind to whitespace and legal word reordering, and that is
//! the whole guarantee. Inserting, deleting or reordering repeated nodes may
//! renumber every id after the change. Persistent identity across edits is
//! Phase 4C's problem, and pretending to solve it here would be pre-deciding
//! a question that task exists to answer.

use std::collections::BTreeMap;

use super::span::Span;

/// A parse result and the locations of what it parsed.
///
/// Generic because the level-2 parser will return one of these too, over a
/// different tree; the side-table architecture does not care what `T` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed<T> {
    /// The tree. Span-free, exactly as `parse` would have returned it.
    pub value: T,
    /// Where each node and field of that tree came from.
    pub source_map: SourceMap,
}

/// Which construct a location belongs to.
///
/// The integer is the occurrence ordinal of that kind in semantic AST
/// traversal order. Level 1 has exactly one of each construct, so every
/// ordinal is `0`; the field exists because level 2 has repeated nodes and
/// the identity model should not have to change when it arrives.
///
/// Non-exhaustive: level 2 appends variants (score, master bar, track, …)
/// without breaking a caller that matches on today's set.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AstId {
    /// The whole program: header through the pattern block.
    Program(u32),
    /// A `pattern <name> { … }` block.
    Pattern(u32),
    /// A `fractalize …` pipeline step.
    Fractalize(u32),
    /// A `linearize …` pipeline step.
    Linearize(u32),
    /// A `map_rhythm …` pipeline step.
    MapRhythm(u32),
    /// A `generate { … }` pipeline step.
    Generate(u32),
    /// An `export …` pipeline step.
    Export(u32),
    /// A level-2 `score { … }` root (SWG-4A-06).
    Score(u32),
}

/// Which value of a construct a location belongs to.
///
/// A kind is not unique on its own: `seed` names both the pruning seed and
/// the generation seed, and the owning [`AstId`] in a [`FieldRef`] is what
/// tells them apart. That is deliberate — the alternative is a variant per
/// (construct, word) pair, which grows quadratically and says nothing the
/// pair does not already say.
///
/// Non-exhaustive for the same reason as [`AstId`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldKind {
    /// The header's language level.
    Level,
    /// A pattern's name.
    Name,
    /// The quoted `ascii` kernel literal.
    Kernel,
    /// `fractalize depth`.
    Depth,
    /// `fractalize max_cells`.
    MaxCells,
    /// `fractalize density`, including its `bps` suffix.
    Density,
    /// A seed: pruning under `Fractalize`, generation under `Generate`.
    Seed,
    /// `linearize`'s traversal word.
    Traversal,
    /// `map_rhythm unit`, the whole `a/b` token.
    Unit,
    /// `map_rhythm tail`.
    Tail,
    /// The quoted `generate { source … }` literal.
    Source,
    /// `generate { bars … }`.
    Bars,
    /// `generate { candidates … }`.
    Candidates,
    /// `generate { strategy … }`.
    Strategy,
    /// The quoted `generate { corpus … }` literal.
    Corpus,
    /// `export`'s format word.
    Format,
    /// The quoted `export` path literal.
    Path,
    /// A level-2 `score { ppqn … }` tick resolution (SWG-4A-06).
    Ppqn,
}

/// One value's address: which construct, and which of its words.
///
/// There is deliberately no `FieldRef::Node` variant. A construct's own
/// location is [`SourceMap::node_span`]'s business, and two ways to ask the
/// same question is one way too many.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldRef {
    node: AstId,
    field: FieldKind,
}

impl FieldRef {
    /// Addresses `field` within `node`.
    #[must_use]
    pub const fn new(node: AstId, field: FieldKind) -> Self {
        Self { node, field }
    }

    /// The construct that owns the value.
    #[must_use]
    pub const fn node(self) -> AstId {
        self.node
    }

    /// Which of the construct's values.
    #[must_use]
    pub const fn field(self) -> FieldKind {
        self.field
    }
}

/// Where every node and value of a parsed tree came from.
///
/// A node span is the smallest contiguous range covering that construct's
/// syntax, without the whitespace around it. A field span covers the
/// **value**, not the word that names it: a diagnostic about `unit` wants to
/// underline `1/16`, not `unit`. String literals include their quotes,
/// because the quotes are part of the value's spelling.
///
/// The maps are private. Reading is the whole contract; a caller that could
/// insert into a source map could make it disagree with the text it claims
/// to describe.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceMap {
    nodes: BTreeMap<AstId, Span>,
    fields: BTreeMap<FieldRef, Span>,
}

impl SourceMap {
    /// Where `id`'s construct sits, if the tree has one.
    #[must_use]
    pub fn node_span(&self, id: AstId) -> Option<Span> {
        self.nodes.get(&id).copied()
    }

    /// Where `reference`'s value sits.
    ///
    /// `None` means the word was never written — an omitted `corpus` has no
    /// location, as opposed to an empty one.
    #[must_use]
    pub fn field_span(&self, reference: FieldRef) -> Option<Span> {
        self.fields.get(&reference).copied()
    }

    /// Every located node, in key order.
    pub fn nodes(&self) -> impl Iterator<Item = (AstId, Span)> + '_ {
        self.nodes.iter().map(|(&id, &span)| (id, span))
    }

    /// Every located value, in key order.
    pub fn fields(&self) -> impl Iterator<Item = (FieldRef, Span)> + '_ {
        self.fields
            .iter()
            .map(|(&reference, &span)| (reference, span))
    }

    /// Records a construct's location. Crate-private: see the type's note.
    pub(crate) fn insert_node(&mut self, id: AstId, span: Span) {
        self.nodes.insert(id, span);
    }

    /// Records a value's location.
    pub(crate) fn insert_field(&mut self, node: AstId, field: FieldKind, span: Span) {
        self.fields.insert(FieldRef::new(node, field), span);
    }
}
