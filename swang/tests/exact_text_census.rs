//! The exhaustive census witness for `docs/swang/exact-score-text.md`
//! (SWG-4A-01 acceptance).
//!
//! The census claims to cover every canonical fact. A one-off script can
//! prove that today; only a committed test proves it after the next field
//! lands. So this reads the canonical model and the comparator straight out
//! of `core/src/`, and fails until the document has caught up.
//!
//! It is evidence, not product code: it adds no public API, changes no
//! behaviour, and reads sources as text rather than linking against them,
//! because the facts it guards are *names in a document* and a type-level
//! check cannot see a markdown table.
//!
//! When this fails, the fix is to update the census — never to relax the
//! test. That direction is the whole point.

// The same allowances the level-1 grammar tests take: this file slices
// source text by byte offsets found with `find`, which are always on
// character boundaries but which clippy cannot prove.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    clippy::missing_assert_message
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// Repository root, from this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("swang/ has a parent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn census() -> String {
    read("docs/swang/exact-score-text.md")
}

/// Every `SemanticField` variant, in declaration order.
fn semantic_field_variants(source: &str) -> Vec<String> {
    let start = source
        .find("pub enum SemanticField {")
        .expect("SemanticField is declared");
    let body = &source[start..];
    let end = body.find("\n}").expect("the enum closes");
    body[..end]
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("//")
                && !line.starts_with('#')
                && line.ends_with(',')
        })
        .map(|line| line.trim_end_matches(',').to_owned())
        .collect()
}

/// The variants the *exact* walker mentions, as opposed to the normalized
/// projection's.
fn exact_walker_variants(source: &str) -> BTreeSet<String> {
    let from = source
        .find("pub fn exact_semantic_diff")
        .expect("the exact walker exists");
    let to = source
        .find("pub fn normalized_musical_diff")
        .expect("the normalized walker exists");
    assert!(from < to, "the exact walker is declared first");
    mentioned_variants(&source[from..to])
}

fn normalized_walker_variants(source: &str) -> BTreeSet<String> {
    let from = source
        .find("pub fn normalized_musical_diff")
        .expect("the normalized walker exists");
    mentioned_variants(&source[from..])
}

/// `SemanticField::Foo` occurrences within `region`.
fn mentioned_variants(region: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (index, _) in region.match_indices("SemanticField::") {
        let tail = &region[index + "SemanticField::".len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            found.insert(name);
        }
    }
    found
}

