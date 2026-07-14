//! The semantic palette both frontends resolve their colours through (ADR-0028).
//!
//! `scene::resolve` places every cell and tags it with a [`CellRole`]; this
//! module answers what that role *looks like*. It is the styling half of the
//! seam `scene` is the layout half of, and it exists because the other half
//! alone was not enough: the cockpit painted the section band as bare colour
//! blocks while the `ratatui` preview printed the class name off the same
//! `Scene` — one core, two renderers, different information (ADR-0016).
//!
//! Renderer-neutral by construction: the core speaks [`Rgb`], the cockpit maps
//! it to `egui::Color32` and the preview to a `ratatui` colour. Nothing here
//! knows about either.
//!
//! Legibility is asserted, not hoped for. [`contrast_ratio`] is the WCAG 2.1
//! ratio, and this module's tests hold every token pair in both modes to the
//! floors: 4.5:1 for text, 3:1 for meaningful graphics, and 4.5:1 for a section's
//! class label against its own fill — because that label, not its colour, is what
//! carries the classification to a reader who cannot tell Breakdown's red from
//! Clean's green.

use griff_core::classify::BarClass;

use crate::scene::{CellRole, SceneCell};

/// An opaque sRGB colour — the core's only colour type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Rgb {
    /// A colour from its channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Relative luminance, per WCAG 2.1.
    // The coefficients are quoted from the spec; folding them into a `mul_add`
    // chain buys precision no colour comparison needs, and costs a reader the
    // ability to check them against WCAG at a glance.
    #[allow(clippy::suboptimal_flops)]
    #[must_use]
    pub fn luminance(self) -> f64 {
        fn channel(v: u8) -> f64 {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }
}

/// The WCAG 2.1 contrast ratio between two opaque colours, in `1.0..=21.0`.
///
/// The floors this codebase holds to: **4.5:1** for text, **3:1** for a
/// graphical object that carries meaning (a section's fill, a note, the
/// playhead). Grid lines are decoration and are exempt by design — they are
/// meant to sit under the data, not compete with it.
#[must_use]
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// How a placed cell draws: a fill behind it, an ink for its glyph, or both.
///
/// `ink: None` means the cell is a solid block and its glyph is not drawn —
/// which is exactly the mistake that produced the band bug, so the band's roles
/// answer with an ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    /// The fill behind the cell, or `None` to leave the surface showing.
    pub fill: Option<Rgb>,
    /// The colour the cell's glyph is drawn in, or `None` when it draws none.
    pub ink: Option<Rgb>,
}

/// The semantic palette, in one mode.
///
/// Transcribed from `preview/design/index.html`, which stays the design master;
/// these tokens are the code master both renderers answer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The surface the plane is painted on.
    pub surface: Rgb,
    /// Panel / gutter background.
    pub panel: Rgb,
    /// Primary text.
    pub text: Rgb,
    /// De-emphasised text — labels, headers.
    pub text_dim: Rgb,
    /// The gutter↔plane separator.
    pub stroke: Rgb,
    /// A bar gridline. Decoration: exempt from the 3:1 floor.
    pub grid_bar: Rgb,
    /// Black-key row shading. Decoration.
    pub row_shade: Rgb,
    /// Selection / focus accent.
    pub accent: Rgb,
    /// The playhead column.
    pub playhead: Rgb,
    /// An S4 phrase-boundary marker.
    pub boundary: Rgb,
    /// Ink for light fills.
    pub ink_on_light: Rgb,
    /// Ink for deep fills.
    pub ink_on_dark: Rgb,
    /// Per-`BarClass` fill for an unselected section.
    classes: [Rgb; 5],
    /// Per-`BarClass` fill for the selected section.
    classes_selected: [Rgb; 5],
    /// The note-lane cycle.
    lanes: [Rgb; 6],
}

/// How many lanes the note palette cycles through.
const LANES: u16 = 6;

