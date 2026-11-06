use super::*;
use crate::inspector_table::inspector_field;
use ketchup_core::connections::{Connection, ConnectionOperation};
use ketchup_core::joinery::{GeometryEvaluation, JointKind, JointSettings};
use std::collections::BTreeMap;

#[path = "joinery_cards.rs"]
mod cards;
#[path = "joint_highlight.rs"]
mod highlight;

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
    orientations: OrientationCache,
    diagnostics: EditorDiagnostics,
    pub members: Option<[u64; 2]>,
    preview: Option<Preview>,
}
/// Keep committed-document diagnostics independent from a transient/history
/// viewport model. Sharing one source snapshot would invalidate both each frame.
#[derive(Default)]
struct EditorDiagnostics {
    source: crate::renderer::ModelSnapshot,
    revision: u64,
    errors: Vec<(u64, String)>,
    #[cfg(test)]
    evaluations: usize,
}
impl EditorDiagnostics {
    fn update(&mut self, source: &Model, geometry: Option<&mut GeometryCache>) {
        let revision = self.source.update(source);
        if revision == self.revision { return; }
        self.revision = revision;
        self.errors = if let Some(geometry) = geometry {
            geometry.source_errors(source).to_vec()
        } else {
            #[cfg(test)] { self.evaluations += 1; }
            source.evaluate_joinery().errors
        };
    }
}

/// Board validation includes solid-volume checks. Reuse orientation choices
/// across redraws, invalidating on source edits and every relevant setting.
#[derive(Default)]
struct OrientationCache {
    revision: Option<u64>,
    entries: std::collections::BTreeMap<[u64; 2], (JointSettings, Vec<(usize, usize)>)>,
    #[cfg(test)]
    builds: usize,
}
impl OrientationCache {
    fn supported(&mut self, model: &Model, revision: u64, members: [u64; 2], settings: &JointSettings) -> &[(usize, usize)] {
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            self.entries.clear();
        }
        if !self.entries.get(&members).is_some_and(|(saved, _)| saved == settings) {
            let supported = model.supported_joinery_axes(members, settings);
            self.entries.insert(members, (settings.clone(), supported));
            #[cfg(test)] { self.builds += 1; }
        }
        &self.entries[&members].1
    }
}

struct Preview {
    source: Model,
    members: [u64; 2],
    settings: JointSettings,
    result: Result<Model, String>,
    region: highlight::Region,
}
/// Cache by source geometry, including transient Move previews. Camera changes
/// do not repeat boolean operations; every actual board move invalidates it.
#[derive(Default)]
pub(super) struct GeometryCache {
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<std::thread::JoinHandle<GeometryCache>>,
    pub prepared_shadows: Option<(ketchup_core::DisplaySettings, crate::shadows::ShadowCache)>,
    evaluation: ketchup_core::joinery::EvaluationCache,
    region_cache: std::collections::BTreeMap<u64, CachedRegion>,
    #[cfg(test)]
    revision: u64,
    source_errors: Option<Vec<(u64, String)>>,
    #[cfg(test)]
    diagnostic_evaluations: usize,
    source: Option<Model>,
    regions: Vec<highlight::Region>,
    pub result: Option<GeometryEvaluation>,
    faces: crate::renderer::FaceCache,
    entities: crate::renderer::EntityCache,
    picking: crate::picking::PickingCache,
}

struct CachedRegion {
    source: ketchup_core::joinery::BoardSnapshot,
    evaluated: ketchup_core::joinery::BoardSnapshot,
    settings: JointSettings,
    region: highlight::Region,
}

/// Root is the assembled overview; inside an object, show only joints between
/// its immediate children. Opening a board therefore exposes its original stock.
fn joint_in_view(model: &Model, connection: &Connection) -> bool {
    model.edit_context.is_none()
        || connection.members.iter().all(|member| model.entity_in_context(*member) == Some(*member))
}

