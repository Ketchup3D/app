use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use ketchup_core::Tool;

pub fn tool(p: &egui::Painter, rect: Rect, tool: Tool, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.36;
    let xy = |x: f32, y: f32| c + egui::vec2(x * r, y * r);
    let stroke = Stroke::new(1.65, color);
    match tool {
        Tool::Select => {
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
        Tool::Line => {
            p.line_segment([xy(-0.75, 0.75), xy(0.55, -0.55)], Stroke::new(2.5, color));
            p.add(egui::Shape::closed_line(
                vec![
                    xy(0.55, -0.55),
                    xy(0.75, -0.95),
                    xy(1., -0.7),
                    xy(0.55, -0.55),
                ],
                stroke,
            ));
            p.line_segment([xy(-0.75, 0.75), xy(-0.95, 0.95)], stroke);
            p.line_segment([xy(-0.9, 1.), xy(-0.25, 1.)], Stroke::new(1., color));
        }
        Tool::Erase => {
            p.add(egui::Shape::closed_line(
                vec![
                    xy(-0.9, 0.2),
                    xy(0.15, -0.85),
                    xy(0.95, -0.05),
                    xy(-0.1, 1.),
                    xy(-0.9, 0.2),
                ],
                stroke,
            ));
            p.line_segment([xy(-0.45, -0.25), xy(0.35, 0.55)], stroke);
            p.line_segment([xy(-0.1, 1.), xy(0.95, 1.)], stroke);
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
            p.circle_filled(c, 2., color);
            p.line_segment([xy(0.75, -0.4), xy(1.06, -0.37)], stroke);
            p.line_segment([xy(1.06, -0.37), xy(0.97, -0.75)], stroke);
        }
    }
}
pub fn logo(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect, 8, super::theme::ACCENT);
    let start = rect.min + egui::vec2(10., 8.);
    let stroke = Stroke::new(2.5, Color32::WHITE);
    p.line_segment([start, start + egui::vec2(0., 18.)], stroke);
    p.line_segment(
        [start + egui::vec2(13., 0.), start + egui::vec2(0., 11.)],
        stroke,
    );
    p.line_segment(
        [start + egui::vec2(1., 10.), start + egui::vec2(14., 18.)],
        stroke,
    );
}
pub fn line_icon(p: &egui::Painter, c: Pos2, kind: &str, color: Color32) {
    let s = Stroke::new(1.5, color);
    match kind {
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
            for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                let corner = c + egui::vec2(x * 7., y * 7.);
                p.line_segment([corner, corner - egui::vec2(x * 5., 0.)], s);
                p.line_segment([corner, corner - egui::vec2(0., y * 5.)], s);
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
