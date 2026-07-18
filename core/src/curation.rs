//! Dedup-cluster builder (curation step 3A).
//!
//! Groups a corpus's phrase chunks into near-duplicate **clusters** — a
//! representative plus the variants that quote it — purely from [`ChunkMeta`].
//! No `Score`, no MIDI import, no I/O: this only reads the persisted
//! `duplicate` links and turns them into the unit a curator *navigates*. It is
//! deliberately not a graph framework; it resolves one link relation and
//! reports every malformed link, so corrupt provenance surfaces rather than
//! quietly masquerading as a clean singleton.
//!
//! A cluster is a unit of **navigation and comparison, never of automatic
//! fate**: no member is accepted or rejected here, not even an exact
//! (`quote_share == 1.0`) repeat. The decision stays per chunk, downstream.

use std::collections::{HashMap, HashSet};

use crate::corpus::{ChunkId, ChunkMeta};

/// A near-duplicate cluster: the representative phrase and the variants linked
/// to it (transitively). `size == 1 + variants.len()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurationCluster {
    /// Cluster key — the representative's id.
    pub key: ChunkId,
    /// The root phrase (no `duplicate` link of its own).
    pub representative: ChunkId,
    /// The near-duplicate variants, sorted by id (deterministic).
    pub variants: Vec<ChunkId>,
    /// `1 + variants.len()`.
    pub size: usize,
}

/// A malformed `duplicate` link. Surfaced, never silently collapsed into a
/// singleton — a broken link is a provenance defect, not a lone phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClusterDiagnostic {
    /// A chunk carries a `duplicate` link but its id has no parseable `_p<N>`
    /// phrase suffix, so the link cannot be resolved.
    UnresolvableProvenance { chunk: ChunkId },
    /// `duplicate.of` names a phrase not present in the corpus.
    DanglingLink { chunk: ChunkId, target: ChunkId },
    /// A chunk links to itself.
    SelfLink { chunk: ChunkId },
    /// `duplicate.of` is not strictly earlier than the chunk's own phrase — the
    /// "quotes an *earlier* phrase" rule is broken.
    ForwardLink {
        chunk: ChunkId,
        of: usize,
        own: usize,
    },
    /// The link resolves to a chunk in a different split run (its ensemble
    /// group/part differs), so the two are not comparable variants.
    CrossRunLink { chunk: ChunkId, target: ChunkId },
    /// The link chain loops.
    Cycle { chunk: ChunkId },
}

