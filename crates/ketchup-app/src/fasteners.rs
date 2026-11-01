use super::*;
use crate::inspector_table::inspector_field;
use ketchup_core::connections::{
    Connection, ConnectionOperation, FastenerKind, FastenerSettings, Pattern,
};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub settings: FastenerSettings,
    pub reverse: bool,
    pub open: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            reverse: false,
            open: false,
        }
    }
}
impl Preferences {
    pub fn sanitize(&mut self) {
        if !self.settings.valid() {
            self.settings = Default::default();
        }
    }
}
#[derive(Default)]
pub(super) struct Dialog {
    pub members: Option<[u64; 2]>,
    pub entry_face: Option<u64>,
    pub picking: Option<PickTarget>,
}
#[derive(Clone, Copy)]
pub(super) enum PickTarget {
    Draft,
    Connection(u64),
}

fn fields(ui: &mut egui::Ui, settings: &mut FastenerSettings) {
    inspector_field(ui, "fastener_kind", "Fastener", |ui| {
        egui::ComboBox::from_id_salt("kind")
            .selected_text(settings.kind.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut settings.kind, FastenerKind::Screw, "Screw");
                ui.selectable_value(&mut settings.kind, FastenerKind::Nail, "Nail");
                ui.selectable_value(
                    &mut settings.kind,
                    FastenerKind::InclinedScrew,
                    "Angled screw",
                );
            })
            .response
    });
    inspector_field(ui, "fastener_pattern", "Pattern", |ui| {
        egui::ComboBox::from_id_salt("pattern")
            .selected_text(match settings.pattern {
                Pattern::Single => "Single",
                Pattern::Row => "Row",
                Pattern::DoubleRow => "Two rows",
                Pattern::Grid => "Grid",
            })
            .show_ui(ui, |ui| {
                for (value, label) in [
                    (Pattern::Single, "Single"),
                    (Pattern::Row, "Row"),
                    (Pattern::DoubleRow, "Two rows"),
                    (Pattern::Grid, "Grid"),
                ] {
                    ui.selectable_value(&mut settings.pattern, value, label);
                }
            })
            .response
    });
    if settings.pattern != Pattern::Single {
        inspector_field(ui, "fastener_count", "Count per row", |ui| {
            ui.add(egui::DragValue::new(&mut settings.count).range(1..=100))
        });
        millimetres(
            ui,
            "spacing",
            "Spacing (0 = auto)",
            &mut settings.spacing,
            0.,
            10000.,
        );
    }
    if matches!(settings.pattern, Pattern::Grid | Pattern::DoubleRow) {
        if settings.pattern == Pattern::Grid {
            inspector_field(ui, "rows", "Rows", |ui| {
                ui.add(egui::DragValue::new(&mut settings.rows).range(1..=100))
            });
        }
        millimetres(
            ui,
            "row_spacing",
            "Row spacing (0 = auto)",
            &mut settings.row_spacing,
            0.,
            10000.,
        );
    }
    millimetres(
        ui,
        "diameter",
        "Preview diameter",
        &mut settings.diameter,
        0.1,
        100.,
    );
    millimetres(
        ui,
        "length",
        "Length (0 = geometry)",
        &mut settings.length,
        0.,
        10000.,
    );
    millimetres(
        ui,
        "clearance",
        "Geometric clearance",
        &mut settings.clearance,
        0.,
        1000.,
    );
    if settings.kind == FastenerKind::InclinedScrew {
        inspector_field(ui, "inclination", "Angle from face normal", |ui| {
            ui.add(
                egui::DragValue::new(&mut settings.angle_degrees)
                    .range(0. ..=60.)
                    .suffix("°"),
            )
        });
        ui.text_weak(
            "Angled axis only; the pocket recess and Kreg jig settings are not simulated.",
        );
    }
}
fn millimetres(ui: &mut egui::Ui, id: &str, label: &str, value: &mut f64, min: f64, max: f64) {
    let mut mm = *value * 1000.;
    if inspector_field(ui, id, label, |ui| {
        ui.add(
            egui::DragValue::new(&mut mm)
                .speed(0.1)
                .range(min..=max)
                .suffix(" mm"),
        )
    })
    .changed()
    {
        *value = mm / 1000.;
    }
}
impl KetchupApp {
    pub(super) fn fastener_contents(&mut self, ui: &mut egui::Ui) {
        let Some(members) = self.fastener_dialog.members else {
            ui.text_weak(
                "Select exactly two objects and reopen Automatic Fasteners from the object menu.",
            );
            return;
        };
        let eligible = self.fastener_pair_eligible(ui.ctx(), members);
        if !eligible {
            ui.colored_label(Color32::LIGHT_RED,"The selected pair is no longer editable. Check object selection, editing context, locks and shared editing approval.");
        }
        let mut objects = members.to_vec();
        ui.checkbox(
            &mut self.doc.display.fasteners,
            "Show fasteners through the model",
        );

        ui.checkbox(
            &mut self.doc.display.fastener_preview,
            "Preview before creating",
        );
        if ui
            .checkbox(&mut self.fasteners.reverse, "Swap the selected parts")
            .changed()
        {
            self.fastener_dialog.entry_face = None;
        }
        if ui
            .add_enabled(eligible, egui::Button::new("Pick entry face in viewport…"))
            .clicked()
        {
            self.fastener_dialog.picking = Some(PickTarget::Draft);
            self.doc.display.fastener_face_handles = true;
        }
        if ui.button("Automatic entry face").clicked() {
            self.fastener_dialog.entry_face = None;
        }
        if let Some(face) = self.fastener_dialog.entry_face {
            ui.text_label(format!("Entry face #{face}"));
        }
        ui.text_weak("Select exactly two touching, closed solid parts. The head enters the first part. Transparent lines show the shaft and tip through the model.");
        ui.push_id("draft", |ui| fields(ui, &mut self.fasteners.settings));
        if self.fasteners.reverse {
            objects.reverse();
        }
        if objects.len() == 2 {
            let connection = Connection {
                members: [objects[0], objects[1]],
                entry_face: self.fastener_dialog.entry_face,
                operation: ConnectionOperation::Fasteners(self.fasteners.settings.clone()),
            };
            for (index, id) in connection.members.iter().enumerate() {
                let Some(object) = self.doc.model.objects.get(id) else {
                    continue;
                };
                let material = self
                    .doc
                    .model
                    .object_stock_material(object)
                    .map(|m| m.name.as_str())
                    .unwrap_or("No material assigned");
                ui.text_label(format!("Part {}: {} · {material}", index + 1, object.name));
            }
            let evaluation = connection.evaluate(&self.doc.model);
            for error in &evaluation.errors {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            for warning in evaluation
                .placements
                .iter()
                .flat_map(|p| &p.warnings)
                .collect::<BTreeSet<_>>()
            {
                ui.colored_label(Color32::YELLOW, warning);
            }
            ui.text_label(format!(
                "Preview: {} fasteners",
                evaluation.placements.len()
            ));
            if ui
                .add_enabled(
                    evaluation.errors.is_empty() && !evaluation.placements.is_empty() && eligible,
                    egui::Button::new("Create fasteners"),
                )
                .clicked()
            {
                self.create_fasteners(ui.ctx(), connection);
            }
        }
        ui.separator();
        ui.text_weak("Geometric sizing: automatic length passes through the first part and halfway into the available depth of the second. The initial diameter is an editable example. Material suitability, pilot drilling, thread type and strength are not specified; consult the product manufacturer.");
        ui.hyperlink_to(
            "Kreg screw selector (thicknesses and materials)",
            "https://www.kregtool.com/screw-selector/",
        );
        ui.hyperlink_to(
            "Simpson Strong-Tie technical guides",
            "https://www.strongtie.com/resources/literature/fastening-systems-technical-supplement",
        );
        ui.text_weak("For cut joints, close this dialog and choose Wood Joinery from the object menu. Fasteners do not modify geometry.");
        let connections: Vec<_> = self
            .doc
            .model
            .connections
            .iter()
            .filter(|(_, c)| same_pair(c.members, members) && matches!(c.operation, ConnectionOperation::Fasteners(_)))
            .map(|(id, c)| (*id, c.clone()))
            .collect();
        for (id, mut connection) in connections {
            ui.separator();
            ui.push_id(id, |ui| {
                let names = connection.members.map(|id| {
                    self.doc
                        .model
                        .objects
                        .get(&id)
                        .map(|o| o.name.clone())
                        .unwrap_or_else(|| format!("Missing part #{id}"))
                });
                ui.text_label(format!("#{id}: {} → {}", names[0], names[1]));
                let original = connection.clone();
                let locked = !self.fastener_pair_eligible(ui.ctx(), connection.members);
                let mut remove = false;
                ui.add_enabled_ui(!locked, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Reverse direction").clicked() {
                            connection.members.swap(0, 1);
                            connection.entry_face = None;
                        }
                        if ui.button("Auto face").clicked() {
                            connection.entry_face = None;
                        }
                        remove = ui.button("Delete").clicked();
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Pick entry face in viewport…").clicked() {
                            self.fastener_dialog.picking = Some(PickTarget::Connection(id));
                            self.doc.display.fastener_face_handles = true;
                        }
                    });
                    let faces = connection.entry_faces(&self.doc.model);
                    egui::ComboBox::from_id_salt("entry")
                        .selected_text(
                            connection
                                .entry_face
                                .map(|id| format!("Face #{id}"))
                                .unwrap_or_else(|| "Automatic face".into()),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut connection.entry_face, None, "Automatic");
                            for id in faces {
                                ui.selectable_value(
                                    &mut connection.entry_face,
                                    Some(id),
                                    format!("Face #{id}"),
                                );
                            }
                        });
                    let ConnectionOperation::Fasteners(settings) = &mut connection.operation else { return; };
                    fields(ui, settings);
                });
                let evaluation = connection.evaluate(&self.doc.model);
                for error in &evaluation.errors {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                for warning in evaluation
                    .placements
                    .iter()
                    .flat_map(|p| &p.warnings)
                    .collect::<BTreeSet<_>>()
                {
                    ui.colored_label(Color32::YELLOW, warning);
                }
                let ConnectionOperation::Fasteners(settings) = &connection.operation else { return; };
                ui.text_label(format!(
                    "Hardware: {} × {}, Ø {:.1} mm",
                    evaluation.placements.len(),
                    settings.kind.label(),
                    settings.diameter * 1000.
                ));
                for (index, p) in evaluation.placements.iter().enumerate().take(8) {
                    ui.text_weak(format!(
                        "{}: length {:.1} · first part {:.1} · penetration {:.1} mm",
                        index + 1,
                        p.head.distance(p.tip) * 1000.,
                        p.first_depth * 1000.,
                        p.penetration * 1000.
                    ));
                }
                if evaluation.placements.len() > 8 {
                    ui.text_weak("Dimensions shown for the first eight fasteners.");
                }
                if remove || original != connection {
                    self.commit_fastener_change(
                        ui.ctx(),
                        id,
                        &original,
                        (!remove).then_some(connection),
                    );
                }
            });
        }
        self.hardware_summary(ui);
    }
    fn hardware_summary(&self, ui: &mut egui::Ui) {
        use crate::inspector_table::{Column, InspectorTable};
        let mut items = std::collections::BTreeMap::<(String, u64, u64), u32>::new();
        let mut invalid = 0;
        for connection in self.doc.model.connections.values().filter(|c| {
            self.fastener_dialog
                .members
                .is_some_and(|m| same_pair(m, c.members))
        }) {
            let evaluation = connection.evaluate(&self.doc.model);
            let ConnectionOperation::Fasteners(settings) = &connection.operation else { continue; };
            if !evaluation.errors.is_empty() {
                invalid += 1;
            }
            let kind = settings.kind.label();
            for placement in evaluation.placements {
                *items
                    .entry((
                        kind.into(),
                        (settings.diameter * 1e6).round() as u64,
                        (placement.head.distance(placement.tip) * 1e6).round() as u64,
                    ))
                    .or_default() += 1;
            }
        }
        if items.is_empty() && invalid == 0 {
            return;
        }
        ui.separator();
        ui.text_label("Hardware list (preview dimensions)");
        let mut table =
            InspectorTable::new(ui.available_width(), [Column::flex(2.), Column::flex(1.)]);
        table.header(ui, ["Type · Ø × length mm", "Quantity"]);
        for ((kind, diameter, length), quantity) in items {
            table.row(ui, (kind.clone(), diameter, length), |column, ui| {
                if column == 0 {
                    ui.text_label(format!(
                        "{kind} · {:.1} × {:.1}",
                        diameter as f64 / 1000.,
                        length as f64 / 1000.
                    ));
                } else {
                    ui.text_label(quantity.to_string());
                }
            });
        }
        if invalid > 0 {
            ui.colored_label(Color32::LIGHT_RED,format!("{invalid} invalid connections: quantities are incomplete. Resolve errors before purchasing."));
        }
        ui.text_weak("Review geometric warnings before purchasing. Structural strength has not been verified.");
    }
    pub(super) fn paint_fasteners(&self, p: &egui::Painter, rect: Rect, model: &Model) {
        if !self.doc.display.fasteners {
            return;
        }
        let view = self.doc.camera.view(dv(rect.size()));
        let screen = |p| view.project(p).map(|p| rect.min + pv(p));
        let mut draft = None;
        if self.doc.display.fastener_preview
            && self.fasteners.open
            && self.history_preview.is_none()
        {
            if let Some(mut members) = self.fastener_dialog.members {
                if self.fasteners.reverse {
                    members.swap(0, 1);
                }
                draft = Some(Connection {
                    members,
                    entry_face: self.fastener_dialog.entry_face,
                    operation: ConnectionOperation::Fasteners(self.fasteners.settings.clone()),
                });
            }
        }
        for connection in model.connections.values().chain(draft.iter()).filter(|c| matches!(c.operation, ConnectionOperation::Fasteners(_))) {
            let result = connection.evaluate(model);
            for placement in result.placements {
                if let (Some(head), Some(tip), Some(interface)) = (
                    screen(placement.head),
                    screen(placement.tip),
                    screen(placement.interface),
                ) {
                    let color = if placement.warnings.is_empty() && result.errors.is_empty() {
                        Color32::from_rgb(80, 210, 240)
                    } else {
                        Color32::LIGHT_RED
                    };
                    p.line_segment(
                        [head, interface],
                        Stroke::new(3., color.gamma_multiply(0.65)),
                    );
                    p.line_segment([interface, tip], Stroke::new(3., color));
                    p.circle_filled(head, 4., color);
                    p.circle_filled(interface, 2., Color32::WHITE);
                    p.arrow(
                        tip - (tip - head).normalized() * 12.,
                        (tip - head).normalized() * 12.,
                        Stroke::new(2., color),
                    );
                }
            }
            if !result.errors.is_empty() {
                // Keep invalid relations visible even when no insertion axis remains.
                for member in connection.members {
                    let entities = model.expand_entities(&BTreeSet::from([member]));
                    if let Some(point) = entities
                        .iter()
                        .filter_map(|id| model.vertices.get(id))
                        .find_map(|v| screen(v.position))
                    {
                        p.text(
                            point,
                            Align2::LEFT_BOTTOM,
                            "⚠ Invalid fastener connection",
                            FontId::proportional(12.),
                            Color32::LIGHT_RED,
                        );
                    }
                }
            }
        }
    }
    fn fastener_pick_connection(&self) -> Option<Connection> {
        match self.fastener_dialog.picking? {
            PickTarget::Draft => {
                let mut members = self.fastener_dialog.members?;
                if self.fasteners.reverse {
                    members.swap(0, 1);
                }
                Some(Connection {
                    members,
                    entry_face: self.fastener_dialog.entry_face,
                    operation: ConnectionOperation::Fasteners(self.fasteners.settings.clone()),
                })
            }
            PickTarget::Connection(id) => self.doc.model.connections.get(&id).cloned(),
        }
    }
    fn fastener_handles(&self, rect: Rect) -> Vec<(u64, Pos2)> {
        let Some(connection) = self.fastener_pick_connection() else {
            return Vec::new();
        };
        connection
            .entry_faces(&self.doc.model)
            .into_iter()
            .filter_map(|id| {
                let face = self.doc.model.faces.get(&id)?;
                let center = face
                    .vertices
                    .iter()
                    .filter_map(|id| self.doc.model.vertices.get(id))
                    .map(|v| v.position)
                    .sum::<DVec3>()
                    / face.vertices.len() as f64;
                let point = rect.min + pv(self.doc.camera.project(center, dv(rect.size()))?);
                rect.contains(point).then_some((id, point))
            })
            .collect()
    }
    pub(super) fn fastener_handle_under_pointer(&self, _ctx: &egui::Context, _rect: Rect) -> bool {
        // While choosing a face, clicks must never select/move the underlying parts.
        self.fastener_dialog.picking.is_some()
    }
    pub(super) fn fastener_face_handles(&mut self, ui: &mut egui::Ui, rect: Rect) {
        if !self.fasteners.open || self.history_preview.is_some() {
            return;
        }
        let Some(target) = self.fastener_dialog.picking else {
            return;
        };
        let Some(connection) = self.fastener_pick_connection() else {
            return;
        };
        let eligible = self.fastener_pair_eligible(ui.ctx(), connection.members);
        for (face, point) in self.fastener_handles(rect) {
            let response = ui
                .put(
                    Rect::from_center_size(point, egui::vec2(42., 24.)),
                    egui::Button::new(format!("{face}")).small(),
                )
                .on_hover_text("Use this entry face");
            if response.clicked() && eligible {
                match target {
                    PickTarget::Draft => self.fastener_dialog.entry_face = Some(face),
                    PickTarget::Connection(id) => {
                        let mut changed = connection.clone();
                        changed.entry_face = Some(face);
                        self.commit_fastener_change(ui.ctx(), id, &connection, Some(changed));
                    }
                }
                self.fastener_dialog.picking = None;
            }
        }
    }
}
fn same_pair(a: [u64; 2], b: [u64; 2]) -> bool {
    a == b || a == [b[1], b[0]]
}

