use super::element::BoundedElement;
use super::history::TransformElement;
use super::tools::{InputControllerTool, pen::PathEvent, shapes::ShapeEvent};
use super::util::{transform_point, transform_rect};
use super::*;
use crate::tab::input_controller::ToolPayload;
use crate::theme::palette_v2::{Mode, Theme, ThemeExt};
use egui::{Pos2, Rect, Vec2};

const DRAWING: &str = "<svg xmlns='http://www.w3.org/2000/svg'><path id='12345678-1234-1234-1234-123456789abc' d='M100 100 L200 200' stroke='black' stroke-width='2' fill='none'/></svg>";

fn viewport() -> ViewportSettings {
    ViewportSettings {
        master_transform: Transform::from_scale(2.0, 2.0).post_translate(70.0, 90.0),
        container_rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)),
        ..Default::default()
    }
}

fn assert_point(actual: Pos2, expected: Pos2) {
    assert!(actual.distance(expected) < 0.001, "{actual:?} != {expected:?}");
}

#[test]
fn viewport_conversions_and_edit_transforms_agree() {
    let view = viewport();
    let point = egui::pos2(100.0, 150.0);
    let screen = view.document_to_screen(point);
    assert_point(screen, egui::pos2(270.0, 390.0));
    assert_point(view.screen_to_document(screen), point);
    let edit = Transform::from_scale(1.5, 0.7).post_translate(25.0, -10.0);
    assert_point(
        view.document_to_screen(transform_point(point, view.screen_transform_to_document(edit))),
        transform_point(screen, edit),
    );
    assert_eq!(view.screen_vector_to_document(egui::vec2(20.0, 10.0)), egui::vec2(10.0, 5.0));
}

#[test]
fn rectangles_include_every_affine_transformed_corner() {
    let rect = Rect::from_min_max(Pos2::ZERO, egui::pos2(10.0, 20.0));
    let rotated = transform_rect(rect, Transform::from_row(0.0, 1.0, -1.0, 0.0, 5.0, 7.0));
    assert_eq!(rotated, Rect::from_min_max(egui::pos2(-15.0, 7.0), egui::pos2(5.0, 17.0)));
}

#[test]
fn viewport_restore_preserves_document_center_across_window_sizes() {
    let view = viewport();
    let state = view.snapshot();
    let mut restored = ViewportSettings {
        container_rect: Rect::from_min_size(egui::pos2(30.0, 40.0), egui::vec2(400.0, 900.0)),
        ..Default::default()
    };
    restored.restore(state);
    assert_eq!(restored.master_transform.sx, state.zoom);
    assert_point(
        restored.screen_to_document(restored.container_rect.center()),
        egui::pos2(state.center.0, state.center.1),
    );
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<CanvasFilePersistence>(&json).unwrap(), state);
}

#[test]
fn viewport_rejects_invalid_zoom() {
    let mut view = viewport();
    let before = view.master_transform;
    for scale in [0.0, -1.0, 0.01, f32::NAN, f32::INFINITY] {
        transform_canvas(&mut view, Transform::from_scale(scale, scale));
        assert_eq!(view.master_transform, before);
    }
}

#[test]
fn legacy_bounds_are_converted_once_and_fit_respects_minimum_zoom() {
    use lb_rs::model::svg::buffer::{WeakRect, WeakViewportSettings};
    let mut view = viewport();
    view.restore_legacy(WeakViewportSettings {
        master_transform: view.master_transform.into(),
        bounded_rect: Some(WeakRect { min: (270.0, 290.0), max: (470.0, 490.0) }),
        ..Default::default()
    });
    assert_eq!(
        view.bounded_rect.unwrap(),
        Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0))
    );
    view.bounded_rect = Some(Rect::from_min_max(Pos2::ZERO, egui::pos2(100_000.0, 100_000.0)));
    let fit = viewport::get_zoom_fit_transform(&view).unwrap();
    transform_canvas(&mut view, fit);
    assert!((view.master_transform.sx - viewport::MIN_ZOOM_LEVEL).abs() < 0.00001);
    assert_point(
        view.document_to_screen(egui::pos2(50_000.0, 50_000.0)),
        view.container_rect.center(),
    );
}

#[test]
fn history_is_independent_of_later_viewport_changes() {
    let mut buffer = Buffer::new(DRAWING);
    let before = buffer.serialize();
    let id = *buffer.elements.keys().next().unwrap();
    let mut view = viewport();
    let edit = view.screen_transform_to_document(Transform::from_translate(20.0, 10.0));
    buffer.elements.get_mut(&id).unwrap().transform(edit);
    let edited = buffer.serialize();
    let mut history = History::default();
    history.save(Event::Transform(vec![TransformElement { id, transform: edit }]));
    transform_canvas(&mut view, Transform::from_scale(3.0, 3.0).post_translate(-300.0, 100.0));
    history.undo(&mut buffer);
    assert_eq!(buffer.serialize(), before);
    history.redo(&mut buffer);
    assert_eq!(buffer.serialize(), edited);
}

