use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use ketchup_core::Tool;

// SketchUp-inspired tool colors stay consistent in the rail and flyouts,
// including when selected. The button background and label indicate selection.
const OUTLINE: Color32 = Color32::from_rgb(63, 70, 76);
const RED: Color32 = Color32::from_rgb(198, 57, 49);
const BLUE: Color32 = Color32::from_rgb(48, 111, 173);
const GOLD: Color32 = Color32::from_rgb(153, 108, 21);
const YELLOW: Color32 = Color32::from_rgb(249, 205, 74);
const PINK: Color32 = Color32::from_rgb(235, 144, 171);
const PINK_EDGE: Color32 = Color32::from_rgb(168, 65, 106);
const FACE: Color32 = Color32::from_rgb(211, 225, 238);
const METAL: Color32 = Color32::from_rgb(226, 231, 233);

pub fn tool(p: &egui::Painter, rect: Rect, tool: Tool) {
    if tool == Tool::Select && p.ctx().global_style().visuals.dark_mode {
        // A contrasting border keeps the cursor visible over light model faces too.
        select_arrow(p, rect, Stroke::new(4.5, OUTLINE));
        select_arrow(p, rect, Stroke::new(2.2, Color32::WHITE));
        return;
    }
    paint_tool(p, rect, tool, OUTLINE);
}

fn select_arrow(p: &egui::Painter, rect: Rect, stroke: Stroke) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.36;
    let xy = |x: f32, y: f32| c + egui::vec2(x * r, y * r);
    p.add(egui::Shape::closed_line(
        vec![
            xy(-0.55, -0.95),
            xy(-0.45, 0.8),
            xy(0.02, 0.34),
            xy(0.43, 1.),
            xy(0.77, 0.8),
            xy(0.34, 0.12),
            xy(0.95, 0.02),
        ],
        stroke,
    ));
}

pub fn ui_tool(p: &egui::Painter, rect: Rect, tool: Tool) {
    let outline = if p.ctx().global_style().visuals.dark_mode {
        Color32::from_rgb(203, 214, 204)
    } else {
        OUTLINE
    };
    paint_tool(p, rect, tool, outline);
}

