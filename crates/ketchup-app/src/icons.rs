use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use ketchup_core::{Axis, Tool};

// Every icon uses the same theme palette in the rail, flyouts, and cursor.
// Selection is indicated by the button background and label.
const OUTLINE: Color32 = Color32::from_rgb(63, 70, 76);

#[derive(Clone, Copy)]
struct ToolPalette {
    outline: Color32,
    red: Color32,
    blue: Color32,
    gold: Color32,
    yellow: Color32,
    pink: Color32,
    pink_edge: Color32,
    face: Color32,
    metal: Color32,
    warm_face: Color32,
}

impl ToolPalette {
    fn for_painter(p: &egui::Painter) -> Self {
        if p.ctx().global_style().visuals.dark_mode {
            Self {
                outline: Color32::from_rgb(203, 214, 204),
                red: Color32::from_rgb(246, 142, 125),
                blue: Color32::from_rgb(131, 185, 239),
                gold: Color32::from_rgb(230, 190, 104),
                yellow: Color32::from_rgb(122, 97, 31),
                pink: Color32::from_rgb(127, 65, 86),
                pink_edge: Color32::from_rgb(244, 169, 195),
                face: Color32::from_rgb(65, 82, 95),
                metal: Color32::from_rgb(90, 106, 113),
                warm_face: Color32::from_rgb(98, 76, 44),
            }
        } else {
            Self {
                outline: OUTLINE,
                red: Color32::from_rgb(198, 57, 49),
                blue: Color32::from_rgb(48, 111, 173),
                gold: Color32::from_rgb(153, 108, 21),
                yellow: Color32::from_rgb(249, 205, 74),
                pink: Color32::from_rgb(235, 144, 171),
                pink_edge: Color32::from_rgb(168, 65, 106),
                face: Color32::from_rgb(211, 225, 238),
                metal: Color32::from_rgb(226, 231, 233),
                warm_face: Color32::from_rgb(249, 219, 166),
            }
        }
    }
}

pub fn tool(p: &egui::Painter, rect: Rect, tool: Tool) {
    if tool == Tool::Select && p.ctx().global_style().visuals.dark_mode {
        // A contrasting border keeps the cursor visible over light model faces too.
        select_arrow(p, rect, Stroke::new(4.5, OUTLINE));
        select_arrow(p, rect, Stroke::new(2.2, Color32::WHITE));
        return;
    }
    ui_tool(p, rect, tool);
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
    paint_tool(p, rect, tool, ToolPalette::for_painter(p));
}