/// `CamelCase` to `snake_case`, the spelling a census row uses.
fn snake_case(camel: &str) -> String {
    let mut out = String::new();
    for (i, c) in camel.char_indices() {
        if c.is_ascii_uppercase() && i != 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Field names declared in a `pub struct` block.
///
/// `public_only` mirrors what a caller outside the crate can see; the
/// opaque-leaf witness needs the private fields too, because a new private
/// field is exact state that no public signature reveals.
fn struct_fields(source: &str, name: &str, public_only: bool) -> Vec<String> {
    let needle = format!("pub struct {name} {{");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("{name} is declared"));
    let body = &source[start + needle.len()..];
    let end = body.find("\n}").unwrap_or_else(|| panic!("{name} closes"));
    body[..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with('#'))
        .filter(|line| !public_only || is_public(line))
        .filter_map(field_name)
        .collect()
}

/// Whether a field declaration is visible outside the crate.
///
/// `pub(crate)` and `pub(super)` are *not*: they are private for the
/// purposes of the public-API witness, and exact state for the purposes of
/// the opaque-leaf one.
fn is_public(declaration: &str) -> bool {
    declaration.starts_with("pub ") && !declaration.starts_with("pub(")
}

/// The identifier of a field declaration, whatever its visibility.
///
/// Taken as the last whitespace-separated token before the `:`, so every
/// visibility spelling reduces to the same name:
///
/// ```text
/// capo: u8              -> capo
/// pub capo: u8          -> capo
/// pub(crate) capo: u8   -> capo
/// pub(super) capo: u8   -> capo
/// ```
///
/// An earlier version stripped the literal `"pub "` and then discarded any
/// remaining token containing a space, which silently dropped
/// `pub(crate) capo` — the same exact-state drift the witness exists to
/// catch, wearing a different visibility.
fn field_name(declaration: &str) -> Option<String> {
    let head = declaration.split(':').next()?.trim();
    let name = head.split_whitespace().last()?;
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .then(|| name.to_owned())
}

/// How many top-level elements a tuple payload declares.
///
/// Commas nested inside `<…>` or `(…)` belong to a type argument, not to
/// the payload: `Vec<(u8, u8)>` is one element, `String, u32` is two.
fn tuple_arity(inner: &str) -> usize {
    if inner.trim().is_empty() {
        return 0;
    }
    let mut depth = 0_i32;
    let mut count = 1_usize;
    for c in inner.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    count
}

/// Payload fields of every variant of an enum, keyed `Enum::Variant.field`.
///
/// Tuple payloads are keyed by position (`Other.0`). Canonical state lives
/// here as much as in structs, and no struct-field scan reaches it.
fn enum_payload_fields(source: &str, enum_name: &str) -> Vec<String> {
    let needle = format!("pub enum {enum_name} {{");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("{enum_name} is declared"));
    let body = &source[start + needle.len()..];
    let end = body
        .find("\n}")
        .unwrap_or_else(|| panic!("{enum_name} closes"));

    let mut keys = Vec::new();
    let mut variant = String::new();
    let mut depth = 0_u32;
    for line in body[..end].lines().map(str::trim) {
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if depth == 0 {
            let head = line.trim_end_matches([' ', '{', ',']);
            if let Some(open) = head.find('(') {
                // Tuple payload on one line: `Other(String)` — one key per
                // element, because `Other(String, u32)` is two facts and a
                // hard-coded `.0` would let the second one vanish.
                head[..open].clone_into(&mut variant);
                let inner = head[open + 1..].trim_end_matches(')');
                for slot in 0..tuple_arity(inner) {
                    keys.push(format!("{enum_name}::{variant}.{slot}"));
                }
                continue;
            }
            head.clone_into(&mut variant);
            if line.ends_with('{') {
                depth = 1;
            }
            continue;
        }
        if line.starts_with('}') {
            depth = 0;
            continue;
        }
        if let Some(field) = field_name(line) {
            keys.push(format!("{enum_name}::{variant}.{field}"));
        }
    }
    keys
}

/// The section of the census a witness is allowed to look in, so that a
/// number appearing elsewhere in 900 lines cannot satisfy a check by
/// coincidence.
fn section<'a>(doc: &'a str, from: &str, to: &str) -> &'a str {
    let start = doc
        .find(from)
        .unwrap_or_else(|| panic!("the census has a {from:?} section"));
    let end = doc[start..]
        .find(to)
        .unwrap_or_else(|| panic!("the {from:?} section ends at {to:?}"))
        + start;
    &doc[start..end]
}

/// The cells of a markdown table, row by row.
///
/// Witnesses match against these and never against surrounding prose: an
/// earlier draft of the opaque-leaf witness checked the whole section, and
/// passed for `Tuning.capo` because the section's own explanation used
/// `capo` as its worked example. A check a narrative sentence can satisfy
/// is not a check.
fn table_rows(section: &str) -> Vec<Vec<String>> {
    section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .filter(|line| !line.replace(['|', '-', ' '], "").is_empty())
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .collect()
}

/// The row whose first cell names `key`, if the table has one.
fn row_for<'a>(rows: &'a [Vec<String>], key: &str) -> Option<&'a Vec<String>> {
    rows.iter()
        .find(|row| row.first().is_some_and(|cell| cell.contains(key)))
}

