use egui::os::OperatingSystem;
use egui::vec2;
use lb_rs::model::text::offset_types::Grapheme;

use super::super::TouchTarget;
use super::super::input::{Event, Location, Region};
use super::harness::TestEditor;

fn android_editor(text: &str) -> TestEditor {
    let mut test = TestEditor::new(text);
    test.editor
        .edit
        .renderer
        .ctx
        .set_os(OperatingSystem::Android);
    test.editor.edit.renderer.touch_mode = true;
    test.editor.edit.phone_mode = true;
    test.enter_frame_at(vec2(360.0, 640.0));
    test.enter_frame_at(vec2(360.0, 640.0));
    test
}

#[test]
fn android_style_reflows_an_open_document() {
    let mut test = android_editor("Some text in a paragraph that wraps on a phone.\n");
    let before = test.editor.edit.cursor_line(Grapheme(0)).unwrap();
    assert!((before[0].x - test.editor.painted_canvas.left() - 32.0).abs() < 1.0);
    test.editor.persistence.set_android_editor_style(32.0, 80.0);
    test.enter_frame_at(vec2(360.0, 640.0));
    test.enter_frame_at(vec2(360.0, 640.0));
    let after = test.editor.edit.cursor_line(Grapheme(0)).unwrap();
    assert!(after[1].y - after[0].y > before[1].y - before[0].y);
    assert!((after[0].x - test.editor.painted_canvas.left() - 80.0).abs() < 1.0);

    test.editor.persistence.set_android_editor_style(16.0, 15.0);
    test.enter_frame_at(vec2(360.0, 640.0));
    test.enter_frame_at(vec2(360.0, 640.0));
    let restored = test.editor.edit.cursor_line(Grapheme(0)).unwrap();
    assert_eq!(test.editor.edit.renderer.layout.margin, 32.0);
    assert_eq!(test.editor.edit.renderer.layout.right_margin, 15.0);
    assert!((restored[0].x - test.editor.painted_canvas.left() - 32.0).abs() < 1.0);
    assert!((restored[1].y - restored[0].y - (before[1].y - before[0].y)).abs() < 1.0);
}

#[test]
fn android_line_spacing_updates_without_changing_font_size() {
    let mut test = android_editor("First line\nSecond line\n");
    let before = test.editor.edit.cursor_line(Grapheme(0)).unwrap();
    test.editor.persistence.set_android_line_spacing(18.0);
    test.enter_frame_at(vec2(360.0, 640.0));
    test.enter_frame_at(vec2(360.0, 640.0));
    let after = test.editor.edit.cursor_line(Grapheme(0)).unwrap();
    assert_eq!(test.editor.edit.renderer.layout.row_height, 16.0);
    assert!(((after[1].y - after[0].y) - (before[1].y - before[0].y) - 12.0).abs() < 1.0);

    test.editor.persistence.set_android_line_spacing(0.0);
    test.enter_frame_at(vec2(360.0, 640.0));
    assert_eq!(test.editor.edit.renderer.layout.row_spacing, 0.0);
}

#[test]
fn android_minimum_margin_fits_heading_and_list_fold_targets() {
    for text in ["# Heading\n\nContents\n", "- Parent\n  - Child\n"] {
        let mut test = android_editor(text);
        let end = Grapheme(text.chars().count());
        test.push(Event::Select {
            region: Region::BetweenLocations {
                start: Location::Grapheme(end),
                end: Location::Grapheme(end),
            },
        });
        for font_size in [12.0, 16.0, 32.0] {
            // Too-small values from outside the settings UI are clamped too.
            test.editor
                .persistence
                .set_android_editor_style(font_size, 0.0);
            test.enter_frame_at(vec2(360.0, 640.0));
            test.enter_frame_at(vec2(360.0, 640.0));
            let folds: Vec<_> = test
                .editor
                .edit
                .renderer
                .touch_targets
                .iter()
                .filter_map(|(rect, target)| match target {
                    TouchTarget::Tap(Event::ToggleFold { .. }) => Some(*rect),
                    _ => None,
                })
                .collect();
            assert!(!folds.is_empty(), "missing fold controls for {text:?}");
            for rect in folds {
                assert!(rect.left() >= 0.0, "fold target clipped at size {font_size}: {rect:?}");
                assert!(rect.right() <= 360.0);
            }
        }
    }
}

#[test]
fn android_font_weight_reshapes_cached_body_text() {
    use super::super::widget::utils::wrap_layout::FragmentContent;
    fn font_id(test: &TestEditor) -> glyphon::fontdb::ID {
        test.editor
            .edit
            .renderer
            .fragments
            .iter()
            .find_map(|fragment| {
                let FragmentContent::Glyphs { buffer, .. } = &fragment.content else { return None };
                buffer
                    .read()
                    .unwrap()
                    .layout_runs()
                    .find_map(|run| run.glyphs.first().map(|glyph| glyph.font_id))
            })
            .unwrap()
    }
    let mut test = android_editor("Weight preview");
    let regular = font_id(&test);
    test.editor.persistence.set_android_font_weight(700);
    test.enter_frame_at(vec2(360.0, 640.0));
    test.enter_frame_at(vec2(360.0, 640.0));
    assert_ne!(font_id(&test), regular, "body text should shape in the bold face");
    test.editor.persistence.set_android_font_weight(400);
    test.enter_frame_at(vec2(360.0, 640.0));
    assert_eq!(font_id(&test), regular);
}
