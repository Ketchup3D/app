use super::*;
use crate::test_ui::ContextExt as _;

fn frame(ctx: &egui::Context, app: &mut KetchupApp, time: f64) -> Rect {
    let mut selected = egui::Color32::TRANSPARENT;
    let output = ctx.run_test_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 1000.))),
            time: Some(time),
            ..Default::default()
        },
        |ui| {
            selected = Palette::for_ui(ui).selected;
            app.tool_rail(ui);
        },
    );
    fn find(shape: &egui::Shape, color: egui::Color32, found: &mut Vec<Rect>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    find(shape, color, found);
                }
            }
            egui::Shape::Rect(shape) if shape.fill == color && shape.rect.width() == 56. => {
                found.push(shape.rect);
            }
            _ => {}
        }
    }
    let mut backgrounds = Vec::new();
    for shape in output.shapes {
        find(&shape.shape, selected, &mut backgrounds);
    }
    assert_eq!(backgrounds.len(), 1, "one shared selection background");
    backgrounds[0]
}

#[test]
fn selection_slides_and_settles_without_delaying_tool_activation() {
    let ctx = egui::Context::default();
    apply(&ctx);
    ctx.style_mut_of(ctx.theme(), |s| s.animation_time = 0.14);
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let start = frame(&ctx, &mut app, 0.);
    app.set_tool(Tool::PushPull);
    let first = frame(&ctx, &mut app, 0.01);
    assert_eq!(app.tool, Tool::PushPull);
    assert_eq!(first, start);
    let middle = frame(&ctx, &mut app, 0.08);
    let end = frame(&ctx, &mut app, 0.3);
    assert!(middle.top() > start.top() && middle.top() < end.top());
    assert_eq!(frame(&ctx, &mut app, 0.6), end);
    app.set_tool(Tool::Select);
    frame(&ctx, &mut app, 0.61);
    let returning = frame(&ctx, &mut app, 0.68);
    assert!(returning.top() > start.top() && returning.top() < end.top());
    // A rapid change continues from the current position.
    app.set_tool(Tool::Move);
    assert_eq!(frame(&ctx, &mut app, 0.68), returning);
}

#[test]
fn zero_duration_selection_snaps_and_nested_tools_share_their_slot() {
    let ctx = egui::Context::default();
    apply(&ctx);
    ctx.style_mut_of(ctx.theme(), |s| s.animation_time = 0.);
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let start = frame(&ctx, &mut app, 0.);
    app.set_tool(Tool::PushPull);
    let end = frame(&ctx, &mut app, 0.01);
    assert!(end.top() > start.top());
    app.set_tool(Tool::Rectangle);
    let drawing = frame(&ctx, &mut app, 0.02);
    app.set_tool(Tool::Circle);
    assert_eq!(frame(&ctx, &mut app, 0.03), drawing);
}
