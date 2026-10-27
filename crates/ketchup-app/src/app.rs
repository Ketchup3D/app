use crate::{
    icons,
    modal::{Modal, primary_button},
    renderer, storage,
    theme::*,
};
use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, RichText, Sense, Stroke, Vec2};
#[cfg(test)]
use ketchup_core::infer_move_with_snapping;
use ketchup_core::{
    Axis, AxisGuide, Camera, DVec2, DVec3, Document, History, Inference, Model, ObjectKind, Snap,
    Tool, selection,
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

#[cfg(not(target_arch = "wasm32"))]
#[path = "app_updates.rs"]
mod app_updates;
#[path = "context_menu.rs"]
mod context_menu;
#[path = "display_scene.rs"]
mod display_scene;
#[path = "drawing_inference.rs"]
mod drawing_inference;
#[path = "measurement_tools.rs"]
mod measurement_tools;
#[path = "move_tool.rs"]
mod move_tool;
#[path = "precision.rs"]
mod precision;
#[path = "projects.rs"]
mod projects;
#[path = "shortcuts.rs"]
mod shortcuts;
#[path = "sidebar.rs"]
mod sidebar;
#[path = "solid_tools.rs"]
mod solid_tools;
#[path = "text_tools.rs"]
mod text_tools;
use context_menu::ContextAction;
use move_tool::{CopyArray, MoveCopy};

enum GestureKind {
    Navigate { pivot: Option<DVec3> },
    DrawingNavigate,
    Select { additive: bool },
    Line,
    Shape,
    Freehand { paused: bool },
    Modeling,
    Solid,
    Measure,
    Paste,
    Erase { ids: BTreeSet<u64> },
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct GeometryClipboard {
    model: Model,
    anchor: DVec3,
}
struct PastePreview {
    clipboard: GeometryClipboard,
    point: DVec3,
}
struct Gesture {
    button: egui::PointerButton,
    start: Pos2,
    last: Pos2,
    kind: GestureKind,
}

struct Modeling {
    base: Model,
    entities: BTreeSet<u64>,
    face: Option<u64>,
    anchor: DVec3,
    pixel: Pos2,
    center: DVec3,
    direction: DVec3,
    guides: Vec<AxisGuide>,
    value: f64,
    copy: bool,
    preview: Option<Model>,
    move_snap: Option<ketchup_core::inference::MoveSnapCache>,
    move_preview: Option<ketchup_core::modeling::MovePreview>,
    preview_delta: Option<DVec3>,
    error: Option<&'static str>,
}

struct Drawing {
    origin: DVec3,
    surface_normal: DVec3,
    normal: DVec3,
    u: DVec3,
    v: DVec3,
    points: Vec<DVec3>,
}

impl Drawing {
    fn set_normal(&mut self, normal: DVec3) {
        let u = (DVec3::X - normal * normal.x)
            .try_normalize()
            .unwrap_or(DVec3::Y);
        self.normal = normal;
        self.u = u;
        self.v = normal.cross(u);
    }

    fn set_lock(&mut self, lock: Option<Axis>) {
        let normal = lock.map(Axis::vector).unwrap_or(self.surface_normal);
        if normal == self.normal {
            return;
        }
        let (u, v) = (self.u, self.v);
        self.set_normal(normal);
        // Preserve a sampled stroke's size and shape when changing its plane.
        for point in &mut self.points {
            let delta = *point - self.origin;
            *point = self.origin + self.u * delta.dot(u) + self.v * delta.dot(v);
        }
    }

    fn align_to(&mut self, point: DVec3, eye: DVec3) {
        let delta = point - self.origin;
        if delta.dot(self.normal).abs() < 1e-6 {
            return;
        }
        let Some(direction) = delta.try_normalize() else {
            return;
        };
        let view = (eye - self.origin).normalize();
        // Prefer a world plane containing both points. For a vertical radius,
        // choose the plane that faces the camera rather than one seen edge-on.
        let normal = [DVec3::X, DVec3::Y, DVec3::Z]
            .into_iter()
            .filter(|normal| delta.dot(*normal).abs() < 1e-6)
            .max_by(|a, b| a.dot(view).abs().total_cmp(&b.dot(view).abs()))
            .or_else(|| {
                // A tilted target still needs an exact, planar shape. Keep the
                // normal as close to the current orientation as possible.
                let projected = self.normal - direction * self.normal.dot(direction);
                (projected.length_squared() > 1e-12).then(|| projected.normalize())
            })
            .or_else(|| {
                let projected = view - direction * view.dot(direction);
                (projected.length_squared() > 1e-12).then(|| projected.normalize())
            })
            .unwrap_or_else(|| direction.cross(DVec3::X).normalize());
        let normal = if normal.dot(self.normal) < 0. {
            -normal
        } else {
            normal
        };
        // Automatic inference changes the working plane, never earlier samples.
        self.set_normal(normal);
    }
}

struct ObjectCreation {
    component: bool,
    name: String,
}

pub struct KetchupApp {
    #[cfg(not(target_arch = "wasm32"))]
    updates: crate::updates::Updater,
    doc: Document,
    project: projects::Project,
    history: History,
    tool: Tool,
    drawing_tool: Tool,
    measurement_tool: Tool,
    solid_tool: Tool,
    solid_first: Option<u64>,
    flip_hover: Option<Axis>,
    drawing: Option<Drawing>,
    polygon_sides: usize,
    selected: BTreeSet<u64>,
    group_emphasis: bool,
    hover: Option<u64>,
    outliner_hover: BTreeSet<u64>,
    outliner_anchor: Option<u64>,
    outliner_filter: String,
    focus_outliner_search: bool,
    line_start: Option<DVec3>,
    snap: Option<Snap>,
    drawing_reference: Option<drawing_inference::DrawingReference>,
    drawing_guide: Option<AxisGuide>,
    drawing_axis_guides: Vec<AxisGuide>,
    drawing_projection: Option<(DVec3, DVec3)>,
    lock: Option<Axis>,
    length_input: String,
    modeling: Option<Modeling>,
    move_copy: bool,
    move_modifier_down: bool,
    last_move_copy: Option<MoveCopy>,
    measurement: Option<measurement_tools::Measurement>,
    text_editor: Option<text_tools::TextEditor>,
    paint_color: [u8; 3],
    gesture: Option<Gesture>,
    drawing_orbit_down: bool,
    drawing_orbit_pivot: Option<DVec3>,
    viewport_pointer: Option<Pos2>,
    viewport: Rect,
    inspector: bool,
    inspector_width: f32,
    inspector_preferences: sidebar::InspectorPreferences,
    inspector_order: [sidebar::InspectorSection; 6],
    inspector_layout: sidebar::InspectorLayout,
    component_filter: String,
    new_tag_name: String,
    tag_rename: Option<(u64, String)>,
    help: bool,
    about: bool,
    release_notes: crate::release_notes::ReleaseNotes,
    shortcut_filter: String,
    notice: Option<(String, f64)>,
    dirty_since: Option<f64>,
    save_error: Option<String>,
    storage_recovery: bool,
    gpu_ready: bool,
    entity_cache: renderer::EntityCache,
    face_cache: renderer::FaceCache,
    scene_cache: renderer::SceneCache,
    shadow_cache: crate::shadows::ShadowCache,
    picking_cache: crate::picking::PickingCache,
    #[cfg(test)]
    picking_preparations: usize,
    paste_face_cache: renderer::FaceCache,
    clipboard: Option<GeometryClipboard>,
    paste_preview: Option<PastePreview>,
    context_position: Option<Pos2>,
    object_creation: Option<ObjectCreation>,
    object_rename: Option<(u64, String)>,
}

impl KetchupApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply(&cc.egui_ctx);
        restore_preference(&cc.egui_ctx, cc.storage);
        let (doc, error) = match storage::load() {
            Ok(doc) => (doc.unwrap_or_default(), None),
            Err(e) => (Document::default(), Some(e)),
        };
        let mut app = Self::with_document(doc, error, renderer::install(cc));
        app.restore_inspector_preferences(&cc.egui_ctx, cc.storage);
        app.release_notes.open_if_unread();
        #[cfg(not(target_arch = "wasm32"))]
        let app = {
            let mut app = app;
            app.updates = crate::updates::Updater::new(&cc.egui_ctx);
            match crate::updates::Resume::take(&app.doc) {
                Ok(Some(resume)) => {
                    app.project.path = resume.path;
                    app.project.modified = resume.modified;
                }
                Ok(None) => {}
                Err(error) => {
                    app.updates.error = Some(format!(
                        "Could not restore the project file location after updating: {error}"
                    ))
                }
            }
            app
        };
        app
    }
    fn with_document(doc: Document, error: Option<String>, gpu_ready: bool) -> Self {
        let recovery = error.is_some();
        let modified = doc.model != Model::default()
            || doc.name != Document::default().name
            || doc.camera != Camera::default()
            || !doc.snap_to_geometry
            || !doc.snap_to_axes
            || doc.display != ketchup_core::DisplaySettings::default();
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            updates: crate::updates::Updater::default(),
            doc,
            project: projects::Project {
                modified,
                ..Default::default()
            },
            history: History::default(),
            tool: Tool::Select,
            drawing_tool: Tool::Line,
            measurement_tool: Tool::Dimensions,
            solid_tool: Tool::Union,
            solid_first: None,
            flip_hover: None,
            drawing: None,
            polygon_sides: 6,
            selected: BTreeSet::new(),
            group_emphasis: true,
            hover: None,
            outliner_hover: BTreeSet::new(),
            outliner_anchor: None,
            outliner_filter: String::new(),
            focus_outliner_search: false,
            line_start: None,
            snap: None,
            drawing_reference: None,
            drawing_guide: None,
            drawing_axis_guides: vec![],
            drawing_projection: None,
            lock: None,
            length_input: String::new(),
            modeling: None,
            move_copy: false,
            move_modifier_down: false,
            last_move_copy: None,
            measurement: None,
            text_editor: None,
            paint_color: [195, 96, 67],
            gesture: None,
            drawing_orbit_down: false,
            drawing_orbit_pivot: None,
            viewport_pointer: None,
            viewport: Rect::NOTHING,
            inspector: true,
            inspector_width: 300.,
            inspector_preferences: sidebar::InspectorPreferences::default(),
            inspector_order: sidebar::InspectorSection::ALL,
            inspector_layout: sidebar::InspectorLayout::default(),
            component_filter: String::new(),
            new_tag_name: String::new(),
            tag_rename: None,
            help: false,
            about: false,
            release_notes: crate::release_notes::ReleaseNotes::default(),
            shortcut_filter: String::new(),
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
            entity_cache: renderer::EntityCache::default(),
            face_cache: renderer::FaceCache::default(),
            scene_cache: renderer::SceneCache::default(),
            shadow_cache: crate::shadows::ShadowCache::default(),
            picking_cache: crate::picking::PickingCache::default(),
            #[cfg(test)]
            picking_preparations: 0,
            paste_face_cache: renderer::FaceCache::default(),
            clipboard: None,
            paste_preview: None,
            context_position: None,
            object_creation: None,
            object_rename: None,
        }
    }
    fn changed(&mut self, ctx: &egui::Context) {
        self.project.modified = true;
        self.dirty_since = Some(ctx.input(|i| i.time));
        ctx.request_repaint_after(Duration::from_millis(550));
    }
    fn say(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.notice = Some((text.into(), ctx.input(|i| i.time) + 5.));
        ctx.request_repaint_after(Duration::from_secs(5));
    }
    fn set_tool(&mut self, tool: Tool) {
        self.drawing_reference = None;
        self.drawing_guide = None;
        self.drawing_axis_guides.clear();
        self.drawing_projection = None;
        self.drawing_orbit_down = false;
        self.drawing_orbit_pivot = None;
        self.hover = None;
        self.solid_first = None;
        self.flip_hover = None;
        if tool.is_solid() {
            self.solid_tool = tool;
        }
        self.move_copy = false;
        self.last_move_copy = None;
        self.measurement = None;
        self.text_editor = None;
        self.tool = tool;
        if tool.is_drawing() {
            self.drawing_tool = tool;
        }
        if tool.is_measuring() {
            self.measurement_tool = tool;
        }
        self.drawing = None;
        if tool == Tool::Paint {
            self.inspector = true;
        }
        self.modeling = None;
        self.line_start = None;
        self.snap = None;
        self.lock = None;
        self.length_input.clear();
        self.gesture = None;
        self.paste_preview = None;
    }
    fn enter_object(&mut self, id: u64, ctx: &egui::Context) {
        if self.doc.model.enter_object(id) {
            self.outliner_anchor = None;
            self.set_tool(Tool::Select);
            self.selected.clear();
            self.hover = None;
            self.notice = None;
            ctx.request_repaint();
        } else if self.doc.model.is_locked(id) {
            self.say(ctx, "Unlock the object before opening it");
        }
    }
    fn navigate_context(&mut self, context: Option<u64>, ctx: &egui::Context) {
        self.outliner_anchor = None;
        self.doc.model.edit_context = context;
        self.set_tool(Tool::Select);
        self.selected.clear();
        self.hover = None;
        self.notice = None;
        ctx.request_repaint();
    }
    fn close_edit_context(&mut self, ctx: &egui::Context) {
        let parent = self
            .doc
            .model
            .edit_context
            .and_then(|id| self.doc.model.parent(id));
        self.navigate_context(parent, ctx);
    }
    fn finish_context_state_edit(&mut self, label: &str, before: Model, ctx: &egui::Context) {
        if let Err(message) = self.doc.model.synchronize_edit_context() {
            self.doc.model = before;
            self.say(ctx, message);
        } else {
            self.finish_edit(label, before, ctx);
        }
    }
    fn finish_edit(&mut self, label: &str, before: Model, ctx: &egui::Context) {
        if before != self.doc.model {
            self.solid_first = None;
            self.last_move_copy = None;
            self.measurement = None;
            self.modeling = None;
            self.history.record(label, before, &self.doc.model);
            self.selected.retain(|id| {
                self.doc.model.entity_in_context(*id) == Some(*id) && !self.doc.model.is_hidden(*id)
            });
            self.changed(ctx);
            #[cfg(target_arch = "wasm32")]
            self.persist();
        }
    }
    fn undo(&mut self, ctx: &egui::Context, redo: bool) {
        self.drawing_reference = None;
        self.drawing_guide = None;
        self.drawing_axis_guides.clear();
        self.drawing_projection = None;
        self.solid_first = None;
        self.flip_hover = None;
        self.last_move_copy = None;
        self.measurement = None;
        self.drawing = None;
        self.modeling = None;
        self.gesture = None;
        self.paste_preview = None;
        self.line_start = None;
        self.snap = None;
        self.length_input.clear();
        let path = self.doc.model.edit_path();
        let did = if redo {
            self.history.redo(&mut self.doc.model)
        } else {
            self.history.undo(&mut self.doc.model)
        };
        if did {
            self.outliner_anchor = None;
            self.doc.model.edit_context = path.into_iter().rev().find(|id| {
                self.doc.model.objects.contains_key(id)
                    && !self.doc.model.is_locked(*id)
                    && !self.doc.model.is_hidden(*id)
            });
            self.selected.clear();
            self.changed(ctx);
        }
    }
    fn erase_selected(&mut self, ctx: &egui::Context) {
        let before = self.doc.model.clone();
        self.doc.model.erase(&self.selected);
        self.finish_edit("Erase selection", before, ctx);
    }
    fn copy_selection(&mut self, ctx: &egui::Context) {
        if self.selected.is_empty() {
            return;
        }
        let model = self.doc.model.copy_entities(&self.selected);
        let Some((lo, hi)) = model.bounds() else {
            return;
        };
        let clipboard = GeometryClipboard {
            model,
            anchor: (lo + hi) * 0.5,
        };
        if let Ok(json) = serde_json::to_string(&clipboard) {
            ctx.copy_text(format!("ketchup-geometry-v1\n{json}"));
        }
        self.clipboard = Some(clipboard);
    }
    fn create_object(&mut self, ctx: &egui::Context, component: bool, name: &str) {
        let before = self.doc.model.clone();
        if let Some(id) = self.doc.model.make_object(&self.selected, component, name) {
            self.selected = BTreeSet::from([id]);
            self.set_tool(Tool::Select);
            self.finish_edit(
                if component {
                    "Create component"
                } else {
                    "Make group"
                },
                before,
                ctx,
            );
        }
    }
    fn paste_at(&mut self, ctx: &egui::Context, clipboard: &GeometryClipboard, offset: DVec3) {
        let before = self.doc.model.clone();
        let selected = self.doc.model.paste_entities(&clipboard.model, offset);
        if selected.is_empty() {
            self.say(
                ctx,
                "Cannot paste this geometry here; a component cannot contain its own definition",
            );
            return;
        }
        self.selected = selected;
        self.finish_edit("Paste", before, ctx);
    }
    fn request_name_input_focus(ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|data| data.insert_temp(id.with("focus_name"), true));
    }
    fn focus_name_input(ui: &mut egui::Ui, name: &str, id: egui::Id) {
        // A new window first runs an invisible sizing pass. Wait until the field
        // is visible so that pass cannot discard the initial keyboard focus.
        if ui.is_visible()
            && ui.ctx().data_mut(|data| {
                data.remove_temp::<bool>(id.with("focus_name"))
                    .unwrap_or(false)
            })
        {
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all_str(name)));
            state.store(ui.ctx(), id);
            ui.memory_mut(|memory| memory.request_focus(id));
        }
    }
    fn name_input(ui: &mut egui::Ui, name: &mut String, id: egui::Id) -> egui::Response {
        Self::focus_name_input(ui, name, id);
        ui.add(
            egui::TextEdit::singleline(name)
                .id(id)
                .desired_width(ui.available_width()),
        )
    }
    fn begin_object_creation(&mut self, ctx: &egui::Context, component: bool) {
        if self.selected.is_empty() || self.doc.model.selection_protected(&self.selected) {
            return;
        }
        let name = if component { "Component" } else { "Group" };
        self.object_creation = Some(ObjectCreation {
            component,
            name: name.into(),
        });
        self.gesture = None;
        Self::request_name_input_focus(ctx, egui::Id::unique("create_object_name"));
    }
    fn object_creation_window(&mut self, ctx: &egui::Context) {
        let Some(ObjectCreation {
            component,
            mut name,
        }) = self.object_creation.take()
        else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        let response = Modal::new("create_object", 360.).show(ctx, |modal| {
            modal.header(|ui| {
                ui.heading(if component {
                    "Create Component"
                } else {
                    "Make Group"
                });
                ui.weak("Give your selection a name.");
            });
            modal.body(|ui| {
                ui.label("Name");
                let input = Self::name_input(ui, &mut name, egui::Id::unique("create_object_name"));
                create = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if name.len() > 256 {
                    name.truncate(name.floor_char_boundary(256));
                }
            });
            modal.footer(|ui| {
                cancel = ui.button("Cancel").clicked();
                let valid = !name.trim().is_empty();
                create = (primary_button(ui, "Create", valid).clicked() || create) && valid;
            });
        });
        if create {
            self.create_object(ctx, component, &name);
        } else if !cancel && !response.should_close() {
            self.object_creation = Some(ObjectCreation { component, name });
        }
    }
    fn new_model(&mut self, ctx: &egui::Context) {
        let doc = Document {
            display: self.doc.display.clone(),
            camera: Camera {
                projection: self.doc.camera.projection,
                fov: self.doc.camera.fov,
                yaw: self.doc.camera.yaw,
                pitch: self.doc.camera.pitch,
                ..Camera::default()
            },
            ..Document::default()
        };
        self.replace_project(doc, None, ctx);
        self.say(ctx, "New project ready");
    }
    fn fit(&mut self, ctx: &egui::Context) {
        self.doc.camera.fit(
            self.doc.model.bounds(),
            dv(self.viewport.size().max(Vec2::splat(1.))),
        );
        self.changed(ctx);
    }
    fn fit_origin(&mut self, ctx: &egui::Context) {
        self.doc.camera.target = DVec3::ZERO;
        self.changed(ctx);
    }
    fn commit_line(&mut self, ctx: &egui::Context, precise: bool) {
        self.refresh_snap(ctx);
        let Some(start) = self.line_start else {
            return;
        };
        let Some(snap) = self.snap else {
            self.say(ctx, "Move over the drawing plane to set a direction");
            return;
        };
        let mut end = snap.point;
        if precise && !self.length_input.trim().is_empty() {
            match precision::positive_length(&self.length_input) {
                Ok(length) => {
                    let Some(direction) = (end - start).try_normalize() else {
                        self.say(ctx, "Move the pointer to choose a direction first");
                        return;
                    };
                    end = start + direction * length;
                }
                Err(message) => {
                    self.say(ctx, message);
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
                self.refresh_snap(ctx);
            }
            Err(message) => self.say(ctx, message),
        }
    }
    fn begin_drawing(&mut self, pos: Pos2) {
        self.update_pointer(pos);
        let Some(snap) = self.snap else { return };
        let ray = self
            .doc
            .camera
            .ray(dv(pos - self.viewport.min), dv(self.viewport.size()));
        let normal = self
            .doc
            .model
            .nearest_face(ray)
            .and_then(|(id, _, _)| {
                ketchup_core::faces::plane(&self.doc.model, &self.doc.model.faces[&id].vertices)
            })
            .filter(|plane| (snap.point - plane.origin).dot(plane.normal).abs() < 1e-6)
            .map_or(DVec3::Z, |plane| plane.normal);
        let u = (DVec3::X - normal * normal.x)
            .try_normalize()
            .unwrap_or(DVec3::Y);
        let mut drawing = Drawing {
            origin: snap.point,
            surface_normal: normal,
            normal,
            u,
            v: normal.cross(u),
            points: vec![snap.point],
        };
        drawing.set_lock(self.lock);
        self.drawing = Some(drawing);
        self.update_pointer(pos);
    }
    fn set_axis_lock(&mut self, lock: Option<Axis>, ctx: &egui::Context) {
        self.lock = lock;
        if let Some(drawing) = &mut self.drawing {
            drawing.set_lock(lock);
            self.snap = None;
            self.drawing_guide = None;
            self.drawing_axis_guides.clear();
            self.drawing_projection = None;
        }
        if let Some(pos) = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|pos| self.viewport.contains(*pos))
        {
            self.update_pointer(pos);
        }
    }
    fn drawing_snap(&self, pos: Pos2) -> Option<Snap> {
        let drawing = self.drawing.as_ref()?;
        let size = dv(self.viewport.size());
        let pixel = dv(pos - self.viewport.min);
        // Unlocked shapes can acquire targets on any axis; their working plane
        // follows the target. Explicit plane locks reject incompatible targets.
        let inferred = self.infer_snap(
            pos,
            (self.lock.is_none()).then_some(drawing.origin),
            None,
            self.lock.is_none() && self.doc.snap_to_axes,
        );
        if let Some(snap) = inferred.filter(|snap| {
            snap.kind != Inference::Plane
                && (self.lock.is_none()
                    || (snap.point - drawing.origin).dot(drawing.normal).abs() < 1e-6)
        }) {
            return Some(snap);
        }
        let ray = self.doc.camera.ray(pixel, size);
        let denominator = ray.direction.dot(drawing.normal);
        if denominator.abs() < 1e-8 {
            return None;
        }
        let t = (drawing.origin - ray.origin).dot(drawing.normal) / denominator;
        if !(0.0..1e7).contains(&t) {
            return None;
        }
        let point = ray.origin + ray.direction * t;
        if self.tool == Tool::Freehand
            && self.doc.snap_to_geometry
            && drawing.points.len() > 2
            && self
                .doc
                .camera
                .project(drawing.origin, size)
                .is_some_and(|p| p.distance(pixel) < 9.)
        {
            return Some(Snap {
                point: drawing.origin,
                kind: Inference::Endpoint,
            });
        }
        Some(Snap {
            point,
            kind: Inference::Plane,
        })
    }
    fn align_drawing_plane(&mut self) {
        if self.lock.is_none()
            && let Some(snap) = self.snap.filter(|snap| snap.kind != Inference::Plane)
            && let Some(drawing) = &mut self.drawing
        {
            drawing.align_to(snap.point, self.doc.camera.eye());
        }
    }
    fn drawing_points(&self) -> Vec<DVec3> {
        self.precise_drawing_points()
            .unwrap_or_else(|_| self.raw_drawing_points())
    }
    fn raw_drawing_points(&self) -> Vec<DVec3> {
        let Some(drawing) = &self.drawing else {
            return vec![];
        };
        let Some(snap) = self.snap else {
            return drawing.points.clone();
        };
        let origin = drawing.origin;
        let delta = snap.point - origin;
        match self.tool {
            Tool::Rectangle => {
                let a = drawing.u * delta.dot(drawing.u);
                let b = drawing.v * delta.dot(drawing.v);
                vec![origin, origin + a, origin + a + b, origin + b, origin]
            }
            Tool::Circle | Tool::Polygon => {
                let sides = if self.tool == Tool::Circle {
                    48
                } else {
                    self.polygon_sides
                };
                let tangent = drawing.normal.cross(delta);
                (0..=sides)
                    .map(|i| {
                        let angle = (i % sides) as f64 * std::f64::consts::TAU / sides as f64;
                        origin + delta * angle.cos() + tangent * angle.sin()
                    })
                    .collect()
            }
            Tool::Freehand => {
                let mut points = drawing.points.clone();
                if points.last().is_none_or(|p| p.distance(snap.point) > 1e-7) {
                    points.push(snap.point);
                }
                points
            }
            _ => vec![],
        }
    }
    fn sample_freehand(&mut self, pos: Pos2) {
        if !self.viewport.contains(pos) {
            return;
        }
        self.update_pointer(pos);
        if let (Some(drawing), Some(snap)) = (&mut self.drawing, self.snap) {
            let last = *drawing.points.last().unwrap();
            let far_enough = self
                .doc
                .camera
                .project(last, dv(self.viewport.size()))
                .is_some_and(|p| p.distance(dv(pos - self.viewport.min)) >= 4.);
            if far_enough && last.distance(snap.point) > 1e-7 {
                drawing.points.push(snap.point);
            }
        }
    }
    fn commit_drawing(&mut self, ctx: &egui::Context) {
        if self.tool != Tool::Freehand {
            if let Some(pos) = ctx
                .input(|i| i.pointer.hover_pos())
                .filter(|p| self.viewport.contains(*p))
            {
                self.update_pointer(pos);
            }
        }
        let points = match self.precise_drawing_points() {
            Ok(points) => points,
            Err(message) => {
                self.say(ctx, message);
                return;
            }
        };
        let flat_rectangle = self.tool == Tool::Rectangle
            && self.length_input.trim().is_empty()
            && points.windows(2).any(|pair| {
                self.doc
                    .camera
                    .segment(pair[0], pair[1], dv(self.viewport.size()))
                    .is_some_and(|(a, b)| a.distance(b) < 1.)
            });
        if points.len() < 2
            || flat_rectangle
            || points.windows(2).any(|p| p[0].distance(p[1]) < 1e-7)
        {
            if self.tool == Tool::Freehand {
                self.drawing = None;
            } else {
                self.say(ctx, "Move the pointer to give the shape some size");
            }
            return;
        }
        let before = self.doc.model.clone();
        let mut model = before.clone();
        for pair in points.windows(2) {
            match model.add_edge(pair[0], pair[1]) {
                Ok(_) | Err("That edge already exists") => {}
                Err(message) => {
                    self.say(ctx, message);
                    return;
                }
            }
        }
        self.doc.model = model;
        self.finish_edit(
            &format!("Draw {}", self.tool.label().to_lowercase()),
            before,
            ctx,
        );
        self.drawing = None;
        self.snap = None;
        self.gesture = None;
        self.length_input.clear();
    }
    fn begin_modeling(&mut self, pos: Pos2, ctx: &egui::Context) {
        self.last_move_copy = None;
        let size = dv(self.viewport.size());
        let pixel = dv(pos - self.viewport.min);
        let ray = self.doc.camera.ray(pixel, size);
        if self.tool == Tool::Paint {
            if let Some((face, _, _)) = self.doc.model.nearest_context_face(ray) {
                let before = self.doc.model.clone();
                match self.doc.model.paint_face(face, self.paint_color) {
                    Ok(()) => self.finish_edit("Paint face", before, ctx),
                    Err(message) => self.say(ctx, message),
                }
            } else {
                self.say(ctx, "Click a filled face to paint it");
            }
            return;
        }
        let mut face_id = None;
        let (anchor, direction) = if matches!(self.tool, Tool::PushPull | Tool::Offset) {
            let Some((id, _, point)) = self.doc.model.nearest_editable_face(ray) else {
                self.say(ctx, "Click a filled face first");
                return;
            };
            if self.doc.model.is_locked(id) {
                self.say(ctx, "Unlock the face before editing it");
                return;
            }
            face_id = Some(id);
            let face = &self.doc.model.faces[&id];
            let plane = ketchup_core::faces::plane(&self.doc.model, &face.vertices).unwrap();
            let direction = if self.tool == Tool::PushPull {
                plane.normal
                    * if plane.normal.dot(self.doc.camera.eye() - point) >= 0. {
                        1.
                    } else {
                        -1.
                    }
            } else {
                let closest = std::iter::once(&face.vertices)
                    .chain(face.holes.iter())
                    .flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()])))
                    .map(|(a, b)| {
                        let a = self.doc.model.vertices[&a].position;
                        let b = self.doc.model.vertices[&b].position;
                        a + (b - a)
                            * ((point - a).dot(b - a) / (b - a).length_squared()).clamp(0., 1.)
                    })
                    .min_by(|a, b| {
                        a.distance_squared(point)
                            .total_cmp(&b.distance_squared(point))
                    })
                    .unwrap();
                (point - closest).try_normalize().unwrap_or(plane.u)
            };
            self.selected = BTreeSet::from([id]);
            (point, direction)
        } else {
            if self.selected.is_empty() {
                if let Some(id) = self.pick_at(pixel, size, true) {
                    self.selected.insert(id);
                }
            }
            if self.selected.is_empty() {
                self.say(ctx, "Select or click an edge or face first");
                return;
            }
            if self.doc.model.selection_locked(&self.selected) {
                self.say(ctx, "Unlock the selection before editing it");
                return;
            }
            self.prepare_picking();
            let Some(snap) = self.infer_snap(pos, None, None, self.doc.snap_to_axes) else {
                return;
            };
            (snap.point, DVec3::ZERO)
        };
        let center = self
            .doc
            .model
            .selection_center(&self.selected)
            .unwrap_or(anchor);
        self.modeling = Some(Modeling {
            base: self.doc.model.clone(),
            entities: self.selected.clone(),
            face: face_id,
            anchor,
            pixel: pos,
            center,
            direction,
            guides: vec![],
            value: if self.tool == Tool::Scale { 1. } else { 0. },
            copy: self.tool == Tool::Move && self.move_copy,
            preview: None,
            move_snap: (self.tool == Tool::Move)
                .then(|| ketchup_core::inference::MoveSnapCache::new(&self.doc.model)),
            move_preview: (self.tool == Tool::Move)
                .then(|| {
                    ketchup_core::modeling::MovePreview::new(&self.doc.model, &self.selected).ok()
                })
                .flatten(),
            preview_delta: None,
            error: None,
        });
        self.length_input.clear();
    }

    fn update_modeling(&mut self, pos: Pos2) {
        let Some(edit) = self.modeling.as_mut() else {
            return;
        };
        let size = dv(self.viewport.size());
        let ray = self.doc.camera.ray(dv(pos - self.viewport.min), size);
        edit.value = match self.tool {
            Tool::PushPull => ray
                .axis(edit.anchor, edit.direction)
                .map(|p| (p - edit.anchor).dot(edit.direction))
                .unwrap_or((edit.pixel.y - pos.y) as f64 * self.doc.camera.distance / size.y),
            Tool::Offset => {
                let face = &edit.base.faces[&edit.face.unwrap()];
                let plane = ketchup_core::faces::plane(&edit.base, &face.vertices).unwrap();
                let denominator = ray.direction.dot(plane.normal);
                if denominator.abs() > 1e-8 {
                    let t = (edit.anchor - ray.origin).dot(plane.normal) / denominator;
                    (ray.origin + ray.direction * t - edit.anchor).dot(edit.direction)
                } else {
                    edit.value
                }
            }
            Tool::Scale => {
                let center = self
                    .doc
                    .camera
                    .project(edit.center, size)
                    .map(|p| self.viewport.min + pv(p))
                    .unwrap_or(edit.pixel);
                let reference = center.distance(edit.pixel);
                if reference < 12. {
                    ((edit.pixel.y - pos.y) as f64 / 100.).exp()
                } else {
                    (center.distance(pos) / reference) as f64
                }
            }
            Tool::Move => {
                edit.guides.clear();
                if let Some(inference) = edit.move_snap.as_mut().unwrap().infer(
                    &self.doc.camera,
                    size,
                    dv(pos - self.viewport.min),
                    edit.anchor,
                    self.lock,
                    self.doc.snap_to_geometry,
                    self.doc.snap_to_axes,
                ) {
                    edit.direction = inference.snap.point - edit.anchor;
                    edit.guides = inference.guides;
                }
                edit.direction.length()
            }
            _ => return,
        };
        let value = precision::modeling_value(self.tool, &self.length_input).unwrap_or(edit.value);
        if matches!(self.tool, Tool::PushPull | Tool::Offset)
            && value.abs() < 1e-7
            && self.length_input.trim().is_empty()
        {
            edit.preview = None;
            edit.error = None;
            return;
        }
        if self.tool == Tool::Move && !edit.copy {
            let delta = if value.abs() < 1e-7 {
                Some(DVec3::ZERO)
            } else {
                edit.direction
                    .try_normalize()
                    .map(|direction| direction * value)
            };
            if let (Some(plan), Some(delta)) = (&edit.move_preview, delta) {
                if edit.preview_delta == Some(delta) {
                    return;
                }
                let mut preview = edit.preview.take().unwrap_or_else(|| edit.base.clone());
                match plan.update(&mut preview, delta) {
                    Ok(()) => {
                        edit.preview = Some(preview);
                        edit.error = None;
                    }
                    Err(error) => {
                        edit.error = Some(error);
                    }
                }
                edit.preview_delta = Some(delta);
                return;
            }
        }
        match Self::modeling_result(self.tool, edit, value) {
            Ok(model) => {
                edit.preview = Some(model);
                edit.error = None;
            }
            Err(error) => {
                edit.preview = None;
                edit.error = Some(error);
            }
        }
    }

    fn modeling_result(tool: Tool, edit: &Modeling, value: f64) -> Result<Model, &'static str> {
        let mut model = edit.base.clone();
        match tool {
            Tool::PushPull => {
                let face = &model.faces[&edit.face.unwrap()];
                let normal = ketchup_core::faces::plane(&model, &face.vertices)
                    .unwrap()
                    .normal;
                model.push_pull(face.id, value * normal.dot(edit.direction))?;
            }
            Tool::Offset => model.offset_face(edit.face.unwrap(), value)?,
            Tool::Scale => model.scale_selection(&edit.entities, edit.center, value)?,
            Tool::Move => {
                let delta = Self::move_delta(edit, value)?;
                if edit.copy {
                    let source = edit.base.copy_entities(&edit.entities);
                    return Self::move_copies(&edit.base, &source, delta, CopyArray::default())
                        .map(|(model, _)| model);
                }
                model.move_selection(&edit.entities, delta)?;
            }
            _ => return Err("Choose a modeling tool"),
        }
        Ok(model)
    }

    fn commit_modeling(&mut self, ctx: &egui::Context) {
        if let Some(pos) = ctx
            .input(|i| i.pointer.hover_pos())
            .filter(|p| self.viewport.contains(*p))
        {
            self.update_modeling(pos);
        }
        let Some(edit) = self.modeling.as_ref() else {
            return;
        };
        let value = if self.length_input.trim().is_empty() {
            edit.value
        } else {
            match precision::modeling_value(self.tool, &self.length_input) {
                Ok(value) => value,
                Err(message) => {
                    self.say(ctx, message);
                    return;
                }
            }
        };
        match Self::modeling_result(self.tool, edit, value) {
            Ok(model) => {
                let before = edit.base.clone();
                if self.tool == Tool::PushPull {
                    self.selected = model
                        .faces
                        .keys()
                        .filter(|id| {
                            !before.faces.contains_key(id)
                                && model.parent(**id) == model.edit_context
                        })
                        .next_back()
                        .copied()
                        .or(edit.face)
                        .into_iter()
                        .collect();
                }
                let copy = (self.tool == Tool::Move && edit.copy).then(|| MoveCopy {
                    base: before.clone(),
                    source: before.copy_entities(&edit.entities),
                    delta: Self::move_delta(edit, value).expect("validated move"),
                });
                if copy.is_some() {
                    self.selected = model
                        .selectable_entities()
                        .into_iter()
                        .filter(|id| !before.contains_entity(id))
                        .collect();
                }
                self.doc.model = model;
                self.modeling = None;
                self.length_input.clear();
                self.finish_edit(
                    if copy.is_some() {
                        "Move copy"
                    } else {
                        self.tool.label()
                    },
                    before,
                    ctx,
                );
                self.last_move_copy = copy;
            }
            Err(message) => self.say(ctx, message),
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.updates.open {
            return;
        }

        if self.project.blocked() {
            return;
        }
        if self.help || self.about || self.release_notes.open {
            let modifiers = ctx.input(|i| i.modifiers);
            self.move_modifier_down = if ctx.os() == egui::os::OperatingSystem::Mac {
                modifiers.alt
            } else {
                modifiers.ctrl
            };
            if (!self.release_notes.open || !egui::Popup::is_any_open(ctx))
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape))
            {
                self.help = false;
                self.about = false;
                self.release_notes.open = false;
                self.shortcut_filter.clear();
                ctx.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
            return;
        }
        if self.object_creation.is_none()
            && self.object_rename.is_none()
            && self.text_editor.is_none()
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, Key::F)
                    | i.consume_key(egui::Modifiers::CTRL, Key::F)
            })
        {
            self.request_outliner_search(ctx);
            return;
        }
        for (modifiers, key, action) in [
            (egui::Modifiers::COMMAND, Key::N, 0),
            (egui::Modifiers::COMMAND, Key::O, 1),
            (egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::S, 2),
            (egui::Modifiers::COMMAND, Key::S, 3),
        ] {
            if ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                match action {
                    0 => self.request_project(projects::Action::New, ctx),
                    1 => self.request_project(projects::Action::Open, ctx),
                    2 => self.save_project(ctx, true),
                    _ => self.save_project(ctx, false),
                }
                return;
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.drawing_reference = None;
            self.drawing_guide = None;
            self.drawing_axis_guides.clear();
            self.drawing_projection = None;
            let solid_operation = self.solid_first.take().is_some();
            self.flip_hover = None;
            let measurement_operation = self.measurement.take().is_some();
            let editing_operation = self.last_move_copy.is_some()
                || solid_operation
                || measurement_operation
                || self.drawing.is_some()
                || self.modeling.is_some()
                || self.gesture.is_some()
                || self.line_start.is_some()
                || self.paste_preview.is_some()
                || self.object_creation.is_some()
                || self.object_rename.is_some()
                || self.text_editor.is_some()
                || self.help
                || ctx.egui_wants_keyboard_input();
            if !editing_operation && self.doc.model.edit_context.is_some() {
                self.close_edit_context(ctx);
            }
            self.move_copy = false;
            self.last_move_copy = None;
            self.drawing = None;
            self.modeling = None;
            self.gesture = None;
            self.line_start = None;
            self.snap = None;
            self.lock = None;
            self.length_input.clear();
            self.notice = None;
            self.help = false;
            self.paste_preview = None;
            self.object_creation = None;
            self.object_rename = None;
            self.text_editor = None;
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        if ctx.egui_wants_keyboard_input()
            || self.help
            || self.object_creation.is_some()
            || self.object_rename.is_some()
            || self.text_editor.is_some()
        {
            let modifiers = ctx.input(|i| i.modifiers);
            self.move_modifier_down = if ctx.os() == egui::os::OperatingSystem::Mac {
                modifiers.alt
            } else {
                modifiers.ctrl
            };
            return;
        }
        let emphasis_tab = ctx.input_mut(|i| {
            let mut consumed = false;
            if i.focused {
                i.events.retain(|event| {
                    if let egui::Event::Key {
                        key: Key::Tab,
                        pressed: true,
                        repeat,
                        modifiers,
                        ..
                    } = event
                        && !modifiers.any()
                    {
                        consumed = true;
                        if !repeat {
                            self.group_emphasis = !self.group_emphasis;
                        }
                        false
                    } else {
                        true
                    }
                });
            }
            consumed
        });
        if emphasis_tab {
            // egui schedules Tab focus traversal before application shortcuts run.
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
            ctx.request_repaint();
        }
        let events = ctx.input(|i| i.events.clone());
        let copy_event = events.iter().any(|e| matches!(e, egui::Event::Copy));
        let cut_event = events.iter().any(|e| matches!(e, egui::Event::Cut));
        let paste_event = events.iter().any(|e| matches!(e, egui::Event::Paste(_)));
        let shortcuts = events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Copy
                    | egui::Event::Cut
                    | egui::Event::Paste(_)
                    | egui::Event::Key {
                        pressed: true,
                        modifiers: egui::Modifiers { ctrl: true, .. },
                        ..
                    }
                    | egui::Event::Key {
                        pressed: true,
                        modifiers: egui::Modifiers { mac_cmd: true, .. },
                        ..
                    }
            )
        });
        for event in events {
            match event {
                egui::Event::ModifiersChanged(modifiers) => {
                    self.move_modifier(ctx, modifiers, shortcuts)
                }
                egui::Event::Copy => self.context_action(ctx, ContextAction::Copy),
                egui::Event::Cut => self.context_action(ctx, ContextAction::Cut),
                egui::Event::Paste(text) => {
                    self.context_position = None;
                    if let Some(json) = text.strip_prefix("ketchup-geometry-v1\n") {
                        match serde_json::from_str::<GeometryClipboard>(json) {
                            Ok(clipboard)
                                if clipboard.anchor.is_finite()
                                    && clipboard.model.validate().is_ok() =>
                            {
                                self.clipboard = Some(clipboard)
                            }
                            _ => {
                                self.say(ctx, "The clipboard geometry could not be read");
                                continue;
                            }
                        }
                    }
                    self.context_action(
                        ctx,
                        if ctx.input(|i| i.modifiers.shift) {
                            ContextAction::PasteInPlace
                        } else {
                            ContextAction::Paste
                        },
                    );
                }
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
                            Key::A => self.selected = self.doc.model.selectable_entities(),
                            Key::C if !copy_event => self.context_action(ctx, ContextAction::Copy),
                            Key::X if !cut_event => self.context_action(ctx, ContextAction::Cut),
                            Key::V if !paste_event => {
                                self.context_position = None;
                                self.context_action(
                                    ctx,
                                    if modifiers.shift {
                                        ContextAction::PasteInPlace
                                    } else {
                                        ContextAction::Paste
                                    },
                                );
                            }
                            _ => {}
                        }
                        continue;
                    }
                    // Letter keys belong to an in-progress dimension, including
                    // unit suffixes. Space always returns the canvas to Select.
                    if self.precision_available()
                        && !self.length_input.is_empty()
                        && (Key::A..=Key::Z).contains(&key)
                    {
                        continue;
                    }
                    match key {
                        Key::Space => self.set_tool(Tool::Select),
                        Key::L => self.set_tool(Tool::Line),
                        Key::R => self.set_tool(Tool::Rectangle),
                        Key::C => self.set_tool(Tool::Circle),
                        Key::G => self.set_tool(Tool::Polygon),
                        Key::D => self.set_tool(Tool::Freehand),
                        Key::E => self.set_tool(Tool::Erase),
                        Key::O => self.set_tool(Tool::Orbit),
                        Key::P => self.set_tool(Tool::PushPull),
                        Key::S => self.set_tool(Tool::Scale),
                        Key::F => self.set_tool(Tool::Offset),
                        Key::M => self.set_tool(Tool::Move),
                        Key::B => self.set_tool(Tool::Paint),
                        Key::T => self.set_tool(Tool::TapeMeasure),
                        Key::A => self.set_tool(Tool::Protractor),
                        Key::I => self.set_tool(Tool::Dimensions),
                        Key::Delete => self.erase_selected(ctx),
                        Key::Backspace => {
                            if self.precision_available() {
                                self.length_input.pop();
                            } else {
                                self.erase_selected(ctx);
                            }
                        }
                        Key::Enter if self.tool == Tool::Line => self.commit_line(ctx, true),
                        Key::Enter if self.drawing.is_some() => self.commit_drawing(ctx),
                        Key::Enter if self.modeling.is_some() => self.commit_modeling(ctx),
                        Key::Enter if self.last_move_copy.is_some() => self.commit_move_array(ctx),
                        Key::ArrowRight | Key::ArrowLeft | Key::ArrowUp
                            if self.tool == Tool::Flip =>
                        {
                            self.apply_flip(
                                match key {
                                    Key::ArrowRight => Axis::X,
                                    Key::ArrowLeft => Axis::Y,
                                    _ => Axis::Z,
                                },
                                ctx,
                            );
                        }
                        Key::ArrowRight | Key::ArrowLeft | Key::ArrowUp
                            if self.tool.is_drawing()
                                || self.tool == Tool::Move
                                || self.tool.is_measuring() && self.tool != Tool::Text =>
                        {
                            let axis = match key {
                                Key::ArrowRight => Axis::X,
                                Key::ArrowLeft => Axis::Y,
                                _ => Axis::Z,
                            };
                            let lock = if self.lock == Some(axis) {
                                None
                            } else {
                                Some(axis)
                            };
                            self.set_axis_lock(lock, ctx);
                        }
                        Key::ArrowDown => self.set_axis_lock(None, ctx),
                        Key::F1 => self.help = true,
                        _ => {}
                    }
                }
                egui::Event::Text(text) if self.precision_available() => {
                    if text.chars().all(|c| {
                        c.is_ascii_alphanumeric()
                            || matches!(
                                c,
                                '.' | '-' | '+' | '*' | '/' | '(' | ')' | ',' | '\'' | '"' | ' '
                            )
                    }) && self.length_input.len() + text.len() <= precision::MAX_INPUT_LEN
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
            ui.painter().rect_filled(
                rect,
                5,
                if kind == "close" {
                    ACCENT
                } else {
                    ui.visuals().widgets.hovered.weak_bg_fill
                },
            );
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
    #[cfg(not(target_arch = "wasm32"))]
    fn window_controls(ui: &mut egui::Ui) {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        let color = ui.visuals().text_color();
        // The toolbar flows right to left, placing Close at the outer edge.
        if Self::icon_button(ui, "close", "Close", true, color) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if Self::icon_button(
            ui,
            if maximized { "restore" } else { "maximize" },
            if maximized { "Restore" } else { "Maximize" },
            true,
            color,
        ) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if Self::icon_button(ui, "minimize", "Minimize", true, color) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        ui.add_space(12.);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn window_resize_handles(ui: &mut egui::Ui) {
        use egui::{CursorIcon, ResizeDirection};

        if ui.input(|i| {
            i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
        }) {
            return;
        }
        let rect = ui.ctx().viewport_rect();
        // Removing native decorations also removes their resize grips on some platforms.
        // Keep thin edge grips and larger corner targets without taking space from the UI.
        for (x, y, direction, cursor) in [
            (-1, -1, ResizeDirection::NorthWest, CursorIcon::ResizeNwSe),
            (0, -1, ResizeDirection::North, CursorIcon::ResizeVertical),
            (1, -1, ResizeDirection::NorthEast, CursorIcon::ResizeNeSw),
            (-1, 0, ResizeDirection::West, CursorIcon::ResizeHorizontal),
            (1, 0, ResizeDirection::East, CursorIcon::ResizeHorizontal),
            (-1, 1, ResizeDirection::SouthWest, CursorIcon::ResizeNeSw),
            (0, 1, ResizeDirection::South, CursorIcon::ResizeVertical),
            (1, 1, ResizeDirection::SouthEast, CursorIcon::ResizeNwSe),
        ] {
            let thickness = if x != 0 && y != 0 { 12. } else { 4. };
            let range = |min: f32, max: f32, side| match side {
                -1 => (min, min + thickness),
                1 => (max - thickness, max),
                _ => (min + 12., max - 12.),
            };
            let (left, right) = range(rect.left(), rect.right(), x);
            let (top, bottom) = range(rect.top(), rect.bottom(), y);
            let response = ui
                .interact(
                    Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom)),
                    ui.scope_id().with(("window_resize", x, y)),
                    Sense::drag(),
                )
                .on_hover_cursor(cursor);
            if response.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
            }
        }
    }

    fn app_bar_menu(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
        // MenuConfig carries the same padding into nested submenus.
        egui::menu::MenuButton::new(title)
            .config(egui::menu::MenuConfig::new().style(Self::menu_style))
            .ui(ui, content);
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let dark = ui.visuals().dark_mode;
        let foreground = if dark { PAPER } else { colors.ink };
        let ctx = ui.ctx().clone();
        egui::Panel::top("top_bar")
            .exact_size(64.)
            .frame(
                egui::Frame::new()
                    .fill(if dark { colors.canvas } else { Color32::from_gray(235) })
                    .inner_margin(egui::Margin::symmetric(18, 12)),
            )
            .show(ui, |ui| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Register the background first so buttons and text fields own their input.
                    let title_bar = ui.interact(
                        ui.max_rect().expand2(egui::vec2(18., 12.)),
                        ui.scope_id().with("window_drag"),
                        Sense::click_and_drag(),
                    );
                    if title_bar.double_clicked() {
                        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    } else if title_bar.drag_started_by(egui::PointerButton::Primary) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }
                let compact = ui.available_width() < 1000.;
                ui.visuals_mut().override_text_color = Some(foreground);
                ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(1., foreground);
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().widgets.hovered.weak_bg_fill = if dark {
                    Color32::from_rgb(65, 77, 68)
                } else {
                    Color32::from_gray(215)
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    Self::window_controls(ui);
                    #[cfg(not(target_arch = "wasm32"))]
                    if self.updates.ready() && ui.button("Update ready").clicked() {
                        self.updates.open = true;
                    }
                    let (text, color) = if self.save_error.is_some() {
                        ("Unsaved", Color32::from_rgb(243, 177, 145))
                    } else if self.dirty_since.is_some() {
                        ("Saving…", Color32::from_rgb(190, 202, 188))
                    } else if self.storage_recovery {
                        ("New workspace", Color32::from_rgb(190, 202, 188))
                    } else if self.project.modified {
                        ("Modified · saved locally", Color32::from_rgb(190, 202, 188))
                    } else {
                        ("Saved locally", Color32::from_rgb(190, 202, 188))
                    };
                    let color = if dark {
                        color
                    } else if self.save_error.is_some() {
                        colors.accent
                    } else {
                        colors.muted
                    };
                    if !compact {
                        let saved = ui.label(RichText::new(text).size(12.).color(color));
                        if let Some(error) = &self.save_error {
                            saved.on_hover_text(error);
                        } else {
                            saved.on_hover_text(if self.project.modified {
                                "Autosaved on this device. Use File → Save to update your .ketchup file."
                            } else {
                                "Your model stays on this device"
                            });
                        }
                    }
                    ui.scope(|ui| {
                        dot(
                            ui,
                            if self.save_error.is_some() {
                                ACCENT
                            } else {
                                Color32::from_rgb(152, 183, 142)
                            },
                        );
                    })
                    .response
                    .on_hover_text(self.save_error.as_deref().unwrap_or(text));
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let (logo, _) =
                            ui.allocate_exact_size(egui::vec2(35., 35.), Sense::hover());
                        icons::logo(ui.painter(), logo);
                        ui.add_space(4.);
                        if !compact {
                            ui.label(RichText::new("KetchUp").size(23.).strong().color(foreground));
                        }
                        ui.add_space(18.);
                        Self::app_bar_menu(ui, "File", |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            for (label, modifiers, key, action) in [
                                ("New project", egui::Modifiers::COMMAND, Key::N, 0),
                                ("Open…", egui::Modifiers::COMMAND, Key::O, 1),
                                ("Save", egui::Modifiers::COMMAND, Key::S, 2),
                                ("Save As…", egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::S, 3),
                            ] {
                                let shortcut = ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key));
                                if ui.add(egui::Button::new(label).shortcut_text(shortcut)).clicked() {
                                    match action {
                                        0 => self.request_project(projects::Action::New, &ctx),
                                        1 => self.request_project(projects::Action::Open, &ctx),
                                        2 => self.save_project(&ctx, false),
                                        _ => self.save_project(&ctx, true),
                                    }
                                    ui.close();
                                }
                            }
                            ui.separator();
                            ui.menu_button("Import", |ui| {
                                if ui.button("Import SketchUp (.skp)…")
                                    .on_hover_text("Add SketchUp geometry as editable groups. Keeps placement, face holes, and solid colors. Textures and annotations are omitted. Maximum file size: 64 MiB.")
                                    .clicked() {
                                    self.import_model(&ctx);
                                    ui.close();
                                }
                            });
                            ui.menu_button("Export", |ui| {
                                if ui.button("Export Wavefront OBJ (.obj)…")
                                    .on_hover_text("Export visible edges and triangulated faces in metres, with Z up.")
                                    .clicked() {
                                    self.export_model(&ctx);
                                    ui.close();
                                }
                            });
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                ui.separator();
                                if ui.button("Quit").clicked() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    ui.close();
                                }
                            }
                        });
                        Self::app_bar_menu(ui, "Edit", |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            Self::configure_entity_menu(ui);
                            for (redo, action, label, modifiers) in [
                                (false, "Undo", self.history.undo_label().map(str::to_owned), egui::Modifiers::COMMAND),
                                (true, "Redo", self.history.redo_label().map(str::to_owned), egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
                            ] {
                                let enabled = label.is_some();
                                let label = label.map(|label| format!("{action} {label}")).unwrap_or_else(|| action.to_owned());
                                let shortcut = ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, Key::Z));
                                if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked() {
                                    self.undo(&ctx, redo);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            self.context_position = None;
                            self.entity_menu(ui, |_, _| {});
                        });
                        Self::app_bar_menu(ui, "Help", |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            if ui.button("Keyboard shortcuts").clicked() {
                                self.help = true;
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Report a bug").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab("https://github.com/Ketchup3D/app/issues"));
                                ui.close();
                            }
                            if ui.button("GitHub").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab("https://github.com/Ketchup3D/app"));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Download desktop app").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab("https://ketchup3d.github.io/app/"));
                                ui.close();
                            }
                            if ui.button("Release notes").clicked() {
                                self.release_notes.open();
                                ui.close();
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            if ui.button("Check for updates…").clicked() {
                                self.updates.open = true;
                                self.updates.check(&ctx, false);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("About KetchUp").clicked() {
                                self.about = true;
                                ui.close();
                            }
                        });
                        ui.add_space(18.);
                        ui.add(egui::Label::new(RichText::new(&self.doc.name).color(foreground)).truncate())
                            .on_hover_text(&self.doc.name);
                    });
                });
            });
    }
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let ctx = ui.ctx().clone();
        egui::Panel::bottom("status")
            .exact_size(42.)
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin::symmetric(16, 5)),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let drawing_orbit = self.drawing_orbit_modifier(ctx.input(|i| i.modifiers));
                    let navigating = self
                        .gesture
                        .as_ref()
                        .is_some_and(|g| matches!(g.kind, GestureKind::Navigate { .. }));
                    let message = if drawing_orbit {
                        "Move the cursor to orbit · Hold Shift to pan · Release Ctrl / ⌘ to resume drawing"
                    } else if navigating {
                        if ctx.input(|i| i.modifiers.shift) {
                            "Panning · Release Shift to orbit"
                        } else {
                            "Orbiting · Hold Shift to pan"
                        }
                    } else if self.paste_preview.is_some() {
                        "Click to place the copy · Escape to cancel"
                    } else if self.tool == Tool::Move {
                        if self.last_move_copy.is_some() {
                            "Copy placed · Type /3 to divide the span or 3x to repeat · Enter to apply"
                        } else {
                            match (ctx.os() == egui::os::OperatingSystem::Mac, self.move_copy) {
                                (true, true) => "Copy · Option toggles Move · Click to place · Type distance + Enter",
                                (true, false) => "Move · Option toggles Copy · Click base point and destination",
                                (false, true) => "Copy · Ctrl toggles Move · Click to place · Type distance + Enter",
                                (false, false) => "Move · Ctrl toggles Copy · Click base point and destination",
                            }
                        }
                    } else if self.tool.is_solid() {
                        self.solid_status()
                    } else if self.tool.is_measuring() {
                        self.measurement_status()
                    } else if self.tool == Tool::Line && self.line_start.is_some() {
                        "Click to finish · Type length + Enter · Esc to stop"
                    } else if self.drawing.is_some() && self.tool == Tool::Rectangle {
                        "Click the opposite corner · Type width,height (5m,50mm) + Enter"
                    } else if self.drawing.is_some()
                        && matches!(self.tool, Tool::Circle | Tool::Polygon)
                    {
                        "Click to set the radius · Type a radius (5cm) + Enter · Esc to cancel"
                    } else if self.drawing.is_some() && self.tool == Tool::Freehand {
                        "Release to finish · Type total length + Enter · Esc to cancel"
                    } else {
                        self.tool.instructions()
                    };
                    // Reserve the precision controls before fitting the status message.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.tool == Tool::Text { return; }
                        if self.tool.is_measuring() {
                            ui.label(
                                RichText::new(
                                    self.measurement_value().unwrap_or_else(|| "—".into()),
                                )
                                .monospace()
                                .strong(),
                            );
                            ui.label(
                                RichText::new(if self.tool == Tool::Protractor {
                                    "Angle"
                                } else {
                                    "Distance"
                                })
                                .size(12.)
                                .color(colors.muted),
                            );
                            return;
                        }
                        if self.tool == Tool::Scale {
                            ui.label(RichText::new("×").color(colors.muted));
                        }
                        let available = self.precision_available();
                        let hint = if let Some(edit) = &self.modeling {
                            if self.tool == Tool::Scale { format!("{:.3}", edit.value) } else { format!("{:.3} m", edit.value) }
                        } else if self.last_move_copy.is_some() {
                            "/3 or 3x".into()
                        } else if self.drawing.is_some() && self.tool == Tool::Rectangle {
                            "5m,50mm".into()
                        } else if self.drawing.is_some() {
                            "5cm".into()
                        } else if let (Some(a), Some(b)) = (self.line_start, self.snap) {
                            format!("{:.3} m", a.distance(b.point))
                        } else {
                            "0.000 m".into()
                        };
                        let field = ui.add_enabled(
                            available,
                            egui::TextEdit::singleline(&mut self.length_input)
                                .desired_width(if self.tool == Tool::Rectangle { 150. } else { 106. })
                                .font(egui::TextStyle::Monospace)
                                .hint_text(hint)
                                .char_limit(precision::MAX_INPUT_LEN)
                                .margin(egui::vec2(9., 6.)),
                        ).on_hover_text("Use lengths or formulas: 60cm - 89mm, (1m + 20cm) / 2. Operators: + - * / and parentheses. Plain numbers default to meters; multipliers are unitless. Units: mm, cm, m, km, in, ft, yd. Rectangles accept one formula per dimension, separated by a comma. Scale uses numbers without units.");
                        if (field.has_focus() || field.lost_focus())
                            && ctx.input(|i| i.key_pressed(Key::Enter))
                        {
                            if self.modeling.is_some() {
                                self.commit_modeling(&ctx);
                            } else if self.last_move_copy.is_some() {
                                self.commit_move_array(&ctx);
                            } else if self.drawing.is_some() {
                                self.commit_drawing(&ctx);
                            } else {
                                self.commit_line(&ctx, true);
                            }
                            field.surrender_focus();
                        }
                        ui.label(
                            RichText::new(match self.tool {
                                Tool::Scale => "Factor",
                                Tool::PushPull => "Distance",
                                Tool::Offset => "Offset",
                                Tool::Move if self.last_move_copy.is_some() => "Copies",
                                Tool::Move => "Distance",
                                Tool::Rectangle => "Width, height",
                                Tool::Circle | Tool::Polygon => "Radius",
                                _ => "Length",
                            })
                            .size(12.)
                            .color(colors.muted),
                        );
                    });
                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            dot(ui, ACCENT);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(message).size(12.).color(colors.muted),
                                )
                                .truncate(),
                            );
                        },
                    );
                });
            });
    }
    fn tool_tooltip(response: egui::Response, tool: Tool) -> egui::Response {
        response.on_hover_ui(|ui| {
            let title = if tool.shortcut().is_empty() {
                tool.label().to_owned()
            } else {
                format!("{} ({})", tool.label(), tool.shortcut())
            };
            ui.strong(title);
            ui.label(tool.instructions());
        })
    }
    fn tool_menu_item(ui: &mut egui::Ui, tool: Tool, active: bool) -> egui::Response {
        let colors = Palette::for_ui(ui);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, true, active, tool.label())
        });
        if active || response.hovered() || response.has_focus() {
            ui.painter().rect_filled(
                rect,
                5,
                if active {
                    colors.selected
                } else {
                    colors.hovered
                },
            );
        }
        let color = if active { colors.accent } else { colors.ink };
        icons::ui_tool(
            ui.painter(),
            Rect::from_center_size(
                egui::pos2(rect.left() + 18., rect.center().y),
                egui::vec2(22., 22.),
            ),
            tool,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 38., rect.center().y),
            Align2::LEFT_CENTER,
            tool.label(),
            FontId::proportional(13.),
            color,
        );
        ui.painter().text(
            egui::pos2(rect.right() - 8., rect.center().y),
            Align2::RIGHT_CENTER,
            tool.shortcut(),
            FontId::proportional(11.),
            colors.muted,
        );
        Self::tool_tooltip(response, tool)
    }
    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        egui::Panel::left("tools")
            .exact_size(76.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(colors.paper)
                    .inner_margin(egui::Margin::symmetric(10, 18)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("tool_scroll")
                    .show(ui, |ui| {
                        for rail_tool in Tool::RAIL {
                            let tool = if rail_tool == Tool::Line {
                                self.drawing_tool
                            } else if rail_tool == Tool::Dimensions {
                                self.measurement_tool
                            } else if rail_tool == Tool::Union {
                                self.solid_tool
                            } else {
                                rail_tool
                            };
                            let active = self.tool == tool;
                            let nested =
                                matches!(rail_tool, Tool::Line | Tool::Dimensions | Tool::Union);
                            let tooltip = if tool.shortcut().is_empty() {
                                tool.label().to_owned()
                            } else {
                                format!("{} ({})", tool.label(), tool.shortcut())
                            };
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(56., 55.), Sense::click());
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    if nested {
                                        egui::Role::ComboBox
                                    } else {
                                        egui::Role::RadioButton
                                    },
                                    true,
                                    active,
                                    tooltip.clone(),
                                )
                            });
                            if active || response.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(egui::vec2(0., 3.)),
                                    8,
                                    if active {
                                        colors.selected
                                    } else {
                                        colors.hovered
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
                                    colors.accent,
                                );
                            }
                            let color = if active { colors.accent } else { colors.ink };
                            icons::ui_tool(
                                ui.painter(),
                                Rect::from_center_size(
                                    rect.center() - egui::vec2(if nested { 4. } else { 0. }, 8.),
                                    egui::vec2(27., 27.),
                                ),
                                tool,
                            );
                            ui.painter().text(
                                rect.center() + egui::vec2(0., 17.),
                                Align2::CENTER_CENTER,
                                if tool == Tool::TapeMeasure {
                                    "Tape"
                                } else {
                                    tool.label()
                                },
                                FontId::proportional(
                                    if matches!(
                                        tool,
                                        Tool::Protractor | Tool::Dimensions | Tool::OuterShell
                                    ) {
                                        9.
                                    } else {
                                        10.
                                    },
                                ),
                                color,
                            );
                            if nested {
                                let tip = rect.right_center() + egui::vec2(-5., -8.);
                                ui.painter().add(egui::Shape::line(
                                    vec![
                                        tip + egui::vec2(-4., -4.),
                                        tip,
                                        tip + egui::vec2(-4., 4.),
                                    ],
                                    Stroke::new(1.8, color),
                                ));
                                egui::Popup::menu(&response)
                                    .align(egui::RectAlign::RIGHT_START)
                                    .width(190.)
                                    .show(|ui| {
                                        let options: &[Tool] = match rail_tool {
                                            Tool::Line => &Tool::DRAWING,
                                            Tool::Dimensions => &Tool::MEASURING,
                                            Tool::Union => &Tool::SOLID,
                                            _ => unreachable!(),
                                        };
                                        for &option in options {
                                            if Self::tool_menu_item(ui, option, self.tool == option)
                                                .clicked()
                                            {
                                                if option.is_solid() {
                                                    self.start_solid_tool(option, ui.ctx());
                                                } else {
                                                    self.set_tool(option);
                                                }
                                                ui.close();
                                            }
                                        }
                                        if rail_tool == Tool::Line {
                                            ui.separator();
                                            ui.horizontal(|ui| {
                                                ui.label("Polygon sides");
                                                ui.add(
                                                    egui::DragValue::new(&mut self.polygon_sides)
                                                        .range(3..=64),
                                                );
                                            });
                                        }
                                    });
                            }
                            if Self::tool_tooltip(response, tool).clicked() && !nested {
                                self.set_tool(tool);
                            }
                            ui.add_space(5.);
                        }
                    });
            });
    }
    fn entity_info_contents(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        self.solid_inspector(ui);
        self.measurement_inspector(ui);
        self.text_entity_info(ui);
        if self.tool == Tool::Paint {
            ui.heading("Face color");
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut self.paint_color);
                ui.label("Choose paint");
            });
            ui.horizontal_wrapped(|ui| {
                for color in [
                    [195, 96, 67],
                    [225, 179, 87],
                    [108, 148, 102],
                    [95, 143, 177],
                    [163, 121, 167],
                    [236, 231, 215],
                    [78, 84, 81],
                ] {
                    let rgb = Color32::from_rgb(color[0], color[1], color[2]);
                    if ui
                        .add(
                            egui::Button::new(" ")
                                .fill(rgb)
                                .min_size(egui::vec2(24., 24.)),
                        )
                        .on_hover_text(format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]))
                        .clicked()
                    {
                        self.paint_color = color;
                    }
                }
            });
            ui.label("Click a face to apply this color.");
            ui.add_space(12.);
        }
        if self.selected.is_empty() {
            ui.label(
                RichText::new("Nothing selected")
                    .strong()
                    .color(colors.muted),
            );
            ui.label(
                RichText::new("Select an entity to see its details.")
                    .small()
                    .color(colors.muted),
            );
            ui.add_space(4.);
        } else {
            self.face_cache.update(&self.doc.model);
            let entities = self.doc.model.expand_entities(&self.selected);
            let edges = entities
                .iter()
                .filter(|id| self.doc.model.edges.contains_key(id))
                .count();
            let faces = entities
                .iter()
                .filter(|id| self.doc.model.faces.contains_key(id))
                .count();
            let dimensions = entities
                .iter()
                .filter(|id| self.doc.model.dimensions.contains_key(id))
                .count();
            let texts = entities
                .iter()
                .filter(|id| self.doc.model.texts.contains_key(id))
                .count();
            let noun =
                |n: usize, single: &str| format!("{n} {single}{}", if n == 1 { "" } else { "s" });
            let objects = self
                .selected
                .iter()
                .filter_map(|id| self.doc.model.objects.get(id))
                .collect::<Vec<_>>();
            let selection = if objects.len() == 1 && self.selected.len() == 1 {
                let object = objects[0];
                let kind = if object.kind == ObjectKind::Group {
                    "Group"
                } else {
                    "Component"
                };
                format!("{kind}: {}", object.name)
            } else if !objects.is_empty() {
                noun(self.selected.len(), "entity")
            } else if dimensions > 0 || texts > 0 {
                let mut parts = vec![];
                if edges > 0 {
                    parts.push(noun(edges, "edge"));
                }
                if faces > 0 {
                    parts.push(noun(faces, "face"));
                }
                if dimensions > 0 {
                    parts.push(noun(dimensions, "dimension"));
                }
                if texts > 0 {
                    parts.push(noun(texts, "text label"));
                }
                parts.join(" · ")
            } else if faces == 0 {
                noun(edges, "edge")
            } else if edges == 0 {
                noun(faces, "face")
            } else {
                format!("{} · {}", noun(edges, "edge"), noun(faces, "face"))
            };
            ui.label(
                RichText::new(format!("{selection} selected"))
                    .size(17.)
                    .strong(),
            );
            ui.add_space(12.);
            if self.doc.model.selection_locked(&self.selected) {
                ui.label(RichText::new("Locked").color(axis_color(Axis::Z)));
            }
            let length: f64 = entities
                .iter()
                .filter_map(|id| self.doc.model.edges.get(id))
                .map(|e| self.doc.model.edge_length(e))
                .sum();
            if edges > 0 {
                Self::property(ui, "Total length", &format_length(length));
            }
            if dimensions > 0 {
                let length: f64 = entities
                    .iter()
                    .filter_map(|id| self.doc.model.dimensions.get(id))
                    .map(|d| d.length(&self.doc.model))
                    .sum();
                Self::property(ui, "Measured length", &format_length(length));
            }
            if faces > 0 {
                let area: f64 = entities
                    .iter()
                    .filter_map(|id| self.doc.model.faces.get(id))
                    .map(|f| self.face_cache.get(f.id).area)
                    .sum();
                Self::property(ui, "Surface area", &format!("{area:.3} m²"));
            }
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
        }
    }
    fn property(ui: &mut egui::Ui, label: &str, value: &str) {
        let colors = Palette::for_ui(ui);
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(12.).color(colors.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(value).monospace().size(12.));
            });
        });
        ui.add_space(3.);
    }
    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        if !self.inspector {
            return;
        }
        if ui.available_width() >= 900. {
            let panel = egui::Panel::right("inspector")
                .default_size(self.inspector_width)
                .size_range(260. ..=ui.available_width() * 0.5)
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.paper))
                .show(ui, |ui| {
                    ui.spacing_mut().scroll.bar_outer_margin = 0.;
                    egui::ScrollArea::vertical()
                        .content_margin(18)
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.inspector_contents(ui));
                });
            self.inspector_width = panel.response.rect.width();
        } else {
            let mut open = true;
            egui::Window::new("Inspector")
                .open(&mut open)
                .default_width(280.)
                .default_pos(egui::pos2(90., 120.))
                .vscroll(true)
                .show(ui.ctx(), |ui| self.inspector_contents(ui));
            if !open {
                self.inspector = false;
            }
        }
    }

    fn erase_target(&mut self, pos: Pos2) -> Option<u64> {
        let pixel = dv(pos - self.viewport.min);
        let size = dv(self.viewport.size());
        let id = self.pick_at(pixel, size, false).or_else(|| {
            self.picking_cache
                .nearest(
                    &self.face_cache,
                    &self.entity_cache,
                    self.doc.camera.ray(pixel, size),
                    false,
                )
                .and_then(|(id, _, _)| self.entity_cache.get(id).context)
        })?;
        (!self.doc.model.is_locked(id)).then_some(id)
    }
    fn sweep_erase(&mut self, from: Pos2, to: Pos2) -> BTreeSet<u64> {
        let a = dv(from - self.viewport.min);
        let b = dv(to - self.viewport.min);
        let size = dv(self.viewport.size());
        let mut ids: BTreeSet<_> = self
            .doc
            .model
            .edges
            .values()
            .filter(|edge| {
                !self.doc.model.is_hidden(edge.id)
                    && !self.doc.model.is_locked(edge.id)
                    && self.doc.model.entity_in_context(edge.id).is_some()
            })
            .filter_map(|edge| {
                let (start, end) = self.doc.model.points(edge);
                let (c, d) = self.doc.camera.segment(start, end, size)?;
                if selection::segment_distance(a, b, c, d) >= 9. {
                    return None;
                }
                let pixel = selection::closest_on_second_segment(a, b, c, d);
                let point = self.doc.camera.ray(pixel, size).segment_point(start, end);
                selection::visible(&self.doc.model, &self.doc.camera, size, point)
                    .then(|| self.doc.model.entity_in_context(edge.id))
                    .flatten()
            })
            .collect();
        // Boundary crossings are handled by the edge sweep. Pick interiors
        // explicitly so clicks and strokes contained within a face work too.
        if let Some(id) = self.erase_target(from) {
            ids.insert(id);
        }
        if to != from {
            if let Some(id) = self.erase_target(to) {
                ids.insert(id);
            }
        }
        ids
    }
    fn prepare_picking(&mut self) {
        #[cfg(test)]
        {
            self.picking_preparations += 1;
        }
        self.entity_cache.update(&self.doc.model);
        self.face_cache.update(&self.doc.model);
        self.picking_cache.update(&self.face_cache);
    }
    // Call after prepare_picking so inference shares the viewport's cached
    // planes and spatial hierarchy instead of rebuilding every face per target.
    fn infer_snap(
        &self,
        pos: Pos2,
        start: Option<DVec3>,
        lock: Option<Axis>,
        axes: bool,
    ) -> Option<Snap> {
        ketchup_core::inference::infer_with_face_query(
            &self.doc.model,
            &self.doc.camera,
            dv(self.viewport.size()),
            dv(pos - self.viewport.min),
            start,
            lock,
            self.doc.snap_to_geometry,
            axes,
            Some(
                &self
                    .picking_cache
                    .snap_query(&self.face_cache, &self.entity_cache),
            ),
        )
    }
    fn pick_at(&mut self, pixel: DVec2, size: DVec2, include_faces: bool) -> Option<u64> {
        self.prepare_picking();
        self.picking_cache.pick(
            &self.doc.model,
            &self.face_cache,
            &self.entity_cache,
            &self.doc.camera,
            size,
            pixel,
            8.,
            include_faces,
        )
    }
    fn update_pointer(&mut self, pos: Pos2) {
        // Navigation and selection rectangles do not need hover queries.
        // Keep their feedback responsive on large imported models.
        let selecting_box = self.gesture.as_ref().is_some_and(|gesture| {
            matches!(gesture.kind, GestureKind::Select { .. })
                && gesture.start.distance(gesture.last) > 4.
        });
        if self.tool == Tool::Orbit || self.navigating() || selecting_box {
            self.hover = None;
            self.flip_hover = None;
            return;
        }
        if self.modeling.is_some() && matches!(self.tool, Tool::Move | Tool::Scale) {
            self.hover = None;
            self.update_modeling(pos);
            return;
        }
        let local = dv(pos - self.viewport.min);
        let size = dv(self.viewport.size());
        self.hover = if self.tool == Tool::Erase {
            self.erase_target(pos)
        } else if matches!(self.tool, Tool::PushPull | Tool::Offset | Tool::Paint) {
            self.prepare_picking();
            self.picking_cache
                .nearest(
                    &self.face_cache,
                    &self.entity_cache,
                    self.doc.camera.ray(local, size),
                    self.tool != Tool::Paint,
                )
                .map(|(id, _, _)| id)
        } else if matches!(self.tool, Tool::Select | Tool::Move | Tool::Scale)
            || self.tool.is_solid()
        {
            self.pick_at(local, size, true)
        } else {
            self.pick_at(local, size, false)
        };
        if self.tool == Tool::Flip {
            self.flip_hover = self.flip_plane_at(pos);
        }
        if self.tool.is_measuring() {
            self.update_measurement(pos);
        }
        if self.tool.is_drawing() || self.paste_preview.is_some() {
            self.snap = self.infer_snap(
                pos,
                if self.paste_preview.is_some() {
                    None
                } else {
                    self.line_start
                },
                if self.paste_preview.is_some() {
                    None
                } else {
                    self.lock
                },
                self.doc.snap_to_axes,
            );
            if let (Some(preview), Some(snap)) = (&mut self.paste_preview, self.snap) {
                preview.point = snap.point;
            }
            if self.drawing.is_some() && self.paste_preview.is_none() {
                self.snap = self.drawing_snap(pos);
                self.align_drawing_plane();
            }
            self.update_drawing_inference(pos);
            if self.paste_preview.is_none() {
                self.align_drawing_plane();
            }
        }
        if self.modeling.is_some() {
            self.update_modeling(pos);
        }
    }
    fn refresh_snap(&mut self, ctx: &egui::Context) {
        if self.tool == Tool::Line {
            if let Some(pos) = ctx
                .input(|i| i.pointer.hover_pos())
                .filter(|pos| self.viewport.contains(*pos))
            {
                self.update_pointer(pos);
            }
        }
    }
    fn preview_alignment(&self) -> Option<Axis> {
        if self.lock.is_some() {
            return self.lock;
        }
        if !self.doc.snap_to_axes {
            return None;
        }
        let delta = self.snap?.point - self.line_start?;
        if delta.length() < 1e-7 {
            return None;
        }
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .find(|axis| delta.cross(axis.vector()).length() < 1e-7)
    }
    fn orbit_center(&mut self) -> DVec3 {
        // Match the renderer's inherited object/tag visibility, including the
        // options that reveal hidden objects and geometry. Prepare it once at
        // the start of an orbit; keep the pivot fixed throughout the gesture.
        let model = &self.doc.model;
        self.entity_cache.update(model);
        let entities = &self.entity_cache;
        let display = &self.doc.display;
        let points = model
            .edges
            .values()
            .filter(|edge| entities.get(edge.id).displayed(display))
            .flat_map(|edge| {
                let (a, b) = model.points(edge);
                [a, b]
            })
            .chain(
                model
                    .faces
                    .values()
                    .filter(|face| entities.get(face.id).displayed(display))
                    .flat_map(|face| {
                        face.vertices
                            .iter()
                            .chain(face.holes.iter().flatten())
                            .map(|id| model.vertices[id].position)
                    }),
            )
            .chain(
                model
                    .dimensions
                    .values()
                    .filter(|d| display.dimensions && entities.get(d.id).displayed(display))
                    .flat_map(|d| d.layout(model)),
            );
        let mut points = points.chain(
            model
                .texts
                .values()
                .filter(|t| entities.get(t.id).displayed(display))
                .flat_map(|t| t.layout(model)),
        );
        let Some(first) = points.next() else {
            return DVec3::ZERO;
        };
        let (lo, hi) = points.fold((first, first), |(lo, hi), point| {
            (lo.min(point), hi.max(point))
        });
        (lo + hi) * 0.5
    }

    fn navigating(&self) -> bool {
        self.drawing_orbit_down
            || self
                .gesture
                .as_ref()
                .is_some_and(|g| matches!(g.kind, GestureKind::Navigate { .. }))
    }

    fn drawing_orbit_modifier(&self, modifiers: egui::Modifiers) -> bool {
        self.tool.is_drawing() && (modifiers.ctrl || modifiers.command || modifiers.mac_cmd)
    }

    fn update_drawing_orbit(&mut self, modifiers: egui::Modifiers) {
        let down = self.drawing_orbit_modifier(modifiers);
        if down != self.drawing_orbit_down {
            self.drawing_orbit_down = down;
            self.drawing_orbit_pivot = None;
            if let Some(gesture) = &mut self.gesture {
                // An interrupted point click must not place geometry on release.
                if down && matches!(gesture.kind, GestureKind::Line | GestureKind::Shape) {
                    gesture.kind = GestureKind::DrawingNavigate;
                }
                if down && let GestureKind::Freehand { paused } = &mut gesture.kind {
                    *paused = true;
                }
            }
        }
        if down {
            self.hover = None;
            self.flip_hover = None;
        }
    }

    fn navigate(&mut self, ctx: &egui::Context, delta: DVec2, pivot: &mut Option<DVec3>) {
        if ctx.input(|i| i.modifiers.shift) {
            self.doc.camera.pan(delta, self.viewport.height() as f64);
            *pivot = None;
        } else if delta != DVec2::ZERO {
            let center = *pivot.get_or_insert_with(|| self.orbit_center());
            self.doc.camera.orbit_around(delta, center);
        }
        self.changed(ctx);
    }

    fn viewport_input(&mut self, ctx: &egui::Context, response: &egui::Response) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.updates.open {
            return;
        }

        if self.project.blocked() {
            return;
        }
        if self.help
            || self.about
            || self.release_notes.open
            || self.object_creation.is_some()
            || self.object_rename.is_some()
            || self.text_editor.is_some()
        {
            self.drawing_orbit_down = false;
            self.drawing_orbit_pivot = None;
            self.viewport_pointer = None;
            if self.tool == Tool::Freehand {
                self.drawing = None;
            }
            self.gesture = None;
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let focused = ctx.input(|i| i.focused);
        if !focused {
            self.drawing_orbit_down = false;
            self.drawing_orbit_pivot = None;
            self.viewport_pointer = None;
            if self.tool == Tool::Freehand {
                self.drawing = None;
            }
            self.gesture = None;
            self.hover = None;
            return;
        }
        let mut refresh_pointer = !events
            .iter()
            .any(|event| matches!(event, egui::Event::PointerMoved(_)));
        // Native mouse input interleaves absolute positions and raw deltas.
        // We do not use raw deltas; discard them before looking ahead so they
        // cannot turn one pending pointer position into hundreds of snap queries.
        let mut events = events
            .into_iter()
            .filter(|event| !matches!(event, egui::Event::MouseMoved(_)))
            .peekable();
        while let Some(event) = events.next() {
            // Keep every stroke sample, but coalesce consecutive hover/navigation
            // positions. Button and keyboard events remain ordered barriers.
            let stroking = self.gesture.as_ref().is_some_and(|gesture| {
                matches!(
                    gesture.kind,
                    GestureKind::Freehand { .. } | GestureKind::Erase { .. }
                )
            });
            if matches!(event, egui::Event::PointerMoved(_))
                && matches!(events.peek(), Some(egui::Event::PointerMoved(_)))
                && !stroking
            {
                continue;
            }
            match event {
                egui::Event::ModifiersChanged(modifiers) => {
                    refresh_pointer |=
                        self.drawing_orbit_down && !self.drawing_orbit_modifier(modifiers);
                    self.update_drawing_orbit(modifiers);
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers,
                } if response.contains_pointer() && self.viewport.contains(pos) => {
                    self.update_drawing_orbit(modifiers);
                    if button == egui::PointerButton::Secondary {
                        self.solid_first = None;
                        self.flip_hover = None;
                        self.measurement = None;
                        self.drawing = None;
                        self.gesture = None;
                        self.paste_preview = None;
                        self.modeling = None;
                        self.last_move_copy = None;
                        self.length_input.clear();
                        self.context_position = Some(pos);
                        let hit = self.pick_at(
                            dv(pos - self.viewport.min),
                            dv(self.viewport.size()),
                            true,
                        );
                        self.select_context_target(hit);
                        continue;
                    }
                    if self.gesture.is_some() {
                        continue;
                    }
                    let kind = if button == egui::PointerButton::Middle
                        || (button == egui::PointerButton::Primary && self.tool == Tool::Orbit)
                    {
                        self.hover = None;
                        self.flip_hover = None;
                        self.drawing_orbit_pivot = None;
                        GestureKind::Navigate { pivot: None }
                    } else if button == egui::PointerButton::Primary {
                        if self.drawing_orbit_down {
                            GestureKind::DrawingNavigate
                        } else if self.paste_preview.is_some() {
                            GestureKind::Paste
                        } else {
                            match self.tool {
                                Tool::Select => GestureKind::Select {
                                    additive: modifiers.shift,
                                },
                                Tool::TapeMeasure
                                | Tool::Protractor
                                | Tool::Dimensions
                                | Tool::Text => GestureKind::Measure,
                                Tool::Flip
                                | Tool::OuterShell
                                | Tool::Union
                                | Tool::Intersect
                                | Tool::Trim
                                | Tool::Split => GestureKind::Solid,
                                Tool::Line => GestureKind::Line,
                                Tool::Rectangle | Tool::Circle | Tool::Polygon => {
                                    GestureKind::Shape
                                }
                                Tool::Freehand => {
                                    self.begin_drawing(pos);
                                    GestureKind::Freehand { paused: false }
                                }
                                Tool::PushPull
                                | Tool::Scale
                                | Tool::Offset
                                | Tool::Move
                                | Tool::Paint => GestureKind::Modeling,
                                Tool::Erase => GestureKind::Erase {
                                    ids: self.sweep_erase(pos, pos),
                                },
                                Tool::Orbit => unreachable!(),
                            }
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
                    let in_viewport = self.viewport.contains(pos) && response.contains_pointer();
                    let mouse_navigation = self.gesture.as_ref().is_some_and(|gesture| {
                        matches!(gesture.kind, GestureKind::Navigate { .. })
                    });
                    // Modifier navigation follows hover motion as well as drags.
                    // Use the last processed position, not egui's final position
                    // for this frame, so the first movement after key-down counts.
                    if self.drawing_orbit_down && in_viewport && !mouse_navigation {
                        if let Some(previous) = self.viewport_pointer {
                            let mut pivot = self.drawing_orbit_pivot;
                            self.navigate(ctx, dv(pos - previous), &mut pivot);
                            self.drawing_orbit_pivot = pivot;
                        }
                    }
                    self.viewport_pointer = in_viewport.then_some(pos);
                    if let Some(mut gesture) = self.gesture.take() {
                        match &mut gesture.kind {
                            GestureKind::Navigate { pivot } => {
                                self.navigate(ctx, dv(pos - gesture.last), pivot);
                            }
                            GestureKind::Erase { ids } => {
                                ids.extend(self.sweep_erase(gesture.last, pos));
                            }
                            GestureKind::Freehand { paused } if !self.drawing_orbit_down => {
                                if !*paused || pos != gesture.last {
                                    *paused = false;
                                    self.sample_freehand(pos);
                                }
                            }
                            _ => {}
                        }
                        gesture.last = pos;
                        self.gesture = Some(gesture);
                    }
                    if in_viewport {
                        self.update_pointer(pos);
                    } else {
                        self.hover = None;
                        self.flip_hover = None;
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: false,
                    modifiers,
                } => {
                    self.update_drawing_orbit(modifiers);
                    if self.gesture.as_ref().is_some_and(|g| g.button == button) {
                        let gesture = self.gesture.take().unwrap();
                        match gesture.kind {
                            GestureKind::Navigate { .. } | GestureKind::DrawingNavigate => {}
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
                                    self.pick_at(
                                        dv(pos - self.viewport.min),
                                        dv(self.viewport.size()),
                                        true,
                                    )
                                    .into_iter()
                                    .collect()
                                };
                                if !dragged
                                    && !additive
                                    && !modifiers.shift
                                    && self.viewport.contains(pos)
                                {
                                    if hits.is_empty() && self.doc.model.edit_context.is_some() {
                                        self.close_edit_context(ctx);
                                        return;
                                    }
                                    if response.double_clicked_by(egui::PointerButton::Primary) {
                                        if let Some(id) = hits
                                            .iter()
                                            .find(|id| self.doc.model.texts.contains_key(id))
                                            .copied()
                                        {
                                            self.selected = BTreeSet::from([id]);
                                            self.begin_text_edit(id, ctx);
                                            return;
                                        }
                                        if let Some(id) = hits
                                            .iter()
                                            .find(|id| self.doc.model.objects.contains_key(id))
                                            .copied()
                                        {
                                            self.enter_object(id, ctx);
                                            return;
                                        }
                                    }
                                }
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
                            GestureKind::Paste
                                if self.viewport.contains(pos)
                                    && gesture.start.distance(pos) < 5. =>
                            {
                                self.update_pointer(pos);
                                if let Some(preview) = self.paste_preview.take() {
                                    self.paste_at(
                                        ctx,
                                        &preview.clipboard,
                                        preview.point - preview.clipboard.anchor,
                                    );
                                    self.snap = None;
                                }
                            }
                            GestureKind::Solid
                                if self.viewport.contains(pos)
                                    && gesture.start.distance(pos) < 5. =>
                            {
                                self.solid_click(pos, ctx);
                            }
                            GestureKind::Measure
                                if self.viewport.contains(pos)
                                    && gesture.start.distance(pos) < 5. =>
                            {
                                self.measurement_click(pos, ctx);
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
                                    self.update_pointer(pos);
                                }
                            }
                            GestureKind::Shape
                                if self.viewport.contains(pos)
                                    && gesture.start.distance(pos) < 5. =>
                            {
                                self.update_pointer(pos);
                                if self.drawing.is_some() {
                                    self.commit_drawing(ctx);
                                } else {
                                    self.begin_drawing(pos);
                                }
                            }
                            GestureKind::Freehand { paused } => {
                                if !paused && self.viewport.contains(pos) {
                                    self.update_pointer(pos);
                                } else {
                                    self.snap =
                                        self.drawing.as_ref().and_then(|d| d.points.last()).map(
                                            |&point| Snap {
                                                point,
                                                kind: Inference::Plane,
                                            },
                                        );
                                }
                                self.commit_drawing(ctx);
                            }
                            GestureKind::Erase { mut ids } => {
                                ids.extend(self.sweep_erase(gesture.last, pos));
                                let before = self.doc.model.clone();
                                self.doc.model.erase(&ids);
                                self.finish_edit("Erase geometry", before, ctx);
                            }
                            GestureKind::Modeling
                                if self.viewport.contains(pos)
                                    && gesture.start.distance(pos) < 5. =>
                            {
                                if self.modeling.is_some() {
                                    self.update_modeling(pos);
                                    self.commit_modeling(ctx);
                                } else {
                                    self.begin_modeling(pos, ctx);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                egui::Event::PointerGone => {
                    self.drawing_reference = None;
                    self.drawing_guide = None;
                    self.drawing_axis_guides.clear();
                    self.drawing_projection = None;
                    self.viewport_pointer = None;
                    self.drawing_orbit_pivot = None;
                    if self.tool == Tool::Freehand {
                        self.drawing = None;
                    }
                    self.gesture = None;
                    self.hover = None;
                }
                _ => {}
            }
        }
        self.update_drawing_orbit(ctx.input(|i| i.modifiers));
        // Menus and other overlays can cover the viewport without pointer motion.
        if !response.contains_pointer() {
            self.hover = None;
            self.flip_hover = None;
        } else if response.hovered() && refresh_pointer {
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                self.update_pointer(pos);
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
        }
    }

    fn paint_tool_cursor(&self, ctx: &egui::Context, response: &egui::Response) {
        if !response.enabled() || !response.hovered() || !response.contains_pointer() {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let tool = if self.navigating() {
            Tool::Orbit
        } else {
            match self.tool {
                Tool::Rectangle | Tool::Circle | Tool::Polygon => Tool::Line,
                tool => tool,
            }
        };
        if tool == Tool::Select {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
            return;
        }
        let size = 28.;
        // Keep the pencil point at the actual interaction position.
        // Symmetric tool icons use their center as the hotspot.
        let hotspot = match tool {
            Tool::Line => egui::vec2(-0.95, 0.95),
            _ => Vec2::ZERO,
        } * (size * 0.36);
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::unique("viewport_tool_cursor"),
            ))
            .with_clip_rect(self.viewport);
        ctx.set_cursor_icon(egui::CursorIcon::None);
        icons::tool(
            &painter,
            Rect::from_center_size(pos - hotspot, Vec2::splat(size)),
            tool,
        );
    }

    fn paint_scene(&mut self, ui: &egui::Ui, rect: Rect) {
        let colors = Palette::for_ui(ui);
        let faces = FacePalette::for_ui(ui);
        let model = self
            .modeling
            .as_ref()
            .and_then(|edit| edit.preview.as_ref())
            .unwrap_or(&self.doc.model);
        self.entity_cache.update(model);
        let entities = &self.entity_cache;
        let selected_entities = if let Some(edit) = self
            .modeling
            .as_ref()
            .filter(|_| self.tool == Tool::PushPull)
        {
            let cap = model
                .faces
                .keys()
                .filter(|id| {
                    !edit.base.faces.contains_key(id) && model.parent(**id) == model.edit_context
                })
                .next_back()
                .copied()
                .or(edit.face);
            cap.into_iter().collect()
        } else if let Some(edit) = self.modeling.as_ref().filter(|edit| edit.copy) {
            let copies = model
                .selectable_entities()
                .into_iter()
                .filter(|id| !edit.base.contains_entity(id))
                .collect();
            model.expand_entities(&copies)
        } else {
            model.expand_entities(&self.selected)
        };
        let emphasized_entities = if self.group_emphasis
            && let Some(context) = model.edit_context
        {
            model.expand_entities(&BTreeSet::from([context]))
        } else {
            BTreeSet::new()
        };
        let emphasize_group = !emphasized_entities.is_empty();
        let size = dv(rect.size());
        let camera = &self.doc.camera;
        let p = ui.painter().with_clip_rect(rect);
        let mut themed_display =
            crate::environment::for_theme(&self.doc.display, ui.visuals().dark_mode);
        if emphasize_group {
            // These overrides last only for this frame; the saved style stays intact.
            if themed_display.face_style == ketchup_core::FaceStyle::Wireframe {
                themed_display.face_style = ketchup_core::FaceStyle::Shaded;
            }
            themed_display.x_ray = false;
            themed_display.shadows = false;
            themed_display.section_fill = false;
        }
        let display = &themed_display;
        crate::environment::paint(&p, rect, camera, display, ui.visuals().dark_mode);
        let mut mesh = renderer::Mesh::cached(camera, size, &mut self.scene_cache);
        mesh.set_pixels_per_point(ui.ctx().pixels_per_point());
        mesh.set_display(display);
        let spacing = 10.0_f64.powf((camera.distance / 15.).log10().floor());
        if self.doc.display.grid {
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
                mesh.background_line(
                    camera,
                    DVec3::new(x + i as f64 * spacing, y - span, 0.),
                    DVec3::new(x + i as f64 * spacing, y + span, 0.),
                    if major { 0.8 } else { 0.55 },
                    color,
                );
                mesh.background_line(
                    camera,
                    DVec3::new(x - span, y + i as f64 * spacing, 0.),
                    DVec3::new(x + span, y + i as f64 * spacing, 0.),
                    if major { 0.8 } else { 0.55 },
                    color,
                );
            }
        }
        for axis in [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .filter(|_| display.axes)
        {
            let v = axis.vector() * camera.distance * 80.;
            mesh.axis_line(-v, DVec3::ZERO, 0.85, axis_color(axis).gamma_multiply(0.35));
            mesh.axis_line(DVec3::ZERO, v, 1.1, axis_color(axis).gamma_multiply(0.8));
        }
        self.face_cache.update(model);
        self.shadow_cache
            .update(&self.face_cache, entities, display);
        let mut visible_faces: Vec<_> = model
            .faces
            .values()
            .filter(|f| entities.get(f.id).displayed(display))
            .filter(|f| !emphasize_group || emphasized_entities.contains(&f.id))
            .collect();
        if display.x_ray {
            visible_faces.sort_by(|a, b| {
                let center = |face: &ketchup_core::Face| {
                    face.vertices
                        .iter()
                        .map(|id| model.vertices[id].position)
                        .sum::<DVec3>()
                        / face.vertices.len() as f64
                };
                camera
                    .view_distance(center(b))
                    .total_cmp(&camera.view_distance(center(a)))
            });
        }
        let lighting = display_scene::FaceLighting::new(camera);
        let erasing = match &self.gesture {
            Some(Gesture {
                kind: GestureKind::Erase { ids },
                ..
            }) => Some(ids),
            _ => None,
        };
        for face in visible_faces
            .into_iter()
            .filter(|_| display.face_style != ketchup_core::FaceStyle::Wireframe)
        {
            let cached = self.face_cache.get(face.id);
            mesh.face_edges(face, cached);
            let normal = cached.normal;
            let tag_color = entities.get(face.id).tag_color;
            let rgb = if display.color_by_tag {
                tag_color.as_ref()
            } else if display.face_style == ketchup_core::FaceStyle::Materials {
                model.face_colors.get(&face.id)
            } else {
                None
            };
            let material = if display.face_style == ketchup_core::FaceStyle::HiddenLine {
                let bg = display.background;
                Color32::from_rgb(bg[0], bg[1], bg[2])
            } else if display.face_style == ketchup_core::FaceStyle::Monochrome
                && normal.dot(camera.toward_eye(model.vertices[&face.vertices[0]].position)) < 0.
            {
                faces.back
            } else if let Some(rgb) = rgb {
                Color32::from_rgb(rgb[0], rgb[1], rgb[2])
            } else {
                faces.front
            };
            let outliner_hover = self.outliner_hover.contains(&face.id);
            let color = if entities.get(face.id).context.is_none() && !outliner_hover {
                faces.inactive
            } else if erasing.is_some_and(|ids| {
                entities
                    .get(face.id)
                    .context
                    .is_some_and(|id| ids.contains(&id))
            }) {
                Color32::from_rgb(220, 91, 76)
            } else if self.tool == Tool::Paint
                && model.face_colors.contains_key(&face.id)
                && !outliner_hover
            {
                material
            } else if selected_entities.contains(&face.id) {
                faces.selected
            } else if self.hover.is_some() && self.hover == entities.get(face.id).context
                || self.hover == Some(face.id)
                || outliner_hover
            {
                faces.hovered
            } else {
                material
            };
            let color = if display.face_style == ketchup_core::FaceStyle::HiddenLine {
                color
            } else {
                lighting.shade(color, normal, model.vertices[&face.vertices[0]].position)
            };
            let color = if display.x_ray || entities.get(face.id).entity_hidden {
                color.gamma_multiply(0.18)
            } else {
                color
            };
            if let Some(shadow) = self.shadow_cache.get(face.id) {
                let shadow_color = display_scene::shadow_color(color, display.shadow_darkness);
                mesh.shadow_face(&cached.triangles, shadow, color, shadow_color);
            } else {
                for &triangle in &cached.triangles {
                    mesh.world_triangle(camera, triangle, color);
                }
            }
        }
        display_scene::paint_sections(
            &mut mesh,
            camera,
            model,
            &self.face_cache,
            entities,
            display,
            &faces,
        );
        let profiles = if display.profiles && !emphasize_group {
            display_scene::profile_edges(model, camera, &self.face_cache, entities, display)
        } else {
            BTreeSet::new()
        };
        for e in model
            .edges
            .values()
            .filter(|e| entities.get(e.id).displayed(display))
        {
            let selected = selected_entities.contains(&e.id);
            let emphasized = emphasized_entities.contains(&e.id);
            let root = entities.get(e.id).context;
            let outliner_hover = self.outliner_hover.contains(&e.id);
            let hover = outliner_hover || self.hover.is_some() && self.hover == root;
            let pending = erasing.is_some_and(|ids| root.is_some_and(|id| ids.contains(&id)));
            let color = if emphasize_group && !emphasized {
                colors.muted.gamma_multiply(0.55)
            } else if root.is_none() && !outliner_hover {
                if ui.visuals().dark_mode {
                    colors.border
                } else {
                    Color32::from_rgb(184, 193, 181)
                }
            } else if pending {
                Color32::from_rgb(220, 91, 76)
            } else if entities.get(e.id).locked {
                Color32::from_rgb(90, 133, 176)
            } else if selected {
                ACCENT
            } else if hover && self.tool == Tool::Erase && !outliner_hover {
                ACCENT
            } else if hover {
                colors.green
            } else {
                colors.ink
            };
            let (a, b) = model.points(e);
            // Emphasis changes the surrounding geometry, preserving interaction feedback.
            let active = selected || hover || pending;
            let width = if emphasize_group && !emphasized {
                1.
            } else if active {
                3.
            } else if profiles.contains(&e.id) {
                display.profile_width
            } else {
                1.8
            };
            let width = if display.depth_cue && !active && !emphasize_group {
                width
                    * (camera.distance / camera.view_distance((a + b) * 0.5).max(0.01))
                        .clamp(0.35, 1.8)
            } else {
                width
            };
            let color = if display.color_by_tag && !active && (!emphasize_group || emphasized) {
                entities
                    .get(e.id)
                    .tag_color
                    .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
                    .unwrap_or(color)
            } else {
                color
            };
            if display.back_edges || entities.get(e.id).entity_hidden {
                mesh.overlay_line(a, b, 1., color.gamma_multiply(0.4), true);
            }
            if entities.get(e.id).entity_hidden {
                continue;
            }
            if display.edges
                || emphasize_group
                || profiles.contains(&e.id)
                || active
                || display.face_style == ketchup_core::FaceStyle::Wireframe
            {
                if display.x_ray || display.face_style == ketchup_core::FaceStyle::Wireframe {
                    mesh.overlay_line(a, b, width, color, false);
                } else {
                    mesh.styled_edge(
                        e,
                        model,
                        width,
                        color,
                        display.extensions,
                        display.endpoints,
                    );
                }
            }
        }
        if let (Some(start), Some(snap)) = (self.line_start, self.line_preview_snap()) {
            let color = self
                .preview_alignment()
                .or_else(|| self.active_drawing_guide().map(|g| g.axis))
                .map(axis_color)
                .unwrap_or(colors.ink);
            mesh.world_line(camera, start, snap.point, 2.2, color);
        }
        for pair in self.drawing_points().windows(2) {
            mesh.world_line(
                camera,
                pair[0],
                pair[1],
                2.2,
                self.lock.map(axis_color).unwrap_or(ACCENT),
            );
        }
        if let Some(preview) = &self.paste_preview {
            self.paste_face_cache.update(&preview.clipboard.model);
            let offset = preview.point - preview.clipboard.anchor;
            for face in preview
                .clipboard
                .model
                .faces
                .values()
                .filter(|f| !preview.clipboard.model.is_hidden(f.id))
            {
                for triangle in &self.paste_face_cache.get(face.id).triangles {
                    mesh.world_triangle(camera, triangle.map(|p| p + offset), faces.paste);
                }
            }
            for edge in preview
                .clipboard
                .model
                .edges
                .values()
                .filter(|e| !preview.clipboard.model.is_hidden(e.id))
            {
                let (a, b) = preview.clipboard.model.points(edge);
                mesh.world_line(camera, a + offset, b + offset, 2.4, colors.green);
            }
        }
        let dimensions = self.dimension_shapes(&mut mesh, &p, rect, model);
        let texts = self.text_shapes(&mut mesh, &p, rect, model);
        mesh.paint_cached(&p, rect, &mut self.scene_cache);
        p.extend(dimensions);
        p.extend(texts);
        let screen = |point: DVec3| camera.project(point, size).map(|v| rect.min + pv(v));
        if let Some((lo, hi)) = self.face_cache.context_bounds(model, entities) {
            let padding = ((hi - lo).max_element() * 0.04).max(0.05);
            let lo = lo - DVec3::splat(padding);
            let hi = hi + DVec3::splat(padding);
            let corner = |i: usize| {
                DVec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                )
            };
            for i in 0..8 {
                for axis in 0..3 {
                    if i & (1 << axis) != 0 {
                        continue;
                    }
                    if let Some((a, b)) = camera.segment(corner(i), corner(i | (1 << axis)), size) {
                        p.extend(egui::Shape::dashed_line(
                            &[rect.min + pv(a), rect.min + pv(b)],
                            Stroke::new(1., colors.muted),
                            4.,
                            4.,
                        ));
                    }
                }
            }
        }
        if display.axes {
            // Reuse the rendered geometry, including a live modeling preview.
            // A full membership/face scan here makes orbiting grouped imports
            // dramatically slower whenever axes are enabled.
            self.picking_cache.update(&self.face_cache);
            if let Some(origin) = screen(DVec3::ZERO) {
                if self
                    .picking_cache
                    .visible(&self.face_cache, entities, camera, size, DVec3::ZERO)
                {
                    p.circle_filled(origin, 2.5, colors.muted);
                }
            }
        }
        for id in &self.selected {
            if let Some(e) = model.edges.get(id) {
                let (a, b) = model.points(e);
                for pt in [a, b] {
                    if let Some(point) = screen(pt) {
                        p.circle_filled(point, 3., colors.paper);
                        p.circle_stroke(point, 3., Stroke::new(1.3, colors.accent));
                    }
                }
            }
        }
        let guide = self
            .active_drawing_guide()
            .map(|g| (g.origin, g.axis))
            .or_else(|| {
                if !self.tool.is_drawing() {
                    return None;
                }
                match self.line_preview_snap()?.kind {
                    Inference::OnAxis(axis) => Some((DVec3::ZERO, axis)),
                    Inference::Axis(axis) => self
                        .line_start
                        .or_else(|| self.drawing.as_ref().map(|d| d.origin))
                        .map(|origin| (origin, axis)),
                    _ => self
                        .preview_alignment()
                        .zip(self.line_start)
                        .map(|(axis, start)| (start, axis)),
                }
            });
        let combined: Vec<_> = self
            .active_drawing_axis_guides()
            .map(|g| (g.origin, g.axis))
            .collect();
        for (origin, axis) in combined
            .iter()
            .copied()
            .chain(guide.filter(|_| combined.is_empty()))
            .filter(|_| display.guides)
        {
            let extent = axis.vector() * camera.distance * 5.;
            if let Some((a, b)) = camera.segment(origin - extent, origin + extent, size) {
                let a = rect.min + pv(a);
                let b = rect.min + pv(b);
                let length = a.distance(b);
                for step in 0..(length / 10.).ceil() as usize {
                    let from = (step as f32 * 10. / length).min(1.);
                    let to = ((step as f32 * 10. + 4.) / length).min(1.);
                    p.line_segment(
                        [a.lerp(b, from), a.lerp(b, to)],
                        Stroke::new(1., axis_color(axis).gamma_multiply(0.55)),
                    );
                }
            }
            if self.drawing_guide.is_some() || !combined.is_empty() {
                if let Some(point) = screen(origin) {
                    p.circle_filled(point, 3., colors.paper);
                    p.circle_stroke(point, 3., Stroke::new(1.5, axis_color(axis)));
                }
            }
        }
        if let Some((source, target)) = self.active_drawing_projection().filter(|_| display.guides)
        {
            let color = self.lock.map(axis_color).unwrap_or(colors.green);
            if let Some((a, b)) = camera.segment(source, target, size) {
                p.extend(egui::Shape::dashed_line(
                    &[rect.min + pv(a), rect.min + pv(b)],
                    Stroke::new(1.2, color),
                    4.,
                    4.,
                ));
            }
            if let Some(point) = screen(source) {
                p.circle_filled(point, 3., colors.paper);
                p.circle_stroke(point, 3., Stroke::new(1.5, color));
            }
        }
        if self.tool.is_drawing() {
            if let Some(start) = self.line_start.or(self.drawing.as_ref().map(|d| d.origin)) {
                if let Some(point) = screen(start) {
                    p.circle_filled(point, 3.5, colors.paper);
                    p.circle_stroke(point, 3.5, Stroke::new(1.5, colors.ink));
                }
            }
            if let Some(snap) = self.line_preview_snap() {
                if let Some(point) = screen(snap.point) {
                    let color = match snap.kind {
                        Inference::Axis(a) | Inference::OnAxis(a) => axis_color(a),
                        Inference::Midpoint => Color32::from_rgb(85, 150, 183),
                        _ => colors.green,
                    };
                    let color = self
                        .lock
                        .filter(|_| self.tool != Tool::Line)
                        .map(axis_color)
                        .unwrap_or(color);
                    match snap.kind {
                        Inference::Midpoint => {
                            p.add(egui::Shape::convex_polygon(
                                vec![
                                    point + egui::vec2(0., -5.),
                                    point + egui::vec2(-5., 4.),
                                    point + egui::vec2(5., 4.),
                                ],
                                colors.paper,
                                Stroke::new(1.5, color),
                            ));
                        }
                        Inference::Endpoint | Inference::Origin => {
                            p.rect_filled(
                                Rect::from_center_size(point, egui::vec2(7., 7.)),
                                1,
                                colors.paper,
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
                        if self.tool != Tool::Line {
                            format!("{} · Plane locked", axis.label())
                        } else {
                            format!("{} · Locked", axis.label())
                        }
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
                        colors.paper.gamma_multiply(0.95),
                    );
                    p.galley(at, galley, color);
                    if let Some(start) = self.line_start {
                        if let Some(a) = screen(start) {
                            // Keep the length clear of the endpoint's inference
                            // tooltip, including short vertical previews.
                            let offset = if (point.x - a.x).abs() < (point.y - a.y).abs() {
                                egui::vec2(-44., 0.)
                            } else {
                                egui::vec2(0., -16.)
                            };
                            let at = a.lerp(point, 0.5) + offset;
                            p.text(
                                at,
                                Align2::CENTER_CENTER,
                                format_length(start.distance(snap.point)),
                                FontId::monospace(11.),
                                colors.ink,
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
                let color = Color32::from_rgb(89, 136, 173);
                p.rect_filled(r, 0, color.gamma_multiply(0.1));
                p.rect_stroke(r, 0, Stroke::new(1., color), egui::StrokeKind::Inside);
            }
        }
        if let Some(edit) = &self.modeling {
            let text = edit.error.map(str::to_owned).unwrap_or_else(|| {
                let value =
                    precision::modeling_value(self.tool, &self.length_input).unwrap_or(edit.value);
                if self.tool == Tool::Scale {
                    format!("{value:.3} × · Click to confirm · Esc to cancel")
                } else {
                    let alignment = if edit.guides.len() > 1 && self.length_input.is_empty() {
                        format!(
                            " · {} aligned",
                            edit.guides
                                .iter()
                                .map(|g| match g.axis {
                                    Axis::X => "X",
                                    Axis::Y => "Y",
                                    Axis::Z => "Z",
                                })
                                .collect::<Vec<_>>()
                                .join(" + ")
                        )
                    } else {
                        String::new()
                    };
                    format!(
                        "{}{}{alignment} · Click to confirm · Esc to cancel",
                        if self.tool == Tool::Move {
                            if edit.copy { "Copy · " } else { "Move · " }
                        } else {
                            ""
                        },
                        format_length(value)
                    )
                }
            });
            let galley = p.layout_no_wrap(
                text,
                FontId::monospace(12.),
                if edit.error.is_some() {
                    colors.accent
                } else {
                    colors.ink
                },
            );
            let at = rect.left_bottom() + egui::vec2(20., -40.);
            p.rect_filled(
                Rect::from_min_size(
                    at - egui::vec2(8., 6.),
                    galley.size() + egui::vec2(16., 12.),
                ),
                5,
                colors.paper,
            );
            p.galley(at, galley, colors.ink);
            if self.tool == Tool::Move {
                if display.guides && self.length_input.is_empty() {
                    for guide in &edit.guides {
                        if let Some((a, b)) =
                            camera.segment(guide.origin, edit.anchor + edit.direction, size)
                        {
                            let (a, b) = (rect.min + pv(a), rect.min + pv(b));
                            let length = a.distance(b);
                            for step in 0..(length / 10.).ceil() as usize {
                                p.line_segment(
                                    [
                                        a.lerp(b, (step as f32 * 10. / length).min(1.)),
                                        a.lerp(b, ((step as f32 * 10. + 5.) / length).min(1.)),
                                    ],
                                    Stroke::new(1.5, axis_color(guide.axis)),
                                );
                            }
                            p.circle_stroke(a, 3., Stroke::new(1., axis_color(guide.axis)));
                        }
                    }
                    if !edit.guides.is_empty() {
                        if let Some(point) = screen(edit.anchor + edit.direction) {
                            p.circle_filled(point, 4., colors.paper);
                            p.circle_stroke(point, 4., Stroke::new(1.5, colors.green));
                        }
                    }
                }
                if let (Some(a), Some(b)) =
                    (screen(edit.anchor), screen(edit.anchor + edit.direction))
                {
                    p.line_segment(
                        [a, b],
                        Stroke::new(1.5, self.lock.map(axis_color).unwrap_or(colors.green)),
                    );
                }
            }
        }
        self.paint_solid_tools(&p, rect);
        self.paint_measurements(&p, rect);
        if let Some((notice, until)) = &self.notice {
            if ui.input(|i| i.time) < *until {
                let galley = p.layout(
                    notice.clone(),
                    FontId::proportional(12.),
                    colors.ink,
                    (rect.width() - 80.).clamp(120., 600.),
                );
                let r = Rect::from_min_size(
                    rect.left_top() + egui::vec2(20., 20.),
                    galley.size() + egui::vec2(24., 20.),
                );
                p.rect_filled(r, 7, colors.paper);
                p.rect_stroke(
                    r,
                    7,
                    Stroke::new(1., colors.border),
                    egui::StrokeKind::Inside,
                );
                p.galley(r.min + egui::vec2(12., 10.), galley, colors.ink);
            }
        }
    }
    fn edit_context_bar(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let path = self.doc.model.edit_path();
        let mut destination = None;
        egui::ScrollArea::horizontal()
            .id_salt("edit_path_scroll")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if path.is_empty() {
                        ui.label(RichText::new("Model").size(13.).strong());
                        ui.label(RichText::new("/").color(colors.border));
                        ui.label(RichText::new("No group open").size(12.).color(colors.muted));
                    } else {
                        if ui.small_button("Model").clicked() {
                            destination = Some(None);
                        }
                        for id in &path {
                            ui.label(RichText::new("›").color(colors.muted));
                            if ui.small_button(&self.doc.model.objects[id].name).clicked() {
                                destination = Some(Some(*id));
                            }
                        }
                        ui.add_space(10.);
                        if ui.small_button("Close level  (Esc)").clicked() {
                            destination = Some(
                                self.doc
                                    .model
                                    .edit_context
                                    .and_then(|id| self.doc.model.parent(id)),
                            );
                        }
                    }
                });
            });
        if let Some(context) = destination {
            self.navigate_context(context, ui.ctx());
        }
    }
    fn drawing_lock_buttons(&mut self, ui: &mut egui::Ui) {
        let enabled = self.tool.is_drawing() || self.tool == Tool::Move || self.tool.is_measuring();
        let colors = Palette::for_ui(ui);
        // The viewport toolbar flows right to left: show X, Y, then Z.
        for (axis, label) in [
            (Axis::Z, "Toggle drawing lock to Z axis / XY plane (↑)"),
            (Axis::Y, "Toggle drawing lock to Y axis / XZ plane (←)"),
            (Axis::X, "Toggle drawing lock to X axis / YZ plane (→)"),
        ] {
            let active = self.lock == Some(axis);
            let response = ui.add_enabled(
                enabled,
                egui::Button::new("")
                    .min_size(egui::vec2(32., 32.))
                    .selected(active),
            );
            let opacity = if enabled { 1. } else { 0.3 };
            icons::axis_lock(
                ui.painter(),
                response.rect.center(),
                axis,
                active,
                axis_color(axis).gamma_multiply(opacity),
                colors.muted.gamma_multiply(opacity),
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::Button, enabled, active, label)
            });
            if response.on_hover_text(label).clicked() {
                self.set_axis_lock(if active { None } else { Some(axis) }, ui.ctx());
            }
        }
    }
    fn snapping_menu(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let chevron =
            ui.painter()
                .layout_no_wrap("▾".into(), FontId::proportional(12.), colors.muted);
        let response = ui
            .add(egui::Button::new("").min_size(egui::vec2(chevron.size().x + 40., 32.)))
            .on_hover_text("Configure object and axis snapping");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Snapping"));
        icons::line_icon(
            ui.painter(),
            egui::pos2(response.rect.left() + 16., response.rect.center().y),
            "magnet",
            colors.muted,
        );
        ui.painter().galley(
            egui::pos2(
                response.rect.left() + 32.,
                response.rect.center().y - chevron.size().y * 0.5,
            ),
            chevron,
            colors.muted,
        );
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                Self::menu_style(ui.style_mut());
                let objects_changed = Self::menu_checkbox(ui, &mut self.doc.snap_to_geometry, "Snap to objects")
                    .on_hover_text("Snap drawing to existing endpoints, midpoints, edges, and faces")
                    .changed();
                let axes_changed = Self::menu_checkbox(ui, &mut self.doc.snap_to_axes, "Snap to axes")
                    .on_hover_text("Automatically align drawing with the red, green, and blue axes. Hover an endpoint, midpoint, or edge to acquire axes through that position. Arrow keys can still lock an axis when this is off.")
                    .changed();
                if objects_changed || axes_changed {
                    self.snap = None;
                    self.changed(ui.ctx());
                }
            });
    }
    fn group_emphasis_button(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let response = ui.add(
            egui::Button::new("")
                .min_size(egui::vec2(32., 32.))
                .selected(self.group_emphasis),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::Button,
                ui.is_enabled(),
                self.group_emphasis,
                "Emphasize group",
            )
        });
        icons::group_emphasis(
            ui.painter(),
            response.rect.center(),
            if self.group_emphasis {
                colors.accent
            } else {
                colors.muted
            },
            colors.muted,
        );
        if response.on_hover_text("Emphasize group (Tab): emphasize the group/component currently being edited and show everything else in muted wireframe. Has no visual effect outside a group/component.").clicked() {
            self.group_emphasis = !self.group_emphasis;
            ui.ctx().request_repaint();
        }
    }
    fn viewport_panel(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let ctx = ui.ctx().clone();
        let [r, g, b] =
            crate::environment::for_theme(&self.doc.display, ui.visuals().dark_mode).background;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(r, g, b))).show(ui,|ui|{
            egui::Panel::top("view_options").exact_size(47.).frame(egui::Frame::new().fill(colors.paper).inner_margin(egui::Margin::symmetric(18,7))).show(ui,|ui|{
                ui.horizontal_centered(|ui|{
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                        if Self::icon_button(ui,"panel","Toggle model inspector",true,colors.muted){self.inspector=!self.inspector;}
                        let dark = ctx.theme() == egui::Theme::Dark;
                        if Self::icon_button(ui, if dark { "sun" } else { "moon" }, if dark { "Switch to light mode" } else { "Switch to dark mode" }, true, colors.muted) {
                            ctx.set_theme(if dark { egui::Theme::Light } else { egui::Theme::Dark });
                            ctx.request_repaint();
                        }
                        if Self::icon_button(ui,"fit","Fit model to view",true,colors.muted){self.fit(&ctx);}
                        if Self::icon_button(ui,"origin","Center view on axis origin (0, 0, 0)",true,colors.muted){self.fit_origin(&ctx);}
                        ui.separator();
                        self.drawing_lock_buttons(ui);
                        ui.separator();
                        self.snapping_menu(ui);
                        // This row flows right to left, so emphasis appears before Snapping.
                        self.group_emphasis_button(ui);
                        ui.allocate_ui_with_layout(ui.available_size(),egui::Layout::left_to_right(egui::Align::Center),|ui|{
                            self.edit_context_bar(ui);
                        });
                    });
                });
            });
            let rect=ui.available_rect_before_wrap();self.viewport=rect;
            let response=ui.allocate_rect(rect,Sense::click_and_drag());
            if rect.width()>1.&&rect.height()>1. {
                self.viewport_input(&ctx,&response);
                self.context_menu(&response);
                if self.gpu_ready {self.paint_scene(ui,rect);} else {ui.painter().text(rect.center(),Align2::CENTER_CENTER,"A compatible GPU renderer is required. Try updating your graphics driver.",FontId::proportional(15.),ACCENT);}
                self.paint_tool_cursor(&ctx,&response);
            }
        });
    }
    fn about_window(&mut self, ctx: &egui::Context) {
        let response = Modal::new("about_ketchup", 408.).show(ctx, |modal| {
            modal.header(|ui| {
                let colors = Palette::for_ui(ui);
                ui.horizontal(|ui| {
                    let (logo, _) = ui.allocate_exact_size(egui::vec2(48., 48.), Sense::hover());
                    icons::logo(ui.painter(), logo);
                    ui.add_space(8.);
                    ui.vertical(|ui| {
                        ui.heading("About KetchUp");
                        ui.label(
                            RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                                .color(colors.muted),
                        );
                    });
                });
            });
            modal.body(|ui| {
                let colors = Palette::for_ui(ui);
                ui.label("KetchUp is a 3D modeling app for turning ideas into shapes. Draw precise geometry, shape surfaces, and organize your model with groups and components.");
                ui.add_space(8.);
                ui.label(
                    RichText::new("Your work is saved automatically on this device. Save a .ketchup project to keep a portable copy.")
                        .small()
                        .color(colors.muted),
                );
            });
            modal.footer(|ui| {
                if primary_button(ui, "Close", true).clicked() {
                    ui.close();
                }
            });
        });
        if response.should_close() {
            self.about = false;
        }
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
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Rebuilt by visible Outliner rows; closing the inspector clears the highlight too.
        self.outliner_hover.clear();
        self.poll_project(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.updates.poll(&ctx);
            if self.updates.restarting {
                ui.disable();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        self.keyboard(&ctx);
        if self.project.blocked() {
            let opacity = ui.opacity();
            ui.disable();
            ui.set_opacity(opacity);
        }
        self.top_bar(ui);
        self.status_bar(ui);
        self.tool_rail(ui);
        let inspector_order_before = self.inspector_order;
        self.inspector_panel(ui);
        let theme_before = ctx.options(|options| options.theme_preference);
        self.viewport_panel(ui);
        if theme_before != ctx.options(|options| options.theme_preference)
            && let Some(storage) = frame.storage_mut()
        {
            save_preference(&ctx, storage);
        }
        self.object_creation_window(&ctx);
        self.rename_object_window(&ctx);
        self.text_editor_window(&ctx);
        if self.help {
            self.help_window(&ctx);
        }
        if self.about {
            self.about_window(&ctx);
        }
        self.release_notes.show(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.update_window(&ctx);
        self.project_dialogs(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        Self::window_resize_handles(ui);
        self.persist_inspector_changes(
            &ctx,
            frame.storage_mut(),
            inspector_order_before != self.inspector_order,
        );
        #[cfg(target_arch = "wasm32")]
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            let title = format!("{} — KetchUp", self.doc.name);
            if document.title() != title {
                document.set_title(&title);
            }
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
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_inspector_preferences(storage);
        if self.dirty_since.is_some() {
            self.persist();
        }
    }
    fn on_exit(&mut self) {
        if self.dirty_since.is_some() {
            self.persist();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.updates.ready()
            && !self.updates.restarting
            && self.save_error.is_none()
            && !self.storage_recovery
        {
            // Preserve the file association when the pending update installs on
            // next launch, just as for the explicit Restart now action.
            if let Err(error) = crate::updates::Resume::save(
                &self.doc,
                self.project.path.clone(),
                self.project.modified,
            ) {
                eprintln!("Could not save update resume information: {error}");
            }
        }
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "app_tests.rs"]
mod tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "project_tests.rs"]
mod project_tests;
