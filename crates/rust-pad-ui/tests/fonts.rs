//! Font registration smoke tests.
//!
//! `include_bytes!` + `FontData::from_static` cannot fail at registration
//! time; invalid bytes or a missing named family only blow up at first
//! layout. These tests force that layout headlessly for every family the
//! chrome relies on, so a font packaging mistake fails CI instead of
//! panicking at runtime.

mod common;

use egui::{Color32, FontFamily, FontId};
use rust_pad_ui::{FONT_FAMILY_MEDIUM, FONT_FAMILY_SEMIBOLD};

fn layout_width(ctx: &egui::Context, family: FontFamily, text: &str) -> f32 {
    ctx.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), FontId::new(13.0, family), Color32::WHITE)
            .size()
            .x
    })
}

#[test]
fn all_ui_font_families_resolve_and_lay_out_text() {
    let mut harness = common::create_harness();
    harness.run();

    let ctx = harness.ctx.clone();
    for family in [
        FontFamily::Proportional,
        FontFamily::Monospace,
        FontFamily::Name(FONT_FAMILY_MEDIUM.into()),
        FontFamily::Name(FONT_FAMILY_SEMIBOLD.into()),
    ] {
        let width = layout_width(&ctx, family.clone(), "Sample 123");
        assert!(width > 0.0, "family {family:?} produced an empty layout");
    }
}

#[test]
fn phosphor_icons_render_in_weight_families() {
    let mut harness = common::create_harness();
    harness.run();

    let ctx = harness.ctx.clone();
    for family in [
        FontFamily::Proportional,
        FontFamily::Name(FONT_FAMILY_MEDIUM.into()),
        FontFamily::Name(FONT_FAMILY_SEMIBOLD.into()),
    ] {
        let width = layout_width(&ctx, family.clone(), egui_phosphor::regular::PUSH_PIN);
        assert!(width > 0.0, "phosphor glyph missing from {family:?}");
    }
}

/// Every icon constant must be an actual glyph in the merged proportional
/// font. A layout-width check is not enough: missing glyphs lay out as the
/// replacement character with positive advance. Raw dingbats like "✕" are
/// worse still: they resolve only through egui's low-fidelity emoji
/// fallback fonts (the boxy close-button glyph bug), which is why every
/// icon must go through the phosphor constants in `icons::ALL`.
#[test]
fn every_icon_constant_has_a_real_glyph() {
    let mut harness = common::create_harness();
    harness.run();

    let ctx = harness.ctx.clone();
    let font_id = FontId::new(13.0, FontFamily::Proportional);
    ctx.fonts_mut(|fonts| {
        for (value, name) in rust_pad_ui::icons::ALL {
            assert!(
                fonts.has_glyphs(&font_id, value),
                "icon constant {name} has no glyph in the merged proportional font"
            );
        }
        // Negative control: an unassigned Unicode codepoint proves the
        // positive assertions above can actually fail.
        assert!(
            !fonts.has_glyphs(&font_id, "\u{0378}"),
            "unassigned codepoint unexpectedly resolved; the glyph check has no teeth"
        );
    });
}

#[test]
fn monospace_and_proportional_use_distinct_metrics() {
    let mut harness = common::create_harness();
    harness.run();

    let ctx = harness.ctx.clone();
    let proportional = layout_width(&ctx, FontFamily::Proportional, "illustration");
    let monospace = layout_width(&ctx, FontFamily::Monospace, "illustration");
    assert!(
        (proportional - monospace).abs() > 0.5,
        "proportional and monospace resolved to the same font"
    );
}