/// A section with every whitespace run collapsed to one space.
///
/// Prose assertions must survive the document's line wrapping: the phrase
/// they look for is a sentence, and a sentence in an 76-column markdown file
/// is split across lines at an arbitrary word.
fn flattened(section: &str) -> String {
    section.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── the witnesses ──────────────────────────────────────────────────────────

#[test]
fn every_exact_walker_field_appears_in_the_census() {
    let diff = read("core/src/semantic_diff.rs");
    let doc = census();
    // §2 only. The acceptance criterion is that each exact-walker variant
    // maps to a row of Inventory *B*; a variant left behind in Inventory A
    // is exactly the case that must fail, and a document-wide search would
    // pass it on the strength of its own §1 listing. §2.9 is inside this
    // span, so the opaque leaves stay covered.
    let inventory_b = section(&doc, "## 2. Inventory B", "## 3. Writer domain");
    let missing: Vec<String> = exact_walker_variants(&diff)
        .into_iter()
        .filter(|variant| {
            let snake = snake_case(variant);
            !inventory_b.contains(&snake) && !inventory_b.contains(variant)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "these exact-walker fields are compared but absent from Inventory B \
         of docs/swang/exact-score-text.md: {missing:?}. Add a census row \
         before relaxing this test."
    );
}

#[test]
fn the_census_records_the_current_semantic_field_partition() {
    let diff = read("core/src/semantic_diff.rs");
    let doc = census();

    // Scoped to Inventory A: a "23" elsewhere in 900 lines of markdown must
    // not be able to satisfy this by coincidence.
    let inventory_a = section(&doc, "## 1. Inventory A", "## 2. Inventory B");

    let all = semantic_field_variants(&diff);
    let exact = exact_walker_variants(&diff);
    let normalized = normalized_walker_variants(&diff);
    let normalized_only = normalized.difference(&exact).count();
    let exact_only = exact.difference(&normalized).count();
    let shared = exact.intersection(&normalized).count();

    let unused: Vec<&String> = all
        .iter()
        .filter(|v| !exact.contains(*v) && !normalized.contains(*v))
        .collect();
    assert!(
        unused.is_empty(),
        "SemanticField variants walked by neither comparator: {unused:?}"
    );

    // The document states these four numbers in prose; if the model moves,
    // the prose is now a lie and must be corrected.
    for (label, value) in [
        ("total", all.len()),
        ("exact-only", exact_only),
        ("shared", shared),
        ("normalized-only", normalized_only),
    ] {
        assert!(
            inventory_a.contains(&format!("**{value}**"))
                || inventory_a.contains(&format!("| {value} |")),
            "Inventory A does not state {label} = {value}; \
             SemanticField changed and docs/swang/exact-score-text.md \
             must be updated"
        );
    }
    assert!(
        inventory_a.contains(&format!("uses {} of the", exact.len())),
        "Inventory A does not state that the exact walker uses {} variants",
        exact.len()
    );
}

#[test]
fn every_canonical_field_appears_in_the_syntax_location_map() {
    let score = read("core/src/score.rs");
    let event = read("core/src/event.rs");
    let doc = census();
    // Ends at §2.9, not at §3: otherwise the map inherits the opaque-leaf
    // section's prose and a field named only there would count as mapped.
    let map = section(
        &doc,
        "### 2.8 Canonical field",
        "### 2.9 Opaque exact leaves",
    );

    let mut missing = Vec::new();
    for (source, type_name) in [
        (&score, "Score"),
        (&score, "MasterBar"),
        (&score, "Track"),
        (&score, "Voice"),
        (&score, "EventGroup"),
        (&score, "AtomNote"),
        (&score, "AtomRest"),
        (&score, "TechniqueSpan"),
        (&score, "RepeatMarker"),
        (&score, "SourceMeta"),
        (&score, "LossReport"),
        (&event, "TimeSignature"),
        (&event, "TechniqueEvidence"),
        (&event, "FretboardPosition"),
        (&event, "NotePosition"),
    ] {
        for field in struct_fields(source, type_name, true) {
            // The qualified key, not the bare field name: `start` alone
            // matches `TickRange.start` and would let `RepeatMarker.start`
            // vanish unnoticed.
            let key = format!("{type_name}.{field}");
            if !map.contains(&key) {
                missing.push(key);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "canonical fields with no entry in the §2.8 syntax-location map: \
         {missing:?}. A new field needs a syntax location before the \
         census can claim to be exhaustive."
    );
}

#[test]
fn every_import_warning_variant_has_a_syntax_location() {
    let score = read("core/src/score.rs");
    let doc = census();
    // §2.8 only. The variant name appears in §2.7 and §6.4 as well, so a
    // document-wide search would call a variant "located" on the strength
    // of a mention that is not a location — the Tuning.capo failure mode
    // with a different noun.
    let map = section(
        &doc,
        "### 2.8 Canonical field",
        "### 2.9 Opaque exact leaves",
    );
    let start = score
        .find("pub enum ImportWarning {")
        .expect("ImportWarning is declared");
    let body = &score[start..];
    let end = body.find("\n}").expect("the enum closes");
    let variants: Vec<String> = body[..end]
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|l| {
            l.chars().next().is_some_and(char::is_uppercase)
                && (l.ends_with('{') || l.ends_with(',') || l.ends_with("),"))
        })
        .map(|l| {
            l.trim_end_matches([' ', '{', ',', ')'])
                .split(['(', ' '])
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .filter(|v| !v.is_empty())
        .collect();

    assert!(
        variants.len() >= 4,
        "expected at least the four known ImportWarning variants, found {variants:?}"
    );
    let missing: Vec<&String> = variants
        .iter()
        .filter(|v| !map.contains(&format!("ImportWarning::{v}")))
        .collect();
    assert!(
        missing.is_empty(),
        "ImportWarning variants with no entry in the §2.8 syntax-location \
         map: {missing:?}"
    );
}

#[test]
fn every_closed_word_set_member_is_spelled_in_the_census() {
    let event = read("core/src/event.rs");
    let score = read("core/src/score.rs");
    let doc = census();
    // §6.4 only — the section whose acceptance criterion this is. The
    // message already claimed §6.4; now the search agrees with it.
    let word_sets = section(&doc, "### 6.4 Closed word sets", "### 6.4b Field words");

    let mut missing = Vec::new();
    for (source, enum_name) in [
        (&event, "SpanTechnique"),
        (&event, "NoteMark"),
        (&event, "TechniqueSource"),
        (&score, "EventGroupKind"),
    ] {
        let needle = format!("pub enum {enum_name} {{");
        let start = source
            .find(&needle)
            .unwrap_or_else(|| panic!("{enum_name} is declared"));
        let body = &source[start + needle.len()..];
        let end = body
            .find("\n}")
            .unwrap_or_else(|| panic!("{enum_name} closes"));
        for line in body[..end].lines().map(str::trim) {
            let Some(first) = line.chars().next() else {
                continue;
            };
            if !first.is_ascii_uppercase() {
                continue;
            }
            let variant = line
                .trim_end_matches([' ', '{', ','])
                .split([' ', '{', ','])
                .next()
                .unwrap_or_default();
            if variant.is_empty() {
                continue;
            }
            if !word_sets.contains(&snake_case(variant)) {
                missing.push(format!("{enum_name}::{variant}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "closed-set variants with no spelling in §6.4: {missing:?}. \
         Every variant is listed with no wildcard arm — that is the \
         acceptance criterion."
    );
}

/// The blind spot Inventory A cannot see.
///
/// `Tempo`, `Tuning`, `NoteMarks`, and `ConfidenceBps` keep their state
/// private. A new private field inside one of them is new exact semantics
/// that adds no `SemanticField` variant, changes no public signature, and
/// would leave every Inventory-A witness above perfectly green while the
/// text quietly stopped carrying a fact. `Tuning.capo` is the worked
/// example. This is the only witness that looks at private shape.
#[test]
fn opaque_exact_leaves_keep_their_documented_shape() {
    let event = read("core/src/event.rs");
    let doc = census();
    let rows = table_rows(section(
        &doc,
        "### 2.9 Opaque exact leaves",
        "## 3. Writer domain",
    ));

    let mut missing = Vec::new();

    // Brace structs: every field, private included, must appear in the
    // "private shape" cell of that leaf's own row.
    for leaf in ["Tempo", "Tuning"] {
        let fields = struct_fields(&event, leaf, false);
        assert!(
            !fields.is_empty(),
            "{leaf} parsed as having no fields — the parser, not the model, is wrong"
        );
        let row =
            row_for(&rows, leaf).unwrap_or_else(|| panic!("§2.9 has no table row for {leaf}"));
        let shape = row.get(1).map_or("", String::as_str);
        for field in fields {
            if !shape.contains(&field) {
                missing.push(format!("{leaf}.{field}"));
            }
        }
    }

    // Tuple newtypes: §2.9 must state the carried type, since that is the
    // whole of their shape.
    for (leaf, carried) in [("NoteMarks", "u16"), ("ConfidenceBps", "u16")] {
        let needle = format!("pub struct {leaf}(");
        let start = event
            .find(&needle)
            .unwrap_or_else(|| panic!("{leaf} is declared as a tuple struct"));
        let tail = &event[start + needle.len()..];
        let end = tail.find(')').expect("the tuple struct closes");
        let declared = tail[..end].trim();
        assert_eq!(
            declared, carried,
            "{leaf} now carries `{declared}`, not `{carried}`; §2.9 must be updated"
        );
        let row =
            row_for(&rows, leaf).unwrap_or_else(|| panic!("§2.9 has no table row for {leaf}"));
        assert!(
            row.get(1).is_some_and(|shape| shape.contains(carried)),
            "§2.9's row for {leaf} does not describe it as carrying {carried}"
        );
    }

    assert!(
        missing.is_empty(),
        "opaque leaf state absent from the §2.9 decomposition table: \
         {missing:?}. This is exact semantics that Inventory A cannot see — \
         the text must open it, so the census must name it."
    );
}

/// Multiplicity is part of the closed word sets (§6.4b), and `SWG0404`
/// applies to singletons only. A `*` word that lost its marker would make
/// the reference document in §6.1 reject itself at its second `master_bar`.
#[test]
fn repeated_structural_blocks_are_marked_as_such() {
    let doc = census();
    let sets = section(&doc, "### 6.4b Field words are closed", "### 6.5 Strings");
    // `note` and `rest` are deliberately absent: they are tags of the
    // `atom *` slot, not repeated fields of their own (§6.4b).
    for word in [
        "master_bar *",
        "track *",
        "voice *",
        "group *",
        "atom *",
        "span *",
        "warning *",
    ] {
        assert!(
            sets.contains(word),
            "§6.4b does not mark `{word}` as a repeated block; without the \
             multiplicity, SWG0404 would reject a second one"
        );
    }
    assert!(
        flattened(sets).contains("`SWG0404` applies to `1` and `?` words only"),
        "§6.4b must scope SWG0404 to singleton words"
    );
}

/// Canonical state that lives in enum payloads, which no struct-field scan
/// reaches.
///
/// Adding `TempoApproximated.source_tick` and repairing the compile would
/// leave the variant name present, the `SemanticField` set unchanged, and
/// every struct witness green — while exact text quietly stopped carrying
/// the field. §2.8 keys these `Enum::Variant.field` so this can match them.
#[test]
fn every_enum_payload_field_has_a_syntax_location() {
    let score = read("core/src/score.rs");
    let doc = census();
    let map = section(
        &doc,
        "### 2.8 Canonical field",
        "### 2.9 Opaque exact leaves",
    );

    let mut keys = enum_payload_fields(&score, "ImportWarning");
    keys.extend(enum_payload_fields(&score, "EventGroupKind"));
    assert!(
        keys.len() >= 5,
        "expected at least the five known payload fields, found {keys:?}"
    );

    let missing: Vec<&String> = keys.iter().filter(|key| !map.contains(*key)).collect();
    assert!(
        missing.is_empty(),
        "enum payload fields with no entry in the §2.8 syntax-location map: \
         {missing:?}. Payload state is canonical state; it adds no \
         SemanticField variant and no struct field, so this entry is the \
         only thing standing between it and silent loss."
    );
}

/// `EventGroup.atoms` and `LossReport.warnings` are each **one** vector, so
/// their variant tags are alternatives at one position — not independent
/// repeated fields a formatter may group.
///
/// Without the `:=` lines, a canonical writer could tidily emit every
/// `note` before every `rest` and destroy the vector order it exists to
/// preserve.
#[test]
fn sum_type_collections_are_single_slots() {
    let score = read("core/src/score.rs");
    let doc = census();
    let sets = section(&doc, "### 6.4b Field words are closed", "### 6.5 Strings");

    // The model really does hold one vector each.
    assert!(
        score.contains("pub atoms: Vec<AtomEvent>"),
        "EventGroup.atoms is no longer a single Vec<AtomEvent>; the joint \
         slot contract in §6.4b must be revisited"
    );
    assert!(
        score.contains("pub warnings: Vec<ImportWarning>"),
        "LossReport.warnings is no longer a single Vec<ImportWarning>; the \
         joint slot contract in §6.4b must be revisited"
    );

    for slot in [
        "atom *                 := note | rest",
        "warning *              :=",
    ] {
        assert!(
            sets.contains(slot),
            "§6.4b does not declare the joint slot `{slot}`; without it, \
             note/rest and the warning names read as separate repeated \
             fields that a formatter may group and reorder"
        );
    }
    assert!(
        !sets.contains("group                      note * | rest *"),
        "§6.4b still lists note and rest as independent repeated fields"
    );

    let ordering = flattened(section(&doc, "### 6.2 Canonical order", "### 6.3 Scalars"));
    assert!(
        ordering.contains("do not create separate slots"),
        "§6.2 must state that variant tags inside one sum-type slot do not \
         create separate slots"
    );
    assert!(
        ordering.contains("singleton and repeated alike"),
        "§6.2 must give the formatter a canonical order over repeated slots \
         too, or format(parse(x)) has no single answer where the model does \
         not store the interleaving between two slots"
    );
}

/// The payload parser is itself checked, against a synthetic source rather
/// than by mutilating `griff-core`.
///
/// Widening a tuple payload is the one enum drift the compiler does *not*
/// force into every call site's face as a named field, and an earlier
/// version hard-coded `.0` for every tuple variant — so
/// `Other(String, u32)` would have kept the census green while `.1` went
/// unwritten.
#[test]
fn the_payload_parser_counts_tuple_elements() {
    assert_eq!(tuple_arity(""), 0);
    assert_eq!(tuple_arity("String"), 1);
    assert_eq!(tuple_arity("String, u32"), 2);
    assert_eq!(
        tuple_arity("Vec<(u8, u8)>"),
        1,
        "nested commas are not elements"
    );
    assert_eq!(tuple_arity("Vec<(u8, u8)>, u32"), 2);

    let synthetic = "\
pub enum Probe {
    Named {
        alpha: u8,
        beta: u32,
    },
    Bare,
    One(String),
    Two(String, u32),
}
";
    let keys = enum_payload_fields(synthetic, "Probe");
    assert_eq!(
        keys,
        vec![
            "Probe::Named.alpha".to_owned(),
            "Probe::Named.beta".to_owned(),
            "Probe::One.0".to_owned(),
            "Probe::Two.0".to_owned(),
            "Probe::Two.1".to_owned(),
        ],
        "a widened tuple payload must yield one key per element"
    );
}