fn frame(editor: &mut SVGEditor, ctx: &egui::Context, size: Vec2) -> bool {
    frame_with_input(editor, ctx, size, Vec::new())
}

fn frame_with_input(
    editor: &mut SVGEditor, ctx: &egui::Context, size: Vec2, events: Vec<egui::Event>,
) -> bool {
    let mut save = false;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            ctx.set_lb_theme(Theme::default(Mode::Dark));
            crate::register_font_system(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                save = editor.show(ui).request_save;
            });
        },
    );
    save
}

fn editor(
    ctx: &egui::Context, core: Lb, cfg: &WsPersistentStore, id: Uuid, read_only: bool,
) -> SVGEditor {
    SVGEditor::new(DRAWING.as_bytes(), ctx, core, id, None, cfg, read_only)
}

#[test]
fn opening_navigating_resizing_and_reopening_never_request_a_document_save() {
    let ctx = egui::Context::default();
    let core = Lb::init(lb_rs::model::core_config::Config {
        writeable_path: format!("/tmp/canvas-test-{}", Uuid::new_v4()),
        logs: false,
        stdout_logs: false,
        colored_logs: false,
        background_work: false,
        client_type: lb_rs::model::core_config::ClientType::Unknown,
    })
    .unwrap();
    let state_path = std::path::PathBuf::from(format!("/tmp/canvas-state-{}.json", Uuid::new_v4()));
    let cfg = WsPersistentStore::new(false, state_path.clone(), true);
    let id = Uuid::new_v4();
    let mut canvas = editor(&ctx, core.clone(), &cfg, id, false);
    let initial = canvas.buffer.serialize();
    for _ in 0..3 {
        assert!(!frame(&mut canvas, &ctx, egui::vec2(800.0, 600.0)));
    }
    for transform in [Transform::from_translate(120.0, -50.0), Transform::from_scale(2.0, 2.0)] {
        transform_canvas(&mut canvas.viewport_settings, transform);
        assert!(!frame(&mut canvas, &ctx, egui::vec2(800.0, 600.0)));
        assert!(!frame(&mut canvas, &ctx, egui::vec2(800.0, 600.0)));
        assert_eq!(canvas.buffer.serialize(), initial);
    }
    let remembered = canvas.viewport_settings.snapshot();
    assert!(!frame(&mut canvas, &ctx, egui::vec2(400.0, 900.0)));
    assert_point(
        canvas
            .viewport_settings
            .screen_to_document(canvas.viewport_settings.container_rect.center()),
        egui::pos2(remembered.center.0, remembered.center.1),
    );
    let mut reopened = editor(&ctx, core, &cfg, id, true);
    assert!(!frame(&mut reopened, &ctx, egui::vec2(600.0, 500.0)));
    assert_point(
        reopened
            .viewport_settings
            .screen_to_document(reopened.viewport_settings.container_rect.center()),
        egui::pos2(remembered.center.0, remembered.center.1),
    );
    assert_eq!(reopened.buffer.serialize(), initial);
    assert_eq!(cfg.data.read().unwrap().canvas_files[&id].zoom, remembered.zoom);
    canvas.flush_viewport();
    reopened.flush_viewport();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|json| serde_json::from_str::<crate::workspace::WsPresistentData>(&json).ok())
            .is_some_and(|data| {
                data.canvas_files
                    .get(&id)
                    .is_some_and(|state| state.zoom == remembered.zoom)
            })
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "viewport state was not flushed to disk");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let restarted = WsPersistentStore::new(false, state_path, true);
    assert_eq!(restarted.data.read().unwrap().canvas_files[&id].zoom, remembered.zoom);

    let before_reload = canvas.viewport_settings.master_transform;
    canvas.reload_document(&DRAWING.replace("L200 200", "L250 200"), None);
    assert_eq!(canvas.viewport_settings.master_transform, before_reload);
    assert!(!frame(&mut canvas, &ctx, egui::vec2(400.0, 900.0)));
    assert_eq!(
        canvas.buffer.elements.first().unwrap().1.bounding_box().max,
        egui::pos2(250.0, 200.0)
    );

    // Navigation remains local, but a real stroke must still request a save.
    let size = egui::vec2(800.0, 600.0);
    assert!(!frame(&mut canvas, &ctx, size));
    let pos = egui::pos2(500.0, 300.0);
    let started = frame_with_input(
        &mut canvas,
        &ctx,
        size,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    let ended = frame_with_input(
        &mut canvas,
        &ctx,
        size,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let idle = frame(&mut canvas, &ctx, size);
    assert!(started || ended || idle, "drawing must request a content save");
    assert_ne!(canvas.buffer.serialize(), initial);
}

#[test]
fn selection_resize_lasso_eraser_and_undo_use_document_space() {
    use super::tools::eraser::EraseEvent;
    use super::tools::selection::{BuildPayload, SelectionEvent, SelectionOperation};
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut buffer = Buffer::new(DRAWING);
            let before = buffer.serialize();
            let mut view = viewport();
            let mut history = History::default();
            let mut settings = CanvasSettings::default();
            let mut painter = ui.painter().clone();
            let mut tool_ctx = ToolContext {
                buffer: &mut buffer,
                viewport_settings: &mut view,
                history: &mut history,
                settings: &mut settings,
                painter: &mut painter,
            };
            let mut selection = tools::selection::Selection::default();
            selection.handle_tool_event(
                ui,
                SelectionEvent::StartLaso(BuildPayload {
                    pos: egui::pos2(250.0, 270.0),
                    modifiers: Default::default(),
                }),
                &mut tool_ctx,
            );
            selection.handle_tool_event(
                ui,
                SelectionEvent::LasoBuild(BuildPayload {
                    pos: egui::pos2(490.0, 510.0),
                    modifiers: Default::default(),
                }),
                &mut tool_ctx,
            );
            selection.handle_tool_event(ui, SelectionEvent::EndLaso, &mut tool_ctx);
            assert_eq!(selection.selected_elements.len(), 1);
            selection.handle_tool_event(
                ui,
                SelectionEvent::StartTransform(SelectionOperation::EastScale),
                &mut tool_ctx,
            );
            selection.handle_tool_event(
                ui,
                SelectionEvent::Transform(egui::pos2(670.0, 490.0)),
                &mut tool_ctx,
            );
            selection.handle_tool_event(ui, SelectionEvent::EndTransform, &mut tool_ctx);
            assert_eq!(
                tool_ctx.buffer.elements.first().unwrap().1.bounding_box(),
                Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0))
            );
            transform_canvas(tool_ctx.viewport_settings, Transform::from_scale(3.0, 3.0));
            tool_ctx.history.undo(tool_ctx.buffer);
            assert_eq!(tool_ctx.buffer.serialize(), before);
            let mut eraser = tools::eraser::Eraser::default();
            let pos = tool_ctx
                .viewport_settings
                .document_to_screen(egui::pos2(150.0, 150.0));
            eraser.handle_tool_event(ui, EraseEvent::Build(pos), &mut tool_ctx);
            eraser.handle_tool_event(ui, EraseEvent::End, &mut tool_ctx);
            assert!(tool_ctx.buffer.elements.first().unwrap().1.deleted());
            tool_ctx.history.undo(tool_ctx.buffer);
            assert_eq!(tool_ctx.buffer.serialize(), before);
        });
    });
}