impl Theme {
    /// The dark mode — the cockpit's and the preview's default.
    ///
    /// Chrome is the design mock's dark token set verbatim. The section fills
    /// are the deep hues; **selection lifts** them toward white rather than
    /// dimming the rest, because these hues sit low on this surface (Breakdown
    /// clears the 3:1 floor by 0.09) and dimming drove the unselected sections
    /// under it while leaving the selected one the quietest thing in the band.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            surface: Rgb::new(0x1c, 0x1c, 0x1e),
            panel: Rgb::new(0x24, 0x24, 0x27),
            text: Rgb::new(0xdc, 0xdc, 0xde),
            text_dim: Rgb::new(0x9a, 0x9a, 0xa2),
            stroke: Rgb::new(0x46, 0x46, 0x4d),
            grid_bar: Rgb::new(0x45, 0x45, 0x4e),
            row_shade: Rgb::new(0x20, 0x20, 0x24),
            accent: Rgb::new(0x3b, 0x9d, 0xff),
            playhead: Rgb::new(0xff, 0xcf, 0x4d),
            boundary: Rgb::new(0xff, 0x5d, 0x6c),
            ink_on_light: Rgb::new(0x11, 0x11, 0x14),
            ink_on_dark: Rgb::new(0xff, 0xff, 0xff),
            classes: [
                Rgb::new(0x16, 0x68, 0xdc), // Riff
                Rgb::new(0xcf, 0x13, 0x22), // Breakdown
                Rgb::new(0xd4, 0x88, 0x06), // Solo
                Rgb::new(0x38, 0x9e, 0x0d), // Clean
                Rgb::new(0x6e, 0x6e, 0x76), // Unknown
            ],
            classes_selected: [
                Rgb::new(0x68, 0x9d, 0xe8),
                Rgb::new(0xe0, 0x66, 0x6f),
                Rgb::new(0xe3, 0xb2, 0x5d),
                Rgb::new(0x7e, 0xc0, 0x62),
                Rgb::new(0xa1, 0xa1, 0xa6),
            ],
            lanes: [
                Rgb::new(0xff, 0x7a, 0x45),
                Rgb::new(0x36, 0xcf, 0xc9),
                Rgb::new(0x92, 0x54, 0xde),
                Rgb::new(0x40, 0x96, 0xff),
                Rgb::new(0x73, 0xd1, 0x3d),
                Rgb::new(0xf7, 0x59, 0xab),
            ],
        }
    }

    /// The light mode.
    ///
    /// Not a mirror of the dark one: on a light surface the deep hues carry and
    /// the bright ones vanish, so the section and lane palettes are their own
    /// values, and **selection deepens** instead of lifting. Two of the mock's
    /// light tokens did not survive the contrast floors — `--playhead`
    /// (`#d48806`) read at 2.32:1 and is darkened here to `#ad6800` — which is
    /// what the tests are for.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            surface: Rgb::new(0xe7, 0xe7, 0xea),
            panel: Rgb::new(0xf3, 0xf3, 0xf5),
            text: Rgb::new(0x1f, 0x1f, 0x24),
            text_dim: Rgb::new(0x5a, 0x5a, 0x63),
            stroke: Rgb::new(0xc2, 0xc2, 0xc9),
            grid_bar: Rgb::new(0xbc, 0xbc, 0xc4),
            row_shade: Rgb::new(0xde, 0xde, 0xe3),
            accent: Rgb::new(0x16, 0x77, 0xff),
            playhead: Rgb::new(0xad, 0x68, 0x00),
            boundary: Rgb::new(0xf5, 0x22, 0x2d),
            ink_on_light: Rgb::new(0x11, 0x11, 0x14),
            ink_on_dark: Rgb::new(0xff, 0xff, 0xff),
            classes: [
                Rgb::new(0x09, 0x58, 0xd9), // Riff
                Rgb::new(0xa8, 0x07, 0x1a), // Breakdown
                Rgb::new(0x87, 0x4d, 0x00), // Solo
                Rgb::new(0x23, 0x78, 0x04), // Clean
                Rgb::new(0x59, 0x59, 0x59), // Unknown
            ],
            classes_selected: [
                Rgb::new(0x00, 0x2c, 0x8c),
                Rgb::new(0x5c, 0x00, 0x11),
                Rgb::new(0x61, 0x34, 0x00),
                Rgb::new(0x09, 0x2b, 0x00),
                Rgb::new(0x26, 0x26, 0x26),
            ],
            lanes: [
                Rgb::new(0xd4, 0x38, 0x0d),
                Rgb::new(0x00, 0x6d, 0x75),
                Rgb::new(0x53, 0x1d, 0xab),
                Rgb::new(0x1d, 0x39, 0xc4),
                Rgb::new(0x3f, 0x66, 0x00),
                Rgb::new(0xc4, 0x1d, 0x7f),
            ],
        }
    }

    /// The fill a section of `class` draws with.
    #[must_use]
    pub fn class_fill(&self, class: BarClass, selected: bool) -> Rgb {
        let table = if selected {
            &self.classes_selected
        } else {
            &self.classes
        };
        table
            .get(class_index(class))
            .copied()
            .unwrap_or(self.text_dim)
    }

    /// The ink a section's class label draws in, against [`Self::class_fill`].
    ///
    /// Picked per fill, not per class: whichever of the two inks reads on it.
    #[must_use]
    pub fn class_ink(&self, class: BarClass, selected: bool) -> Rgb {
        let fill = self.class_fill(class, selected);
        if contrast_ratio(self.ink_on_dark, fill) >= contrast_ratio(self.ink_on_light, fill) {
            self.ink_on_dark
        } else {
            self.ink_on_light
        }
    }

    /// The colour of note lane `lane`; the palette cycles.
    #[must_use]
    pub fn lane(&self, lane: u16) -> Rgb {
        let index = usize::from(lane.checked_rem(LANES).unwrap_or(0));
        self.lanes.get(index).copied().unwrap_or(self.text_dim)
    }
}