impl GeometryCache {
    /// Keep one in-flight machining job during a gesture. Completed previews
    /// are adopted in order; the next job uses the newest pointer position,
    /// rather than queuing every intermediate position. Exact picking and the
    /// end of a gesture synchronize through update().
    pub fn update_render(&mut self, source: &Model, transient: bool, ctx: &egui::Context,
        display: &ketchup_core::DisplaySettings, shadows: &crate::shadows::ShadowCache) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.pending.as_ref().is_some_and(|job| job.is_finished()) {
                self.finish_pending();
            }
            if transient && self.result.is_some() {
                if self.pending.is_none() && self.source.as_ref() != Some(source) {
                    let source = source.clone();
                    let display = display.clone();
                    let mut shadows = shadows.clone();
                    let mut worker = Self {
                        evaluation: std::mem::take(&mut self.evaluation),
                        region_cache: std::mem::take(&mut self.region_cache),
                        faces: std::mem::take(&mut self.faces),
                        entities: std::mem::take(&mut self.entities),
                        #[cfg(test)]
                        revision: self.revision,
                        ..Default::default()
                    };
                    self.pending = Some(std::thread::spawn(move || {
                        worker.update(&source);
                        let model = &worker.result.as_ref().unwrap().model;
                        worker.faces.update(model);
                        worker.entities.update(model);
                        shadows.update(&worker.faces, &worker.entities, &display);
                        worker.prepared_shadows = Some((display, shadows));
                        worker
                    }));
                }
                if self.pending.is_some() { ctx.request_repaint(); }
                return;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (transient, ctx, display, shadows);
        self.update(source);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn finish_pending(&mut self) {
        if let Some(job) = self.pending.take() {
            if let Ok(worker) = job.join() {
                self.evaluation = worker.evaluation;
                self.region_cache = worker.region_cache;
                self.source = worker.source;
                self.source_errors = worker.source_errors;
                self.result = worker.result;
                self.regions = worker.regions;
                self.faces = worker.faces;
                self.entities = worker.entities;
                self.prepared_shadows = worker.prepared_shadows;
                #[cfg(test)] { self.revision = worker.revision; }
            }
        }
    }

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
    fn source_errors(&mut self, source: &Model) -> &[(u64, String)] {
        self.update(source);
        self.source_errors.get_or_insert_with(|| {
            // Viewport evaluation may suspend relations while opening stock.
            // The editor must still validate the saved relations in that case.
            #[cfg(test)] { self.diagnostic_evaluations += 1; }
            source.evaluate_joinery().errors
        })
    }

    pub fn update(&mut self, source: &Model) {
        #[cfg(not(target_arch = "wasm32"))]
        self.finish_pending();
        if self.source.as_ref() != Some(source) {
            self.prepared_shadows = None;
            // Scope machining and guides together without changing saved
            // relations or the fully assembled geometry used by exports.
            let mut viewport_source = source.clone();
            viewport_source.connections.retain(|_, connection| {
                !matches!(connection.operation, ConnectionOperation::Joinery(_))
                    || joint_in_view(source, connection)
            });
            let evaluation = {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = super::viewport_benchmark::scope("joinery_evaluate");
                self.evaluation.evaluate(&viewport_source)
            };
            self.source_errors = (viewport_source.connections == source.connections)
                .then(|| evaluation.errors.clone());
            #[cfg(test)] { self.revision = self.revision.wrapping_add(1); }
            let mut boards = highlight::BoardGeometryCache::default();
            #[cfg(not(target_arch = "wasm32"))]
            let _regions_profile = super::viewport_benchmark::scope("joinery_regions");
            self.region_cache.retain(|id, _| viewport_source.connections.contains_key(id));
            self.regions.clear();
            for (&id, connection) in &viewport_source.connections {
                let ConnectionOperation::Joinery(settings) = &connection.operation else { continue };
                if evaluation.errors.iter().any(|(invalid, _)| *invalid == id) { continue; }
                let stock = ketchup_core::joinery::BoardSnapshot::new(source, connection.members);
                let machined = ketchup_core::joinery::BoardSnapshot::new(&evaluation.model, connection.members);
                let reusable = self.region_cache.get(&id).is_some_and(|entry|
                    entry.settings == *settings && entry.source.same_geometry(&stock)
                        && entry.evaluated.same_geometry(&machined));
                if !reusable {
                    self.region_cache.insert(id, CachedRegion {
                        source: stock, evaluated: machined, settings: settings.clone(),
                        region: highlight::Region::build_cached(source, &evaluation.model,
                            connection.members, settings, &mut boards),
                    });
                }
                self.regions.push(self.region_cache[&id].region.clone());
            }
            self.result=Some(evaluation);
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
fn fields(ui: &mut egui::Ui, settings: &mut JointSettings, model: &Model, members: [u64; 2],
    orientations: &mut OrientationCache, revision: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    let _profile = super::viewport_benchmark::scope("joinery_fields");
    cards::selector(ui, &mut settings.kind);
    let supported = orientations.supported(model, revision, members, settings);
    let mut selected = (settings.depth_axis, settings.width_axis);
    if !supported.contains(&selected) {
        if let Some(pair) = supported.first() { selected = *pair; }
    }
    let label = |(depth, width): (usize, usize)|
        format!("Depth {} / Width {}", ["X", "Y", "Z"][depth], ["X", "Y", "Z"][width]);
    inspector_field(ui, "joint_orientation", "Orientation", |ui| {
        ui.add_enabled_ui(!supported.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("joint_orientation")
                .selected_text(if supported.is_empty() { "No supported orientation".to_owned() } else { label(selected) })
                .show_ui(ui, |ui| {
                    for pair in supported { ui.selectable_value(&mut selected, *pair, label(*pair)); }
                }).response
        }).inner
    });
    (settings.depth_axis, settings.width_axis) = selected;
    if matches!(
        settings.kind,
        JointKind::MortiseTenon | JointKind::TongueGroove | JointKind::Rabbet
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
        let span = match (highlight::bounds(model, members[0]), highlight::bounds(model, members[1])) {
            (Some(a), Some(b)) => (a.1.min(b.1) - a.0.max(b.0))[settings.width_axis],
            _ => 0.,
        };
        let mut width_mm = settings.dovetail_width.unwrap_or_else(||
            settings.dovetail_layout(span).map_or(0.025, |(_, width)| width)) * 1000.;
        if inspector_field(ui, "joint_dovetail_width", "Tail width", |ui| {
            ui.add(egui::DragValue::new(&mut width_mm).range(0.1..=10000.).speed(0.5).suffix(" mm"))
        }).changed() {
            settings.dovetail_width = Some(width_mm / 1000.);
        }
        if let Some((count, _)) = settings.dovetail_layout(span) {
            ui.text_weak(format!("{count} tails fit across this joint."));
        }
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
fn warning_colors(dark: bool) -> (Color32, Color32) {
    if dark {
        (Color32::from_rgb(255, 221, 155), Color32::from_rgba_unmultiplied(48, 34, 12, 235))
    } else {
        (Color32::from_rgb(94, 52, 0), Color32::from_rgba_unmultiplied(255, 239, 204, 235))
    }
}
fn warning(ui: &mut egui::Ui, message: impl Into<String>) {
    let (text, background) = warning_colors(ui.visuals().dark_mode);
    egui::Frame::new().fill(background).corner_radius(4).inner_margin(8).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(message.into()).color(text)).wrap());
    });
}
impl KetchupApp {
    pub(super) fn joinery_edit_members(&self) -> Option<[u64; 2]> {
        if !self.joinery.open || self.history_preview.is_some() { return None; }
        let mut members = self.joinery_dialog.members?;
        if self.joinery.reverse { members.swap(0, 1); }
        Some(members)
    }
    pub(super) fn joinery_part_color(index: usize) -> Color32 {
        if index == 0 { Color32::from_rgb(45, 160, 235) }
        else { Color32::from_rgb(245, 155, 45) }
    }

