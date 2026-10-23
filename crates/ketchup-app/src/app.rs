use crate::{icons, renderer, storage, theme::*};
use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use ketchup_core::{
    Axis, Camera, DVec2, DVec3, Document, History, Inference, Model, Snap, Tool, infer, selection,
};
use std::{collections::BTreeSet, time::Duration};

fn dv(p: Vec2) -> DVec2 {
    DVec2::new(p.x as f64, p.y as f64)
}
fn pv(p: DVec2) -> Vec2 {
    egui::vec2(p.x as f32, p.y as f32)
}
fn axis_color(a: Axis) -> Color32 {
    match a {
        Axis::X => Color32::from_rgb(192, 94, 76),
        Axis::Y => Color32::from_rgb(97, 145, 92),
        Axis::Z => Color32::from_rgb(90, 133, 176),
    }
}
fn format_length(m: f64) -> String {
    if m.abs() >= 1000. {
        format!("{:.2} km", m / 1000.)
    } else {
        format!("{m:.3} m")
    }
}
fn dot(ui: &mut egui::Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(8., 12.), Sense::hover());
    ui.painter().circle_filled(r.center(), 2.5, color);
}

enum GestureKind {
    Navigate,
    Select { additive: bool },
    Line,
    Erase { ids: BTreeSet<u64> },
}
struct Gesture {
    button: egui::PointerButton,
    start: Pos2,
    last: Pos2,
    kind: GestureKind,
}

pub struct KetchupApp {
    doc: Document,
    history: History,
    tool: Tool,
    selected: BTreeSet<u64>,
    hover: Option<u64>,
    line_start: Option<DVec3>,
    snap: Option<Snap>,
    lock: Option<Axis>,
    length_input: String,
    gesture: Option<Gesture>,
    viewport: Rect,
    inspector: bool,
    grid: bool,
    help: bool,
    notice: Option<(String, f64)>,
    dirty_since: Option<f64>,
    save_error: Option<String>,
    storage_recovery: bool,
    gpu_ready: bool,
}