fn paint_tool(p: &egui::Painter, rect: Rect, tool: Tool, colors: ToolPalette) {
    let ToolPalette {
        outline,
        red,
        blue,
        gold,
        yellow,
        pink,
        pink_edge,
        face,
        metal,
        warm_face,
    } = colors;
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.36;
    let xy = |x: f32, y: f32| c + egui::vec2(x * r, y * r);
    let color = match tool {
        Tool::Select => outline,
        Tool::TapeMeasure | Tool::Protractor => gold,
        Tool::Erase
        | Tool::PushPull
        | Tool::Scale
        | Tool::Offset
        | Tool::Paint
        | Tool::Flip
        | Tool::Dimensions
        | Tool::Text => blue,
        Tool::OuterShell | Tool::Union | Tool::Intersect | Tool::Trim | Tool::Split => blue,
        _ => red,
    };
    let stroke = Stroke::new(1.65, color);
    match tool {
        Tool::Select => select_arrow(p, rect, stroke),
        Tool::Line => {
            p.line_segment([xy(-0.75, 0.75), xy(0.55, -0.55)], Stroke::new(3.5, color));
            p.add(egui::Shape::convex_polygon(
                vec![xy(0.55, -0.55), xy(0.75, -0.95), xy(1., -0.7)],
                pink,
                Stroke::new(1., pink_edge),
            ));
            p.line_segment([xy(-0.75, 0.75), xy(-0.95, 0.95)], Stroke::new(2., gold));
            p.line_segment([xy(-0.9, 1.), xy(-0.25, 1.)], Stroke::new(1., outline));
        }
        Tool::Rectangle => {
            p.rect_filled(Rect::from_two_pos(xy(-1., -0.7), xy(1., 0.7)), 0, face);
            p.rect_stroke(
                Rect::from_two_pos(xy(-1., -0.7), xy(1., 0.7)),
                0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Tool::Circle => {
            p.circle_filled(c, r, face);
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
                face,
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
                blue,
                stroke,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![
                    xy(-0.9, 0.2),
                    xy(-0.45, -0.25),
                    xy(0.35, 0.55),
                    xy(-0.1, 1.),
                ],
                face,
                stroke,
            ));
            p.line_segment([xy(-0.1, 1.), xy(0.95, 1.)], Stroke::new(1.2, blue));
        }
        Tool::PushPull => {
            p.add(egui::Shape::convex_polygon(
                vec![xy(-1., 0.5), xy(0., 0.), xy(1., 0.5), xy(0., 1.)],
                face,
                stroke,
            ));
            p.arrow(
                xy(0., 0.35),
                egui::vec2(0., -r * 1.35),
                Stroke::new(2., red),
            );
        }
        Tool::Flip => {
            p.line_segment([xy(0., -1.), xy(0., 1.)], Stroke::new(1.2, outline));
            for sign in [-1., 1.] {
                let (fill, edge) = if sign < 0. { (face, blue) } else { (pink, red) };
                p.add(egui::Shape::convex_polygon(
                    vec![xy(sign * 0.3, -0.7), xy(sign, 0.6), xy(sign * 0.3, 0.6)],
                    fill,
                    Stroke::new(1.65, edge),
                ));
            }
        }
        Tool::OuterShell | Tool::Union | Tool::Intersect | Tool::Trim | Tool::Split => {
            p.rect_filled(Rect::from_two_pos(xy(-1., -0.8), xy(0.3, 0.5)), 0, face);
            p.rect_filled(Rect::from_two_pos(xy(-0.3, -0.2), xy(1., 1.)), 0, warm_face);
            p.rect_stroke(
                Rect::from_two_pos(xy(-1., -0.8), xy(0.3, 0.5)),
                0,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                Rect::from_two_pos(xy(-0.3, -0.2), xy(1., 1.)),
                0,
                Stroke::new(1.65, gold),
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
                p.line_segment([xy(0.3, -0.2), xy(0.3, 0.5)], Stroke::new(3., red));
            }
            if tool == Tool::Split {
                p.line_segment([xy(-0.1, -1.), xy(-0.1, 1.1)], Stroke::new(2.5, red));
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
                Stroke::new(1.65, red),
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
                        Stroke::new(1.65, red)
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
                metal,
                stroke,
            ));
            p.add(egui::Shape::convex_polygon(
                (0..24)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 24.;
                        bucket(0.64 * angle.cos(), -0.48 + 0.16 * angle.sin())
                    })
                    .collect(),
                blue,
                Stroke::new(1.1, blue),
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
                blue,
                Stroke::NONE,
            ));
        }
        Tool::TapeMeasure => {
            p.rect_filled(Rect::from_two_pos(xy(-0.9, -0.8), xy(0.5, 0.6)), 3, yellow);
            p.rect_stroke(
                Rect::from_two_pos(xy(-0.9, -0.8), xy(0.5, 0.6)),
                3,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.circle_filled(xy(-0.2, -0.1), r * 0.35, metal);
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
                yellow,
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
        Tool::Text => {
            p.line_segment([xy(-0.9, 0.8), xy(0., -0.1)], stroke);
            p.line_segment([xy(-0.9, 0.8), xy(-0.75, 0.25)], stroke);
            p.line_segment([xy(-0.9, 0.8), xy(-0.35, 0.65)], stroke);
            p.line_segment([xy(-0.15, -0.85), xy(0.95, -0.85)], stroke);
            p.line_segment([xy(0.4, -0.85), xy(0.4, 0.55)], stroke);
            p.line_segment([xy(0.1, 0.55), xy(0.7, 0.55)], stroke);
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
                p.circle_filled(xy(x, -0.3), 2., red);
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
            p.circle_filled(c, 2., red);
            p.line_segment([xy(0.75, -0.4), xy(1.06, -0.37)], stroke);
            p.line_segment([xy(1.06, -0.37), xy(0.97, -0.75)], stroke);
        }
    }
}
pub fn logo(p: &egui::Painter, rect: Rect) {
    super::brand_mark::paint(p, rect);
}
pub fn axis_lock(
    p: &egui::Painter,
    c: Pos2,
    axis: Axis,
    locked: bool,
    color: Color32,
    muted: Color32,
) {
    let origin = c + egui::vec2(-3., 4.);
    for (candidate, offset) in [
        (Axis::X, egui::vec2(10., 5.)),
        (Axis::Y, egui::vec2(-10., 5.)),
        (Axis::Z, egui::vec2(0., -15.)),
    ] {
        let highlighted = axis == candidate;
        let stroke = Stroke::new(
            if highlighted { 2.3 } else { 1.2 },
            if highlighted {
                color
            } else {
                muted.gamma_multiply(0.5)
            },
        );
        let tip = origin + offset;
        let direction = offset.normalized();
        let side = egui::vec2(-direction.y, direction.x);
        p.line_segment([origin, tip], stroke);
        p.add(egui::Shape::line(
            vec![
                tip - direction * 4. + side * 2.5,
                tip,
                tip - direction * 4. - side * 2.5,
            ],
            stroke,
        ));
    }
    if !locked {
        return;
    }
    // Only the active axis shows a padlock.
    let at = |x: f32, y: f32| c + egui::vec2(x, y);
    let stroke = Stroke::new(1.5, color);
    p.rect_stroke(
        Rect::from_center_size(at(8., -1.), egui::vec2(8., 7.)),
        1.5,
        stroke,
        egui::StrokeKind::Inside,
    );
    p.add(egui::Shape::line(
        vec![
            at(6., -4.5),
            at(6., -7.5),
            at(7., -9.),
            at(9., -9.),
            at(10., -7.5),
            at(10., -4.5),
        ],
        stroke,
    ));
    p.circle_filled(at(8., -1.), 0.8, color);
}

