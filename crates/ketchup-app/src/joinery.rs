use super::*;
use crate::inspector_table::inspector_field;
use ketchup_core::connections::{Connection, ConnectionOperation};
use ketchup_core::joinery::{GeometryEvaluation, JointKind, JointSettings};

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub settings: JointSettings,
    pub reverse: bool,
    pub open: bool,
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
    preview: Option<Preview>,
}
struct Preview {
    source: Model,
    members: [u64; 2],
    settings: JointSettings,
    result: Result<Model, String>,
}
/// Cache by source geometry, including transient Move previews. Camera changes
/// do not repeat boolean operations; every actual board move invalidates it.
#[derive(Default)]
pub(super) struct GeometryCache {
    source: Option<Model>,
    pub result: Option<GeometryEvaluation>,
    faces: crate::renderer::FaceCache,
    entities: crate::renderer::EntityCache,
    picking: crate::picking::PickingCache,
}
impl GeometryCache {
    pub fn pick(
        &mut self,
        source: &Model,
        camera: &ketchup_core::Camera,
        display: &ketchup_core::DisplaySettings,
        size: ketchup_core::DVec2,
        pixel: ketchup_core::DVec2,
        include_faces: bool,
    ) -> Option<u64> {
        self.update(source);
        let model = &self.result.as_ref().unwrap().model;
        self.faces.update(model);
        self.entities.update(model);
        self.picking.set_display(display);
        self.picking.update(&self.faces);
        self.picking
            .pick(
                model,
                &self.faces,
                &self.entities,
                camera,
                size,
                pixel,
                8.,
                include_faces,
            )
            .filter(|id| source.contains_entity(id))
    }
    pub fn update(&mut self, source: &Model) {
        if self.source.as_ref() != Some(source) {
            self.result = Some(source.evaluate_joinery());
            self.source = Some(source.clone());
        }
    }
}
fn fraction(ui: &mut egui::Ui, id: &str, label: &str, value: &mut f64) {
    let mut percentage = *value * 100.;
    if inspector_field(ui, id, label, |ui| {
        ui.add(
            egui::DragValue::new(&mut percentage)
                .range(10. ..=90.)
                .suffix(" %"),
        )
    })
    .changed()
    {
        *value = percentage / 100.;
    }
}
fn fields(ui: &mut egui::Ui, settings: &mut JointSettings) {
    inspector_field(ui, "wood_joint_kind", "Joint", |ui| {
        egui::ComboBox::from_id_salt("kind")
            .selected_text(settings.kind.label())
            .show_ui(ui, |ui| {
                for kind in JointKind::ALL {
                    ui.selectable_value(&mut settings.kind, kind, kind.label());
                }
            })
            .response
    });
    for (id, label, depth) in [
        ("depth_axis", "Depth axis", true),
        ("width_axis", "Width axis", false),
    ] {
        let mut axis = if depth {
            settings.depth_axis
        } else {
            settings.width_axis
        };
        inspector_field(ui, id, label, |ui| {
            egui::ComboBox::from_id_salt(id)
                .selected_text(["X", "Y", "Z"][axis])
                .show_ui(ui, |ui| {
                    for (i, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                        ui.selectable_value(&mut axis, i, label);
                    }
                })
                .response
        });
        if depth {
            if axis == settings.width_axis {
                settings.width_axis = settings.depth_axis;
            }
            settings.depth_axis = axis;
        } else {
            if axis == settings.depth_axis {
                settings.depth_axis = settings.width_axis;
            }
            settings.width_axis = axis;
        }
    }
    if matches!(
        settings.kind,
        JointKind::MortiseTenon | JointKind::TongueGroove | JointKind::Rabbet | JointKind::Dovetail
    ) {
        fraction(
            ui,
            "joint_width",
            "Profile width (% of overlap)",
            &mut settings.width_ratio,
        );
    }
    if settings.kind == JointKind::MortiseTenon {
        fraction(
            ui,
            "joint_height",
            "Tenon height (% of overlap)",
            &mut settings.height_ratio,
        );
    }
    if settings.kind == JointKind::Finger {
        inspector_field(ui, "joint_fingers", "Fingers on first part", |ui| {
            ui.add(egui::DragValue::new(&mut settings.fingers).range(2..=16))
        });
    }
    if settings.kind == JointKind::Dovetail {
        inspector_field(ui, "joint_taper", "Dovetail angle", |ui| {
            ui.add(
                egui::DragValue::new(&mut settings.taper_degrees)
                    .range(1. ..=30.)
                    .suffix("°"),
            )
        });
    }
    let mut mm = settings.clearance * 1000.;
    if inspector_field(ui, "joint_clearance", "Receiver clearance per side", |ui| {
        ui.add(
            egui::DragValue::new(&mut mm)
                .range(0. ..=10.)
                .speed(0.01)
                .suffix(" mm"),
        )
    })
    .changed()
    {
        settings.clearance = mm / 1000.;
    }
}
impl KetchupApp {
    pub(super) fn open_joinery(&mut self, ctx: &egui::Context) {
        let Some(members) = self.selected_fastener_pair(ctx) else {
            return;
        };
        self.joinery_dialog = Dialog {
            members: Some(members),
            ..Default::default()
        };
        self.fasteners.open = false;
        self.joinery.open = true;
        self.set_tool(Tool::Select);
    }
    fn refresh_joint_preview(&mut self) {
        let Some(mut members) = self.joinery_dialog.members else {
            return;
        };
        if self.joinery.reverse {
            members.swap(0, 1);
        }
        if self.joinery_dialog.preview.as_ref().is_some_and(|p| {
            p.source == self.doc.model
                && p.members == members
                && p.settings == self.joinery.settings
        }) {
            return;
        }
        let mut model = self.doc.model.clone();
        let result = model
            .add_connection(Connection {
                members,
                entry_face: None,
                operation: ConnectionOperation::Joinery(self.joinery.settings.clone()),
            })
            .and_then(|_| {
                let evaluation = model.evaluate_joinery();
                if evaluation.errors.is_empty() {
                    Ok(evaluation.model)
                } else {
                    Err(evaluation
                        .errors
                        .into_iter()
                        .map(|(_, e)| e)
                        .collect::<Vec<_>>()
                        .join(" · "))
                }
            });
        self.joinery_dialog.preview = Some(Preview {
            source: self.doc.model.clone(),
            members,
            settings: self.joinery.settings.clone(),
            result,
        });
    }
    fn apply_joint_preview(&mut self, ctx: &egui::Context) -> bool {
        let Some(preview) = self.joinery_dialog.preview.as_ref() else {
            return false;
        };
        let mut members = preview.members;
        if self.joinery.reverse {
            members.swap(0, 1);
        }
        if self.joinery_dialog.members != Some(members)
            || !self.fastener_pair_eligible(ctx, preview.members)
            || preview.source != self.doc.model
            || preview.settings != self.joinery.settings
        {
            self.say(ctx,"The boards or permissions changed. Review the updated joint preview before applying.");
            return false;
        }
        if preview.result.is_err() {
            return false;
        }
        let connection = Connection {
            members: preview.members,
            entry_face: None,
            operation: ConnectionOperation::Joinery(preview.settings.clone()),
        };
        let before = self.doc.model.clone();
        match self.doc.model.add_connection(connection) {
            Ok(_) => {
                self.finish_edit("Create dynamic wood joint", before, ctx);
                self.joinery_dialog.preview = None;
                self.joinery.open = false;
                true
            }
            Err(error) => {
                self.say(ctx, error);
                false
            }
        }
    }
    fn joint_connections(&mut self, ui: &mut egui::Ui, members: [u64; 2]) {
        let connections: Vec<_> = self
            .doc
            .model
            .connections
            .iter()
            .filter(|(_, c)| {
                (c.members == members || c.members == [members[1], members[0]])
                    && matches!(c.operation, ConnectionOperation::Joinery(_))
            })
            .map(|(id, c)| (*id, c.clone()))
            .collect();
        for (id, original) in connections {
            ui.separator();
            ui.push_id(("wood_joint", id), |ui| {
                ui.text_heading(format!("Dynamic wood joint #{id}"));
                let mut changed = original.clone();
                let mut remove = false;
                let eligible = self.fastener_pair_eligible(ui.ctx(), original.members);
                ui.add_enabled_ui(eligible, |ui| {
                    if let ConnectionOperation::Joinery(settings) = &mut changed.operation {
                        fields(ui, settings);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Swap parts").clicked() {
                            changed.members.swap(0, 1);
                        }
                        remove = ui.button("Delete joint").clicked();
                    });
                });
                for (_, error) in self
                    .doc
                    .model
                    .evaluate_joinery()
                    .errors
                    .into_iter()
                    .filter(|(key, _)| *key == id)
                {
                    ui.colored_label(Color32::YELLOW, error);
                }
                if eligible && (remove || changed != original) {
                    let before = self.doc.model.clone();
                    if remove {
                        self.doc.model.connections.remove(&id);
                    } else {
                        self.doc.model.connections.insert(id, changed);
                    }
                    self.finish_edit("Edit dynamic wood joint", before, ui.ctx());
                    self.joinery_dialog.preview = None;
                }
            });
        }
    }
    pub(super) fn wood_joinery_window(&mut self, ctx: &egui::Context) {
        if !self.joinery.open {
            self.joinery_dialog = Default::default();
            return;
        }
        if self.joinery_dialog.members.is_none() {
            self.joinery_dialog.members = self.selected_fastener_pair(ctx);
        }
        let mut close = false;
        let response=Modal::new("connections/wood_joinery",600.).resizable(true).show(ctx,|modal| {
            modal.header(|ui| {
                ui.text_heading("Wood Joinery");
                ui.text_weak("Create a dynamic relation. Moving either board updates the joint automatically.");
            });
            modal.body(|ui| {
                let scroll=sidebar::restored_scroll_area(ui,"connections/wood_joinery",egui::ScrollArea::vertical().id_salt("wood_joinery_scroll").max_height((ctx.viewport_rect().height()-230.).max(180.))).show(ui,|ui| {
                    let Some(members)=self.joinery_dialog.members else {
                        ui.text_weak("Select exactly two boards and reopen Wood Joinery from the object menu.");return;
                    };
                    ui.text_weak("Use rectangular boards aligned with the project axes. Position them with an overlap equal to the joint depth; both boards must extend past opposite ends of the overlap along the depth axis. Touching faces alone do not provide stock for a cut joint.");
                    ui.text_weak("The first part retains the tenon, tongue, tail or fingers; the second receives the matching cut. Half laps divide the overlap along the remaining axis. Source boards, components and material assignments stay intact. Separating the parts suspends the joint; bringing them back reactivates it.");
                    ui.checkbox(&mut self.joinery.reverse,"Swap the selected parts");
                    for (index,id) in (if self.joinery.reverse {[members[1],members[0]]}else{members}).into_iter().enumerate() {
                        if let Some(object)=self.doc.model.objects.get(&id) {ui.text_label(format!("Part {}: {}",index+1,object.name));}
                    }
                    self.joint_connections(ui,members);
                    ui.separator();
                    ui.text_heading("New dynamic joint");
                    fields(ui,&mut self.joinery.settings);
                    ui.checkbox(&mut self.doc.display.joinery_preview,"Show resulting edges through the model");
                    self.refresh_joint_preview();
                    let eligible=self.fastener_pair_eligible(ctx,members);
                    if !eligible {ui.colored_label(Color32::LIGHT_RED,"The selected pair is no longer editable. Check selection, locks and shared editing approval.");}
                    let mut valid=false;
                    if let Some(preview)=&self.joinery_dialog.preview {
                        match &preview.result {
                            Err(error)=> {ui.colored_label(Color32::LIGHT_RED,error);}
                            Ok(_)=>{valid=true;ui.text_label("Preview ready: cyan edges = first part; amber edges = second part.");}
                        }
                    }
                    if ui.add_enabled(eligible&&valid,egui::Button::new("Create dynamic joint")).clicked() {self.apply_joint_preview(ctx);}
                });
                sidebar::remember_scroll(ctx,"connections/wood_joinery",scroll.state.clamped_offset());
            });
            modal.footer(|ui| {close=ui.button("Close").clicked();});
        });
        close |= response.should_close();
        if close {
            self.joinery.open = false;
            self.joinery_dialog = Default::default();
        }
    }
    pub(super) fn paint_joinery(&self, p: &egui::Painter, rect: Rect) {
        if let Some(evaluation) = &self.joinery_geometry.result {
            for (index, (id, error)) in evaluation.errors.iter().take(4).enumerate() {
                p.text(
                    rect.left_top() + egui::vec2(16., 18. + index as f32 * 18.),
                    Align2::LEFT_TOP,
                    format!("Wood joint #{id} suspended: {error}"),
                    FontId::proportional(12.),
                    Color32::YELLOW,
                );
            }
        }
        if !self.joinery.open || !self.doc.display.joinery_preview || self.history_preview.is_some()
        {
            return;
        }
        let Some(preview) = &self.joinery_dialog.preview else {
            return;
        };
        if preview.source != self.doc.model || preview.settings != self.joinery.settings {
            return;
        }
        let Ok(model) = &preview.result else {
            return;
        };
        let view = self.doc.camera.view(dv(rect.size()));
        for (i, id) in preview.members.into_iter().enumerate() {
            let color = if i == 0 {
                Color32::from_rgb(80, 210, 240)
            } else {
                Color32::from_rgb(255, 190, 70)
            };
            for entity in model.expand_entities(&BTreeSet::from([id])) {
                let Some(edge) = model.edges.get(&entity) else {
                    continue;
                };
                if let (Some(a), Some(b)) = (
                    view.project(model.vertices[&edge.a].position),
                    view.project(model.vertices[&edge.b].position),
                ) {
                    p.line_segment([rect.min + pv(a), rect.min + pv(b)], Stroke::new(2., color));
                }
            }
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
            let x = i as f64 * 0.25;
            let mut body = Model::default();
            let corners = [
                DVec3::new(x, 0., 0.),
                DVec3::new(x + 0.3, 0., 0.),
                DVec3::new(x + 0.3, 0.1, 0.),
                DVec3::new(x, 0.1, 0.),
            ];
            for j in 0..4 {
                body.add_edge(corners[j], corners[(j + 1) % 4]).unwrap();
            }
            let face = *body.faces.keys().next().unwrap();
            let normal = ketchup_core::faces::plane(&body, &body.faces[&face].vertices)
                .unwrap()
                .normal;
            body.push_pull(face, 0.04 / normal.z).unwrap();
            let ids = model.paste_entities(&body, DVec3::ZERO);
            *member = model.make_object(&ids, false, "Board").unwrap();
        }
        let mut app = KetchupApp::with_document(
            Document {
                model,
                ..Default::default()
            },
            None,
            true,
        );
        app.selected = BTreeSet::from(members);
        (app, egui::Context::default(), members)
    }
    #[test]
    fn creating_joint_saves_a_relation_and_leaves_source_boards_intact() {
        let (mut app, ctx, members) = fixture();
        let source = app.doc.model.clone();
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        assert!(!app.joinery.open);
        assert_eq!(app.doc.model.vertices, source.vertices);
        assert_eq!(app.doc.model.faces, source.faces);
        assert_eq!(app.doc.model.objects, source.objects);
        assert_eq!(app.doc.model.connections.len(), 1);
        let connection = app.doc.model.connections.values().next().unwrap();
        assert_eq!(connection.members, members);
        assert!(matches!(
            connection.operation,
            ConnectionOperation::Joinery(_)
        ));
        assert!(app.doc.model.evaluate_joinery().errors.is_empty());
        assert_eq!(
            app.doc.history.undo_label(),
            Some("Create dynamic wood joint")
        );
    }
    #[test]
    fn stale_selection_permissions_and_source_changes_cannot_apply_preview() {
        for variant in 0..3 {
            let (mut app, ctx, members) = fixture();
            app.open_joinery(&ctx);
            app.refresh_joint_preview();
            match variant {
                0 => {
                    app.doc.model.locked.insert(members[1]);
                }
                1 => {
                    app.selected.remove(&members[1]);
                }
                _ => {
                    let vertex = *app.doc.model.vertices.keys().next().unwrap();
                    app.doc.model.vertices.get_mut(&vertex).unwrap().position.x += 0.001;
                }
            }
            let source = app.doc.model.clone();
            assert!(!app.apply_joint_preview(&ctx));
            assert_eq!(app.doc.model, source);
        }
    }
    #[test]
    fn invalid_saved_settings_are_sanitized_without_restoring_project_members() {
        let mut preferences: Preferences =
            serde_json::from_str(r#"{"settings":{"depth_axis":99},"open":true}"#).unwrap();
        preferences.sanitize();
        assert_eq!(preferences.settings, JointSettings::default());
        assert!(preferences.open);
        let dialog = Dialog::default();
        assert!(dialog.members.is_none());
    }
}