/// Builds the near-duplicate clusters of `chunks`, with a diagnostic for every
/// malformed link.
///
/// On a clean corpus every chunk lands in exactly one cluster and the
/// diagnostics are empty; a chunk on a broken link is left out of the clusters
/// and reported instead. Deterministic: the result does not depend on the input
/// order.
#[must_use]
#[allow(clippy::too_many_lines)] // one cohesive pass: resolve links, roots, then group
pub fn build_clusters(chunks: &[ChunkMeta]) -> (Vec<CurationCluster>, Vec<ClusterDiagnostic>) {
    let by_id: HashMap<&str, &ChunkMeta> = chunks.iter().map(|c| (c.id.0.as_str(), c)).collect();
    let mut diagnostics = Vec::new();
    let mut bad: HashSet<String> = HashSet::new();
    // id -> None (root) or Some(parent id); absent means excluded by a diagnostic.
    let mut parent: HashMap<String, Option<String>> = HashMap::new();

    for c in chunks {
        let id = c.id.0.clone();
        let Some(dup) = &c.duplicate else {
            parent.insert(id, None);
            continue;
        };
        let Some((prefix, own)) = split_phrase(&id) else {
            diagnostics.push(ClusterDiagnostic::UnresolvableProvenance {
                chunk: c.id.clone(),
            });
            bad.insert(id);
            continue;
        };
        if dup.of == own {
            diagnostics.push(ClusterDiagnostic::SelfLink {
                chunk: c.id.clone(),
            });
            bad.insert(id);
            continue;
        }
        if dup.of > own {
            diagnostics.push(ClusterDiagnostic::ForwardLink {
                chunk: c.id.clone(),
                of: dup.of,
                own,
            });
            bad.insert(id);
            continue;
        }
        let target = format!("{prefix}_p{}", dup.of);
        let Some(target_meta) = by_id.get(target.as_str()) else {
            diagnostics.push(ClusterDiagnostic::DanglingLink {
                chunk: c.id.clone(),
                target: ChunkId(target),
            });
            bad.insert(id);
            continue;
        };
        if target_meta.ensemble != c.ensemble {
            diagnostics.push(ClusterDiagnostic::CrossRunLink {
                chunk: c.id.clone(),
                target: ChunkId(target),
            });
            bad.insert(id);
            continue;
        }
        parent.insert(id, Some(target));
    }

    // Resolve each chunk to its root, detecting a cycle or a chain into an
    // excluded chunk. The "earlier only" rule already forbids cycles, so this
    // is a defensive net rather than an expected path.
    let mut root_of: HashMap<String, String> = HashMap::new();
    for c in chunks {
        let start = c.id.0.clone();
        if bad.contains(&start) || root_of.contains_key(&start) {
            continue;
        }
        let mut path: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut cur = start;
        let root = loop {
            if !seen.insert(cur.clone()) {
                for node in &path {
                    diagnostics.push(ClusterDiagnostic::Cycle {
                        chunk: ChunkId(node.clone()),
                    });
                }
                break None;
            }
            match parent.get(&cur) {
                Some(None) => break Some(cur.clone()),
                Some(Some(next)) => {
                    path.push(cur.clone());
                    cur = next.clone();
                }
                None => break None,
            }
        };
        match root {
            Some(root_id) => {
                for node in path {
                    root_of.insert(node, root_id.clone());
                }
                root_of.insert(root_id.clone(), root_id);
            }
            None => {
                for node in path {
                    bad.insert(node);
                }
            }
        }
    }

    let mut members: HashMap<String, Vec<String>> = HashMap::new();
    for c in chunks {
        let id = &c.id.0;
        if bad.contains(id) {
            continue;
        }
        if let Some(root) = root_of.get(id) {
            members.entry(root.clone()).or_default().push(id.clone());
        }
    }

    let mut clusters: Vec<CurationCluster> = members
        .into_iter()
        .map(|(root, mut ids)| {
            ids.sort();
            let variants: Vec<ChunkId> = ids
                .into_iter()
                .filter(|i| i != &root)
                .map(ChunkId)
                .collect();
            let size = variants.len().saturating_add(1);
            CurationCluster {
                key: ChunkId(root.clone()),
                representative: ChunkId(root),
                variants,
                size,
            }
        })
        .collect();
    clusters.sort_by(|a, b| a.key.0.cmp(&b.key.0));
    (clusters, diagnostics)
}