/// Where a classification sits in the palette tables.
const fn class_index(class: BarClass) -> usize {
    match class {
        BarClass::Riff => 0,
        BarClass::Breakdown => 1,
        BarClass::Solo => 2,
        BarClass::Clean => 3,
        BarClass::Unknown => 4,
    }
}

/// What a placed cell looks like — the one answer both renderers blit.
///
/// A role that carries a glyph answers with an ink. That is the whole point:
/// the band's class name is not decoration a renderer may drop.
#[must_use]
pub fn cell_style(cell: SceneCell, theme: &Theme) -> CellStyle {
    let block = |fill: Rgb| CellStyle {
        fill: Some(fill),
        ink: None,
    };
    match cell.role {
        CellRole::Empty => CellStyle {
            fill: cell.shade.then_some(theme.row_shade),
            ink: None,
        },
        CellRole::Separator => block(theme.stroke),
        CellRole::GridLine => block(theme.grid_bar),
        CellRole::SectionMark(class) => block(theme.class_fill(class, false)),
        CellRole::BoundaryMark => block(theme.boundary),
        CellRole::Note(lane) => block(theme.lane(lane)),
        CellRole::Playhead => block(theme.playhead),
        CellRole::PitchLabel => CellStyle {
            fill: None,
            ink: Some(theme.text_dim),
        },
        CellRole::BandFill { class, selected } => CellStyle {
            fill: Some(theme.class_fill(class, selected)),
            ink: Some(theme.class_ink(class, selected)),
        },
        CellRole::BandHeader => CellStyle {
            fill: Some(theme.panel),
            ink: Some(theme.text_dim),
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    /// Every bar classification.
    const CLASSES: [BarClass; 5] = [
        BarClass::Riff,
        BarClass::Breakdown,
        BarClass::Solo,
        BarClass::Clean,
        BarClass::Unknown,
    ];

    /// WCAG's floor for text.
    const TEXT_FLOOR: f64 = 4.5;
    /// WCAG's floor for a graphical object that carries meaning.
    const GRAPHIC_FLOOR: f64 = 3.0;

    /// Both modes, named for the assertion messages.
    fn modes() -> [(&'static str, Theme); 2] {
        [("dark", Theme::dark()), ("light", Theme::light())]
    }

    #[test]
    fn the_contrast_ratio_is_wcags() {
        let black = Rgb::new(0, 0, 0);
        let white = Rgb::new(255, 255, 255);
        let ratio = contrast_ratio(black, white);
        assert!(
            (ratio - 21.0).abs() < 0.01,
            "black on white is WCAG's maximum 21:1, got {ratio:.2}"
        );
        let same = contrast_ratio(white, white);
        assert!(
            (same - 1.0).abs() < 0.01,
            "a colour against itself is 1:1, got {same:.2}"
        );
        assert!(
            (contrast_ratio(black, white) - contrast_ratio(white, black)).abs() < 0.01,
            "the ratio is symmetric"
        );
    }

    #[test]
    fn text_clears_the_text_floor_in_both_modes() {
        for (name, theme) in modes() {
            for (what, ink, under) in [
                ("text", theme.text, theme.surface),
                ("text_dim", theme.text_dim, theme.surface),
                ("text_dim on the panel", theme.text_dim, theme.panel),
            ] {
                let ratio = contrast_ratio(ink, under);
                assert!(
                    ratio >= TEXT_FLOOR,
                    "{name}: {what} reads at {ratio:.2}:1, under the {TEXT_FLOOR}:1 text floor"
                );
            }
        }
    }

    #[test]
    fn the_meaningful_graphics_clear_the_graphic_floor_in_both_modes() {
        for (name, theme) in modes() {
            for (what, colour) in [
                ("the playhead", theme.playhead),
                ("a boundary mark", theme.boundary),
                ("the accent", theme.accent),
            ] {
                let ratio = contrast_ratio(colour, theme.surface);
                assert!(
                    ratio >= GRAPHIC_FLOOR,
                    "{name}: {what} sits at {ratio:.2}:1 on the surface, \
                     under the {GRAPHIC_FLOOR}:1 floor"
                );
            }
            for lane in 0..6 {
                let ratio = contrast_ratio(theme.lane(lane), theme.surface);
                assert!(
                    ratio >= GRAPHIC_FLOOR,
                    "{name}: lane {lane} sits at {ratio:.2}:1 on the surface"
                );
            }
        }
    }

    #[test]
    fn every_section_stays_visible_selected_or_not() {
        for (name, theme) in modes() {
            for class in CLASSES {
                for selected in [true, false] {
                    let ratio = contrast_ratio(theme.class_fill(class, selected), theme.surface);
                    assert!(
                        ratio >= GRAPHIC_FLOOR,
                        "{name}: {class:?} (selected={selected}) sits at {ratio:.2}:1 \
                         on the surface — the section's extent disappears"
                    );
                }
            }
        }
    }

    #[test]
    fn every_class_label_is_legible_on_its_own_fill() {
        // The label is what carries the classification: Breakdown's red and
        // Clean's green are one colour to a deuteranope (WCAG 1.4.1).
        for (name, theme) in modes() {
            for class in CLASSES {
                for selected in [true, false] {
                    let fill = theme.class_fill(class, selected);
                    let ink = theme.class_ink(class, selected);
                    let ratio = contrast_ratio(ink, fill);
                    assert!(
                        ratio >= TEXT_FLOOR,
                        "{name}: the {class:?} label (selected={selected}) reads at \
                         {ratio:.2}:1 on its fill"
                    );
                }
            }
        }
    }

    #[test]
    fn the_selected_section_is_the_loudest_thing_in_the_band() {
        // The invariant the cockpit had backwards: it dimmed the *unselected*
        // sections, which on a dark surface left the selected one quieter than
        // its neighbours — and drove the rest under the graphic floor.
        for (name, theme) in modes() {
            for class in CLASSES {
                let selected = contrast_ratio(theme.class_fill(class, true), theme.surface);
                let unselected = contrast_ratio(theme.class_fill(class, false), theme.surface);
                assert!(
                    selected > unselected,
                    "{name}: the selected {class:?} ({selected:.2}:1) does not out-contrast \
                     the unselected one ({unselected:.2}:1)"
                );
            }
        }
    }

    #[test]
    fn every_class_and_lane_keeps_its_own_colour() {
        for (name, theme) in modes() {
            for (i, a) in CLASSES.iter().enumerate() {
                for b in CLASSES.iter().skip(i + 1) {
                    assert!(
                        theme.class_fill(*a, false) != theme.class_fill(*b, false),
                        "{name}: {a:?} and {b:?} share a fill"
                    );
                }
            }
            for a in 0..6_u16 {
                for b in a.saturating_add(1)..6 {
                    assert!(
                        theme.lane(a) != theme.lane(b),
                        "{name}: lanes {a} and {b} share a colour"
                    );
                }
            }
            assert!(
                theme.lane(0) == theme.lane(6),
                "{name}: the six-lane palette wraps"
            );
        }
    }

    #[test]
    fn the_band_answers_with_an_ink_so_no_renderer_can_drop_its_label() {
        let theme = Theme::dark();
        for class in CLASSES {
            for selected in [true, false] {
                let cell = SceneCell {
                    glyph: 'R',
                    role: CellRole::BandFill { class, selected },
                    shade: false,
                };
                let style = cell_style(cell, &theme);
                assert!(
                    style.ink.is_some(),
                    "{class:?} (selected={selected}) answers with no ink — \
                     a renderer painting this draws a bare block"
                );
                assert!(
                    style.fill.is_some(),
                    "{class:?} (selected={selected}) answers with no fill"
                );
            }
        }
    }

    #[test]
    fn a_pitch_label_is_text_and_a_note_is_a_block() {
        let theme = Theme::dark();
        let label = cell_style(
            SceneCell {
                glyph: 'C',
                role: CellRole::PitchLabel,
                shade: false,
            },
            &theme,
        );
        assert!(label.fill.is_none(), "a pitch label draws no block");
        assert!(label.ink.is_some(), "a pitch label draws text");

        let note = cell_style(
            SceneCell {
                glyph: '█',
                role: CellRole::Note(0),
                shade: false,
            },
            &theme,
        );
        assert!(note.fill.is_some(), "a note fills its cell");
    }

    #[test]
    fn a_shaded_empty_cell_shades_and_a_plain_one_does_not() {
        let theme = Theme::dark();
        let shaded = cell_style(
            SceneCell {
                glyph: ' ',
                role: CellRole::Empty,
                shade: true,
            },
            &theme,
        );
        assert_eq!(shaded.fill, Some(theme.row_shade), "a black-key row shades");
        let plain = cell_style(
            SceneCell {
                glyph: ' ',
                role: CellRole::Empty,
                shade: false,
            },
            &theme,
        );
        assert!(
            plain.fill.is_none(),
            "an unshaded empty cell shows the surface"
        );
    }

    #[test]
    fn the_two_modes_are_different_palettes() {
        assert!(
            Theme::dark() != Theme::light(),
            "the modes must not be the same palette"
        );
        assert!(
            contrast_ratio(Theme::dark().surface, Theme::light().surface) > 3.0,
            "the two surfaces should be, well, dark and light"
        );
    }
}