impl KetchupApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply(&cc.egui_ctx);
        let (doc, error) = match storage::load() {
            Ok(doc) => (doc.unwrap_or_default(), None),
            Err(e) => (Document::default(), Some(e)),
        };
        Self::with_document(doc, error, renderer::install(cc))
    }
    fn with_document(doc: Document, error: Option<String>, gpu_ready: bool) -> Self {
        let recovery = error.is_some();
        Self {
            doc,
            history: History::default(),
            tool: Tool::Select,
            selected: BTreeSet::new(),
            hover: None,
            line_start: None,
            snap: None,
            lock: None,
            length_input: String::new(),
            gesture: None,
            viewport: Rect::NOTHING,
            inspector: true,
            grid: true,
            help: false,
            notice: error.map(|e| {
                (
                    format!("Could not restore your model: {e}. A new workspace is ready."),
                    30.,
                )
            }),
            dirty_since: None,
            save_error: None,
            storage_recovery: recovery,
            gpu_ready,
        }
    }
    fn changed(&mut self, ctx: &egui::Context) {
        self.dirty_since = Some(ctx.input(|i| i.time));
        ctx.request_repaint_after(Duration::from_millis(550));
    }
    fn say(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.notice = Some((text.into(), ctx.input(|i| i.time) + 5.));
        ctx.request_repaint_after(Duration::from_secs(5));
    }
    fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.line_start = None;
        self.snap = None;
        self.lock = None;
        self.length_input.clear();
        self.gesture = None;
    }
    fn finish_edit(&mut self, label: &str, before: Model, ctx: &egui::Context) {
        if before != self.doc.model {
            self.history.record(label, before, &self.doc.model);
            self.selected
                .retain(|id| self.doc.model.edges.contains_key(id));
            self.changed(ctx);
            #[cfg(target_arch = "wasm32")]
            self.persist();
        }
    }
    fn undo(&mut self, ctx: &egui::Context, redo: bool) {
        self.gesture = None;
        self.line_start = None;
        self.snap = None;
        self.length_input.clear();
        let did = if redo {
            self.history.redo(&mut self.doc.model)
        } else {
            self.history.undo(&mut self.doc.model)
        };
        if did {
            self.selected.clear();
            self.changed(ctx);
        }
    }
    fn erase_selected(&mut self, ctx: &egui::Context) {
        let before = self.doc.model.clone();
        self.doc.model.erase(&self.selected);
        self.finish_edit("Erase selection", before, ctx);
    }
    fn new_model(&mut self, ctx: &egui::Context) {
        let before = std::mem::take(&mut self.doc.model);
        self.finish_edit("New model", before, ctx);
        self.set_tool(Tool::Select);
        self.selected.clear();
        self.doc.camera = Camera::default();
        self.doc.name = "Untitled model".into();
        self.changed(ctx);
    }
    fn fit(&mut self, ctx: &egui::Context) {
        self.doc.camera.fit(
            self.doc.model.bounds(),
            dv(self.viewport.size().max(Vec2::splat(1.))),
        );
        self.changed(ctx);
    }
    fn commit_line(&mut self, ctx: &egui::Context, precise: bool) {
        let Some(start) = self.line_start else {
            return;
        };
        let Some(snap) = self.snap else {
            self.say(ctx, "Move over the drawing plane to set a direction");
            return;
        };
        let mut end = snap.point;
        if precise && !self.length_input.trim().is_empty() {
            match self.length_input.trim().parse::<f64>() {
                Ok(length) if length.is_finite() && length > 1e-7 && length <= 1e7 => {
                    let Some(direction) = (end - start).try_normalize() else {
                        self.say(ctx, "Move the pointer to choose a direction first");
                        return;
                    };
                    end = start + direction * length;
                }
                _ => {
                    self.say(ctx, "Enter a positive length in meters, such as 2.5");
                    return;
                }
            }
        }
        let before = self.doc.model.clone();
        match self.doc.model.add_edge(start, end) {
            Ok(_) => {
                self.finish_edit("Draw line", before, ctx);
                self.line_start = Some(end);
                self.length_input.clear();
            }
            Err(message) => self.say(ctx, message),
        }
    }
    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.gesture = None;
            self.line_start = None;
            self.snap = None;
            self.lock = None;
            self.length_input.clear();
            self.notice = None;
            self.help = false;
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        if ctx.egui_wants_keyboard_input() || self.help {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => {
                    let command = modifiers.command || modifiers.ctrl;
                    if command {
                        match key {
                            Key::Z => self.undo(ctx, modifiers.shift),
                            Key::Y => self.undo(ctx, true),
                            Key::A => {
                                self.selected = self.doc.model.edges.keys().copied().collect();
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match key {
                        Key::Space => self.set_tool(Tool::Select),
                        Key::L => self.set_tool(Tool::Line),
                        Key::E => self.set_tool(Tool::Erase),
                        Key::O => self.set_tool(Tool::Orbit),
                        Key::F => self.fit(ctx),
                        Key::Delete => self.erase_selected(ctx),
                        Key::Backspace => {
                            if self.tool == Tool::Line && self.line_start.is_some() {
                                self.length_input.pop();
                            } else {
                                self.erase_selected(ctx);
                            }
                        }
                        Key::Enter if self.tool == Tool::Line => self.commit_line(ctx, true),
                        Key::ArrowRight | Key::ArrowLeft | Key::ArrowUp
                            if self.tool == Tool::Line =>
                        {
                            let axis = match key {
                                Key::ArrowRight => Axis::X,
                                Key::ArrowLeft => Axis::Y,
                                _ => Axis::Z,
                            };
                            self.lock = if self.lock == Some(axis) {
                                None
                            } else {
                                Some(axis)
                            };
                        }
                        Key::ArrowDown => self.lock = None,
                        Key::F1 => self.help = true,
                        _ => {}
                    }
                }
                egui::Event::Text(text) if self.tool == Tool::Line && self.line_start.is_some() => {
                    if text
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == '+')
                        && self.length_input.len() + text.len() < 24
                    {
                        self.length_input.push_str(&text);
                    }
                }
                _ => {}
            }
        }
    }

    fn icon_button(
        ui: &mut egui::Ui,
        kind: &str,
        label: &str,
        enabled: bool,
        color: Color32,
    ) -> bool {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(32., 32.),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
        if response.hovered() && enabled {
            ui.painter()
                .rect_filled(rect, 5, Color32::from_white_alpha(20));
        }
        icons::line_icon(
            ui.painter(),
            rect.center(),
            kind,
            if enabled {
                color
            } else {
                color.gamma_multiply(0.3)
            },
        );
        response.on_hover_text(label).clicked() && enabled
    }
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("top_bar")
            .exact_size(64.)
            .frame(
                egui::Frame::new()
                    .fill(INK)
                    .inner_margin(egui::Margin::symmetric(18, 12)),
            )
            .show(ui, |ui| {
                ui.visuals_mut().override_text_color = Some(PAPER);
                ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(1., PAPER);
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().widgets.hovered.weak_bg_fill = Color32::from_rgb(65, 77, 68);
                ui.horizontal_centered(|ui| {
                    let (logo, _) = ui.allocate_exact_size(egui::vec2(35., 35.), Sense::hover());
                    icons::logo(ui.painter(), logo);
                    ui.add_space(4.);
                    ui.label(RichText::new("Ketchup").size(23.).strong().color(PAPER));
                    ui.add_space(18.);
                    ui.menu_button(RichText::new("File").color(PAPER), |ui| {
                        ui.visuals_mut().override_text_color = Some(INK);
                        if ui.button("New model").clicked() {
                            self.new_model(&ctx);
                            ui.close();
                        }
                        if ui.button("Save now").clicked() {
                            self.persist();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Keyboard shortcuts").clicked() {
                            self.help = true;
                            ui.close();
                        }
                    });
                    ui.add_space(8.);
                    let undo_label = self
                        .history
                        .undo_label()
                        .map(|s| format!("Undo {s} · Ctrl / ⌘ Z"))
                        .unwrap_or("Nothing to undo".into());
                    let redo_label = self
                        .history
                        .redo_label()
                        .map(|s| format!("Redo {s} · Ctrl / ⌘ Shift Z"))
                        .unwrap_or("Nothing to redo".into());
                    if Self::icon_button(
                        ui,
                        "undo",
                        &undo_label,
                        self.history.undo_label().is_some(),
                        PAPER,
                    ) {
                        self.undo(&ctx, false);
                    }
                    if Self::icon_button(
                        ui,
                        "redo",
                        &redo_label,
                        self.history.redo_label().is_some(),
                        PAPER,
                    ) {
                        self.undo(&ctx, true);
                    }
                    ui.add_space(18.);
                    ui.scope(|ui| {
                        ui.visuals_mut().extreme_bg_color = INK;
                        ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.doc.name)
                                .desired_width(180.)
                                .font(egui::TextStyle::Body)
                                .text_color(PAPER)
                                .margin(egui::vec2(8., 7.)),
                        );
                        if response.changed() {
                            self.changed(&ctx);
                        }
                        response.on_hover_text("Rename your model");
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(30., 30.), Sense::hover());
                        ui.painter()
                            .circle_filled(r.center(), 15., Color32::from_rgb(75, 91, 77));
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            "K",
                            FontId::proportional(13.),
                            PAPER,
                        );
                        ui.add_space(12.);
                        let (text, color) = if self.save_error.is_some() {
                            ("Unsaved", Color32::from_rgb(243, 177, 145))
                        } else if self.dirty_since.is_some() {
                            ("Saving…", Color32::from_rgb(190, 202, 188))
                        } else if self.storage_recovery {
                            ("New workspace", Color32::from_rgb(190, 202, 188))
                        } else {
                            ("Saved locally", Color32::from_rgb(190, 202, 188))
                        };
                        let saved = ui.label(RichText::new(text).size(12.).color(color));
                        if let Some(error) = &self.save_error {
                            saved.on_hover_text(error);
                        } else {
                            saved.on_hover_text("Your model stays on this device");
                        }
                        dot(
                            ui,
                            if self.save_error.is_some() {
                                ACCENT
                            } else {
                                Color32::from_rgb(152, 183, 142)
                            },
                        );
                    });
                });
            });
    }
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::bottom("status")
            .exact_size(42.)
            .frame(
                egui::Frame::new()
                    .fill(PAPER)
                    .inner_margin(egui::Margin::symmetric(16, 5)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    dot(ui, ACCENT);
                    let navigating = self
                        .gesture
                        .as_ref()
                        .is_some_and(|g| matches!(g.kind, GestureKind::Navigate));
                    let message = if navigating {
                        if ctx.input(|i| i.modifiers.shift) {
                            "Panning · Release Shift to orbit"
                        } else {
                            "Orbiting · Hold Shift to pan"
                        }
                    } else if self.tool == Tool::Line && self.line_start.is_some() {
                        "Click to finish · Type length + Enter · Esc to stop"
                    } else {
                        self.tool.instructions()
                    };
                    ui.label(RichText::new(message).size(12.).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("m").color(MUTED));
                        let available = self.tool == Tool::Line && self.line_start.is_some();
                        let hint = if let (Some(a), Some(b)) = (self.line_start, self.snap) {
                            format!("{:.3}", a.distance(b.point))
                        } else {
                            "0.000".into()
                        };
                        let field = ui.add_enabled(
                            available,
                            egui::TextEdit::singleline(&mut self.length_input)
                                .desired_width(106.)
                                .font(egui::TextStyle::Monospace)
                                .hint_text(hint)
                                .margin(egui::vec2(9., 6.)),
                        );
                        if field.has_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                            self.commit_line(&ctx, true);
                            field.surrender_focus();
                        }
                        ui.label(RichText::new("Length").size(12.).color(MUTED));
                    });
                });
            });
    }
    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("tools")
            .exact_size(76.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(PAPER)
                    .inner_margin(egui::Margin::symmetric(10, 18)),
            )
            .show(ui, |ui| {
                for tool in Tool::ALL {
                    let active = self.tool == tool;
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(56., 64.), Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::RadioButton,
                            true,
                            active,
                            format!("{} ({})", tool.label(), tool.shortcut()),
                        )
                    });
                    if active || response.hovered() {
                        ui.painter().rect_filled(
                            rect.shrink2(egui::vec2(0., 3.)),
                            8,
                            if active {
                                Color32::from_rgb(248, 229, 218)
                            } else {
                                Color32::from_rgb(235, 239, 229)
                            },
                        );
                    }
                    if active {
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                rect.left_center() - egui::vec2(10., 12.),
                                egui::vec2(3., 24.),
                            ),
                            2,
                            ACCENT,
                        );
                    }
                    let color = if active { ACCENT } else { INK };
                    icons::tool(
                        ui.painter(),
                        Rect::from_center_size(
                            rect.center() - egui::vec2(0., 8.),
                            egui::vec2(27., 27.),
                        ),
                        tool,
                        color,
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0., 17.),
                        Align2::CENTER_CENTER,
                        tool.label(),
                        FontId::proportional(10.),
                        color,
                    );
                    if response
                        .on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()))
                        .clicked()
                    {
                        self.set_tool(tool);
                    }
                    ui.add_space(5.);
                }
                ui.add_space(10.);
                ui.separator();
                ui.add_space(12.);
                let (rect, response) = ui.allocate_exact_size(egui::vec2(56., 42.), Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "Reset camera")
                });
                icons::line_icon(ui.painter(), rect.center(), "home", MUTED);
                if response.on_hover_text("Reset camera").clicked() {
                    self.doc.camera = Camera::default();
                    self.changed(ui.ctx());
                }
                let remaining = (ui.available_height() - 55.).max(0.);
                ui.add_space(remaining);
                let (r, response) = ui.allocate_exact_size(egui::vec2(56., 36.), Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "Keyboard shortcuts")
                });
                ui.painter()
                    .circle_stroke(r.center(), 10., Stroke::new(1., MUTED));
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "?",
                    FontId::proportional(13.),
                    MUTED,
                );
                if response
                    .on_hover_text("Shortcuts & getting started (F1)")
                    .clicked()
                {
                    self.help = true;
                }
            });
    }
    fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Model inspector").size(15.).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("×")
                    .on_hover_text("Close inspector")
                    .clicked()
                {
                    self.inspector = false;
                }
            });
        });
        ui.add_space(10.);
        ui.separator();
        ui.add_space(14.);
        if self.selected.is_empty() {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 74.), Sense::hover());
            let c = rect.center();
            let p = ui.painter();
            let s = Stroke::new(1.3, Color32::from_rgb(167, 181, 158));
            let points = [
                c + egui::vec2(0., -23.),
                c + egui::vec2(24., -10.),
                c + egui::vec2(24., 17.),
                c + egui::vec2(0., 30.),
                c + egui::vec2(-24., 17.),
                c + egui::vec2(-24., -10.),
            ];
            p.add(egui::Shape::closed_line(points.to_vec(), s));
            p.line_segment([points[1], c + egui::vec2(0., 3.)], s);
            p.line_segment([points[5], c + egui::vec2(0., 3.)], s);
            p.line_segment([points[3], c + egui::vec2(0., 3.)], s);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Nothing selected").size(14.).strong());
                ui.add_space(3.);
                ui.label(
                    RichText::new("Select an edge to see its details.")
                        .size(12.)
                        .color(MUTED),
                );
            });
            ui.add_space(26.);
        } else {
            ui.label(
                RichText::new(format!(
                    "{} {} selected",
                    self.selected.len(),
                    if self.selected.len() == 1 {
                        "edge"
                    } else {
                        "edges"
                    }
                ))
                .size(17.)
                .strong(),
            );
            ui.add_space(12.);
            let length: f64 = self
                .selected
                .iter()
                .filter_map(|id| self.doc.model.edges.get(id))
                .map(|e| self.doc.model.edge_length(e))
                .sum();
            Self::property(ui, "Total length", &format_length(length));
            if self.selected.len() == 1 {
                if let Some(edge) = self
                    .selected
                    .first()
                    .and_then(|id| self.doc.model.edges.get(id))
                {
                    let (a, b) = self.doc.model.points(edge);
                    for (label, point) in [("Start", a), ("End", b)] {
                        ui.add_space(14.);
                        ui.label(RichText::new(label).strong().size(12.));
                        ui.add_space(4.);
                        for (label, number, color) in [
                            ("X", point.x, axis_color(Axis::X)),
                            ("Y", point.y, axis_color(Axis::Y)),
                            ("Z", point.z, axis_color(Axis::Z)),
                        ] {
                            ui.horizontal(|ui| {
                                ui.colored_label(color, label);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.monospace(format!("{number:.3} m"));
                                    },
                                );
                            });
                        }
                    }
                }
            }
            ui.add_space(14.);
            if ui
                .add(
                    egui::Button::new(RichText::new("Erase selection").color(ACCENT))
                        .min_size(egui::vec2(ui.available_width(), 32.)),
                )
                .clicked()
            {
                self.erase_selected(ui.ctx());
            }
            ui.add_space(22.);
        }
        ui.separator();
        ui.add_space(16.);
        ui.label(RichText::new("A little guidance").size(13.).strong());
        ui.add_space(10.);
        for (key, title, detail) in [
            (
                "L",
                "Begin with a line",
                "Click two points to make an edge.",
            ),
            (
                "↑",
                "Give it some height",
                "Lock the blue axis to draw vertically.",
            ),
            (
                "O",
                "Find your angle",
                "Orbit to explore. Hold Shift to pan.",
            ),
        ] {
            ui.horizontal_top(|ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(233, 237, 225))
                    .corner_radius(5)
                    .inner_margin(egui::Margin::symmetric(7, 4))
                    .show(ui, |ui| {
                        ui.label(RichText::new(key).monospace().size(12.).color(GREEN));
                    });
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(12.).strong());
                    ui.add_space(1.);
                    ui.add(egui::Label::new(RichText::new(detail).size(11.).color(MUTED)).wrap());
                });
            });
            ui.add_space(12.);
        }
        ui.add_space(12.);
        ui.separator();
        ui.add_space(16.);
        ui.label(RichText::new("This model").size(13.).strong());
        ui.add_space(8.);
        Self::property(ui, "Edges", &self.doc.model.edges.len().to_string());
        Self::property(ui, "Vertices", &self.doc.model.vertices.len().to_string());
        Self::property(ui, "Units", "Meters");
        ui.add_space(20.);
        ui.horizontal(|ui| {
            for (label, axis) in [("X", Axis::X), ("Y", Axis::Y), ("Z", Axis::Z)] {
                dot(ui, axis_color(axis));
                ui.label(RichText::new(label).size(11.).color(MUTED));
                ui.add_space(7.);
            }
        });
    }
    fn property(ui: &mut egui::Ui, label: &str, value: &str) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(12.).color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(value).monospace().size(12.));
            });
        });
        ui.add_space(3.);
    }
    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        if !self.inspector {
            return;
        }
        if ui.available_width() >= 900. {
            egui::Panel::right("inspector")
                .exact_size(270.)
                .resizable(false)
                .frame(egui::Frame::new().fill(PAPER).inner_margin(18))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.inspector_contents(ui));
                });
        } else {
            let mut open = true;
            egui::Window::new("Inspector")
                .open(&mut open)
                .default_width(250.)
                .default_pos(egui::pos2(90., 120.))
                .show(ui.ctx(), |ui| self.inspector_contents(ui));
            if !open {
                self.inspector = false;
            }
        }
    }

    fn sweep_erase(&self, from: Pos2, to: Pos2) -> BTreeSet<u64> {
        let a = dv(from - self.viewport.min);
        let b = dv(to - self.viewport.min);
        let size = dv(self.viewport.size());
        self.doc
            .model
            .edges
            .values()
            .filter_map(|edge| {
                let (c, d) = self.doc.model.points(edge);
                let (c, d) = self.doc.camera.segment(c, d, size)?;
                (selection::segment_distance(a, b, c, d) < 9.).then_some(edge.id)
            })
            .collect()
    }
    fn update_pointer(&mut self, pos: Pos2) {
        let local = dv(pos - self.viewport.min);
        let size = dv(self.viewport.size());
        self.hover = selection::pick(&self.doc.model, &self.doc.camera, size, local, 8.);
        if self.tool == Tool::Line {
            self.snap = infer(
                &self.doc.model,
                &self.doc.camera,
                size,
                local,
                self.line_start,
                self.lock,
            );
        }
    }
    fn viewport_input(&mut self, ctx: &egui::Context, response: &egui::Response) {
        if self.help {
            self.gesture = None;
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let focused = ctx.input(|i| i.focused);
        if !focused {
            self.gesture = None;
            self.hover = None;
            return;
        }
        if response.hovered() {
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                self.update_pointer(pos);
            }
        }
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers,
                } if response.contains_pointer() && self.viewport.contains(pos) => {
                    if self.gesture.is_some() {
                        continue;
                    }
                    let kind = if button == egui::PointerButton::Middle
                        || (button == egui::PointerButton::Primary && self.tool == Tool::Orbit)
                    {
                        GestureKind::Navigate
                    } else if button == egui::PointerButton::Primary {
                        match self.tool {
                            Tool::Select => GestureKind::Select {
                                additive: modifiers.shift,
                            },
                            Tool::Line => GestureKind::Line,
                            Tool::Erase => GestureKind::Erase {
                                ids: self.sweep_erase(pos, pos),
                            },
                            Tool::Orbit => unreachable!(),
                        }
                    } else {
                        continue;
                    };
                    self.gesture = Some(Gesture {
                        button,
                        start: pos,
                        last: pos,
                        kind,
                    });
                    ctx.memory_mut(|m| {
                        if let Some(id) = m.focused() {
                            m.surrender_focus(id);
                        }
                    });
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some(mut gesture) = self.gesture.take() {
                        match &mut gesture.kind {
                            GestureKind::Navigate => {
                                let delta = dv(pos - gesture.last);
                                if ctx.input(|i| i.modifiers.shift) {
                                    self.doc.camera.pan(delta, self.viewport.height() as f64);
                                } else {
                                    self.doc.camera.orbit(delta);
                                }
                                self.changed(ctx);
                            }
                            GestureKind::Erase { ids } => {
                                ids.extend(self.sweep_erase(gesture.last, pos));
                            }
                            _ => {}
                        }
                        gesture.last = pos;
                        self.gesture = Some(gesture);
                    }
                    if self.viewport.contains(pos) {
                        self.update_pointer(pos);
                    } else {
                        self.hover = None;
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: false,
                    modifiers,
                } => {
                    if self.gesture.as_ref().is_some_and(|g| g.button == button) {
                        let gesture = self.gesture.take().unwrap();
                        match gesture.kind {
                            GestureKind::Navigate => {}
                            GestureKind::Select { additive } => {
                                let dragged = gesture.start.distance(pos) > 4.;
                                let hits = if dragged {
                                    selection::box_select(
                                        &self.doc.model,
                                        &self.doc.camera,
                                        dv(self.viewport.size()),
                                        dv(gesture.start - self.viewport.min),
                                        dv(pos - self.viewport.min),
                                    )
                                } else {
                                    selection::pick(
                                        &self.doc.model,
                                        &self.doc.camera,
                                        dv(self.viewport.size()),
                                        dv(pos - self.viewport.min),
                                        8.,
                                    )
                                    .into_iter()
                                    .collect()
                                };
                                if additive || modifiers.shift {
                                    for id in hits {
                                        if !self.selected.remove(&id) {
                                            self.selected.insert(id);
                                        }
                                    }
                                } else {
                                    self.selected = hits;
                                }
                            }
                            GestureKind::Line
                                if self.viewport.contains(pos)
                                    && gesture.start.distance(pos) < 5. =>
                            {
                                self.update_pointer(pos);
                                if self.line_start.is_some() {
                                    self.commit_line(ctx, !self.length_input.is_empty());
                                } else if let Some(snap) = self.snap {
                                    self.line_start = Some(snap.point);
                                }
                            }
                            GestureKind::Erase { mut ids } => {
                                ids.extend(self.sweep_erase(gesture.last, pos));
                                let before = self.doc.model.clone();
                                self.doc.model.erase(&ids);
                                self.finish_edit("Erase edges", before, ctx);
                            }
                            _ => {}
                        }
                    }
                }
                egui::Event::PointerGone => {
                    self.gesture = None;
                    self.hover = None;
                }
                _ => {}
            }
        }
        if response.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01 {
                if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                    self.doc.camera.zoom(
                        scroll as f64,
                        dv(pos - self.viewport.min),
                        dv(self.viewport.size()),
                    );
                    self.changed(ctx);
                    self.update_pointer(pos);
                }
            }
            let navigating = self
                .gesture
                .as_ref()
                .is_some_and(|g| matches!(g.kind, GestureKind::Navigate));
            ctx.set_cursor_icon(if navigating || self.tool == Tool::Orbit {
                if ctx.input(|i| i.pointer.any_down()) {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                }
            } else if self.tool == Tool::Line {
                egui::CursorIcon::Crosshair
            } else if self.tool == Tool::Erase {
                egui::CursorIcon::NotAllowed
            } else {
                egui::CursorIcon::Default
            });
        }
    }

    fn paint_scene(&self, ui: &egui::Ui, rect: Rect) {
        let size = dv(rect.size());
        let camera = &self.doc.camera;
        let p = ui.painter().with_clip_rect(rect);
        let mut mesh = renderer::Mesh::new(size);
        let spacing = 10.0_f64.powf((camera.distance / 15.).log10().floor());
        if self.grid {
            let x = (camera.target.x / spacing).round() * spacing;
            let y = (camera.target.y / spacing).round() * spacing;
            let span = spacing * 35.;
            for i in -35_i32..=35 {
                let major = i % 5 == 0;
                let color = if major {
                    Color32::from_rgb(211, 218, 202)
                } else {
                    Color32::from_rgb(225, 230, 217)
                };
                mesh.world_line(
                    camera,
                    DVec3::new(x + i as f64 * spacing, y - span, 0.),
                    DVec3::new(x + i as f64 * spacing, y + span, 0.),
                    if major { 0.8 } else { 0.55 },
                    color,
                );
                mesh.world_line(
                    camera,
                    DVec3::new(x - span, y + i as f64 * spacing, 0.),
                    DVec3::new(x + span, y + i as f64 * spacing, 0.),
                    if major { 0.8 } else { 0.55 },
                    color,
                );
            }
        }
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let v = axis.vector() * camera.distance * 80.;
            mesh.world_line(
                camera,
                -v,
                DVec3::ZERO,
                0.85,
                axis_color(axis).gamma_multiply(0.35),
            );
            mesh.world_line(
                camera,
                DVec3::ZERO,
                v,
                1.1,
                axis_color(axis).gamma_multiply(0.8),
            );
        }
        let erasing = match &self.gesture {
            Some(Gesture {
                kind: GestureKind::Erase { ids },
                ..
            }) => Some(ids),
            _ => None,
        };
        for e in self.doc.model.edges.values() {
            let selected = self.selected.contains(&e.id);
            let hover = self.hover == Some(e.id);
            let pending = erasing.is_some_and(|ids| ids.contains(&e.id));
            let color = if pending {
                Color32::from_rgb(220, 91, 76)
            } else if selected {
                ACCENT
            } else if hover && self.tool == Tool::Erase {
                ACCENT
            } else if hover {
                GREEN
            } else {
                INK
            };
            let (a, b) = self.doc.model.points(e);
            mesh.world_line(
                camera,
                a,
                b,
                if selected || hover || pending {
                    3.
                } else {
                    1.8
                },
                color,
            );
        }
        if let (Some(start), Some(snap)) = (self.line_start, self.snap) {
            let color = match snap.kind {
                Inference::Axis(a) => axis_color(a),
                _ => self.lock.map(axis_color).unwrap_or(INK),
            };
            mesh.world_line(camera, start, snap.point, 2.2, color);
        }
        mesh.paint(&p, rect);
        let screen = |point: DVec3| camera.project(point, size).map(|v| rect.min + pv(v));
        if let Some(origin) = screen(DVec3::ZERO) {
            p.circle_filled(origin, 2.5, MUTED);
        }
        for id in &self.selected {
            if let Some(e) = self.doc.model.edges.get(id) {
                let (a, b) = self.doc.model.points(e);
                for pt in [a, b] {
                    if let Some(point) = screen(pt) {
                        p.circle_filled(point, 3., PAPER);
                        p.circle_stroke(point, 3., Stroke::new(1.3, ACCENT));
                    }
                }
            }
        }
        if self.tool == Tool::Line {
            if let Some(start) = self.line_start {
                if let Some(point) = screen(start) {
                    p.circle_filled(point, 3.5, PAPER);
                    p.circle_stroke(point, 3.5, Stroke::new(1.5, INK));
                }
            }
            if let Some(snap) = self.snap {
                if let Some(point) = screen(snap.point) {
                    let color = match snap.kind {
                        Inference::Axis(a) => axis_color(a),
                        Inference::Midpoint => Color32::from_rgb(85, 150, 183),
                        _ => GREEN,
                    };
                    match snap.kind {
                        Inference::Midpoint => {
                            p.add(egui::Shape::convex_polygon(
                                vec![
                                    point + egui::vec2(0., -5.),
                                    point + egui::vec2(-5., 4.),
                                    point + egui::vec2(5., 4.),
                                ],
                                PAPER,
                                Stroke::new(1.5, color),
                            ));
                        }
                        Inference::Endpoint | Inference::Origin => {
                            p.rect_filled(
                                Rect::from_center_size(point, egui::vec2(7., 7.)),
                                1,
                                PAPER,
                            );
                            p.rect_stroke(
                                Rect::from_center_size(point, egui::vec2(7., 7.)),
                                1,
                                Stroke::new(1.5, color),
                                egui::StrokeKind::Inside,
                            );
                        }
                        _ => {
                            p.circle_stroke(point, 4., Stroke::new(1.5, color));
                        }
                    }
                    let label = if let Some(axis) = self.lock {
                        format!("{} · Locked", axis.label())
                    } else {
                        snap.kind.label().into()
                    };
                    let galley = p.layout_no_wrap(label, FontId::proportional(11.), color);
                    let at = point + egui::vec2(14., 16.);
                    p.rect_filled(
                        Rect::from_min_size(
                            at - egui::vec2(6., 4.),
                            galley.size() + egui::vec2(12., 8.),
                        ),
                        4,
                        PAPER.gamma_multiply(0.95),
                    );
                    p.galley(at, galley, color);
                    if let Some(start) = self.line_start {
                        if let Some(a) = screen(start) {
                            let at = a.lerp(point, 0.5) - egui::vec2(0., 16.);
                            p.text(
                                at,
                                Align2::CENTER_CENTER,
                                format_length(start.distance(snap.point)),
                                FontId::monospace(11.),
                                INK,
                            );
                        }
                    }
                }
            }
        }
        if let Some(Gesture {
            start,
            last,
            kind: GestureKind::Select { .. },
            ..
        }) = &self.gesture
        {
            if start.distance(*last) > 4. {
                let r = Rect::from_two_pos(*start, *last);
                let crossing = start.x > last.x;
                let color = if crossing {
                    GREEN
                } else {
                    Color32::from_rgb(89, 136, 173)
                };
                p.rect_filled(r, 0, color.gamma_multiply(0.1));
                p.rect_stroke(r, 0, Stroke::new(1., color), egui::StrokeKind::Inside);
            }
        }
        self.paint_compass(&p, rect);
        if self.doc.model.edges.is_empty() && self.line_start.is_none() {
            let at = rect.left_bottom() + egui::vec2(28., -75.);
            p.text(
                at,
                Align2::LEFT_TOP,
                "A little line. A big idea.",
                FontId::proportional(18.),
                INK,
            );
            p.text(
                at + egui::vec2(0., 28.),
                Align2::LEFT_TOP,
                "Press L, then click two points to get started.",
                FontId::proportional(12.),
                MUTED,
            );
        }
        if let Some((notice, until)) = &self.notice {
            if ui.input(|i| i.time) < *until {
                let galley = p.layout(
                    notice.clone(),
                    FontId::proportional(12.),
                    INK,
                    (rect.width() - 80.).clamp(120., 600.),
                );
                let r = Rect::from_min_size(
                    rect.left_top() + egui::vec2(20., 20.),
                    galley.size() + egui::vec2(24., 20.),
                );
                p.rect_filled(r, 7, PAPER);
                p.rect_stroke(r, 7, Stroke::new(1., BORDER), egui::StrokeKind::Inside);
                p.galley(r.min + egui::vec2(12., 10.), galley, INK);
            }
        }
    }
    fn paint_compass(&self, p: &egui::Painter, rect: Rect) {
        let center = rect.right_bottom() - egui::vec2(58., 55.);
        let camera = &self.doc.camera;
        p.circle_filled(center, 32., PAPER.gamma_multiply(0.85));
        let forward = (camera.target - camera.eye()).normalize();
        let right = forward.cross(DVec3::Z).normalize();
        let up = right.cross(forward);
        let mut axes = [Axis::X, Axis::Y, Axis::Z];
        axes.sort_by(|a, b| a.vector().dot(forward).total_cmp(&b.vector().dot(forward)));
        for axis in axes {
            let direction = axis.vector();
            let end = center
                + egui::vec2(
                    direction.dot(right) as f32 * 23.,
                    -direction.dot(up) as f32 * 23.,
                );
            p.line_segment([center, end], Stroke::new(1.6, axis_color(axis)));
            p.circle_filled(end, 7., axis_color(axis));
            p.text(
                end,
                Align2::CENTER_CENTER,
                match axis {
                    Axis::X => "X",
                    Axis::Y => "Y",
                    Axis::Z => "Z",
                },
                FontId::proportional(9.),
                PAPER,
            );
        }
    }
    fn viewport_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(CANVAS)).show(ui,|ui|{
            egui::Panel::top("view_options").exact_size(47.).frame(egui::Frame::new().fill(PAPER).inner_margin(egui::Margin::symmetric(18,7))).show(ui,|ui|{
                ui.horizontal_centered(|ui|{
                    ui.label(RichText::new("Model space").size(13.).strong());ui.add_space(10.);
                    ui.label(RichText::new("/").color(BORDER));ui.add_space(10.);
                    ui.label(RichText::new("Perspective").size(12.).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                        if Self::icon_button(ui,"panel","Toggle model inspector",true,MUTED){self.inspector=!self.inspector;}
                        if Self::icon_button(ui,"fit","Fit model to view (F)",true,MUTED){self.fit(&ctx);}
                        ui.separator();ui.checkbox(&mut self.grid,RichText::new("Grid").size(12.).color(MUTED));
                    });
                });
            });
            let rect=ui.available_rect_before_wrap();self.viewport=rect;
            let response=ui.allocate_rect(rect,Sense::click_and_drag());
            if rect.width()>1.&&rect.height()>1. {
                self.viewport_input(&ctx,&response);
                if self.gpu_ready {self.paint_scene(ui,rect);} else {ui.painter().text(rect.center(),Align2::CENTER_CENTER,"A compatible GPU renderer is required. Try updating your graphics driver.",FontId::proportional(15.),ACCENT);}
            }
        });
    }
    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.help;
        egui::Window::new("Make yourself at home").open(&mut open).collapsible(false).resizable(false).default_width(440.).anchor(Align2::CENTER_CENTER,Vec2::ZERO).show(ctx,|ui|{
            ui.add_space(8.);ui.label(RichText::new("Four tools. Room for anything.").size(20.).strong());ui.add_space(8.);
            ui.label("Build linework in three dimensions. Your model is saved automatically on this device.");ui.add_space(16.);
            egui::Grid::new("shortcuts").spacing(egui::vec2(30.,12.)).show(ui,|ui|{
                for (key,label) in [("Space","Select"),("L","Draw connected lines"),("E","Erase edges"),("O","Orbit"),("Shift + orbit drag","Pan the camera"),("Middle mouse drag","Orbit temporarily from any tool"),("Shift + middle drag","Pan temporarily from any tool"),("Scroll","Zoom toward the pointer"),("→ / ← / ↑","Lock X / Y / Z while drawing"),("↓","Clear the axis lock"),("Number + Enter","Set line length in meters"),("Escape","Cancel the unfinished operation"),("Shift + click","Toggle edge selection"),("Delete / Backspace","Erase selected edges"),("Ctrl / ⌘ Z","Undo"),("Ctrl / ⌘ Shift Z","Redo"),("F","Fit model in the viewport")] {
                    ui.label(RichText::new(key).monospace().size(12.).color(GREEN));ui.label(label);ui.end_row();
                }
            });ui.add_space(14.);ui.label(RichText::new("This version models edges. Closed outlines remain wireframes.").small().color(MUTED));
        });
        self.help = open;
    }
    fn persist(&mut self) {
        match storage::save(&self.doc) {
            Ok(()) => {
                self.dirty_since = None;
                self.save_error = None;
                self.storage_recovery = false;
            }
            Err(e) => {
                self.save_error = Some(e);
                self.dirty_since = None;
            }
        }
    }
}
impl eframe::App for KetchupApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.keyboard(&ctx);
        self.top_bar(ui);
        self.status_bar(ui);
        self.tool_rail(ui);
        self.inspector_panel(ui);
        self.viewport_panel(ui);
        if self.help {
            self.help_window(&ctx);
        }
        if let Some(since) = self.dirty_since {
            let now = ctx.input(|i| i.time);
            if now - since >= 0.5 {
                self.persist();
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(Duration::from_millis(550));
            }
        }
    }
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if self.dirty_since.is_some() {
            self.persist();
        }
    }
    fn on_exit(&mut self) {
        if self.dirty_since.is_some() {
            self.persist();
        }
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "app_tests.rs"]
mod tests;
