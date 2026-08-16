//! The level-1 canonical formatter (spec §3.5 laws 2–3).

use griff_pattern::Traversal;

use crate::syntax::ast::v1::{ExportFormat, Program, StrategyName, StrategyPolicy};
use crate::TailPolicy;

/// Formats a [`Program`] into its canonical text — the unique fixed point of
/// `format ∘ parse` (spec §3.5 laws 2–3).
#[must_use]
pub fn format(program: &Program) -> String {
    let p = &program.pattern;
    let prune = p.fractalize.prune.map_or_else(String::new, |prune| {
        format!(" density {}bps seed {}", prune.density.get(), prune.seed)
    });
    let corpus = p
        .generate
        .corpus
        .as_ref()
        .map_or_else(String::new, |corpus| {
            format!("        corpus \"{}\"\n", corpus.as_str())
        });
    format!(
        "swang {level}\n\
         \n\
         pattern {name} {{\n\
         \x20   ascii \"{kernel}\"\n\
         \x20   |> fractalize depth {depth} max_cells {max_cells}{prune}\n\
         \x20   |> linearize {traversal}\n\
         \x20   |> map_rhythm unit {numerator}/{denominator} tail {tail}\n\
         \x20   |> generate {{\n\
         \x20       source \"{source}\"\n\
         \x20       bars {bars}\n\
         \x20       seed {seed}\n\
         \x20       candidates {candidates}\n\
         \x20       strategy {strategy}\n\
         {corpus}\
         \x20   }}\n\
         \x20   |> export {export} \"{path}\"\n\
         }}\n",
        level = program.level.get(),
        name = p.name.as_str(),
        kernel = p.kernel.as_str(),
        depth = p.fractalize.depth,
        max_cells = p.fractalize.max_cells,
        traversal = traversal_word(p.linearize.traversal),
        numerator = p.map_rhythm.unit.numerator(),
        denominator = p.map_rhythm.unit.denominator(),
        tail = tail_word(p.map_rhythm.tail),
        source = p.generate.source.as_str(),
        bars = p.generate.bars,
        seed = p.generate.seed,
        candidates = p.generate.candidates,
        strategy = strategy_word(p.generate.strategy),
        export = export_word(p.export.format),
        path = p.export.path.as_str(),
    )
}

/// The canonical spelling of a traversal.
const fn traversal_word(traversal: Traversal) -> &'static str {
    match traversal {
        Traversal::RowMajor => "row_major",
        Traversal::Snake => "snake",
    }
}

/// The canonical spelling of a tail policy.
const fn tail_word(tail: TailPolicy) -> &'static str {
    match tail {
        TailPolicy::Reject => "reject",
        TailPolicy::RestPad => "rest_pad",
    }
}

/// The canonical spelling of a strategy policy.
const fn strategy_word(strategy: StrategyPolicy) -> &'static str {
    match strategy {
        StrategyPolicy::Auto => "auto",
        StrategyPolicy::Named(StrategyName::RhythmCopy) => "rhythm_copy",
        StrategyPolicy::Named(StrategyName::MotifTranspose) => "motif_transpose",
        StrategyPolicy::Named(StrategyName::ConstrainedWalk) => "constrained_walk",
        StrategyPolicy::Named(StrategyName::ShuffleMotifs) => "shuffle_motifs",
        StrategyPolicy::Named(StrategyName::RepeatVariation) => "repeat_variation",
    }
}

/// The canonical spelling of an export format.
const fn export_word(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Midi => "midi",
    }
}