pub fn group_emphasis(p: &egui::Painter, c: Pos2, solid: Color32, wireframe: Color32) {
    // Keep the surrounding cubes hollow; only the edited object has shaded faces.
    let cube = |center: Pos2, radius: f32, filled: bool| {
        let at = |x: f32, y: f32| center + egui::vec2(x, y) * radius;
        let color = if filled { solid } else { wireframe };
        let stroke = Stroke::new(if filled { 1.4 } else { 1.1 }, color);
        if filled {
            for (face, shade) in [
                ([(0., -1.), (1., -0.5), (0., 0.), (-1., -0.5)], 0.25),
                ([(-1., -0.5), (0., 0.), (0., 1.), (-1., 0.5)], 0.5),
                ([(0., 0.), (1., -0.5), (1., 0.5), (0., 1.)], 0.8),
            ] {
                p.add(egui::Shape::convex_polygon(
                    face.map(|(x, y)| at(x, y)).to_vec(),
                    solid.gamma_multiply(shade),
                    Stroke::NONE,
                ));
            }
        }
        p.add(egui::Shape::closed_line(
            [
                (0., -1.),
                (1., -0.5),
                (1., 0.5),
                (0., 1.),
                (-1., 0.5),
                (-1., -0.5),
            ]
            .map(|(x, y)| at(x, y))
            .to_vec(),
            stroke,
        ));
        for (x, y) in [(-1., -0.5), (1., -0.5), (0., 1.)] {
            p.line_segment([center, at(x, y)], stroke);
        }
    };
    cube(c + egui::vec2(-7., -5.), 3.5, false);
    cube(c + egui::vec2(7., -5.), 3.5, false);
    cube(c + egui::vec2(0., 4.), 5.5, true);
}

