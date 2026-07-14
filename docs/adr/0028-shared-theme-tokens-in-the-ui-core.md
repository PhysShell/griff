# ADR 0028: Shared theme tokens in the UI core — one palette, two renderers, asserted contrast

Date: 2026-07-14
Status: Proposed

Completes ADR-0016 (shared UI core across frontends) in the one dimension it
left to the renderers, and consumes ADR-0027 (the egui cockpit) unchanged.

## Context

ADR-0016 gave the frontends one core so that *"a change verified in one cannot
silently diverge in another"*: view-model → interaction core → `scene::resolve`,
which places every cell and tags it with a semantic `CellRole`. Renderers were
told to blit that grid, "never re-derive or diverge from it".

Colour was never part of that promise, and the promise duly broke. The section
band's cells carry the class name — `scene::resolve_band` centres "Riff" or
"Breakdown" in each section's span — and the `ratatui` preview prints it. The
cockpit's `glyph_color` had no `BandFill` arm, so it painted a bare colour block:
same `Scene`, two renderers, different information, and the classification
encoded by colour alone — which WCAG 1.4.1 forbids, and which the Breakdown-red
/ Clean-green pair makes unreadable to a deuteranope. Nothing caught it, because
nothing *could*: no shared artefact said what a `BandFill` cell means visually.

The palette itself is copied by hand. `preview/design/index.html` is the real
master — a complete token set (`--bg`, `--panel`, `--text{,-dim,-faint}`,
`--accent`, `--grid-{beat,bar}`, `--row-black`, `--playhead`, `--boundary`,
plus widget states) in **both** light and dark. The cockpit transcribed eight of
them into raw `Color32` constants, dropped the rest, dropped light mode
entirely, and never installed an `egui::Visuals`, so its chrome is stock egui
dark while its plane is the mock's. The `ratatui` preview carries a third,
independent set. Three copies of one palette, no seam between them, and a value
duplicated by accident inside the cockpit itself (the redistributable `↗` mark
hardcodes the lane-4 green).

The copies are also, measurably, wrong. Auditing the cockpit's constants against
WCAG: the `SEC` band header read at **3.06:1** (text wants 4.5:1), and dimming
the unselected sections with `gamma_multiply(0.55)` put Riff at **1.54:1** and
Breakdown at **1.45:1** against the surface — under the 3:1 floor for meaningful
graphics, erasing the very classification the fill encodes. Those are fixed, but
they were fixed *in one renderer*, by hand, with the numbers recomputed from
first principles. The next palette edit starts that over.

## Decision

Add **`griff_ui_core::theme`**: the semantic palette, renderer-neutral, next to
the `Scene` whose cells it colours.

- **Tokens, not hexes.** A `Token` names a semantic role — surface, panel, text,
  text-dim, accent, grid-bar, playhead, boundary, the per-`BarClass` fills and
  their inks, the note lanes. A `Theme` resolves each to an `Rgb` triple. No
  renderer type appears in the core: the cockpit maps `Rgb` to `Color32`, the
  preview to `ratatui::style::Color`.
- **Roles resolve, renderers blit.** `theme::cell_style(role, theme)` answers
  what a `CellRole` looks like — fill, ink, and whether it draws a glyph — so
  neither renderer decides again, and neither can drop the band's label without
  the other. This is the styling half of the same seam `scene::resolve` is the
  layout half of.
- **Two modes.** `Theme::dark()` and `Theme::light()`, transcribed once from
  `preview/design/index.html`, which stays the design master; the Rust tokens
  become the *code* master that both renderers and the mock answer to.
- **Contrast is a test, not a hope.** `theme` exposes the WCAG relative-luminance
  ratio, and ui-core asserts the floors over every token pair in both modes: text
  ≥ 4.5:1 on its surface, meaningful graphics ≥ 3:1, every class label ≥ 4.5:1 on
  its own fill. A palette edit that breaks legibility fails `cargo test`, in the
  crate that owns the palette, before either renderer sees it.

Out of scope, deliberately: pixel layout (it is `scene`'s, and stays there),
widget chrome beyond the tokens the cockpit feeds to `egui::Visuals`, and any
notion of a user-editable theme.

## Consequences

- ui-core grows a styling seam it did not have. That is the point — it is where
  the divergence ADR-0016 forbids actually lives — but it does widen the core's
  remit from "what is placed where" to "and what it means visually".
- Both renderers must stop inventing colour. The cockpit's constants and the
  preview's palette both collapse into token lookups; the accidental duplicate
  (`↗` vs lane-4) disappears by construction.
- Light mode becomes reachable for the cockpit for the first time — though the
  mock's light tokens are not clean either: `--text-faint` sits at 2.77:1 and
  `--playhead` at 2.32:1 on the light surface, so the contrast tests will fail
  them and the light palette needs work before it ships. Better to find that in a
  test than on a stage.
- Two crates change together for a palette edit, and ui-core's test suite gets
  slower by a hair. Accepted.

## Alternatives considered

- **A cockpit-local `theme.rs`.** Smaller diff, no ADR — and no seam: the preview
  keeps its own palette, so the exact class of bug this ADR exists to close can
  recur the next time a renderer decides what a cell means.
- **Leave the raw hexes, fix bugs as found.** What we have been doing. The band
  bug survived a code review, a merge, and a release of the Generate panel.
- **Generate the tokens from the CSS mock at build time.** Ties the build to an
  HTML file and inverts the dependency: the mock is a *design* artefact, and a
  parser for it is more machinery than a transcribed table plus a contrast test.
- **A theming crate off crates.io.** Buys nothing here — the palette is twenty
  values — and spends the dependency posture (ADR: lean tree) on it.