fn paint_tool(p: &egui::Painter, rect: Rect, tool: Tool, outline: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.36;
    let xy = |x: f32, y: f32| c + egui::vec2(x * r, y * r);
    let color = match tool {
        Tool::Select => outline,
        Tool::TapeMeasure | Tool::Protractor => GOLD,
        Tool::Erase
        | Tool::PushPull
        | Tool::Scale
        | Tool::Offset
        | Tool::Paint
        | Tool::Flip
        | Tool::Dimensions => BLUE,
        Tool::OuterShell | Tool::Union | Tool::Intersect | Tool::Trim | Tool::Split => BLUE,
        _ => RED,
    };
    let stroke = Stroke::new(1.65, color);
    match tool {
        Tool::Select => select_arrow(p, rect, stroke),
        Tool::Line => {
            p.line_segment([xy(-0.75, 0.75), xy(0.55, -0.55)], Stroke::new(3.5, color));
            p.add(egui::Shape::convex_polygon(
                vec![xy(0.55, -0.55), xy(0.75, -0.95), xy(1., -0.7)],
                PINK,
                Stroke::new(1., PINK_EDGE),
            ));
            p.line_segment([xy(-0.75, 0.75), xy(-0.95, 0.95)], Stroke::new(2., GOLD));
            p.line_segment([xy(-0.9, 1.), xy(-0.25, 1.)], Stroke::new(1., outline));
        }
        Tool::Rectangle => {
            p.rect_filled(Rect::from_two_pos(xy(-1., -0.7), xy(1., 0.7)), 0, FACE);
            p.rect_stroke(
                Rect::from_two_pos(xy(-1., -0.7), xy(1., 0.7)),
                0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Tool::Circle => {
            p.circle_filled(c, r, FACE);
            p.circle_stroke(c, r, stroke);
        }
        Tool::Polygon => {
            p.add(egui::Shape::convex_polygon(
                (0..6)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 6.;
                        xy(angle.cos(), angle.sin())
                    })
                    .collect(),
                FACE,
                stroke,
            ));
        }
        Tool::Freehand => {
            p.add(egui::Shape::line(
                (0..25)
                    .map(|i| {
                        let x = i as f32 / 12. - 1.;
                        xy(x, (x * 5.).sin() * 0.65)
                    })
                    .collect(),
                stroke,
            ));
        }
        Tool::Erase => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    xy(-0.9, 0.2),
                    xy(0.15, -0.85),
                    xy(0.95, -0.05),
                    xy(-0.1, 1.),
                ],
                BLUE,
                stroke,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![
                    xy(-0.9, 0.2),
                    xy(-0.45, -0.25),
                    xy(0.35, 0.55),
                    xy(-0.1, 1.),
                ],
                FACE,
                stroke,
            ));
            p.line_segment([xy(-0.1, 1.), xy(0.95, 1.)], Stroke::new(1.2, BLUE));
        }
        Tool::PushPull => {
            p.add(egui::Shape::convex_polygon(
                vec![xy(-1., 0.5), xy(0., 0.), xy(1., 0.5), xy(0., 1.)],
                FACE,
                stroke,
            ));
            p.arrow(
                xy(0., 0.35),
                egui::vec2(0., -r * 1.35),
                Stroke::new(2., RED),
            );
        }
        Tool::Flip => {
            p.line_segment([xy(0., -1.), xy(0., 1.)], Stroke::new(1.2, outline));
            for sign in [-1., 1.] {
                let (fill, edge) = if sign < 0. { (FACE, BLUE) } else { (PINK, RED) };
                p.add(egui::Shape::convex_polygon(
                    vec![xy(sign * 0.3, -0.7), xy(sign, 0.6), xy(sign * 0.3, 0.6)],
                    fill,
                    Stroke::new(1.65, edge),
                ));
            }
        }
        Tool::OuterShell | Tool::Union | Tool::Intersect | Tool::Trim | Tool::Split => {
            p.rect_filled(Rect::from_two_pos(xy(-1., -0.8), xy(0.3, 0.5)), 0, FACE);
            p.rect_filled(
                Rect::from_two_pos(xy(-0.3, -0.2), xy(1., 1.)),
                0,
                Color32::from_rgb(249, 219, 166),
            );
            p.rect_stroke(
                Rect::from_two_pos(xy(-1., -0.8), xy(0.3, 0.5)),
                0,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                Rect::from_two_pos(xy(-0.3, -0.2), xy(1., 1.)),
                0,
                Stroke::new(1.65, GOLD),
                egui::StrokeKind::Inside,
            );
            if tool == Tool::OuterShell {
                p.add(egui::Shape::closed_line(
                    vec![
                        xy(-1., -0.8),
                        xy(0.3, -0.8),
                        xy(0.3, -0.2),
                        xy(1., -0.2),
                        xy(1., 1.),
                        xy(-0.3, 1.),
                        xy(-0.3, 0.5),
                        xy(-1., 0.5),
                    ],
                    Stroke::new(2.3, outline),
                ));
            }
            if tool == Tool::Union {
                p.line_segment([xy(0.5, -1.), xy(0.5, -0.45)], stroke);
                p.line_segment([xy(0.23, -0.72), xy(0.77, -0.72)], stroke);
            }
            if tool == Tool::Intersect {
                p.rect_filled(Rect::from_two_pos(xy(-0.3, -0.2), xy(0.3, 0.5)), 0, color);
            }
            if tool == Tool::Trim {
                p.line_segment([xy(0.3, -0.2), xy(0.3, 0.5)], Stroke::new(3., RED));
            }
            if tool == Tool::Split {
                p.line_segment([xy(-0.1, -1.), xy(-0.1, 1.1)], Stroke::new(2.5, RED));
            }
        }
        Tool::Scale => {
            p.rect_stroke(
                Rect::from_two_pos(xy(-0.9, -0.9), xy(0.9, 0.9)),
                0,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.arrow(
                xy(-0.45, 0.45),
                egui::vec2(r * 1.1, -r * 1.1),
                Stroke::new(1.65, RED),
            );
            for (x, y) in [(-0.9, -0.9), (-0.9, 0.9), (0.9, 0.9), (0.9, -0.9)] {
                p.rect_filled(
                    Rect::from_center_size(xy(x, y), egui::vec2(4., 4.)),
                    0,
                    color,
                );
            }
        }
        Tool::Offset => {
            for scale in [1., 0.45] {
                p.rect_stroke(
                    Rect::from_two_pos(xy(-scale, -scale), xy(scale, scale)),
                    0,
                    if scale == 1. {
                        stroke
                    } else {
                        Stroke::new(1.65, RED)
                    },
                    egui::StrokeKind::Inside,
                );
            }
        }
        Tool::Move => {
            for (x, y) in [(1., 0.), (-1., 0.), (0., 1.), (0., -1.)] {
                p.arrow(c, egui::vec2(x * r, y * r), stroke);
            }
        }
        Tool::Paint => {
            let bucket = |x: f32, y: f32| {
                let diagonal = std::f32::consts::FRAC_1_SQRT_2;
                xy((x - y) * diagonal - 0.18, (x + y) * diagonal - 0.04)
            };
            p.add(egui::Shape::line(
                (0..=16)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::PI / 16.;
                        bucket(0.56 * angle.cos(), -0.43 - 0.57 * angle.sin())
                    })
                    .collect(),
                Stroke::new(1.4, outline),
            ));
            p.add(egui::Shape::convex_polygon(
                vec![
                    bucket(-0.64, -0.48),
                    bucket(0.64, -0.48),
                    bucket(0.48, 0.65),
                    bucket(-0.48, 0.65),
                ],
                METAL,
                stroke,
            ));
            p.add(egui::Shape::convex_polygon(
                (0..24)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 24.;
                        bucket(0.64 * angle.cos(), -0.48 + 0.16 * angle.sin())
                    })
                    .collect(),
                BLUE,
                Stroke::new(1.1, BLUE),
            ));
            p.add(egui::Shape::line(
                vec![bucket(0.64, -0.48), xy(0.94, 0.16), xy(0.98, 0.36)],
                stroke,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![
                    xy(0.98, 0.4),
                    xy(1.12, 0.63),
                    xy(1.18, 0.79),
                    xy(1.14, 0.94),
                    xy(0.98, 1.04),
                    xy(0.82, 0.94),
                    xy(0.78, 0.79),
                    xy(0.84, 0.63),
                ],
                BLUE,
                Stroke::NONE,
            ));
        }
        Tool::TapeMeasure => {
            p.rect_filled(Rect::from_two_pos(xy(-0.9, -0.8), xy(0.5, 0.6)), 3, YELLOW);
            p.rect_stroke(
                Rect::from_two_pos(xy(-0.9, -0.8), xy(0.5, 0.6)),
                3,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.circle_filled(xy(-0.2, -0.1), r * 0.35, METAL);
            p.circle_stroke(xy(-0.2, -0.1), r * 0.35, Stroke::new(1.2, outline));
            p.add(egui::Shape::line(
                vec![xy(0.5, 0.3), xy(1., 0.3), xy(1., 0.9), xy(-0.1, 0.9)],
                Stroke::new(1.65, outline),
            ));
            for x in [0.2, 0.5, 0.8] {
                p.line_segment([xy(x, 0.9), xy(x, 0.65)], Stroke::new(1.2, outline));
            }
        }
        Tool::Protractor => {
            p.add(egui::Shape::convex_polygon(
                (0..=24)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::PI / 24.;
                        xy(angle.cos(), -angle.sin() + 0.4)
                    })
                    .collect(),
                YELLOW,
                stroke,
            ));
            for i in 0..=6 {
                let a = i as f32 * std::f32::consts::PI / 6.;
                p.line_segment(
                    [
                        xy(a.cos(), -a.sin() + 0.4),
                        xy(a.cos() * 0.72, -a.sin() * 0.72 + 0.4),
                    ],
                    stroke,
                );
            }
            p.circle_filled(xy(0., 0.4), 2., outline);
        }
        Tool::Dimensions => {
            for x in [-0.8, 0.8] {
                p.line_segment([xy(x, -0.9), xy(x, 0.9)], Stroke::new(1.2, outline));
            }
            p.line_segment([xy(-0.8, -0.3), xy(0.8, -0.3)], stroke);
            p.line_segment([xy(-1., 0.), xy(-0.6, -0.6)], stroke);
            p.line_segment([xy(0.6, 0.), xy(1., -0.6)], stroke);
            p.line_segment([xy(-0.4, 0.6), xy(0.4, 0.6)], Stroke::new(1.65, outline));
            for x in [-0.8, 0.8] {
                p.circle_filled(xy(x, -0.3), 2., RED);
            }
        }
        Tool::Orbit => {
            p.add(egui::Shape::ellipse_stroke(
                c,
                egui::vec2(r, r * 0.48),
                stroke,
            ));
            p.add(egui::Shape::ellipse_stroke(
                c,
                egui::vec2(r * 0.48, r),
                stroke,
            ));
            p.circle_filled(c, 2., RED);
            p.line_segment([xy(0.75, -0.4), xy(1.06, -0.37)], stroke);
            p.line_segment([xy(1.06, -0.37), xy(0.97, -0.75)], stroke);
        }
    }
}
pub fn logo(p: &egui::Painter, rect: Rect) {
    super::brand_mark::paint(p, rect);
}
pub fn line_icon(p: &egui::Painter, c: Pos2, kind: &str, color: Color32) {
    let s = Stroke::new(1.5, color);
    match kind {
        "minimize" => {
            p.line_segment([c + egui::vec2(-6., 0.), c + egui::vec2(6., 0.)], s);
        }
        "maximize" | "restore" => {
            let rect = Rect::from_center_size(c, egui::vec2(12., 12.));
            if kind == "restore" {
                p.add(egui::Shape::line(
                    vec![
                        rect.left_top() + egui::vec2(3., 0.),
                        rect.left_top() + egui::vec2(3., -3.),
                        rect.right_top() + egui::vec2(3., -3.),
                        rect.right_bottom() + egui::vec2(3., -3.),
                        rect.right_bottom(),
                    ],
                    s,
                ));
            }
            p.rect_stroke(rect, 0, s, egui::StrokeKind::Inside);
        }
        "close" => {
            p.line_segment([c + egui::vec2(-5., -5.), c + egui::vec2(5., 5.)], s);
            p.line_segment([c + egui::vec2(-5., 5.), c + egui::vec2(5., -5.)], s);
        }
        "undo" | "redo" => {
            let sign = if kind == "undo" { 1. } else { -1. };
            let at = |x: f32, y: f32| c + egui::vec2(x * sign, y);
            p.add(egui::Shape::line(
                vec![at(5., 5.), at(5., -1.), at(2., -4.), at(-5., -4.)],
                s,
            ));
            p.add(egui::Shape::line(
                vec![at(-1., -8.), at(-5., -4.), at(-1., 0.)],
                s,
            ));
        }
        "fit" => {
            // Frame a 3D model so this reads as zoom extents, not fullscreen.
            for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                let corner = c + egui::vec2(x * 8., y * 8.);
                p.line_segment([corner, corner - egui::vec2(x * 4., 0.)], s);
                p.line_segment([corner, corner - egui::vec2(0., y * 4.)], s);
            }
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![
                    at(0., -4.5),
                    at(4., -2.5),
                    at(4., 2.5),
                    at(0., 4.5),
                    at(-4., 2.5),
                    at(-4., -2.5),
                ],
                s,
            ));
            for point in [at(-4., -2.5), at(4., -2.5), at(0., 4.5)] {
                p.line_segment([at(0., -0.5), point], s);
            }
        }
        "sun" => {
            p.circle_stroke(c, 3.5, s);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.;
                let direction = egui::vec2(angle.cos(), angle.sin());
                p.line_segment([c + direction * 6., c + direction * 8.], s);
            }
        }
        "moon" => {
            // Separate open arcs avoid long miter spikes at the crescent's tips.
            for width in [7., 4.] {
                let points = (0..=24)
                    .map(|i| {
                        let angle =
                            std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 24.;
                        c + egui::vec2(angle.cos() * width, angle.sin() * 7.)
                    })
                    .collect();
                p.add(egui::Shape::line(points, s));
            }
        }
        "panel" => {
            p.rect_stroke(
                Rect::from_center_size(c, egui::vec2(18., 15.)),
                2,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([c + egui::vec2(3., -7.), c + egui::vec2(3., 7.)], s);
        }
        "home" => {
            p.add(egui::Shape::line(
                vec![
                    c + egui::vec2(-8., 0.),
                    c + egui::vec2(0., -7.),
                    c + egui::vec2(8., 0.),
                ],
                s,
            ));
            p.add(egui::Shape::line(
                vec![
                    c + egui::vec2(-5., -2.),
                    c + egui::vec2(-5., 7.),
                    c + egui::vec2(5., 7.),
                    c + egui::vec2(5., -2.),
                ],
                s,
            ));
        }
        _ => {}
    }
}