/// Splits a phrase-chunk id into `(run_prefix, phrase_index)` at its trailing
/// `_p<N>` — the suffix the corpus schema pairs with `duplicate.of`. `None`
/// when the id has no such suffix.
fn split_phrase(id: &str) -> Option<(&str, usize)> {
    let at = id.rfind("_p")?;
    let (prefix, rest) = id.split_at(at);
    let index = rest.strip_prefix("_p")?.parse::<usize>().ok()?;
    Some((prefix, index))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{build_clusters, split_phrase, ClusterDiagnostic};
    use crate::corpus::{ChunkId, ChunkMeta, EnsembleRef, SourceFormat, SourceRef};
    use crate::novelty::PhraseDuplicate;

    fn chunk(id: &str, group: &str, part: u32, dup: Option<usize>) -> ChunkMeta {
        ChunkMeta {
            id: ChunkId(id.to_owned()),
            title: String::new(),
            source: SourceRef {
                filename: "s.gp".to_owned(),
                format: SourceFormat::Gp,
                bar_range: Some((0, 1)),
                track_index: Some(part),
                sha256: Some("hash".to_owned()),
            },
            tempo_bpm: 120.0,
            ticks_per_quarter: 480,
            time_signature: (4, 4),
            tuning: "standard_e".to_owned(),
            tags: Vec::new(),
            boundaries: Vec::new(),
            techniques: Vec::new(),
            quality_flags: Vec::new(),
            reviewer: None,
            structure: None,
            gesture: None,
            complexity: None,
            duplicate: dup.map(|of| PhraseDuplicate {
                of,
                quote_share: 1.0,
            }),
            style_cohort: None,
            ensemble: Some(EnsembleRef {
                group_id: group.to_owned(),
                part_index: part,
            }),
            rights: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// `g_p0` <- `g_p1` <- `g_p2`: a transitive chain must be one cluster of 3.
    fn transitive_run() -> Vec<ChunkMeta> {
        vec![
            chunk("g_g0_p0", "g", 0, None),
            chunk("g_g0_p1", "g", 0, Some(0)),
            chunk("g_g0_p2", "g", 0, Some(1)),
        ]
    }

    #[test]
    fn split_phrase_reads_the_trailing_index() {
        assert_eq!(split_phrase("dgd_care_g1_p12"), Some(("dgd_care_g1", 12)));
        assert_eq!(split_phrase("no_suffix"), None);
    }

    #[test]
    fn a_transitive_chain_is_one_cluster() {
        let (clusters, diags) = build_clusters(&transitive_run());
        assert!(diags.is_empty(), "clean links produce no diagnostics");
        assert_eq!(clusters.len(), 1);
        let c = clusters.first().expect("one cluster");
        assert_eq!(c.representative.0, "g_g0_p0", "the linkless root leads");
        assert_eq!(c.size, 3);
        assert_eq!(
            c.variants,
            vec![ChunkId("g_g0_p1".to_owned()), ChunkId("g_g0_p2".to_owned())],
            "both later phrases are variants, resolved transitively"
        );
    }

    #[test]
    fn independent_roots_are_separate_singletons() {
        let chunks = vec![
            chunk("g_g0_p0", "g", 0, None),
            chunk("g_g0_p1", "g", 0, None),
        ];
        let (clusters, diags) = build_clusters(&chunks);
        assert!(diags.is_empty());
        assert_eq!(clusters.len(), 2);
        assert!(clusters
            .iter()
            .all(|c| c.size == 1 && c.variants.is_empty()));
    }

    #[test]
    fn the_result_is_independent_of_input_order() {
        let mut reversed = transitive_run();
        reversed.reverse();
        assert_eq!(build_clusters(&transitive_run()), build_clusters(&reversed));
    }

    #[test]
    fn a_dangling_link_is_a_diagnostic_not_a_singleton() {
        // p2 quotes an *earlier* phrase 1, but no p1 exists in the corpus.
        let chunks = vec![
            chunk("g_g0_p0", "g", 0, None),
            chunk("g_g0_p2", "g", 0, Some(1)),
        ];
        let (clusters, diags) = build_clusters(&chunks);
        assert_eq!(clusters.len(), 1, "only the valid root clusters");
        assert!(matches!(
            diags.as_slice(),
            [ClusterDiagnostic::DanglingLink { .. }]
        ));
    }

    #[test]
    fn a_self_link_is_a_diagnostic() {
        let chunks = vec![chunk("g_g0_p0", "g", 0, Some(0))];
        let (_, diags) = build_clusters(&chunks);
        assert!(matches!(
            diags.as_slice(),
            [ClusterDiagnostic::SelfLink { .. }]
        ));
    }

    #[test]
    fn a_forward_link_is_a_diagnostic() {
        // p0 claims to quote a *later* phrase 1 — violates "earlier only".
        let chunks = vec![
            chunk("g_g0_p0", "g", 0, Some(1)),
            chunk("g_g0_p1", "g", 0, None),
        ];
        let (_, diags) = build_clusters(&chunks);
        assert!(matches!(
            diags.as_slice(),
            [ClusterDiagnostic::ForwardLink { of: 1, own: 0, .. }]
        ));
    }

    #[test]
    fn a_cycle_is_a_diagnostic() {
        // Two chunks quoting each other (both forward/back) — a loop.
        let chunks = vec![
            chunk("g_g0_p0", "g", 0, Some(1)),
            chunk("g_g0_p1", "g", 0, Some(0)),
        ];
        let (_, diags) = build_clusters(&chunks);
        assert!(
            !diags.is_empty(),
            "a link cycle must be reported, never silently clustered"
        );
    }

    #[test]
    fn a_cross_run_link_is_a_diagnostic() {
        // p1's id resolves a target whose ensemble group differs.
        let mut a = chunk("g_g0_p0", "other", 0, None);
        a.id = ChunkId("g_g0_p0".to_owned());
        let b = chunk("g_g0_p1", "g", 0, Some(0));
        let (_, diags) = build_clusters(&[a, b]);
        assert!(matches!(
            diags.as_slice(),
            [ClusterDiagnostic::CrossRunLink { .. }]
        ));
    }

    #[test]
    fn every_chunk_lands_in_exactly_one_cluster_on_clean_input() {
        let chunks = transitive_run();
        let (clusters, diags) = build_clusters(&chunks);
        assert!(diags.is_empty());
        let total: usize = clusters.iter().map(|c| c.size).sum();
        assert_eq!(total, chunks.len(), "cluster sizes sum to the chunk count");
    }
}