#[test]
fn clipboard_placement_preserves_document_size_at_any_zoom() {
    let source = Buffer::new(DRAWING);
    for zoom in [0.5, 2.0, 6.0] {
        let view = ViewportSettings {
            master_transform: Transform::from_scale(zoom, zoom).post_translate(70.0, 90.0),
            ..viewport()
        };
        let paste_pos = egui::pos2(400.0, 300.0);
        let source_rect = viewport::calc_elements_bounds(&source).unwrap();
        let delta = view.screen_to_document(paste_pos) - source_rect.center();
        let mut target = Buffer::default();
        clip::duplicate_elements(
            source.elements.clone(),
            source.weak_path_pressures.clone(),
            &mut target,
            &mut History::default(),
            Some(Transform::from_translate(delta.x, delta.y)),
        );
        let pasted_rect = viewport::calc_elements_bounds(&target).unwrap();
        assert_point(view.document_to_screen(pasted_rect.center()), paste_pos);
        assert!((pasted_rect.width() - 100.0).abs() < 0.001);
        assert!((pasted_rect.height() - 100.0).abs() < 0.001);
        assert!(target.elements.first().unwrap().1.data_changed());
    }
}

#[test]
fn pen_and_shapes_store_document_coordinates() {
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut buffer = Buffer::default();
            let mut view = viewport();
            let mut history = History::default();
            let mut settings = CanvasSettings::default();
            let mut painter = ui.painter().clone();
            let mut tool_ctx = ToolContext {
                buffer: &mut buffer,
                viewport_settings: &mut view,
                history: &mut history,
                settings: &mut settings,
                painter: &mut painter,
            };
            let mut pen = tools::pen::Pen::new(PenSettings::default_pen());
            for pos in [egui::pos2(270.0, 290.0), egui::pos2(470.0, 490.0)] {
                pen.handle_tool_event(
                    ui,
                    PathEvent::Draw(ToolPayload { pos, force: None, id: None }),
                    &mut tool_ctx,
                );
            }
            let path = &tool_ctx.buffer.elements[&pen.current_id];
            assert_eq!(path.bounding_box().min, egui::pos2(100.0, 100.0));
            assert_eq!(path.bounding_box().max, egui::pos2(200.0, 200.0));
            let mut shapes = tools::shapes::ShapesTool::default();
            for pos in [egui::pos2(270.0, 290.0), egui::pos2(470.0, 490.0)] {
                shapes.handle_tool_event(ui, ShapeEvent::Build(pos), &mut tool_ctx);
            }
            assert_eq!(
                tool_ctx.buffer.elements.first().unwrap().1.bounding_box(),
                Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0))
            );
        });
    });
}