pub fn face_icon(p: &egui::Painter, c: Pos2, fill: Color32, outline: Color32) {
    p.add(egui::Shape::convex_polygon(
        [(-8., 4.), (-3., -5.), (8., -5.), (3., 4.)]
            .map(|(x, y)| c + egui::vec2(x, y))
            .to_vec(),
        fill,
        Stroke::new(1.5, outline),
    ));
}

// Disclosure persistence keys vary by location; the shared editor keeps the
// same icon and palette in Settings and Materials.
pub(super) fn section_icon(id: &str) -> &str {
    match id {
        "settings/treatment/sanding" | "entity/treatment/sanding" => "time_estimates_sanding",
        "settings/treatment/finishing" | "entity/treatment/finishing" => "time_estimates_finishing",
        "settings/treatment/pre_stain" | "entity/treatment/pre_stain" => "time_estimates_pre_stain",
        "settings/treatment/stain" | "entity/treatment/stain" => "time_estimates_stain",
        "settings/treatment/varnish" | "entity/treatment/varnish" => "time_estimates_varnish",
        _ => id,
    }
}

pub fn line_icon(p: &egui::Painter, c: Pos2, kind: &str, color: Color32) {
    let kind = section_icon(kind);
    let s = Stroke::new(1.5, color);
    match kind {
        "settings" => {
            let points = (0..32)
                .map(|index| {
                    let angle = index as f32 * std::f32::consts::TAU / 32.;
                    let radius = if matches!(index % 4, 1 | 2) { 9. } else { 6.5 };
                    c + egui::vec2(angle.cos(), angle.sin()) * radius
                })
                .collect();
            p.add(egui::Shape::closed_line(points, s));
            p.circle_stroke(c, 3., s);
        }
        "settings_stock"
        | "settings/stock/wood"
        | "materials"
        | "material_none"
        | "material_partial"
        | "material_full" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            for y in [-4., 3.] {
                let rect = Rect::from_center_size(at(0., y), egui::vec2(18., 5.));
                let filled = kind == "material_full" || kind == "material_partial" && y < 0.;
                if filled {
                    p.rect_filled(rect, 1, color);
                }
                p.rect_stroke(rect, 1, s, egui::StrokeKind::Inside);
                let grain = if filled {
                    Stroke::new(1., crate::theme::Palette::for_painter(p).paper)
                } else {
                    s
                };
                p.line_segment([at(-5., y), at(4., y)], grain);
            }
        }
        "settings_colors" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.line_segment([at(-6., 6.), at(7., 6.)], s);
            p.line_segment([at(-6., 6.), at(-6., -7.)], s);
            p.line_segment([at(-6., 6.), at(4., -4.)], s);
            for point in [at(7., 6.), at(-6., -7.), at(4., -4.)] {
                p.circle_filled(point, 2.2, color);
            }
        }
        "settings_navigation" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.circle_stroke(c, 3., s);
            for (x, y) in [(0., -1.), (1., 0.), (0., 1.), (-1., 0.)] {
                let tip = at(x * 8., y * 8.);
                p.line_segment([at(x * 4., y * 4.), tip], s);
                p.add(egui::Shape::line(
                    vec![
                        tip + egui::vec2(-x * 3. - y * 2.5, -y * 3. + x * 2.5),
                        tip,
                        tip + egui::vec2(-x * 3. + y * 2.5, -y * 3. - x * 2.5),
                    ],
                    s,
                ));
            }
        }
        "settings_shortcuts" => {
            p.rect_stroke(
                Rect::from_center_size(c, egui::vec2(19., 14.)),
                2,
                s,
                egui::StrokeKind::Inside,
            );
            for y in [-3., 1.] {
                for x in [-5., 0., 5.] {
                    p.line_segment([c + egui::vec2(x - 1., y), c + egui::vec2(x + 1., y)], s);
                }
            }
            p.line_segment([c + egui::vec2(-4., 4.), c + egui::vec2(4., 4.)], s);
        }
        "settings_grid" => {
            for offset in [-7., 0., 7.] {
                p.line_segment([c + egui::vec2(offset, -7.), c + egui::vec2(offset, 7.)], s);
                p.line_segment([c + egui::vec2(-7., offset), c + egui::vec2(7., offset)], s);
            }
        }
        "search" => {
            p.circle_stroke(c + egui::vec2(-2., -2.), 5., s);
            p.line_segment([c + egui::vec2(2., 2.), c + egui::vec2(7., 7.)], s);
        }
        "magnet" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            let mut points = vec![at(-7., -3.)];
            points.extend((0..=16).map(|i| {
                let angle = std::f32::consts::PI * (1. - i as f32 / 16.);
                at(7. * angle.cos(), 7. * angle.sin())
            }));
            points.extend([at(7., -3.), at(3., -3.)]);
            points.extend((0..=16).map(|i| {
                let angle = std::f32::consts::PI * i as f32 / 16.;
                at(3. * angle.cos(), 3. * angle.sin())
            }));
            points.push(at(-3., -3.));
            p.add(egui::Shape::closed_line(points, s));
            let tip_stroke = Stroke::new(1.5, Color32::from_rgb(217, 72, 54));
            for sign in [-1., 1.] {
                p.add(egui::Shape::convex_polygon(
                    vec![
                        at(sign * 3., -3.),
                        at(sign * 3., -7.),
                        at(sign * 7., -7.),
                        at(sign * 7., -3.),
                    ],
                    tip_stroke.color,
                    tip_stroke,
                ));
            }
        }
        "eye_closed" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::line(
                vec![
                    at(-8., -2.),
                    at(-4., 1.),
                    at(0., 2.),
                    at(4., 1.),
                    at(8., -2.),
                ],
                s,
            ));
            for (x, y, tip_x, tip_y) in [
                (-6., -0.5, -8., 2.5),
                (-3., 1.25, -4., 5.),
                (0., 2., 0., 6.),
                (3., 1.25, 4., 5.),
                (6., -0.5, 8., 2.5),
            ] {
                p.line_segment([at(x, y), at(tip_x, tip_y)], s);
            }
        }
        "eye" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![
                    at(-8., 0.),
                    at(-4., -4.),
                    at(0., -5.),
                    at(4., -4.),
                    at(8., 0.),
                    at(4., 4.),
                    at(0., 5.),
                    at(-4., 4.),
                ],
                s,
            ));
            p.circle_stroke(c, 2.5, s);
        }
        "outliner" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::line(
                vec![at(-6., -6.), at(-6., 6.), at(-2., 6.)],
                s,
            ));
            p.line_segment([at(-6., 0.), at(-2., 0.)], s);
            for (x, y) in [(-3., -6.), (3., 0.), (3., 6.)] {
                p.rect_stroke(
                    Rect::from_center_size(at(x, y), egui::vec2(7., 4.)),
                    1,
                    s,
                    egui::StrokeKind::Inside,
                );
            }
        }
        "time_estimates_sanding" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![at(-7., -2.), at(3., -6.), at(8., -2.), at(-2., 3.)],
                s,
            ));
            p.add(egui::Shape::line(
                vec![
                    at(-7., -2.),
                    at(-7., 3.),
                    at(-2., 7.),
                    at(8., 2.),
                    at(8., -2.),
                ],
                s,
            ));
            p.line_segment([at(-2., 3.), at(-2., 7.)], s);
            for x in [-3., 0., 3.] {
                p.circle_filled(at(x, -1.), 0.65, color);
            }
        }
        "time_estimates_finishing" | "settings/stock/finishes" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.rect_stroke(
                Rect::from_min_max(at(-2., -8.), at(2., 0.)),
                1,
                s,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                Rect::from_min_max(at(-6., -1.), at(6., 8.)),
                1,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([at(-6., 2.), at(6., 2.)], s);
            for x in [-3., 0., 3.] {
                p.line_segment([at(x, 5.), at(x, 7.)], s);
            }
        }
        "time_estimates_pre_stain" | "time_estimates_stain" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![
                    at(0., -8.),
                    at(4.5, -1.),
                    at(5., 2.),
                    at(3., 5.),
                    at(0., 6.),
                    at(-3., 5.),
                    at(-5., 2.),
                    at(-4.5, -1.),
                ],
                s,
            ));
            if kind == "time_estimates_stain" {
                p.circle_filled(at(0., 2.), 1.7, color);
            } else {
                p.line_segment([at(-2., 2.), at(2., 2.)], s);
            }
            p.line_segment([at(-7., 8.), at(7., 8.)], s);
        }
        "time_estimates_varnish" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![
                    at(0., -8.),
                    at(7., -5.),
                    at(6., 2.),
                    at(3., 6.),
                    at(0., 8.),
                    at(-3., 6.),
                    at(-6., 2.),
                    at(-7., -5.),
                ],
                s,
            ));
            p.add(egui::Shape::line(
                vec![at(-3., 0.), at(-0.5, 2.5), at(3.5, -2.5)],
                s,
            ));
        }
        "time_cost_estimates" => {
            p.circle_stroke(c, 7., s);
            p.add(egui::Shape::line(
                vec![c + egui::vec2(0., -4.), c, c + egui::vec2(3.5, 2.)],
                s,
            ));
        }
        "entity_info" => {
            p.circle_stroke(c, 7., s);
            p.circle_filled(c + egui::vec2(0., -3.5), 1., color);
            p.line_segment([c + egui::vec2(0., -0.5), c + egui::vec2(0., 4.)], s);
        }
        "group" | "group_filled" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            if kind == "group_filled" {
                p.rect_filled(Rect::from_min_max(at(-8., -3.), at(8., 6.)), 0, color);
                p.add(egui::Shape::convex_polygon(
                    vec![at(-8., -6.), at(-3., -6.), at(-1., -3.), at(-8., -3.)],
                    color,
                    Stroke::NONE,
                ));
            } else {
                p.add(egui::Shape::closed_line(
                    vec![
                        at(-8., -6.),
                        at(-3., -6.),
                        at(-1., -3.),
                        at(8., -3.),
                        at(8., 6.),
                        at(-8., 6.),
                    ],
                    s,
                ));
            }
        }
        "components" | "component" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![
                    at(0., -8.),
                    at(7., -4.),
                    at(7., 4.),
                    at(0., 8.),
                    at(-7., 4.),
                    at(-7., -4.),
                ],
                s,
            ));
            for point in [at(-7., -4.), at(7., -4.), at(0., 8.)] {
                p.line_segment([c, point], s);
            }
        }
        "geometry" => {
            let points = [(-7., 5.), (-2., -6.), (7., 4.)].map(|(x, y)| c + egui::vec2(x, y));
            p.add(egui::Shape::convex_polygon(
                points.to_vec(),
                color.gamma_multiply(0.15),
                s,
            ));
            for point in points {
                p.circle_filled(point, 2., color);
            }
        }
        "edge" => {
            let ends = [c + egui::vec2(-6., 6.), c + egui::vec2(6., -6.)];
            p.line_segment(ends, s);
            for end in ends {
                p.circle_filled(end, 2., color);
            }
        }
        "face" => {
            face_icon(p, c, color.gamma_multiply(0.15), color);
        }
        "text" => {
            p.text(
                c,
                egui::Align2::CENTER_CENTER,
                "T",
                egui::FontId::proportional(16.),
                color,
            );
        }
        "dimension" | "units" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.line_segment([at(-6., 0.), at(6., 0.)], s);
            for x in [-6., 6.] {
                p.line_segment([at(x, -6.), at(x, 6.)], s);
                p.line_segment([at(x - 2., 2.), at(x + 2., -2.)], s);
            }
        }
        "tags" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![
                    at(-7., -7.),
                    at(0., -7.),
                    at(8., 1.),
                    at(1., 8.),
                    at(-7., 0.),
                ],
                s,
            ));
            p.circle_stroke(at(-3., -3.), 1.5, s);
        }
        "view" | "view_camera" => {
            p.rect_stroke(
                Rect::from_center_size(c, egui::vec2(13., 10.)),
                2,
                s,
                egui::StrokeKind::Inside,
            );
            p.circle_stroke(c, 3., s);
            p.line_segment([c + egui::vec2(-4., -6.), c + egui::vec2(1., -6.)], s);
        }
        "view_standard_views" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::closed_line(
                vec![
                    at(0., -8.),
                    at(7., -4.),
                    at(7., 4.),
                    at(0., 8.),
                    at(-7., 4.),
                    at(-7., -4.),
                ],
                s,
            ));
            for end in [at(-7., -4.), at(7., -4.), at(0., 8.)] {
                p.line_segment([c, end], s);
            }
        }
        "display" => {
            p.rect_stroke(
                Rect::from_center_size(c + egui::vec2(0., -2.), egui::vec2(16., 11.)),
                2,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([c + egui::vec2(0., 3.), c + egui::vec2(0., 7.)], s);
            p.line_segment([c + egui::vec2(-4., 7.), c + egui::vec2(4., 7.)], s);
        }
        "edge_style" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.add(egui::Shape::line(
                vec![at(-7., 6.), at(-7., -6.), at(7., -6.)],
                s,
            ));
            p.line_segment([at(-2., 6.), at(7., -3.)], Stroke::new(2.5, color));
            for point in [at(-7., 6.), at(-7., -6.), at(7., -6.)] {
                p.circle_filled(point, 2., color);
            }
        }
        "sections" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.rect_stroke(
                Rect::from_center_size(c, egui::vec2(12., 14.)),
                0,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([at(-9., 0.), at(9., 0.)], Stroke::new(2., color));
            for x in [-4., 0., 4.] {
                p.line_segment([at(x, 5.), at(x + 3., 2.)], Stroke::new(1., color));
            }
        }
        "fog" => {
            for (y, half_width) in [(-6., 5.), (-2., 8.), (2., 6.), (6., 8.)] {
                p.line_segment(
                    [
                        c + egui::vec2(-half_width, y),
                        c + egui::vec2(half_width, y),
                    ],
                    s,
                );
            }
        }
        "display_environment" => {
            let at = |x: f32, y: f32| c + egui::vec2(x, y);
            p.circle_stroke(at(4., -4.), 2.5, s);
            p.add(egui::Shape::line(
                vec![at(-8., 6.), at(-2., -3.), at(3., 6.)],
                s,
            ));
            p.add(egui::Shape::line(
                vec![at(1., 3.), at(5., -1.), at(9., 6.), at(-8., 6.)],
                s,
            ));
        }
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
        "origin" => {
            for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                let corner = c + egui::vec2(x * 8., y * 8.);
                p.line_segment([corner, corner - egui::vec2(x * 4., 0.)], s);
                p.line_segment([corner, corner - egui::vec2(0., y * 4.)], s);
            }
            // Three axes meet at the world origin.
            for tip in [egui::vec2(0., -5.), egui::vec2(-4., 3.), egui::vec2(4., 3.)] {
                p.line_segment([c, c + tip], s);
            }
            p.circle_filled(c, 2., color);
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
        "sun" | "shadows" => {
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
