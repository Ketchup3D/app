//! Browser cursors move independently of scene rendering.
use eframe::{
    egui,
    wasm_bindgen::{self, prelude::*},
};
use ketchup_core::Tool;
use serde_json::{Value, json};

#[wasm_bindgen(module = "/src/tool_cursor.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = setToolCursor)]
    fn set_tool_cursor(
        key: &str,
        shapes: &str,
        x: f32,
        y: f32,
        scale: f32,
    ) -> Result<bool, JsValue>;
}

pub fn show(ctx: &egui::Context, tool: Tool, hotspot: egui::Vec2, offset: egui::Vec2) -> bool {
    let dark = ctx.global_style().visuals.dark_mode;
    let scale = ctx.zoom_factor();
    let key = format!("{tool:?}:{dark}:{scale}");
    // Only generate geometry when the tool, theme, or browser zoom changes.
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<String, String>> = Default::default();
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let shapes = cache.entry(key.clone()).or_insert_with(|| {
            let icon_ctx = egui::Context::default();
            icon_ctx.set_global_style(ctx.global_style());
            let painter = icon_ctx.layer_painter(egui::LayerId::background());
            // A 64-point image leaves room for measurement icons' offset.
            crate::icons::tool_vectors(
                &painter,
                egui::Rect::from_center_size(
                    egui::pos2(24., 24.) - hotspot + offset,
                    egui::Vec2::splat(28.),
                ),
                tool,
            );
            let mut commands = Vec::new();
            painter.for_each_shape(|shape| encode(&shape.shape, &mut commands));
            serde_json::to_string(&commands).expect("cursor geometry")
        });
        set_tool_cursor(&key, shapes, 24., 24., scale).unwrap_or(false)
    })
}

fn color(c: egui::Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    format!("rgba({r},{g},{b},{})", a as f32 / 255.)
}
fn point(p: egui::Pos2) -> [f32; 2] {
    [p.x, p.y]
}
fn encode(shape: &egui::Shape, out: &mut Vec<Value>) {
    use egui::Shape;
    match shape {
        Shape::Vec(shapes) => {
            for shape in shapes {
                encode(shape, out);
            }
        }
        Shape::LineSegment { points, stroke } => out.push(json!([
            "path",
            points.map(point),
            false,
            "transparent",
            color(stroke.color),
            stroke.width
        ])),
        Shape::Path(path) => {
            let stroke = match path.stroke.color {
                egui::epaint::ColorMode::Solid(c) => c,
                _ => egui::Color32::TRANSPARENT,
            };
            out.push(json!([
                "path",
                path.points.iter().copied().map(point).collect::<Vec<_>>(),
                path.closed,
                color(path.fill),
                color(stroke),
                path.stroke.width
            ]));
        }
        Shape::Circle(c) => out.push(json!([
            "ellipse",
            point(c.center),
            [c.radius, c.radius],
            0.,
            color(c.fill),
            color(c.stroke.color),
            c.stroke.width
        ])),
        Shape::Ellipse(e) => out.push(json!([
            "ellipse",
            point(e.center),
            [e.radius.x, e.radius.y],
            e.angle,
            color(e.fill),
            color(e.stroke.color),
            e.stroke.width
        ])),
        Shape::Rect(r) => {
            let adjust = match r.stroke_kind {
                egui::StrokeKind::Inside => r.stroke.width / 2.,
                egui::StrokeKind::Outside => -r.stroke.width / 2.,
                egui::StrokeKind::Middle => 0.,
            };
            let rect = r.rect.shrink(adjust);
            out.push(json!([
                "rect",
                point(rect.min),
                [rect.width(), rect.height()],
                [
                    r.corner_radius.nw,
                    r.corner_radius.ne,
                    r.corner_radius.se,
                    r.corner_radius.sw
                ],
                color(r.fill),
                color(r.stroke.color),
                r.stroke.width
            ]));
        }
        _ => {}
    }
}