impl KetchupApp {
    pub(super) fn fastener_pair_eligible(&self, ctx: &egui::Context, members: [u64; 2]) -> bool {
        let pair = BTreeSet::from(members);
        members[0] != members[1]
            && self.selected == pair
            && !self.project.blocked()
            && !self.screenshot.open
            && self.history_preview.is_none()
            && members.iter().all(|id| {
                self.doc.model.objects.contains_key(id)
                    && self.doc.model.entity_in_context(*id) == Some(*id)
                    && !self.doc.model.is_hidden(*id)
            })
            && !self.doc.model.selection_protected(&pair)
            && self.shared_fastener_edit_allowed(ctx, &pair)
    }
    pub(super) fn selected_fastener_pair(&self, ctx: &egui::Context) -> Option<[u64; 2]> {
        if self.selected.len() != 2 {
            return None;
        }
        let mut selected = self.selected.iter().copied();
        let pair = [selected.next()?, selected.next()?];
        self.fastener_pair_eligible(ctx, pair).then_some(pair)
    }
    pub(super) fn open_fasteners(&mut self, ctx: &egui::Context) {
        let Some(members) = self.selected_fastener_pair(ctx) else {
            return;
        };
        self.fastener_dialog = Dialog {
            members: Some(members),
            ..Default::default()
        };
        self.joinery.open = false;
        self.fasteners.open = true;
        self.set_tool(Tool::Select);
    }
    fn create_fasteners(&mut self, ctx: &egui::Context, connection: Connection) -> bool {
        if !self
            .fastener_dialog
            .members
            .is_some_and(|pair| same_pair(pair, connection.members))
            || !self.fastener_pair_eligible(ctx, connection.members)
        {
            self.say(
                ctx,
                "The object pair is no longer editable. No fasteners were created.",
            );
            return false;
        }
        let before = self.doc.model.clone();
        match self.doc.model.add_connection(connection) {
            Ok(_) => {
                self.finish_edit("Create fasteners", before, ctx);
                true
            }
            Err(error) => {
                self.say(ctx, error);
                false
            }
        }
    }
    fn commit_fastener_change(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        original: &Connection,
        replacement: Option<Connection>,
    ) -> bool {
        let valid = self
            .fastener_dialog
            .members
            .is_some_and(|pair| same_pair(pair, original.members))
            && self.fastener_pair_eligible(ctx, original.members)
            && self.doc.model.connections.get(&id) == Some(original)
            && replacement.as_ref().is_none_or(|c| {
                let ConnectionOperation::Fasteners(s) = &c.operation else { return false; };
                same_pair(c.members, original.members)
                    && s.valid()
                    && c.entry_face
                        .is_none_or(|face| c.entry_faces(&self.doc.model).contains(&face))
            });
        if !valid {
            self.say(ctx,"The objects, permissions or connection changed. Reopen Automatic Fasteners before editing.");
            return false;
        }
        let before = self.doc.model.clone();
        if let Some(connection) = replacement {
            self.doc.model.connections.insert(id, connection);
        } else {
            self.doc.model.connections.remove(&id);
        }
        self.finish_edit("Edit fasteners", before, ctx);
        true
    }
    pub(super) fn automatic_fasteners_window(&mut self, ctx: &egui::Context) {
        if !self.fasteners.open {
            self.fastener_dialog = Default::default();
            return;
        }
        // Restoring the dialog never restores member IDs from global preferences.
        if self.fastener_dialog.members.is_none() {
            self.fastener_dialog.members = self.selected_fastener_pair(ctx);
        }
        if self.fastener_dialog.picking.is_some() {
            egui::Area::new(egui::Id::unique("automatic_fasteners_face_prompt"))
                .anchor(Align2::CENTER_TOP, egui::vec2(0., 70.))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.text_heading("Automatic Fasteners");
                        if self
                            .fastener_dialog
                            .members
                            .is_some_and(|pair| self.fastener_pair_eligible(ctx, pair))
                            && self.fastener_pick_connection().is_some()
                        {
                            ui.text_label("Click a numbered entry-face handle in the viewport.");
                        } else {
                            ui.colored_label(
                                Color32::LIGHT_RED,
                                "The object pair or connection is no longer editable.",
                            );
                        }
                        if ui.button("Back to Automatic Fasteners").clicked() {
                            self.fastener_dialog.picking = None;
                        }
                    });
                });
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                self.fastener_dialog.picking = None;
            }
            return;
        }
        let mut close = false;
        let response = Modal::new("connections/fasteners", 600.)
            .resizable(true)
            .show(ctx, |modal| {
                modal.header(|ui| {
                    ui.text_heading("Automatic Fasteners");
                    ui.text_weak(
                        "Changes apply immediately and can be undone after closing this dialog.",
                    );
                });
                modal.body(|ui| {
                    let scroll = sidebar::restored_scroll_area(
                        ui,
                        "connections/fasteners",
                        egui::ScrollArea::vertical()
                            .id_salt("automatic_fasteners_scroll")
                            .max_height((ctx.viewport_rect().height() - 220.).max(180.)),
                    )
                    .show(ui, |ui| self.fastener_contents(ui));
                    sidebar::remember_scroll(
                        ctx,
                        "connections/fasteners",
                        scroll.state.clamped_offset(),
                    );
                });
                modal.footer(|ui| {
                    close = ui.button("Close").clicked();
                });
            });
        if close || response.should_close() {
            self.fasteners.open = false;
            self.fastener_dialog = Default::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (KetchupApp, egui::Context, [u64; 2]) {
        let mut model = Model::default();
        let mut members = [0; 2];
        for (i, member) in members.iter_mut().enumerate() {
            let edge = model
                .add_edge(DVec3::new(i as f64, 0., 0.), DVec3::new(i as f64, 1., 0.))
                .unwrap();
            *member = model
                .make_object(&BTreeSet::from([edge]), false, "Part")
                .unwrap();
        }
        let app = KetchupApp::with_document(
            Document {
                model,
                ..Default::default()
            },
            None,
            true,
        );
        (app, egui::Context::default(), members)
    }
    #[test]
    fn menu_requires_exactly_two_editable_objects_in_current_context() {
        let (mut app, ctx, members) = fixture();
        assert!(app.selected_fastener_pair(&ctx).is_none());
        app.selected = BTreeSet::from([members[0]]);
        assert!(app.selected_fastener_pair(&ctx).is_none());
        app.selected = BTreeSet::from(members);
        assert!(app.selected_fastener_pair(&ctx).is_some());
        let edge = *app.doc.model.edges.keys().next().unwrap();
        app.selected.insert(edge);
        assert!(app.selected_fastener_pair(&ctx).is_none());
        app.selected = BTreeSet::from([members[0], edge]);
        assert!(app.selected_fastener_pair(&ctx).is_none());
        app.selected = BTreeSet::from(members);
        app.doc.model.locked.insert(members[0]);
        app.open_fasteners(&ctx);
        assert!(!app.fasteners.open);
        app.doc.model.locked.clear();
        app.open_fasteners(&ctx);
        assert!(app.fasteners.open);
        app.doc.model.edit_context = Some(members[0]);
        assert!(!app.fastener_pair_eligible(&ctx, members));
    }
    #[test]
    fn stale_deleted_or_locked_connections_cannot_commit() {
        let (mut app, ctx, members) = fixture();
        app.selected = BTreeSet::from(members);
        app.open_fasteners(&ctx);
        let connection = Connection {
            members,
            entry_face: None,
            operation: ConnectionOperation::Fasteners(Default::default()),
        };
        let id = u64::MAX - 1;
        app.doc.model.connections.insert(id, connection.clone());
        let mut changed = connection.clone();
        let ConnectionOperation::Fasteners(s) = &mut changed.operation else { panic!("Expected fasteners"); };
        s.count = 7;
        assert!(app.commit_fastener_change(&ctx, id, &connection, Some(changed.clone())));
        // A remote edit invalidates a locally retained snapshot.
        assert!(!app.commit_fastener_change(&ctx, id, &connection, None));
        app.doc.model.locked.insert(members[1]);
        assert!(!app.commit_fastener_change(&ctx, id, &changed, None));
        app.doc.model.locked.clear();
        app.doc.model.objects.remove(&members[1]);
        let before = app.doc.model.clone();
        assert!(!app.create_fasteners(&ctx, connection));
        assert!(!app.commit_fastener_change(&ctx, id, &changed, None));
        assert_eq!(app.doc.model, before);
    }
}