    fn joint_markers(&self, rect: Rect) -> Vec<(u64, Pos2)> {
        if !self.doc.display.wood_joinery || self.joinery.open || self.history_preview.is_some() {
            return Vec::new();
        }
        self.doc.model.connections.iter().filter_map(|(id, connection)| {
            if !matches!(connection.operation, ConnectionOperation::Joinery(_))
                || !joint_in_view(&self.doc.model, connection)
            {
                return None;
            }
            if connection.members.iter().any(|id| self.doc.model.is_hidden(*id)) {
                return None;
            }
            let a = highlight::bounds(&self.doc.model, connection.members[0])?;
            let b = highlight::bounds(&self.doc.model, connection.members[1])?;
            let (lo, hi) = (a.0.max(b.0), a.1.min(b.1));
            // Separated boards still need an actionable warning marker.
            let center = if (hi - lo).min_element() < 0. {
                (a.0 + a.1 + b.0 + b.1) * 0.25
            } else { (lo + hi) * 0.5 };
            let model = self.joinery_geometry.result.as_ref()
                .map_or(&self.doc.model, |evaluation| &evaluation.model);
            let owners = model.expand_entities(&connection.members.into_iter().collect());
            if !self.render_picking_cache.marker_visible(
                &self.render_face_cache, &self.render_entity_cache, &self.doc.camera,
                dv(rect.size()), center, &owners,
            ) { return None; }
            let point = rect.min + pv(self.doc.camera.project(center, dv(rect.size()))?);
            rect.contains(point).then_some((*id, point))
        }).collect()
    }
    pub(super) fn joint_marker_under_pointer(&self, ctx: &egui::Context, rect: Rect) -> bool {
        ctx.input(|input| input.pointer.hover_pos()).is_some_and(|pos| {
            self.joint_markers(rect).iter().any(|(_, point)|
                Rect::from_center_size(*point, egui::vec2(28., 28.)).contains(pos))
        })
    }
    pub(super) fn joint_marker_handles(&mut self, ui: &mut egui::Ui, rect: Rect) {
        for (id, point) in self.joint_markers(rect) {
            let error = self.joinery_geometry.result.as_ref().and_then(|evaluation|
                evaluation.errors.iter().find(|(key, _)| *key == id).map(|(_, message)| message.clone()));
            let response = ui.put(
                Rect::from_center_size(point, egui::vec2(28., 28.)),
                egui::Button::new("").fill(ui.visuals().window_fill().to_opaque()),
            ).on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text(error.as_ref().map_or_else(|| format!("Edit wood joint #{id}"),
                    |message| format!("Wood joint #{id}: {message}\nClick to edit")));
            if error.is_some() {
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![point + egui::vec2(0., -10.), point + egui::vec2(10., 9.), point + egui::vec2(-10., 9.)],
                    Color32::from_rgb(255, 199, 66), Stroke::new(1.5, Color32::from_rgb(94, 52, 0)),
                ));
                ui.painter().text(point + egui::vec2(0., 2.), Align2::CENTER_CENTER,
                    "!", FontId::proportional(17.), Color32::from_rgb(65, 36, 0));
            } else {
                // Separate outlines keep both mating parts visible at icon size.
                for (index, path) in [
                    vec![(-9., -7.), (-2.5, -7.), (-2.5, -3.5), (2.5, -3.5),
                        (2.5, 2.5), (-2.5, 2.5), (-2.5, 7.), (-9., 7.), (-9., -7.)],
                    vec![(9., -7.), (-0.5, -7.), (-0.5, -5.5), (4.5, -5.5),
                        (4.5, 4.5), (-0.5, 4.5), (-0.5, 7.), (9., 7.), (9., -7.)],
                ].into_iter().enumerate() {
                    ui.painter().add(egui::Shape::line(
                        path.into_iter().map(|(x, y)| point + egui::vec2(x, y)).collect(),
                        Stroke::new(1.8, Self::joinery_part_color(index)),
                    ));
                }
            }
            if response.clicked() {
                let members = self.doc.model.connections[&id].members;
                self.selected = BTreeSet::from(members);
                self.open_joinery(ui.ctx());
                sidebar::set_section_open(ui.ctx(), "connections/wood_joinery", true);
            }
        }
    }
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
        self.inspector = true;
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
        let region=result.as_ref().map(|model|highlight::Region::build(&self.doc.model,model,members,&self.joinery.settings)).unwrap_or_default();
        self.joinery_dialog.preview = Some(Preview {
            source: self.doc.model.clone(),
            members,
            settings: self.joinery.settings.clone(),
            result,
            region,
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
                true
            }
            Err(error) => {
                self.say(ctx, error);
                false
            }
        }
    }
    fn joint_connections(&mut self, ui: &mut egui::Ui, members: [u64; 2]) {
        let shared_view = self.history_preview.is_none()
            && self.modeling.as_ref().and_then(|edit| edit.preview.as_ref()).is_none();
        self.joinery_dialog.diagnostics.update(&self.doc.model,
            shared_view.then_some(&mut self.joinery_geometry));
        let errors = self.joinery_dialog.diagnostics.errors.clone();
        let revision = self.joinery_dialog.diagnostics.revision;
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
                if !eligible {
                    warning(ui, self.fastener_pair_block_reason(ui.ctx(), original.members));
                    if self.account.sharing_available() && self.doc.shared_project.is_some()
                        && ui.button("Review sharing status…").clicked() {
                        self.sharing.preferences.open = true;
                    }
                }
                ui.add_enabled_ui(eligible, |ui| {
                    if let ConnectionOperation::Joinery(settings) = &mut changed.operation {
                        fields(ui, settings, &self.doc.model, original.members,
                            &mut self.joinery_dialog.orientations, revision);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Swap parts").clicked() {
                            changed.members.swap(0, 1);
                        }
                        remove = ui.button("Delete joint").clicked();
                    });
                });
                for (_, error) in errors.iter().filter(|(key, _)| *key == id) {
                    warning(ui, error.clone());
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
    pub(super) fn wood_joinery_sidebar(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = super::viewport_benchmark::scope("joinery_editor");
        let ctx = ui.ctx().clone();
        let scroll = sidebar::restored_scroll_area(
            ui, "connections/wood_joinery",
            egui::ScrollArea::vertical().id_salt("wood_joinery_scroll")
                .content_margin(18)
                .auto_shrink([false, false]),
        ).show(ui, |ui| {
            sidebar::inspector_section(ui, "Wood Joinery", "connections/wood_joinery", true, |ui| {
        ui.text_weak("Create a dynamic relation. Moving either board updates the joint automatically.");
                    let Some(members)=self.joinery_dialog.members else {
                        ui.text_weak("Select exactly two boards and reopen Wood Joinery from the Edit menu.");return;
                    };
                    self.joint_connections(ui,members);
                    if !self.doc.model.has_joinery_between(members) {
                        ui.separator();
                        let eligible = self.fastener_pair_eligible(&ctx, members);
                        if !eligible { warning(ui, self.fastener_pair_block_reason(&ctx, members)); }
                        if ui.add_enabled(eligible, egui::Button::new("Add new dynamic joint")).clicked() {
                            let mut ordered = members;
                            if self.joinery.reverse { ordered.swap(0, 1); }
                            let supported = self.doc.model.supported_joinery_axes(ordered, &self.joinery.settings);
                            if !supported.contains(&(self.joinery.settings.depth_axis, self.joinery.settings.width_axis)) {
                                if let Some(&(depth, width)) = supported.first() {
                                    self.joinery.settings.depth_axis = depth;
                                    self.joinery.settings.width_axis = width;
                                }
                            }
                            self.refresh_joint_preview();
                            self.apply_joint_preview(&ctx);
                        }
                        if let Some(Preview { result: Err(error), .. }) = &self.joinery_dialog.preview {
                            warning(ui, error);
                        }
                    }
            });
        });
        sidebar::remember_scroll(
            &ctx,
            "connections/wood_joinery",
            scroll.state.clamped_offset(),
        );
    }
    pub(super) fn paint_joinery(&self, p: &egui::Painter, rect: Rect) {
        if let Some(evaluation) = &self.joinery_geometry.result {
            let (text, background) = warning_colors(p.ctx().global_style().visuals.dark_mode);
            let mut position = rect.left_top() + egui::vec2(16., 18.);
            for (id, error) in evaluation.errors.iter().take(4) {
                let galley = p.layout(format!("Wood joint #{id} suspended: {error}"),
                    FontId::proportional(12.), text, (rect.width() - 48.).max(1.));
                let bounds = Rect::from_min_size(position, galley.size());
                p.rect_filled(bounds.expand(6.), 4, background);
                let height = bounds.height();
                p.galley(position, galley, text);
                position.y += height + 18.;
            }
        }
    }
    pub(super) fn joint_highlight_preview_key(&self)->Option<([u64;2],JointSettings)> {
        if !self.joinery.open || !self.doc.display.joinery_preview || self.history_preview.is_some() {return None;}
        let preview=self.joinery_dialog.preview.as_ref()?;
        (preview.source==self.doc.model && preview.settings==self.joinery.settings && preview.result.is_ok())
            .then(||(preview.members,preview.settings.clone()))
    }
    pub(super) fn paint_joint_highlights(&self,mesh:&mut renderer::Mesh,model:&Model,display:&ketchup_core::DisplaySettings,dark:bool) {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = super::viewport_benchmark::scope("joint_guides");
        if !display.wood_joinery { return; }
        let preview = self.joint_highlight_preview_key().and(self.joinery_dialog.preview.as_ref());
        let mut surfaces = BTreeMap::new();
        for region in &self.joinery_geometry.regions {
            region.register_guides(mesh);
            region.surface_requests(&mut surfaces);
        }
        if let Some(preview) = preview {
            preview.region.register_guides(mesh);
            preview.region.surface_requests(&mut surfaces);
        }
        // Prepare each shared board once, then balance all strokes together.
        // Nested region/line workers duplicated masks and left uneven chunks.
        mesh.prepare_joint_surfaces(&surfaces.into_values().collect::<Vec<_>>());
        let mut guides = Vec::new();
        for region in &self.joinery_geometry.regions {
            region.prepare(mesh, model, display, dark, false, &mut guides);
        }
        if let Some(preview) = preview {
            preview.region.prepare(mesh, model, display, dark, true, &mut guides);
        }
        mesh.paint_overlays(&guides, 64, |mesh, guide| guide.paint(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ui::ContextExt;
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn background_gesture_previews_settle_to_the_latest_source_and_cancel_cleanly() {
        let (mut app, ctx, members) = fixture();
        app.doc.model.add_connection(Connection {
            members, entry_face: None,
            operation: ConnectionOperation::Joinery(JointSettings::default()),
        }).unwrap();
        let source = app.doc.model.clone();
        let mut cache = GeometryCache::default();
        cache.update(&source);
        let initial_revision = cache.revision;
        let mut latest = source.clone();
        for offset in [0.001, 0.003, 0.007] {
            latest = source.clone();
            latest.move_selection(&BTreeSet::from([members[0]]), DVec3::X * offset).unwrap();
            cache.update_render(&latest, true, &ctx, &app.doc.display, &app.shadow_cache);
            assert!(cache.pending.is_some());
            assert!(cache.result.is_some(), "the last complete preview stays drawable");
        }
        cache.update(&latest);
        assert!(cache.pending.is_none());
        assert_eq!(cache.source.as_ref(), Some(&latest));
        assert_eq!(cache.result.as_ref().unwrap().model, latest.evaluate_joinery().model);
        assert!(cache.revision > initial_revision);
        let revision = cache.revision;
        cache.update_render(&latest, true, &ctx, &app.doc.display, &app.shadow_cache);
        assert!(cache.pending.is_none());
        assert_eq!(cache.revision, revision);
        latest.move_selection(&BTreeSet::from([members[0]]), DVec3::X * 0.01).unwrap();
        cache.update_render(&latest, true, &ctx, &app.doc.display, &app.shadow_cache);
        cache.update(&source);
        assert!(cache.pending.is_none());
        assert_eq!(cache.source.as_ref(), Some(&source));
        assert_eq!(cache.result.as_ref().unwrap().model, source.evaluate_joinery().model);
    }

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
    fn editor_redraws_share_viewport_evaluation_and_refresh_after_settings_change() {
        let (mut app, ctx, _) = fixture();
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        app.doc.display.wood_joinery = true;
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.));
        let render = |app: &mut KetchupApp| {
            let _ = ctx.run_test_ui(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200., 900.))),
                ..Default::default()
            }, |ui| {
                app.wood_joinery_sidebar(ui);
                app.paint_scene(ui, rect);
            });
        };
        render(&mut app);
        render(&mut app);
        let source = app.doc.model.clone();
        let revision = app.joinery_geometry.revision;
        let builds = app.scene_cache.mesh_builds;
        let orientations = app.joinery_dialog.orientations.builds;
        assert_eq!(orientations, 1);
        for _ in 0..4 { render(&mut app); }
        assert_eq!(app.doc.model, source);
        assert_eq!(app.joinery_geometry.revision, revision, "orbit/redraw must not repeat CSG");
        assert_eq!(app.joinery_geometry.diagnostic_evaluations, 0, "diagnostics share the viewport result");
        assert_eq!(app.joinery_dialog.orientations.builds, orientations);
        assert_eq!(app.scene_cache.mesh_builds, builds);

        let connection = app.doc.model.connections.values_mut().next().unwrap();
        let ConnectionOperation::Joinery(settings) = &mut connection.operation else { panic!() };
        settings.clearance += 0.0001;
        render(&mut app);
        assert_eq!(app.joinery_geometry.revision, revision + 1);
        assert_eq!(app.joinery_dialog.orientations.builds, orientations + 1);
        assert_eq!(app.joinery_geometry.result.as_ref().unwrap().model,
            app.doc.model.evaluate_joinery().model, "the preview must reflect edited settings");
        render(&mut app);
        assert_eq!(app.joinery_geometry.revision, revision + 1);
    }

    #[test]
    fn editor_diagnostics_do_not_replace_the_history_preview_geometry_cache() {
        let (mut app, ctx, _) = fixture();
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        app.joinery_dialog.diagnostics.update(&app.doc.model, Some(&mut app.joinery_geometry));
        let mut history = app.doc.model.clone();
        let point = history.vertices.values_mut().next().unwrap();
        point.position.x += 0.001;
        app.joinery_geometry.update(&history);
        let revision = app.joinery_geometry.revision;
        for _ in 0..4 {
            app.joinery_dialog.diagnostics.update(&app.doc.model, None);
            assert_eq!(app.joinery_geometry.revision, revision);
        }
        assert_eq!(app.joinery_dialog.diagnostics.evaluations, 0);
        let ConnectionOperation::Joinery(settings) = &mut app.doc.model.connections.values_mut().next().unwrap().operation else { panic!() };
        settings.clearance += 0.0001;
        app.joinery_dialog.diagnostics.update(&app.doc.model, None);
        assert_eq!(app.joinery_dialog.diagnostics.errors, app.doc.model.evaluate_joinery().errors);
        assert_eq!(app.joinery_dialog.diagnostics.evaluations, 1);
        assert_eq!(app.joinery_geometry.revision, revision, "validation must not replace a separate viewport source");
        app.joinery_dialog.diagnostics.update(&app.doc.model, None);
        assert_eq!(app.joinery_dialog.diagnostics.evaluations, 1);
    }

    #[test]
    fn cached_editor_diagnostics_validate_saved_joints_even_when_stock_editing_suspends_them() {
        let (mut app, ctx, members) = fixture();
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        let valid = app.doc.model.clone();
        for id in app.doc.model.selection_vertices(&BTreeSet::from([members[1]])) {
            app.doc.model.vertices.get_mut(&id).unwrap().position.x += 1.;
        }
        assert!(app.doc.model.enter_object(members[0]));
        let mut cache = GeometryCache::default();
        cache.update(&app.doc.model);
        assert!(cache.result.as_ref().unwrap().errors.is_empty(), "the viewport suspends this relation");
        let expected = app.doc.model.evaluate_joinery().errors;
        assert_eq!(expected.len(), 1);
        assert_eq!(cache.source_errors(&app.doc.model), expected);
        assert_eq!(cache.source_errors(&app.doc.model), expected);
        assert_eq!(cache.diagnostic_evaluations, 1);
        app.doc.model.edit_context = None;
        assert_eq!(cache.source_errors(&app.doc.model), expected);
        assert_eq!(cache.diagnostic_evaluations, 1, "normal editing reuses viewport diagnostics");
        app.doc.model = valid;
        assert!(cache.source_errors(&app.doc.model).is_empty(), "repairs clear stale errors");
    }

    #[test]
    fn editor_orientation_cache_tracks_settings_parts_and_source_geometry() {
        let (mut app, _, members) = fixture();
        let mut cache = OrientationCache::default();
        let mut settings = JointSettings::default();
        let supported = app.doc.model.supported_joinery_axes(members, &settings);
        assert!(!supported.is_empty());
        for _ in 0..4 { assert_eq!(cache.supported(&app.doc.model, 1, members, &settings), supported); }
        assert_eq!(cache.builds, 1);
        settings.kind = JointKind::Finger;
        assert_eq!(cache.supported(&app.doc.model, 1, members, &settings),
            app.doc.model.supported_joinery_axes(members, &settings));
        let reversed = [members[1], members[0]];
        assert_eq!(cache.supported(&app.doc.model, 1, reversed, &settings),
            app.doc.model.supported_joinery_axes(reversed, &settings));
        assert_eq!(cache.builds, 3);
        for id in app.doc.model.selection_vertices(&BTreeSet::from([members[1]])) {
            app.doc.model.vertices.get_mut(&id).unwrap().position.x += 1.;
        }
        assert!(cache.supported(&app.doc.model, 2, members, &settings).is_empty());
        assert_eq!(cache.builds, 4);
        assert_eq!(cache.entries.len(), 1, "drop choices belonging to older geometry");
    }

    #[test]
    fn saved_joint_markers_follow_display_toggle_and_do_not_mark_drafts() {
        let (mut app, ctx, members) = fixture();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 800.));
        app.doc.display.wood_joinery = true;
        assert!(app.joint_markers(rect).is_empty());
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        assert!(app.joint_markers(rect).is_empty(), "icons are hidden while editing");
        app.joinery.open = false;
        let markers = app.joint_markers(rect);
        assert_eq!(markers.len(), 1);
        assert_eq!(app.doc.model.connections[&markers[0].0].members, members);
        assert!(rect.contains(markers[0].1));
        app.doc.display.wood_joinery = false;
        assert!(app.joint_markers(rect).is_empty());
    }
    #[test]
    fn failed_separated_joint_keeps_a_marker_and_clears_error_after_repair() {
        let (mut app, ctx, members) = fixture();
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        app.joinery.open = false;
        app.doc.display.wood_joinery = true;
        let original = app.doc.model.clone();
        for id in app.doc.model.selection_vertices(&BTreeSet::from([members[1]])) {
            app.doc.model.vertices.get_mut(&id).unwrap().position.x += 0.5;
        }
        app.joinery_geometry.update(&app.doc.model);
        assert_eq!(app.joinery_geometry.result.as_ref().unwrap().errors.len(), 1);
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 800.));
        assert_eq!(app.joint_markers(rect).len(), 1);
        app.doc.model = original;
        app.joinery_geometry.update(&app.doc.model);
        assert!(app.joinery_geometry.result.as_ref().unwrap().errors.is_empty());
        assert_eq!(app.joint_markers(rect).len(), 1);
    }

    #[test]
    fn joint_icons_cuts_and_guides_follow_the_current_group_and_restore_at_root() {
        let (mut app, _, members) = fixture();
        let joint = app.doc.model.add_connection(Connection {
            members, entry_face: None,
            operation: ConnectionOperation::Joinery(JointSettings::default()),
        }).unwrap();
        let boards = app.doc.model.copy_entities(&BTreeSet::from(members));
        let group = app.doc.model.make_object(&BTreeSet::from(members), false, "Assembly").unwrap();
        let assembly = app.doc.model.copy_entities(&BTreeSet::from([group]));
        let sibling = *app.doc.model.paste_entities(&assembly, DVec3::Y * 0.4).iter().next().unwrap();
        app.doc.model.paste_entities(&boards, DVec3::Y * 0.8);
        let outer = app.doc.model.make_object(&BTreeSet::from([group]), false, "Outer").unwrap();
        let group = app.doc.model.parent(app.doc.model.connections[&joint].members[0]).unwrap();
        let board = app.doc.model.connections[&joint].members[0];
        let sibling_joint = *app.doc.model.connections.iter().find(|(_, connection)|
            app.doc.model.parent(connection.members[0]) == Some(sibling)).unwrap().0;
        let root_joint = *app.doc.model.connections.iter().find(|(_, connection)|
            app.doc.model.parent(connection.members[0]).is_none()).unwrap().0;
        let saved = app.doc.model.clone();
        let all = BTreeSet::from([joint, sibling_joint, root_joint]);
        app.doc.display.wood_joinery = true;
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 800.));
        app.doc.camera.fit(app.doc.model.bounds(), dv(rect.size()));

        for (context, visible) in [
            (None, all.clone()),
            (Some(outer), BTreeSet::new()),
            (Some(group), BTreeSet::from([joint])),
            (Some(board), BTreeSet::new()),
            (Some(sibling), BTreeSet::from([sibling_joint])),
            (None, all),
        ] {
            app.doc.model.edit_context = context;
            app.joinery_geometry.update(&app.doc.model);
            let evaluated = &app.joinery_geometry.result.as_ref().unwrap();
            assert!(evaluated.errors.is_empty());
            assert_eq!(app.joinery_geometry.regions.len(), visible.len());
            for (&id, connection) in &saved.connections {
                let pair = BTreeSet::from(connection.members);
                let stock = saved.copy_entities(&pair);
                let machined = evaluated.model.copy_entities(&pair);
                let stock_volume: f64 = connection.members.iter().map(|member|
                    stock.solid_volume(*member).unwrap()).sum();
                let visible_volume: f64 = connection.members.iter().map(|member|
                    machined.solid_volume(*member).unwrap()).sum();
                if visible.contains(&id) {
                    assert!(visible_volume < stock_volume - 1e-9,
                        "joint #{id} must cut its boards in context {context:?}");
                } else {
                    assert!((visible_volume - stock_volume).abs() < 1e-9,
                        "joint #{id} must show stock in context {context:?}");
                }
            }
            let markers: BTreeSet<_> = app.joint_markers(rect).into_iter().map(|(id, _)| id).collect();
            assert_eq!(markers, visible, "icons must match machining in context {context:?}");
            assert_eq!(app.doc.model.connections, saved.connections);
        }
        assert_eq!(app.doc.model, saved, "scoping must leave the saved model intact");
    }

    #[test]
    fn object_editing_restores_stock_topology_and_suspends_its_guides() {
        let (mut app, ctx, members) = fixture();
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        let connections = app.doc.model.connections.clone();
        let mut cache = GeometryCache::default();
        cache.update(&app.doc.model);
        let cut_vertices = cache.result.as_ref().unwrap().model.vertices.clone();
        assert_ne!(cut_vertices, app.doc.model.vertices);
        for member in members {
            assert!(app.doc.model.enter_object(member));
            cache.update(&app.doc.model);
            let visible = &cache.result.as_ref().unwrap().model;
            assert_eq!(visible.vertices, app.doc.model.vertices);
            assert_eq!(visible.edges, app.doc.model.edges);
            assert_eq!(visible.faces, app.doc.model.faces);
            assert!(cache.regions.is_empty());
            assert_eq!(app.doc.model.connections, connections);
            app.doc.model.edit_context = None;
            cache.update(&app.doc.model);
            assert_eq!(cache.result.as_ref().unwrap().model.vertices, cut_vertices);
            assert!(!cache.regions.is_empty());
        }
    }

    #[test]
    fn escape_exits_joinery_without_leaving_group_or_changing_selection() {
        let (mut app, ctx, members) = fixture();
        let group = app.doc.model.make_object(&BTreeSet::from(members), true, "Assembly").unwrap();
        assert!(app.doc.model.enter_object(group));
        let members: [u64; 2] = app.doc.model.objects[&group].entities.iter()
            .copied().collect::<Vec<_>>().try_into().unwrap();
        app.selected = BTreeSet::from(members);
        app.open_joinery(&ctx);
        assert!(app.joinery.open);
        let model = app.doc.model.clone();
        let _ = ctx.run_test_ui(egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Escape, physical_key: None, pressed: true,
                repeat: false, modifiers: egui::Modifiers::NONE,
            }], ..Default::default()
        }, |ui| app.keyboard(ui.ctx()));
        assert!(!app.joinery.open);
        assert!(app.joinery_dialog.members.is_none());
        assert!(app.joinery_dialog.preview.is_none());
        assert_eq!(app.joinery_edit_members(), None);
        assert_eq!(app.selected, BTreeSet::from(members));
        assert_eq!(app.doc.model, model);
    }

    #[test]
    fn editing_colors_follow_part_order_and_end_when_sidebar_closes() {
        let (mut app, ctx, members) = fixture();
        assert_eq!(app.joinery_edit_members(), None);
        app.open_joinery(&ctx);
        assert_eq!(app.joinery_edit_members(), Some(members));
        app.joinery.reverse = true;
        assert_eq!(app.joinery_edit_members(), Some([members[1], members[0]]));
        app.joinery.open = false;
        assert_eq!(app.joinery_edit_members(), None);
    }

    #[test]
    fn creating_joint_saves_a_relation_and_leaves_source_boards_intact() {
        let (mut app, ctx, members) = fixture();
        let source = app.doc.model.clone();
        app.open_joinery(&ctx);
        app.refresh_joint_preview();
        assert!(app.apply_joint_preview(&ctx));
        assert!(app.joinery.open);
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
