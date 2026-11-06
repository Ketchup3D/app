use crate::natural_text::TextUi as _;
use crate::{
    icons,
    modal::{Modal, primary_button},
    renderer, storage,
    theme::*,
};
use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, RichText, Sense, Stroke, Vec2};
#[cfg(test)]
use ketchup_core::History;
#[cfg(test)]
use ketchup_core::infer_move_with_snapping;
use ketchup_core::inference::AxisConstraint;
use ketchup_core::{
    Axis, AxisGuide, Camera, DVec2, DVec3, Document, Inference, Model, ObjectKind, Snap, Tool,
    selection,
};
use std::{collections::BTreeSet, time::Duration};

fn dv(p: Vec2) -> DVec2 {
    DVec2::new(p.x as f64, p.y as f64)
}
fn pv(p: DVec2) -> Vec2 {
    egui::vec2(p.x as f32, p.y as f32)
}
#[cfg(test)]
fn axis_color(a: Axis) -> Color32 {
    match a {
        Axis::X => Color32::from_rgb(192, 94, 76),
        Axis::Y => Color32::from_rgb(97, 145, 92),
        Axis::Z => Color32::from_rgb(90, 133, 176),
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
#[path = "currency.rs"]
mod currency;
#[path = "display_scene.rs"]
mod display_scene;
#[path = "drawing_inference.rs"]
mod drawing_inference;
#[path = "entity_gizmo.rs"]
mod entity_gizmo;
#[path = "section_gizmo.rs"]
mod section_gizmo;
#[path = "finish_settings.rs"]
mod finish_settings;
#[path = "grid.rs"]
mod grid;
#[path = "measurement_tools.rs"]
mod measurement_tools;
#[path = "move_tool.rs"]
mod move_tool;
#[path = "rotate_tool.rs"]
mod rotate_tool;
#[path = "scale_tool.rs"]
mod scale_tool;
#[path = "precision.rs"]
mod precision;
#[path = "projects.rs"]
mod projects;
#[path = "drawing_groups.rs"]
mod drawing_groups;
#[path = "joinery.rs"]
mod joinery;
#[cfg(not(target_arch = "wasm32"))]
#[path = "viewport_benchmark.rs"]
pub(crate) mod viewport_benchmark;

#[path = "fasteners.rs"]
mod fasteners;
#[path = "sharing.rs"]
mod sharing;
#[path = "screenshot_dialog.rs"]
mod screenshot_dialog;
#[path = "searchable_selector.rs"]
mod searchable_selector;
#[path = "settings.rs"]
mod settings;
#[path = "shortcut_input.rs"]
mod shortcut_input;
#[path = "shortcuts.rs"]
mod shortcuts;
#[path = "sidebar.rs"]
mod sidebar;
pub(crate) use sidebar::{modal_size, remember_modal_size};
#[path = "solid_tools.rs"]
mod solid_tools;
#[path = "stock_settings.rs"]
mod stock_settings;
#[path = "text_tools.rs"]
mod text_tools;
#[path = "tag_menu.rs"]
mod tag_menu;
#[path = "time_estimates.rs"]
mod time_estimates;
#[path = "viewports.rs"]
mod viewports;
use context_menu::ContextAction;
use move_tool::{CopyArray, MoveCopy};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum PaintTarget {
    #[default]
    Face,
    Group,
}

const COMPASS_SCALE: f32 = 1.3;
const COMPASS_RADIUS: f32 = 32. * COMPASS_SCALE;
const COMPASS_TIP_RADIUS: f32 = 7. * COMPASS_SCALE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CompassAxis {
    axis: Axis,
    negative: bool,
}

impl CompassAxis {
    fn direction(self) -> DVec3 {
        self.axis.vector() * if self.negative { -1. } else { 1. }
    }

    fn label(self) -> &'static str {
        match (self.axis, self.negative) {
            (Axis::X, false) => "X",
            (Axis::Y, false) => "Y",
            (Axis::Z, false) => "Z",
            (Axis::X, true) => "−X",
            (Axis::Y, true) => "−Y",
            (Axis::Z, true) => "−Z",
        }
    }

    fn view(self) -> ketchup_core::StandardView {
        use ketchup_core::StandardView;
        match (self.axis, self.negative) {
            (Axis::X, false) => StandardView::Right,
            (Axis::Y, false) => StandardView::Back,
            (Axis::Z, false) => StandardView::Top,
            (Axis::X, true) => StandardView::Left,
            (Axis::Y, true) => StandardView::Front,
            (Axis::Z, true) => StandardView::Bottom,
        }
    }
}

enum GestureKind {
    Compass {
        axis: Option<CompassAxis>,
        pivot: Option<DVec3>,
        dragged: bool,
    },
    Navigate {
        pivot: Option<DVec3>,
    },
    DrawingNavigate,
    Select {
        additive: bool,
    },
    Line,
    Shape,
    Freehand {
        paused: bool,
    },
    Modeling,
    Solid,
    Measure,
    Paste,
    Erase {
        ids: BTreeSet<u64>,
    },
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
    pointer: Pos2,
    center: DVec3,
    scale_constraint: Option<AxisConstraint>,
    rotation: Option<rotate_tool::Rotation>,
    direction: DVec3,
    // Keep Push/Pull's original normal when switching or clearing axis locks.
    unlocked_direction: DVec3,
    guides: Vec<AxisGuide>,
    value: f64,
    copy: bool,
    preview: Option<Model>,
    move_snap: Option<ketchup_core::inference::MoveSnapCache>,
    snap_excluded: BTreeSet<u64>,
    move_preview: Option<ketchup_core::modeling::MovePreview>,
    preview_delta: Option<DVec3>,
    snap: Option<Snap>,
    error: Option<&'static str>,
}

struct Drawing {
    origin: DVec3,
    surface_normal: DVec3,
    on_surface: bool,
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
                // normal as close to the starting orientation as possible.
                let projected =
                    self.surface_normal - direction * self.surface_normal.dot(direction);
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
    viewport_benchmark: Option<viewport_benchmark::Benchmark>,
    account: crate::account::Account,
    joinery_geometry: joinery::GeometryCache,
    joinery: joinery::Preferences,
    joinery_dialog: joinery::Dialog,
    fasteners: fasteners::Preferences,
    fastener_dialog: fasteners::Dialog,
    sharing: sharing::Sharing,
    #[cfg(not(target_arch = "wasm32"))]
    updates: crate::updates::Updater,
    doc: Document,
    project: projects::Project,
    screenshot: screenshot_dialog::ScreenshotDialog,
    screenshot_gpu: Option<(eframe::wgpu::Device, eframe::wgpu::Queue)>,
    tool: Tool,
    drawing_tool: Tool,
    measurement_tool: Tool,
    solid_tool: Tool,
    solid_first: Option<u64>,
    solid_volume_cache: solid_tools::VolumeCache,
    flip_hover: Option<Axis>,
    drawing: Option<Drawing>,
    polygon_sides: usize,
    selected: BTreeSet<u64>,
    hover: Option<u64>,
    outliner_hover: BTreeSet<u64>,
    history_preview: Option<Model>,
    outliner_anchor: Option<u64>,
    outliner_filter: String,
    outliner_cache: sidebar::OutlinerCache,
    focus_outliner_search: bool,
    line_start: Option<DVec3>,
    line_group: Option<u64>,
    snap: Option<Snap>,
    snap_to_grid: bool,
    snap_to_edge: bool,
    snap_to_face: bool,
    snap_to_face_center: bool,
    drawing_reference: Option<drawing_inference::DrawingReference>,
    drawing_guide: Option<AxisGuide>,
    drawing_axis_guides: Vec<AxisGuide>,
    drawing_projection: Option<(DVec3, DVec3)>,
    lock: Option<Axis>,
    second_lock: Option<Axis>,
    length_input: String,
    modeling: Option<Modeling>,
    move_copy: bool,
    move_modifier_down: bool,
    last_move_copy: Option<MoveCopy>,
    measurement: Option<measurement_tools::Measurement>,
    text_editor: Option<text_tools::TextEditor>,
    paint_color: [u8; 3],
    paint_target: PaintTarget,
    gesture: Option<Gesture>,
    section_drag: Option<section_gizmo::SectionDrag>,
    drawing_orbit_down: bool,
    drawing_orbit_pivot: Option<DVec3>,
    viewport_pointer: Option<Pos2>,
    viewport: Rect,
    viewports: viewports::Workspace,
    inspector: bool,
    inspector_width: f32,
    inspector_preferences: sidebar::InspectorPreferences,
    time_estimates: time_estimates::Settings,
    finish_product_filters: std::collections::BTreeMap<String, String>,
    stock_material_filter: String,
    wood_type_filters: std::collections::BTreeMap<String, String>,
    estimate_cache: time_estimates::EstimateCache,
    inspector_order: [sidebar::InspectorSection; sidebar::InspectorSection::COUNT],
    inspector_layout: sidebar::InspectorLayout,
    component_filter: String,
    new_tag_name: String,
    tag_rename: Option<(u64, String)>,
    help: bool,
    settings_open: bool,
    material_removal: Option<ketchup_core::MaterialId>,
    general_settings: settings::GeneralSettings,
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
    render_model: renderer::ModelSnapshot,
    shadow_cache: crate::shadows::ShadowCache,
    section_cache: display_scene::SectionCache,
    picking_cache: crate::picking::PickingCache,
    // Rendering consumes machined/preview geometry. Input queries consume the
    // source document. Keep both sets alive instead of alternately invalidating
    // their triangulation, BVH and the shadow cache on every pointer event.
    render_face_cache: renderer::FaceCache,
    group_marker_cache: renderer::GroupMarkerCache,
    render_entity_cache: renderer::EntityCache,
    render_picking_cache: crate::picking::PickingCache,
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
        app.screenshot_gpu = cc
            .wgpu_render_state
            .as_ref()
            .map(|state| (state.device.clone(), state.queue.clone()));
        app.restore_inspector_preferences(&cc.egui_ctx, cc.storage);
        app.release_notes.open_if_unread();
        app.account.start(&cc.egui_ctx);
        app.open_shared_startup(&cc.egui_ctx);
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
        #[cfg(not(target_arch = "wasm32"))]
        let app = {
            let mut app = app;
            app.viewport_benchmark = viewport_benchmark::Benchmark::from_environment(&mut app);
            app
        };
        app
    }
    fn with_document(mut doc: Document, error: Option<String>, gpu_ready: bool) -> Self {
        let selected = Self::restore_document_selection(&mut doc);
        let geometry_snapping = doc.snap_to_geometry;
        let recovery = error.is_some();
        let modified = doc.model != Model::default()
            || doc.name != Document::default().name
            || doc.camera != Camera::default()
            || !doc.snap_to_geometry
            || !doc.snap_to_axes
            || doc.display != ketchup_core::DisplaySettings::default();
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            viewport_benchmark: None,
            #[cfg(not(target_arch = "wasm32"))]
            updates: crate::updates::Updater::default(),
            account: Default::default(),
            joinery_geometry: Default::default(),
            joinery: Default::default(),
            joinery_dialog: Default::default(),
            fasteners: Default::default(),
            fastener_dialog: Default::default(),
            sharing: Default::default(),
            doc,
            project: projects::Project {
                modified,
                ..Default::default()
            },
            screenshot: screenshot_dialog::ScreenshotDialog::default(),
            screenshot_gpu: None,
            tool: Tool::Select,
            drawing_tool: Tool::Line,
            measurement_tool: Tool::Dimensions,
            solid_tool: Tool::Union,
            solid_first: None,
            solid_volume_cache: Default::default(),
            flip_hover: None,
            drawing: None,
            polygon_sides: 6,
            selected,
            hover: None,
            outliner_hover: BTreeSet::new(),
            history_preview: None,
            outliner_anchor: None,
            outliner_filter: String::new(),
            outliner_cache: Default::default(),
            focus_outliner_search: false,
            line_start: None,
            line_group: None,
            snap: None,
            snap_to_grid: false,
            snap_to_edge: geometry_snapping,
            snap_to_face: geometry_snapping,
            snap_to_face_center: geometry_snapping,
            drawing_reference: None,
            drawing_guide: None,
            drawing_axis_guides: vec![],
            drawing_projection: None,
            lock: None,
            second_lock: None,
            length_input: String::new(),
            modeling: None,
            move_copy: false,
            move_modifier_down: false,
            last_move_copy: None,
            measurement: None,
            text_editor: None,
            paint_color: [195, 96, 67],
            paint_target: PaintTarget::default(),
            gesture: None,
            section_drag: None,
            drawing_orbit_down: false,
            drawing_orbit_pivot: None,
            viewport_pointer: None,
            viewport: Rect::NOTHING,
            viewports: viewports::Workspace::default(),
            inspector: true,
            inspector_width: 300.,
            inspector_preferences: sidebar::InspectorPreferences::default(),
            time_estimates: Default::default(),
            finish_product_filters: Default::default(),
            stock_material_filter: String::new(),
            wood_type_filters: Default::default(),
            estimate_cache: Default::default(),
            inspector_order: sidebar::InspectorSection::ALL,
            inspector_layout: sidebar::InspectorLayout::default(),
            component_filter: String::new(),
            new_tag_name: String::new(),
            tag_rename: None,
            help: false,
            settings_open: false,
            material_removal: None,
            general_settings: Default::default(),
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
            render_model: renderer::ModelSnapshot::default(),
            shadow_cache: crate::shadows::ShadowCache::default(),
            section_cache: Default::default(),
            picking_cache: crate::picking::PickingCache::default(),
            render_face_cache: renderer::FaceCache::default(),
            group_marker_cache: Default::default(),
            render_entity_cache: renderer::EntityCache::default(),
            render_picking_cache: crate::picking::PickingCache::default(),
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
        self.line_group = None;
        self.snap = None;
        self.lock = None;
        self.second_lock = None;
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
        if before != self.doc.model && !self.shared_edit_allowed(ctx) {
            self.doc.model = before;
            self.say(ctx, self.shared_edit_block_reason(ctx));
            return;
        }
        if before != self.doc.model {
            self.solid_first = None;
            self.last_move_copy = None;
            self.measurement = None;
            self.modeling = None;
            let author = self.current_history_author();
            self.doc.history.record_attributed(label, before, &self.doc.model, Some(author), Some(sharing::now_timestamp()));
            self.selected.retain(|id| {
                self.doc.model.entity_in_context(*id) == Some(*id) && !self.doc.model.is_hidden(*id)
            });
            self.changed(ctx);
            #[cfg(target_arch = "wasm32")]
            self.persist();
        }
    }
    fn undo(&mut self, ctx: &egui::Context, redo: bool) {
        let current = self.doc.history.position();
        let target = if redo {
            current + 1
        } else {
            current.saturating_sub(1)
        };
        self.navigate_history(ctx, target);
    }
    fn preview_history(&mut self, target: usize) {
        self.history_preview = None;
        if target == self.doc.history.position() {
            return;
        }
        let mut history = self.doc.history.clone();
        let mut model = self.doc.model.clone();
        if history.seek(&mut model, target) {
            self.history_preview = Some(model);
        }
    }
    fn navigate_history(&mut self, ctx: &egui::Context, target: usize) {
        if !self.shared_edit_allowed(ctx) { self.say(ctx, self.shared_edit_block_reason(ctx)); return; }
        self.history_preview = None;
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
        self.line_group = None;
        self.snap = None;
        self.length_input.clear();
        let path = self.doc.model.edit_path();
        let author = self.current_history_author();
        let did = self.doc.history.seek_attributed(&mut self.doc.model, target, Some(author), Some(sharing::now_timestamp()));
        if did {
            self.outliner_anchor = None;
            self.doc.model.edit_context = path.into_iter().rev().find(|id| {
                self.doc.model.objects.contains_key(id)
                    && !self.doc.model.is_locked(*id)
                    && !self.doc.model.is_hidden(*id)
            });
            self.selected.clear();
            self.hover = None;
            self.changed(ctx);
            #[cfg(target_arch = "wasm32")]
            self.persist();
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
            ui.ctx().request_repaint();
        }
    }
    fn name_input(ui: &mut egui::Ui, name: &mut String, id: egui::Id) -> egui::Response {
        // On the first visible frame, focus before TextEdit reads typed input.
        // The opening click still needs the post-registration focus below.
        if !ui.input(|input| input.pointer.any_pressed() || input.pointer.any_released()) {
            Self::focus_name_input(ui, name, id);
        }
        let response = ui.add(
            egui::TextEdit::singleline(&mut *name)
                .id(id)
                .desired_width(ui.available_width()),
        );
        // Register the widget first: egui can surrender focus while processing
        // the click that opened a previously sized dialog.
        Self::focus_name_input(ui, name, id);
        response
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
                ui.text_heading(if component {
                    "Create Component"
                } else {
                    "Make Group"
                });
                ui.text_weak("Give your selection a name.");
            });
            modal.body(|ui| {
                ui.text_label("Name");
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
            snap_to_geometry: self.doc.snap_to_geometry,
            snap_to_axes: self.doc.snap_to_axes,
            display: self.doc.display.clone(),
            camera: Camera {
                projection: self.doc.camera.projection,
                fov: self.doc.camera.fov,
                ..Camera::default()
            },
            ..Document::default()
        };
        let mut doc = doc;
        doc.display.units = self.general_settings.default_units;
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
    fn stop_drawing(&mut self) {
        self.drawing_reference = None;
        self.drawing_guide = None;
        self.drawing_axis_guides.clear();
        self.drawing_projection = None;
        self.drawing = None;
        self.gesture = None;
        self.line_start = None;
        self.line_group = None;
        self.snap = None;
        self.lock = None;
        self.second_lock = None;
        self.length_input.clear();
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
            match precision::positive_length(&self.length_input, self.doc.display.units.length) {
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
        match self.drawn_geometry(&[start, end], self.line_group) {
            Ok((model, group, closed_shape)) => {
                self.doc.model = model;
                self.line_group = group;
                if let Some(group) = group { self.selected = BTreeSet::from([group]); }
                self.finish_edit("Draw line", before, ctx);
                if closed_shape {
                    self.stop_drawing();
                    self.prompt_drawn_group_name(ctx, group);
                } else {
                    self.line_start = Some(end);
                    self.length_input.clear();
                    self.refresh_snap(ctx);
                }
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
        let surface = self
            .doc
            .model
            .nearest_face(ray)
            .and_then(|(id, _, _)| {
                ketchup_core::faces::plane(&self.doc.model, &self.doc.model.faces[&id].vertices)
            })
            .filter(|plane| (snap.point - plane.origin).dot(plane.normal).abs() < 1e-6);
        let normal = surface.map_or(DVec3::Z, |plane| plane.normal);
        let u = (DVec3::X - normal * normal.x)
            .try_normalize()
            .unwrap_or(DVec3::Y);
        let mut drawing = Drawing {
            origin: snap.point,
            surface_normal: normal,
            on_surface: surface.is_some(),
            normal,
            u,
            v: normal.cross(u),
            points: vec![snap.point],
        };
        drawing.set_lock(self.shape_lock());
        self.drawing = Some(drawing);
        self.update_pointer(pos);
    }
    fn set_axis_lock(&mut self, lock: Option<Axis>, ctx: &egui::Context) {
        self.lock = lock;
        self.second_lock = None;
        self.refresh_axis_lock(ctx);
    }

    fn plane_lock(&self) -> Option<Axis> {
        let (first, second) = self.lock.zip(self.second_lock)?;
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .find(|axis| *axis != first && *axis != second)
    }

    fn single_axis_lock(&self) -> Option<Axis> {
        self.lock.filter(|_| self.second_lock.is_none())
    }

    fn axis_constraint(&self) -> Option<AxisConstraint> {
        self.plane_lock()
            .map(AxisConstraint::Plane)
            .or_else(|| self.lock.map(AxisConstraint::Axis))
    }

    fn shape_lock(&self) -> Option<Axis> {
        self.plane_lock().or(self.lock)
    }

    fn pull_direction(&self, normal: DVec3) -> DVec3 {
        match self.axis_constraint() {
            Some(AxisConstraint::Axis(axis)) => axis.vector(),
            Some(constraint) => constraint
                .project(normal)
                .try_normalize()
                .unwrap_or_else(|| self.lock.unwrap().vector()),
            None => normal,
        }
    }

    fn toggle_axis_lock(&mut self, axis: Axis, ctx: &egui::Context) {
        if self.lock == Some(axis) {
            self.lock = self.second_lock.take();
        } else if self.second_lock == Some(axis) {
            self.second_lock = None;
        } else if self.lock.is_none() {
            self.lock = Some(axis);
        } else if self.second_lock.is_none() {
            self.second_lock = Some(axis);
        } else {
            self.say(ctx, "Unlock an axis before locking another (maximum two)");
            return;
        }
        self.refresh_axis_lock(ctx);
    }

    fn refresh_axis_lock(&mut self, ctx: &egui::Context) {
        let lock = self.shape_lock();
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
            .or_else(|| {
                self.modeling.as_ref()
                    .filter(|_| matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale | Tool::PushPull | Tool::Offset))
                    .map(|edit| edit.pointer)
            })
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
                // A soft local axis must not pull a shape off its starting face.
                && !(self.lock.is_none()
                    && matches!(snap.kind, Inference::Axis(_))
                    && drawing.on_surface
                    && (snap.point - drawing.origin).dot(drawing.surface_normal).abs() > 1e-6)
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
            && self.snap_to_edge
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
        let (model, group, _) = match self.drawn_geometry(&points, None) {
            Ok(result) => result,
            Err(message) => { self.say(ctx, message); return; }
        };
        self.doc.model = model;
        if let Some(group) = group { self.selected = BTreeSet::from([group]); }
        self.finish_edit(
            &format!("Draw {}", self.tool.label().to_lowercase()),
            before,
            ctx,
        );
        self.stop_drawing();
        self.prompt_drawn_group_name(ctx, group);
    }
    // A selected single-face group can be edited without opening its context.
    fn selected_modeling_face(&self) -> Option<u64> {
        if !matches!(self.tool, Tool::PushPull | Tool::Offset) || self.selected.len() != 1 {
            return None;
        }
        let group = *self.selected.first()?;
        let model = &self.doc.model;
        if !model.objects.contains_key(&group)
            || model.entity_in_context(group) != Some(group)
            || model.is_locked(group)
            || model.is_hidden(group)
        {
            return None;
        }
        let mut faces = model.faces.keys().copied().filter(|id| {
            model.entity_in_context(*id) == Some(group)
        });
        let face = faces.next()?;
        (faces.next().is_none() && !model.is_hidden(face) && !model.is_locked(face))
            .then_some(face)
    }
    fn selection_anchor_snap(&mut self, pos: Pos2) -> Option<Snap> {
        self.prepare_picking();
        let snap = self.infer_snap(pos, None, None, self.doc.snap_to_axes)?;
        Some(if self.snap_to_grid {
            grid::snap_with_geometry(
                snap,
                grid::configured_spacing(&self.doc.camera, &self.doc.display),
                None,
                None,
                &self.doc.model,
                None,
            )
        } else {
            snap
        })
    }

    fn face_anchor_contains(&self, id: u64, point: DVec3) -> bool {
        let model = &self.doc.model;
        let face = &model.faces[&id];
        if model.face_contains(face, point, true) {
            return true;
        }
        // Hole boundaries are also valid edge/vertex anchors, though they are
        // excluded from the filled face's interior hit test.
        let Some(plane) = ketchup_core::faces::plane(model, &face.vertices) else {
            return false;
        };
        (point - plane.origin).dot(plane.normal).abs() < 1e-6
            && face.holes.iter().any(|ring| (0..ring.len()).any(|i| {
                ketchup_core::selection::distance_to_segment(
                    plane.project(point),
                    plane.project(model.vertices[&ring[i]].position),
                    plane.project(model.vertices[&ring[(i + 1) % ring.len()]].position),
                ) < 1e-6
            }))
    }

    fn face_anchor_snap(&mut self, pos: Pos2) -> Option<(u64, Snap)> {
        self.prepare_picking();
        let ray = self.doc.camera.ray(dv(pos - self.viewport.min), dv(self.viewport.size()));
        let selected = self.selected_modeling_face();
        let inferred = self.infer_snap(pos, None, None, self.doc.snap_to_axes);
        let hit = selected.and_then(|id| {
            self.doc.model.face_hit(&self.doc.model.faces[&id], ray)
                .map(|(_, point)| (id, point))
        }).or_else(|| self.doc.model.nearest_editable_face(ray).map(|(id, _, point)| (id, point)));
        // A nearby edge/vertex can acquire the face even when the raw pointer
        // lies just outside its boundary. Closed groups still need a unique face.
        let (id, point) = hit.or_else(|| {
            let snap = inferred.filter(|snap| snap.kind != Inference::Plane)?;
            let id = selected.filter(|id| self.face_anchor_contains(*id, snap.point))
                .or_else(|| self.doc.model.faces.keys().rev().copied().find(|id| {
                    self.doc.model.parent(*id) == self.doc.model.edit_context
                        && !self.doc.model.is_hidden(*id)
                        && self.face_anchor_contains(*id, snap.point)
                }))?;
            Some((id, snap.point))
        })?;
        let snap = inferred.filter(|snap| {
            snap.kind != Inference::Plane && self.face_anchor_contains(id, snap.point)
        }).unwrap_or(Snap { point, kind: Inference::Plane });
        let snap = if self.snap_to_grid {
            let face = &self.doc.model.faces[&id];
            let plane = ketchup_core::faces::plane(&self.doc.model, &face.vertices)?;
            let quantized = grid::snap_with_geometry(
                snap,
                grid::configured_spacing(&self.doc.camera, &self.doc.display),
                Some(plane.normal),
                grid::movement_axis(snap.point, None, None, self.doc.snap_to_axes),
                &self.doc.model,
                None,
            );
            if self.face_anchor_contains(id, quantized.point) { quantized } else { snap }
        } else {
            snap
        };
        Some((id, snap))
    }

    fn begin_modeling(&mut self, pos: Pos2, ctx: &egui::Context) {
        self.last_move_copy = None;
        let size = dv(self.viewport.size());
        let pixel = dv(pos - self.viewport.min);
        let ray = self.doc.camera.ray(pixel, size);
        if self.tool == Tool::Paint {
            if let Some((face, _, _)) = self.doc.model.nearest_context_face(ray) {
                let before = self.doc.model.clone();
                let (result, label) = match self.paint_target {
                    PaintTarget::Face => (
                        self.doc.model.paint_face(face, self.paint_color),
                        "Paint face",
                    ),
                    PaintTarget::Group => {
                        let target = self.doc.model.entity_in_context(face).unwrap();
                        (
                            self.doc.model.paint_object(target, self.paint_color),
                            "Paint group",
                        )
                    }
                };
                match result {
                    Ok(()) => self.finish_edit(label, before, ctx),
                    Err(message) => self.say(ctx, message),
                }
            } else {
                self.say(ctx, self.paint_instructions());
            }
            return;
        }
        let mut face_id = None;
        let (anchor, direction) = if matches!(self.tool, Tool::PushPull | Tool::Offset) {
            let selected_face = self.selected_modeling_face();
            let Some((id, snap)) = self.face_anchor_snap(pos) else {
                self.say(ctx, "Click a filled face first");
                return;
            };
            if self.doc.model.is_locked(id) {
                self.say(ctx, "Unlock the face before editing it");
                return;
            }
            face_id = Some(id);
            let point = snap.point;
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
                let model = &self.doc.model;
                let closest = std::iter::once(&face.vertices)
                    .chain(face.holes.iter())
                    .enumerate()
                    .flat_map(|(hole, ring)| {
                        let area: f64 = (0..ring.len()).map(|i| {
                            plane.project(model.vertices[&ring[i]].position).perp_dot(
                                plane.project(model.vertices[&ring[(i + 1) % ring.len()]].position),
                            )
                        }).sum();
                        let sign = area.signum() * if hole == 0 { 1. } else { -1. };
                        (0..ring.len()).map(move |i| {
                            let a = model.vertices[&ring[i]].position;
                            let b = model.vertices[&ring[(i + 1) % ring.len()]].position;
                            let closest = a + (b - a)
                                * ((point - a).dot(b - a) / (b - a).length_squared()).clamp(0., 1.);
                            (closest, plane.normal.cross(b - a).normalize() * sign)
                        })
                    })
                    .min_by(|a, b| {
                        a.0.distance_squared(point)
                            .total_cmp(&b.0.distance_squared(point))
                    })
                    .unwrap();
                let delta = point - closest.0;
                if delta.length() > 1e-7 { delta.normalize() } else { closest.1 }
            };
            if selected_face != Some(id) {
                self.selected = BTreeSet::from([id]);
            }
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
            let Some(snap) = self.selection_anchor_snap(pos) else {
                return;
            };
            (snap.point, DVec3::ZERO)
        };
        let center = self
            .doc
            .model
            .selection_center(&self.selected)
            .unwrap_or(anchor);
        let snap_excluded = self.modeling_snap_exclusions(face_id);
        let rotation = (self.tool == Tool::Rotate).then(|| self.rotation_at(anchor, ray));
        self.modeling = Some(Modeling {
            base: self.doc.model.clone(),
            entities: self.selected.clone(),
            face: face_id,
            anchor,
            pixel: pos,
            pointer: pos,
            center,
            scale_constraint: self.axis_constraint(),
            rotation,
            direction: if self.tool == Tool::PushPull {
                self.pull_direction(direction)
            } else {
                direction
            },
            unlocked_direction: direction,
            guides: vec![],
            value: if self.tool == Tool::Scale { 1. } else { 0. },
            copy: self.tool == Tool::Move && self.move_copy,
            preview: None,
            move_snap: matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale)
                .then(|| {
                    if self.tool == Tool::Rotate {
                        ketchup_core::inference::MoveSnapCache::new(&self.doc.model)
                    } else {
                        ketchup_core::inference::MoveSnapCache::excluding(&self.doc.model, &snap_excluded)
                            .with_x_ray(self.tool == Tool::Move)
                    }
                }),
            snap_excluded,
            move_preview: (self.tool == Tool::Move)
                .then(|| {
                    ketchup_core::modeling::MovePreview::new(&self.doc.model, &self.selected).ok()
                })
                .flatten(),
            preview_delta: None,
            snap: None,
            error: None,
        });
        self.length_input.clear();
    }

    fn modeling_snap_exclusions(&self, face: Option<u64>) -> BTreeSet<u64> {
        let model = &self.doc.model;
        let selected = face.map(|id| BTreeSet::from([id]));
        let vertices = model.selection_vertices(selected.as_ref().unwrap_or(&self.selected));
        let mut excluded: BTreeSet<_> = model
            .edges
            .values()
            .filter(|edge| {
                if face.is_some() {
                    vertices.contains(&edge.a) && vertices.contains(&edge.b)
                } else {
                    vertices.contains(&edge.a) || vertices.contains(&edge.b)
                }
            })
            .map(|edge| edge.id)
            .collect();
        if let Some(face) = face {
            excluded.insert(face);
        } else {
            // Connected surfaces also change when any of their vertices move.
            excluded.extend(
                model.faces.values().filter(|face| {
                    face.vertices.iter().chain(face.holes.iter().flatten())
                        .any(|id| vertices.contains(id))
                }).map(|face| face.id),
            );
        }
        excluded
    }

    fn offset_snap(&mut self, pos: Pos2) -> Option<Snap> {
        self.prepare_picking();
        let edit = self.modeling.as_ref()?;
        let face = &edit.base.faces[&edit.face?];
        let plane = ketchup_core::faces::plane(&edit.base, &face.vertices)?;
        let constraint = self.axis_constraint();
        let snap = self
            .infer_snap(pos, Some(edit.anchor), self.lock, self.doc.snap_to_axes)
            // Free Offset follows its face, including tilted and vertical faces.
            // Explicit locks use the shared axis/plane through the anchor.
            .filter(|snap| snap.kind != Inference::Plane || constraint.is_some())
            .or_else(|| {
                if constraint.is_some() {
                    return None;
                }
                let ray = self.doc.camera.ray(dv(pos - self.viewport.min), dv(self.viewport.size()));
                let denominator = ray.direction.dot(plane.normal);
                if denominator.abs() < 1e-8 {
                    return None;
                }
                let t = (edit.anchor - ray.origin).dot(plane.normal) / denominator;
                (0.0..1e7).contains(&t).then_some(Snap {
                    point: ray.origin + ray.direction * t,
                    kind: Inference::Plane,
                })
            })?;
        Some(if self.snap_to_grid {
            grid::snap_with_geometry(
                snap,
                grid::configured_spacing(&self.doc.camera, &self.doc.display),
                match constraint {
                    Some(AxisConstraint::Plane(normal)) => Some(normal.vector()),
                    Some(AxisConstraint::Axis(_)) => None,
                    None => Some(plane.normal),
                },
                grid::movement_axis(snap.point, Some(edit.anchor), constraint, self.doc.snap_to_axes),
                &edit.base,
                Some(&edit.snap_excluded),
            )
        } else {
            snap
        })
    }

    fn update_modeling(&mut self, pos: Pos2) {
        if self.tool == Tool::Rotate {
            self.update_rotation(pos);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = viewport_benchmark::scope("modeling_preview");
        // Recompute when a lock is set, switched, or cleared during a pull.
        let constraint = self.axis_constraint();
        let pull_direction = self.modeling.as_ref()
            .map(|edit| self.pull_direction(edit.unlocked_direction));
        if self.tool == Tool::PushPull
            && let Some(edit) = self.modeling.as_mut()
        {
            edit.direction = pull_direction.unwrap();
        }
        // References may be anywhere under the cursor, including geometry
        // outside the current group. Project them onto the extrusion direction.
        let pull_snap = if self.tool == Tool::PushPull
            && self.modeling.is_some()
            && self.length_input.trim().is_empty()
            && (self.doc.snap_to_geometry || self.doc.snap_to_axes)
        {
            self.prepare_picking();
            self.infer_snap(pos, None, None, self.doc.snap_to_axes).filter(|snap| {
                let edit = self.modeling.as_ref().unwrap();
                matches!(snap.kind, Inference::Origin | Inference::Endpoint
                    | Inference::Midpoint | Inference::FaceCenter | Inference::OnEdge | Inference::OnFace
                    | Inference::OnAxis(_) | Inference::Axis(_))
                    // The unchanged starting surface must not swallow free
                    // movement while the preview moves away from it.
                    && (snap.kind != Inference::OnFace
                        || (snap.point - edit.anchor).dot(edit.direction).abs() > 1e-7)
            })
        } else {
            None
        };
        let pull_snap = pull_snap.map(|snap| {
            if self.snap_to_grid {
                grid::snap_with_geometry(
                    snap,
                    grid::configured_spacing(&self.doc.camera, &self.doc.display),
                    None,
                    None,
                    &self.doc.model,
                    self.modeling.as_ref().map(|edit| &edit.snap_excluded),
                )
            } else {
                snap
            }
        });
        let offset_snap = (self.tool == Tool::Offset && self.length_input.trim().is_empty())
            .then(|| self.offset_snap(pos))
            .flatten();
        let Some(edit) = self.modeling.as_mut() else {
            return;
        };
        edit.pointer = pos;
        let size = dv(self.viewport.size());
        let ray = self.doc.camera.ray(dv(pos - self.viewport.min), size);
        edit.value = match self.tool {
            Tool::PushPull => {
                edit.snap = pull_snap;
                pull_snap
                    .map(|snap| (snap.point - edit.anchor).dot(edit.direction))
                    .unwrap_or_else(|| {
                        ray.axis(edit.anchor, edit.direction)
                            .map(|p| (p - edit.anchor).dot(edit.direction))
                            .unwrap_or(
                                (edit.pixel.y - pos.y) as f64 * self.doc.camera.distance / size.y,
                            )
                    })
            }
            Tool::Offset => {
                edit.snap = offset_snap;
                offset_snap.map_or(edit.value, |snap| {
                    (snap.point - edit.anchor).dot(edit.direction)
                })
            }
            Tool::Move | Tool::Scale => {
                edit.scale_constraint = constraint;
                edit.guides.clear();
                edit.snap = None;
                if let Some(inference) = edit.move_snap.as_mut().unwrap().infer_with_constraint(
                    &self.doc.camera,
                    size,
                    dv(pos - self.viewport.min),
                    edit.anchor,
                    constraint,
                    self.doc.snap_to_geometry,
                    self.doc.snap_to_axes,
                    ketchup_core::inference::SnapTargets {
                        edges: self.snap_to_edge,
                        faces: self.snap_to_face,
                        face_centers: self.snap_to_face_center,
                    },
                ) {
                    let snap = if self.tool == Tool::Scale {
                        Self::scale_snap(
                            edit,
                            inference.snap,
                            self.snap_to_grid.then(|| {
                                grid::configured_spacing(&self.doc.camera, &self.doc.display)
                            }),
                            self.doc.snap_to_axes,
                        )
                    } else if self.snap_to_grid {
                        grid::snap_with_geometry(
                            inference.snap,
                            grid::configured_spacing(&self.doc.camera, &self.doc.display),
                            match constraint {
                                Some(AxisConstraint::Plane(normal)) => Some(normal.vector()),
                                _ => None,
                            },
                            grid::movement_axis(
                                inference.snap.point,
                                Some(edit.anchor),
                                constraint,
                                self.doc.snap_to_axes,
                            ),
                            &edit.base,
                            Some(&edit.snap_excluded),
                        )
                    } else {
                        inference.snap
                    };
                    edit.direction = snap.point - edit.anchor;
                    edit.guides = inference.guides;
                    edit.snap = Some(snap);
                } else if let Some(constraint) = constraint {
                    edit.direction = constraint.project(edit.direction);
                }
                if self.tool == Tool::Scale {
                    // Crossing the center changes the reference's sign, not its size.
                    Self::scale_value(edit, &self.doc.camera, size, self.snap_to_grid).abs()
                } else {
                    edit.direction.length()
                }
            }
            _ => return,
        };
        // Reference targets already combine geometry, axes and grid above.
        // Free extrusion uses grid distances along the normal or locked axis.
        if self.tool == Tool::PushPull && self.snap_to_grid && pull_snap.is_none()
            && self.length_input.trim().is_empty()
        {
            let spacing = grid::configured_spacing(&self.doc.camera, &self.doc.display);
            edit.value = (edit.value / spacing).round() * spacing;
        }
        let value =
            precision::modeling_value_with_units(self.tool, &self.length_input, self.doc.display.units)
                .unwrap_or(edit.value);
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

    fn modeling_preview_snap(&self) -> Option<Snap> {
        let edit = self.modeling.as_ref()?;
        if self.length_input.trim().is_empty() && let Some(snap) = edit.snap {
            return Some(snap);
        }
        // Typed values move the live marker to the same point as the preview,
        // just as typed drawing lengths move the drawing tool's endpoint.
        let value = precision::modeling_value_with_units(
            self.tool, &self.length_input, self.doc.display.units,
        ).unwrap_or(edit.value);
        let point = match self.tool {
            Tool::Move => edit.anchor + Self::move_delta(edit, value).ok()?,
            Tool::Rotate => Self::rotation_marker(edit, value)?,
            Tool::Scale => {
                let reference = edit.anchor - edit.center;
                let reference = edit.scale_constraint.map_or(reference, |lock| lock.project(reference));
                edit.anchor + reference * (value - 1.)
            }
            Tool::PushPull | Tool::Offset => edit.anchor + edit.direction * value,
            _ => return None,
        };
        let kind = edit.snap.filter(|snap| snap.point.distance(point) < 1e-6)
            .map(|snap| snap.kind)
            .unwrap_or_else(|| self.single_axis_lock().map_or(Inference::Plane, Inference::Axis));
        Some(Snap { point, kind })
    }

    fn modeling_result(tool: Tool, edit: &Modeling, value: f64) -> Result<Model, &'static str> {
        let mut model = edit.base.clone();
        match tool {
            Tool::PushPull => {
                let face = &model.faces[&edit.face.unwrap()];
                let face_id = face.id;
                let context = model.edit_context;
                model.edit_context = model.parent(face_id);
                model.push_pull_along(face_id, edit.direction * value)?;
                model.edit_context = context;
            }
            Tool::Offset => {
                let face = edit.face.unwrap();
                let context = model.edit_context;
                model.edit_context = model.parent(face);
                model.offset_face(face, value)?;
                model.edit_context = context;
            }
            Tool::Rotate => {
                let rotation = edit.rotation.as_ref().unwrap();
                if rotation.reference.is_none() {
                    return Err("Click a reference point away from the pivot first");
                }
                let reference = rotation.reference.unwrap();
                if (reference - rotation.normal * reference.dot(rotation.normal)).length_squared() < 1e-12 {
                    return Err("The reference lies on the rotation axis; clear the lock or restart with another reference");
                }
                model.rotate_selection(&edit.entities, edit.anchor, rotation.normal, value)?;
            }
            Tool::Scale => model.scale_selection_constrained(
                &edit.entities, edit.center, value, edit.scale_constraint,
            )?,
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
        if self.tool == Tool::Rotate && self.choose_rotation_reference(ctx) {
            return;
        }
        let Some(edit) = self.modeling.as_ref() else {
            return;
        };
        let value = if self.length_input.trim().is_empty() {
            edit.value
        } else {
            match precision::modeling_value_with_units(
                self.tool,
                &self.length_input,
                self.doc.display.units,
            ) {
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
                    let group = edit.face.and_then(|face| before.entity_in_context(face))
                        .filter(|id| before.objects.contains_key(id));
                    self.selected = group.into_iter().chain(model
                        .faces
                        .keys()
                        .filter(|id| {
                            !before.faces.contains_key(id)
                                && model.parent(**id) == model.edit_context
                        })
                        .next_back()
                        .copied()
                        .or(edit.face)
                        .filter(|_| group.is_none()))
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

        if self.project.blocked() || self.screenshot.open {
            return;
        }
        if self.section_drag.is_some() {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.cancel_section_drag(ctx);
            }
            return;
        }
        if self.settings_open {
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
            && self.viewports.renaming.is_none()
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
            if self.joinery.open && self.object_creation.is_none()
                && self.object_rename.is_none() && self.viewports.renaming.is_none()
                && self.text_editor.is_none()
            {
                self.joinery.open = false;
                self.joinery_dialog = Default::default();
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() { memory.surrender_focus(id); }
                });
                ctx.request_repaint();
                return;
            }
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
                || self.viewports.renaming.is_some()
                || self.text_editor.is_some()
                || self.help
                || ctx.egui_wants_keyboard_input();
            if !editing_operation && self.doc.model.edit_context.is_some() {
                self.close_edit_context(ctx);
            }
            self.move_copy = false;
            self.last_move_copy = None;
            let completed_group = self.line_group;
            self.stop_drawing();
            self.modeling = None;
            self.notice = None;
            self.help = false;
            self.paste_preview = None;
            self.object_creation = None;
            self.object_rename = None;
            self.viewports.renaming = None;
            self.text_editor = None;
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            self.prompt_drawn_group_name(ctx, completed_group);
        }
        if ctx.egui_wants_keyboard_input()
            || self.help
            || self.object_creation.is_some()
            || self.object_rename.is_some()
            || self.viewports.renaming.is_some()
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
        let sidebar_tab = ctx.input_mut(|i| {
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
                            self.inspector = !self.inspector;
                        }
                        false
                    } else {
                        true
                    }
                });
            }
            consumed
        });
        if sidebar_tab {
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
                    // unit suffixes. Letter shortcuts resume after the dimension is cleared.
                    if self.precision_available()
                        && !self.length_input.is_empty()
                        && (Key::A..=Key::Z).contains(&key)
                    {
                        continue;
                    }
                    if let Some(tool) = self.general_settings.tool_for_key(key) {
                        self.set_tool(tool);
                        continue;
                    }
                    match key {
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
                                || matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale | Tool::PushPull | Tool::Offset)
                                || self.tool.is_measuring() =>
                        {
                            let axis = match key {
                                Key::ArrowRight => Axis::X,
                                Key::ArrowLeft => Axis::Y,
                                _ => Axis::Z,
                            };
                            self.toggle_axis_lock(axis, ctx);
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
                                '.' | '-' | '+' | '*' | '/' | '(' | ')' | ',' | '\'' | '"' | ' ' | '°'
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
        // Override the stroke for every state, including hover and an open menu.
        egui::menu::MenuButton::from_button(egui::Button::new(title).stroke(Stroke::NONE))
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
                    if self.account.sharing_available() {
                        let avatar = ui.add(egui::Button::new("").min_size(egui::vec2(32., 32.))
                            .corner_radius(16).stroke(Stroke::new(1., foreground.gamma_multiply(0.65))))
                            .on_hover_text(self.account.user.as_ref().map_or("Account · Not connected", |user| user.name.as_str()));
                        avatar.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Account"));
                        if let Some(texture) = &self.account.texture {
                            egui::Image::new((texture.id(), avatar.rect.size())).corner_radius(16).paint_at(ui, avatar.rect.shrink(1.));
                        } else if let Some(user) = &self.account.user {
                            let initial = user.name.chars().next().unwrap_or('?').to_uppercase().to_string();
                            ui.painter().text(avatar.rect.center(), Align2::CENTER_CENTER, initial, FontId::proportional(16.), foreground);
                        } else {
                            icons::line_icon(ui.painter(), avatar.rect.center(), "account", foreground);
                        }
                        if self.account.user.is_some() {
                            self.account.avatar_menu(&avatar);
                        } else if avatar.clicked() {
                            self.account.open = true;
                        }
                        ui.add(egui::Separator::default().vertical().spacing(12.));
                    }
                    let suggestion_icon = egui::Atom::paint(egui::vec2(18., 18.), |ui, args| {
                        icons::line_icon(ui.painter(), args.rect.center(), "suggest_feature", args.fallback_text_color);
                    });
                    if ui.add(egui::Button::new((suggestion_icon, "Suggest a feature")).stroke(Stroke::new(1., foreground.gamma_multiply(0.65))))
                        .on_hover_text("Suggest a feature on GitHub")
                        .clicked()
                    {
                        crate::system_info::request_issue(&ctx);
                    }
                    // This row flows right to left, placing the divider left of the button.
                    ui.add(egui::Separator::default().vertical().spacing(16.));
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
                        let saved = ui.text_label(RichText::new(text).size(12.).color(color));
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
                            ui.text_label(RichText::new("KetchUp").size(23.).strong().color(foreground));
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
                                if action == 1 {
                                    self.recent_projects_menu(ui, &ctx);
                                }
                            }
                            if self.account.sharing_available() && ui.button(self.share_menu_label()).clicked() { self.start_sharing(&ctx); ui.close(); }
                            ui.separator();
                            ui.menu_button("Import", |ui| {
                                for (extension, label, help) in [
                                    ("skp", "SketchUp (.skp)…", "Editable groups, holes and solid colors. No textures or annotations."),
                                    ("stl", "STL (.stl; millimetres)…", "ASCII or binary. Geometry only; coordinates interpreted as millimetres, Z up."),
                                    ("obj", "Wavefront OBJ (.obj; metres)…", "Faces and lines, including negative vertex indices. Metres, Z up. No materials, textures or group hierarchy."),
                                    ("dxf", "DXF 2D (.dxf)…", "Text DXF: LINE, straight LWPOLYLINE, ARC and CIRCLE in XY. Curves approximated with up to 96 segments per circle. No blocks, bulges or widths. Declared units; unitless files use millimetres."),
                                ] {
                                    if ui.button(label).on_hover_text(help).clicked() && !self.project.blocked() {
                                        self.project.import_format = extension.into();
                                        self.import_model(&ctx);
                                        ui.close();
                                    }
                                }
                            });
                            ui.menu_button("Export", |ui| {
                                for (stl, label, help) in [
                                    (false, "Wavefront OBJ (.obj)…", "Visible edges and triangulated faces in metres, Z up. Geometry only."),
                                    (true, "STL (.stl; millimetres)…", "Binary STL of visible faces in millimetres, Z up. No edges or materials. Ensure solids are closed before printing; no automatic repair."),
                                ] {
                                    if ui.button(label).on_hover_text(help).clicked() && !self.project.blocked() {
                                        self.project.export_stl = stl;
                                        self.export_model(&ctx);
                                        ui.close();
                                    }
                                }
                            });
                            ui.separator();
                            if ui.button("Settings…").clicked() {
                                self.settings_open = true;
                                ui.close();
                            }
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
                                (false, "Undo", self.doc.history.undo_label().map(str::to_owned), egui::Modifiers::COMMAND),
                                (true, "Redo", self.doc.history.redo_label().map(str::to_owned), egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
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
                        Self::app_bar_menu(ui, "View", |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            self.view_menu(ui);
                        });
                        Self::app_bar_menu(ui, "Generate", |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            if ui.add_enabled(!self.project.blocked(), egui::Button::new("Prepare cutting plan…"))
                                .on_hover_text("Summarize matching parts and export finished cutting dimensions for the selection or whole project.")
                                .clicked() {
                                self.prepare_cutting_plan();
                                ui.close();
                            }
                            if ui.add_enabled(!self.project.blocked(), egui::Button::new("Screenshot")).clicked() {
                                self.screenshot.open = true;
                                ui.close();
                            }
                        });
                        Self::app_bar_menu(ui, "Help", |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            if ui.button("Keyboard shortcuts").clicked() {
                                self.help = true;
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Report a bug").clicked() {
                                crate::system_info::request_issue(&ctx);
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
                            if ui.button("System information").clicked() {
                                crate::system_info::open(&ctx);
                                ui.close();
                            }
                            if ui.button("About KetchUp").clicked() {
                                self.about = true;
                                ui.close();
                            }
                        });
                        let (separator, _) = ui.allocate_exact_size(egui::vec2(32., 24.), Sense::hover());
                        ui.painter().vline(
                            separator.center().x,
                            separator.y_range(),
                            Stroke::new(1., if dark { colors.border } else { colors.muted.gamma_multiply(0.8) }),
                        );
                        ui.scope(|ui| {
                            // Match the axis-lock buttons despite the AppBar's flat menu style.
                            ui.visuals_mut().widgets.inactive = ctx.global_style().visuals.widgets.inactive;
                            for (redo, kind, action, label) in [
                                (false, "undo", "Undo", self.doc.history.undo_label().map(str::to_owned)),
                                (true, "redo", "Redo", self.doc.history.redo_label().map(str::to_owned)),
                            ] {
                                let enabled = label.is_some();
                                let tooltip = label
                                    .map(|label| format!("{action} {label}"))
                                    .unwrap_or_else(|| action.to_owned());
                                let response = ui.add_enabled(
                                    enabled,
                                    egui::Button::new("").min_size(egui::vec2(32., 32.)).stroke(Stroke::new(1., foreground.gamma_multiply(0.65))),
                                );
                                icons::line_icon(
                                    ui.painter(),
                                    response.rect.center(),
                                    kind,
                                    foreground.gamma_multiply(if enabled { 1. } else { 0.3 }),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::Role::Button, enabled, &tooltip)
                                });
                                if response.on_hover_text(&tooltip).clicked() {
                                    self.undo(&ctx, redo);
                                }
                            }
                        });
                        let (separator, _) = ui.allocate_exact_size(egui::vec2(32., 24.), Sense::hover());
                        ui.painter().vline(
                            separator.center().x,
                            separator.y_range(),
                            Stroke::new(1., if dark { colors.border } else { colors.muted.gamma_multiply(0.8) }),
                        );
                        ui.add(crate::natural_text::Label::new(RichText::new(&self.doc.name).color(foreground)).truncate())
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
                    let shared_status = self.shared_status(ctx.input(|i|i.time));
                    let message = if let Some(status) = shared_status.as_deref() {
                        status
                    } else if drawing_orbit {
                        "Move the cursor to orbit · Hold Shift to pan · Release Ctrl / ⌘ to resume drawing"
                    } else if navigating {
                        if ctx.input(|i| i.modifiers.shift) {
                            "Panning · Release Shift to orbit"
                        } else {
                            "Orbiting · Hold Shift to pan"
                        }
                    } else if self.paste_preview.is_some() {
                        "Click to place the copy · Escape to cancel"
                    } else if self.tool == Tool::Rotate {
                        self.rotation_status()
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
                    } else if self.tool == Tool::Paint {
                        self.paint_instructions()
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
                            ui.text_label(
                                RichText::new(
                                    self.measurement_value().unwrap_or_else(|| "—".into()),
                                )
                                .monospace()
                                .strong(),
                            );
                            ui.text_label(
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
                            ui.text_label(RichText::new("×").color(colors.muted));
                        }
                        let available = self.precision_available();
                        let hint = if let Some(edit) = &self.modeling {
                            if self.tool == Tool::Scale { format!("{:.3}", edit.value) } else if self.tool == Tool::Rotate { self.doc.display.units.format_angle(edit.value) } else { self.doc.display.units.format_length(edit.value) }
                        } else if self.last_move_copy.is_some() {
                            "/3 or 3x".into()
                        } else if self.drawing.is_some() && self.tool == Tool::Rectangle {
                            "5m,50mm".into()
                        } else if self.drawing.is_some() {
                            "5cm".into()
                        } else if let (Some(a), Some(b)) = (self.line_start, self.snap) {
                            self.doc.display.units.format_length(a.distance(b.point))
                        } else {
                            self.doc.display.units.format_length(0.)
                        };
                        let field = ui.add_enabled(
                            available,
                            egui::TextEdit::singleline(&mut self.length_input)
                                .desired_width(if self.tool == Tool::Rectangle { 300. } else { 212. })
                                .font(egui::TextStyle::Monospace)
                                .hint_text(hint)
                                .char_limit(precision::MAX_INPUT_LEN)
                                .margin(egui::vec2(9., 6.)),
                        ).on_hover_text(if self.tool == Tool::Rotate {
                            format!("Enter an angle or formula, such as 90 or (45 + 45) / 2. Plain numbers use {}. Add deg, °, rad, or gon after the formula to override the unit.", self.doc.display.units.angle.label())
                        } else {
                            format!("Use lengths or formulas: 60cm - 89mm, (1m + 20cm) / 2. Operators: + - * / and parentheses. Plain numbers default to {}; multipliers are unitless. Units: mm, cm, m, km, in, ft, yd. Rectangles accept one formula per dimension, separated by a comma. Scale uses numbers without units.", self.doc.display.units.length.label())
                        });
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
                        ui.text_label(
                            RichText::new(match self.tool {
                                Tool::Scale => "Factor",
                                Tool::Rotate => "Angle",
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
                                crate::natural_text::Label::new(
                                    RichText::new(message).size(12.).color(colors.muted),
                                )
                                .truncate(),
                            );
                        },
                    );
                });
            });
    }
    fn tool_tooltip(&self, response: egui::Response, tool: Tool) -> egui::Response {
        response.on_hover_ui(|ui| {
            let title = if self.general_settings.tool_shortcut(tool).is_empty() {
                tool.label().to_owned()
            } else {
                format!(
                    "{} ({})",
                    tool.label(),
                    self.general_settings.tool_shortcut(tool)
                )
            };
            ui.text_strong(title);
            ui.text_label(tool.instructions());
        })
    }
    fn tool_menu_item(&mut self, ui: &mut egui::Ui, tool: Tool, active: bool) -> egui::Response {
        let colors = Palette::for_ui(ui);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, true, active, tool.label())
        });
        if active || response.contains_pointer() || response.has_focus() {
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
        let shortcut = self.general_settings.tool_shortcut(tool);
        let shortcut_size = ui.painter().text(
            egui::pos2(rect.right() - 8., rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::proportional(11.),
            colors.muted,
        );
        if tool == Tool::Polygon {
            let input_rect = Rect::from_min_size(
                egui::pos2(
                    shortcut_size.left() - ui.spacing().item_spacing.x - 48.,
                    rect.top(),
                ),
                egui::vec2(48., rect.height()),
            );
            // Center the widget's padded height within the row without advancing
            // the menu cursor a second time.
            let mut input_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(input_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            input_ui
                .add(
                    egui::DragValue::new(&mut self.polygon_sides)
                        .range(3..=64)
                        .min_size(egui::vec2(48., 0.)),
                )
                .on_hover_text("Polygon sides");
        }
        self.tool_tooltip(response, tool)
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
                if ui.rect_contains_pointer(ui.max_rect().expand2(egui::vec2(10., 18.))) {
                    // Use wheel events, not inertial smoothing, for one tool change
                    // per wheel input. Consume scrolling before the rail scroll area.
                    let wheel = ui.input_mut(|input| {
                        let mut delta = 0.;
                        input.events.retain(|event| {
                            if let egui::Event::MouseWheel { delta: scroll, .. } = event {
                                delta += scroll.y;
                                false
                            } else {
                                true
                            }
                        });
                        input.smooth_scroll_delta = Vec2::ZERO;
                        delta
                    });
                    if wheel != 0. {
                        let current = Tool::RAIL.iter().position(|tool| {
                            *tool == self.tool
                                || (*tool == Tool::Line && self.tool.is_drawing())
                                || (*tool == Tool::Dimensions && self.tool.is_measuring())
                                || (*tool == Tool::Union && self.tool.is_solid())
                        }).unwrap_or(0);
                        let next = if wheel > 0. {
                            current.saturating_sub(1)
                        } else {
                            (current + 1).min(Tool::RAIL.len() - 1)
                        };
                        if next != current {
                            let tool = match Tool::RAIL[next] {
                                Tool::Line => self.drawing_tool,
                                Tool::Dimensions => self.measurement_tool,
                                Tool::Union => self.solid_tool,
                                tool => tool,
                            };
                            self.set_tool(tool);
                        }
                    }
                }
                let scroll = sidebar::restored_scroll_area(
                    ui,
                    "sidebar_left",
                    egui::ScrollArea::vertical().id_salt("tool_scroll"),
                )
                .show(ui, |ui| {
                    // Reserve a shape behind every button, then resolve the final selection
                    // after clicks and nested menus have activated their tool this frame.
                    let selection_shape = ui.painter().add(egui::Shape::Noop);
                    let content_top = ui.cursor().top();
                    let mut tool_rects = Vec::with_capacity(Tool::RAIL.len());
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
                        let tooltip = if self.general_settings.tool_shortcut(tool).is_empty() {
                            tool.label().to_owned()
                        } else {
                            format!(
                                "{} ({})",
                                tool.label(),
                                self.general_settings.tool_shortcut(tool)
                            )
                        };
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(56., 55.), Sense::click());
                        tool_rects.push((rail_tool, rect));
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
                        if !active && response.hovered() {
                            ui.painter().rect_filled(
                                rect.shrink2(egui::vec2(0., 3.)),
                                8,
                                colors.hovered,
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
                                vec![tip + egui::vec2(-4., -4.), tip, tip + egui::vec2(-4., 4.)],
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
                                        let response =
                                            self.tool_menu_item(ui, option, self.tool == option);
                                        if response.clicked() {
                                            if option.is_solid() {
                                                self.start_solid_tool(option, ui.ctx());
                                            } else {
                                                self.set_tool(option);
                                            }
                                            ui.close();
                                        }
                                    }
                                });
                        }
                        if self.tool_tooltip(response, tool).clicked() && !nested {
                            self.set_tool(tool);
                        }
                        ui.add_space(5.);
                    }
                    let selected_rail_tool = if self.tool.is_solid() {
                        Tool::Union
                    } else if Tool::DRAWING.contains(&self.tool) {
                        Tool::Line
                    } else if Tool::MEASURING.contains(&self.tool) {
                        Tool::Dimensions
                    } else {
                        self.tool
                    };
                    if let Some((_, target)) = tool_rects
                        .iter()
                        .find(|(tool, _)| *tool == selected_rail_tool)
                    {
                        // Animate only the content-relative offset: scrolling must not
                        // become another animation target. Zero-duration styles snap.
                        let offset = ui.ctx().animate_value_with_time(
                            ui.scope_id().with("tool_rail_selection"),
                            target.top() - content_top,
                            ui.style().animation_time.min(0.14),
                        );
                        let rect =
                            target.translate(egui::vec2(0., content_top + offset - target.top()));
                        ui.painter().set(
                            selection_shape,
                            egui::Shape::Vec(vec![
                                egui::Shape::rect_filled(
                                    rect.shrink2(egui::vec2(0., 3.)),
                                    8,
                                    colors.selected,
                                ),
                                egui::Shape::rect_filled(
                                    Rect::from_min_size(
                                        rect.left_center() - egui::vec2(10., 12.),
                                        egui::vec2(3., 24.),
                                    ),
                                    2,
                                    colors.accent,
                                ),
                            ]),
                        );
                    }
                });
                sidebar::remember_scroll(ui.ctx(), "sidebar_left", scroll.state.clamped_offset());
            });
    }
    fn paint_instructions(&self) -> &'static str {
        match self.paint_target {
            PaintTarget::Face => "Click a face to apply this color.",
            PaintTarget::Group => "Click a face in a group or component to paint all its faces.",
        }
    }

    fn materials_contents(&mut self, ui: &mut egui::Ui) {
        let objects: Vec<_> = self
            .selected
            .iter()
            .copied()
            .filter(|id| self.doc.model.objects.contains_key(id))
            .collect();
        if objects.is_empty() {
            crate::empty_state::EmptyState::new("No objects selected",
                "Select a group or component to edit its material, sanding, and finishing.").show(ui);
            return;
        }
        let first = self.doc.model.objects[&objects[0]].stock_material_id;
        let mixed = objects
            .iter()
            .any(|id| self.doc.model.objects[id].stock_material_id != first);
        let mut chosen = first;
        let mut assign = false;
        ui.add_enabled_ui(!self.doc.model.selection_locked(&self.selected), |ui| {
            ui.text_label("Stock material");
            let label = if mixed {
                "Mixed"
            } else if let Some(id) = first {
                self.general_settings
                    .stock
                    .wood
                    .iter()
                    .find(|stock| stock.id == id)
                    .map(|stock| stock.name.as_str())
                    .or_else(|| {
                        self.doc
                            .model
                            .stock_materials
                            .get(&id)
                            .map(|material| material.name.as_str())
                    })
                    .unwrap_or("Missing material")
            } else {
                "Unassigned"
            };
            assign = stock_settings::material_selector(
                ui,
                label,
                &mut chosen,
                &self.general_settings.stock.wood,
                &self.doc.model.stock_materials,
                &mut self.stock_material_filter,
            )
            .inner;
        });
        if assign && (mixed || chosen != first) {
            let before = self.doc.model.clone();
            if let Some(material_id) = chosen
                && let Some(stock) = self
                    .general_settings
                    .stock
                    .wood
                    .iter()
                    .find(|stock| stock.id == material_id)
            {
                self.doc
                    .model
                    .stock_materials
                    .insert(material_id, stock.definition());
            }
            for id in objects {
                let object = self.doc.model.objects.get_mut(&id).unwrap();
                object.stock_material_id = chosen;
                object.stock_material = None;
            }
            self.finish_edit("Assign stock material", before, ui.ctx());
        }
        ui.add_space(12.);
        self.surface_treatment_info(ui);
    }

    fn entity_info_contents(&mut self, ui: &mut egui::Ui) {
        if !self.selected.is_empty()
            && let Some(size) = self
                .solid_volume_cache
                .solid_selection_size(&self.doc.model, &self.selected)
        {
            self.entity_axis_gizmo(ui, size);
            ui.add_space(4.);
        }
        self.solid_inspector(ui);
        self.measurement_inspector(ui);
        self.text_entity_info(ui);
        if self.selected.is_empty() {
            crate::empty_state::EmptyState::new("Nothing selected",
                "Select an entity to see its details.").show(ui);
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
            let single_group = self.selected.len() == 1
                && objects.len() == 1
                && objects[0].kind == ObjectKind::Group;
            if !single_group {
                ui.text_label(
                    RichText::new(format!("{selection} selected"))
                        .size(17.)
                        .strong(),
                );
                ui.add_space(12.);
            }
            if self.doc.model.selection_locked(&self.selected) {
                ui.text_label(RichText::new("Locked").color(self.axis_color(Axis::Z)));
            }
            if edges > 0 {
                let length: f64 = entities
                    .iter()
                    .filter_map(|id| self.doc.model.edges.get(id))
                    .map(|edge| self.doc.model.edge_length(edge))
                    .sum();
                Self::property_row(
                    ui,
                    if edges == 1 { "Length" } else { "Total edge length" },
                    &self.doc.display.units.format_length(length),
                );
            }
            if faces > 0 {
                let area: f64 = entities
                    .iter()
                    .filter_map(|id| self.doc.model.faces.get(id))
                    .map(|f| self.face_cache.get(f.id).area)
                    .sum();
                Self::property_row(
                    ui,
                    "Surface area",
                    &self.doc.display.units.format_area(area),
                );
                let side_lengths = |ring: &[u64]| {
                    (0..ring.len())
                        .map(|i| {
                            self.doc.model.vertices[&ring[i]].position.distance(
                                self.doc.model.vertices[&ring[(i + 1) % ring.len()]].position,
                            )
                        })
                        .collect::<Vec<_>>()
                };
                let selected_faces: Vec<_> = entities
                    .iter()
                    .filter_map(|id| self.doc.model.faces.get(id))
                    .collect();
                let perimeter: f64 = selected_faces
                    .iter()
                    .flat_map(|face| std::iter::once(&face.vertices).chain(face.holes.iter()))
                    .flat_map(|ring| side_lengths(ring))
                    .sum();
                Self::property_row(
                    ui,
                    if faces == 1 {
                        "Perimeter"
                    } else {
                        "Total perimeter"
                    },
                    &self.doc.display.units.format_length(perimeter),
                );
                if let [face] = selected_faces.as_slice() {
                    for (index, ring) in std::iter::once(&face.vertices)
                        .chain(face.holes.iter())
                        .enumerate()
                    {
                        let label = if index == 0 {
                            "Side lengths".to_owned()
                        } else {
                            format!("Hole {index} side lengths")
                        };
                        let lengths = side_lengths(ring)
                            .into_iter()
                            .map(|length| self.doc.display.units.format_length(length))
                            .collect::<Vec<_>>()
                            .join(" · ");
                        Self::property_row(ui, &label, &lengths);
                    }
                }
            }
            if self.selected.len() == 1
                && let Some(&id) = self.selected.first()
                && self.doc.model.objects.contains_key(&id)
                && let Ok(volume) = self.solid_volume_cache.volume(&self.doc.model, id)
            {
                Self::property_row(ui, "Volume", &self.doc.display.units.format_volume(volume));
            }
        }
        if self.selected.is_empty() {
            return;
        }
        ui.add_space(8.);
        sidebar::inspector_section(ui, "Appearance", "entity_appearance", true, |ui| {
            self.paint_color_controls(ui, false);
            ui.add_space(8.);
            if ui
                .add_enabled(
                    !self.selected.is_empty()
                        && !self.doc.model.selection_locked(&self.selected),
                    egui::Button::new("Clear Color")
                        .min_size(egui::vec2(ui.available_width(), ui.spacing().interact_size.y)),
                )
                .on_hover_text("Remove color from the selected items and all their children")
                .clicked()
            {
                let before = self.doc.model.clone();
                match self.doc.model.clear_colors(&self.selected) {
                    Ok(()) => self.finish_edit("Clear color", before, ui.ctx()),
                    Err(message) => self.say(ui.ctx(), message),
                }
            }
            ui.text_label(self.paint_instructions());
        });
    }
    fn property(ui: &mut egui::Ui, label: &str, value: &str) {
        Self::property_row(ui, label, value);
        ui.add_space(3.);
    }

    fn property_row(ui: &mut egui::Ui, label: &str, value: &str) {
        let colors = Palette::for_ui(ui);
        crate::inspector_table::inspector_field(
            ui,
            ("property", label),
            RichText::new(label).size(12.).color(colors.muted),
            |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.add(crate::natural_text::Label::new(RichText::new(value).monospace().size(12.)).wrap())
                })
                .inner
            },
        );
    }

    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        self.connection_sidebar_selection(ui.ctx());
        let fasteners_open = fasteners::AVAILABLE && self.fasteners.open;
        let colors = Palette::for_ui(ui);
        if !self.inspector {
            return;
        }
        if ui.available_width() >= 900. || fasteners_open || self.joinery.open {
            let panel = egui::Panel::right("inspector")
                .default_size(self.inspector_width)
                .size_range(260. ..=ui.available_width() * 0.5)
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.paper))
                .show(ui, |ui| {
                    ui.spacing_mut().scroll.bar_outer_margin = 0.;
                    if fasteners_open {
                        self.automatic_fasteners_sidebar(ui);
                    } else if self.joinery.open {
                        self.wood_joinery_sidebar(ui);
                    } else {
                        self.inspector_contents(ui);
                    }
                });
            self.inspector_width = panel.response.rect.width();
        } else {
            let mut open = true;
            egui::Window::new("Inspector")
                .open(&mut open)
                .default_width(280.)
                .default_height(560.)
                .default_pos(egui::pos2(90., 120.))
                .show(ui.ctx(), |ui| {
                    if fasteners_open {
                        self.automatic_fasteners_sidebar(ui);
                    } else if self.joinery.open {
                        self.wood_joinery_sidebar(ui);
                    } else {
                        self.inspector_contents(ui);
                    }
                });
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
        self.picking_cache.set_display(&self.doc.display);
        self.picking_cache.update(&self.face_cache);
    }
    // Call after prepare_picking so inference shares the viewport's cached
    // planes and spatial hierarchy instead of rebuilding every face per target.
    fn snap_targets(&self) -> ketchup_core::inference::SnapTargets {
        ketchup_core::inference::SnapTargets {
            edges: self.snap_to_edge,
            faces: self.snap_to_face,
            face_centers: self.snap_to_face_center,
        }
    }
    // Keep the legacy document flag compatible with older clients. The three
    // target preferences are the controls; there is no separate master switch.
    fn sync_geometry_snapping(&mut self) {
        self.doc.snap_to_geometry =
            self.snap_to_edge || self.snap_to_face || self.snap_to_face_center;
    }
    fn infer_snap(
        &self,
        pos: Pos2,
        start: Option<DVec3>,
        lock: Option<Axis>,
        axes: bool,
    ) -> Option<Snap> {
        ketchup_core::inference::infer_with_constraint(
            &self.doc.model,
            &self.doc.camera,
            dv(self.viewport.size()),
            dv(pos - self.viewport.min),
            start,
            lock.map(|axis| {
                self.plane_lock().map_or(AxisConstraint::Axis(axis), AxisConstraint::Plane)
            }),
            self.doc.snap_to_geometry,
            axes,
            Some(
                &self
                    .picking_cache
                    .snap_query(&self.face_cache, &self.entity_cache)
                    .excluding(self.modeling.as_ref().map(|edit| &edit.snap_excluded)),
            ),
            self.snap_targets(),
        )
    }
    fn pick_at(&mut self, pixel: DVec2, size: DVec2, include_faces: bool) -> Option<u64> {
        // Select and move the visible machined boards, retaining their source
        // object IDs. The cache restores stock geometry in the active edit scope.
        if matches!(self.tool,Tool::Select|Tool::Move|Tool::Rotate|Tool::Scale)
            && self.doc.model.connections.values().any(|c| matches!(c.operation,ketchup_core::connections::ConnectionOperation::Joinery(_)))
            {
            return self.joinery_geometry.pick(&self.doc.model,&self.doc.camera,&self.doc.display,size,pixel,include_faces);
        }
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
        if self.modeling.is_some() && matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale) {
            self.hover = None;
            self.update_modeling(pos);
            return;
        }
        if self.modeling.is_none() && matches!(self.tool, Tool::PushPull | Tool::Offset) {
            let anchor = self.face_anchor_snap(pos);
            self.hover = anchor.map(|(id, _)| id);
            self.snap = anchor.filter(|(id, _)| !self.doc.model.is_locked(*id))
                .map(|(_, snap)| snap)
                .or_else(|| self.selection_anchor_snap(pos));
            return;
        }
        let local = dv(pos - self.viewport.min);
        let size = dv(self.viewport.size());
        self.hover = if self.tool == Tool::Erase {
            self.erase_target(pos)
        } else if let Some(face) = self.selected_modeling_face().filter(|face| {
            self.doc.model.face_hit(&self.doc.model.faces[face], self.doc.camera.ray(local, size)).is_some()
        }) {
            Some(face)
        } else if matches!(self.tool, Tool::PushPull | Tool::Offset | Tool::Paint) {
            self.prepare_picking();
            self.picking_cache
                .nearest(
                    &self.face_cache,
                    &self.entity_cache,
                    self.doc.camera.ray(local, size),
                    self.tool != Tool::Paint,
                )
                .and_then(|(id, _, _)| {
                    if self.tool == Tool::Paint && self.paint_target == PaintTarget::Group {
                        self.doc
                            .model
                            .entity_in_context(id)
                            .filter(|id| self.doc.model.objects.contains_key(id))
                    } else {
                        Some(id)
                    }
                })
        } else if matches!(self.tool, Tool::Select | Tool::Move | Tool::Rotate | Tool::Scale)
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
        if matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale)
            && self.modeling.is_none()
            && self.paste_preview.is_none()
        {
            // Match the first click's anchor query, before geometry starts moving.
            self.snap = self.selection_anchor_snap(pos);
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
            if self.drawing.is_some() && self.paste_preview.is_none() {
                if self.lock.is_none()
                    && let Some(drawing) = &mut self.drawing
                {
                    // Inferred planes belong to the current snap only. Start
                    // each query on the original plane so leaving a target
                    // restores it without moving earlier freehand samples.
                    drawing.set_normal(drawing.surface_normal);
                }
                self.snap = self.drawing_snap(pos);
                self.align_drawing_plane();
            }
            self.update_drawing_inference(pos);
            if self.paste_preview.is_none() {
                self.align_drawing_plane();
            }
            self.apply_grid_snap();
            if let (Some(preview), Some(snap)) = (&mut self.paste_preview, self.snap) {
                preview.point = snap.point;
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
        if self.single_axis_lock().is_some() {
            return self.single_axis_lock();
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
            || self.gesture.as_ref().is_some_and(|g| {
                matches!(
                    g.kind,
                    GestureKind::Navigate { .. } | GestureKind::Compass { dragged: true, .. }
                )
            })
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
            self.doc.camera.pan(
                delta * self.general_settings.pan_speed,
                self.viewport.height() as f64,
            );
            *pivot = None;
        } else if delta != DVec2::ZERO {
            let center = *pivot.get_or_insert_with(|| self.orbit_center());
            self.doc
                .camera
                .orbit_around(delta * self.general_settings.orbit_speed, center);
        }
        self.changed(ctx);
    }

    fn update_compass_orbit(&mut self, ctx: &egui::Context, gesture: &mut Gesture, pos: Pos2) {
        let GestureKind::Compass { pivot, dragged, .. } = &mut gesture.kind else {
            return;
        };
        if !matches!(
            gesture.button,
            egui::PointerButton::Primary | egui::PointerButton::Middle
        ) {
            return;
        }
        // Once a drag starts, returning to its start must not align an axis.
        if !*dragged && gesture.start.distance(pos) < 5. {
            return;
        }
        let delta = if *dragged {
            pos - gesture.last
        } else {
            pos - gesture.start
        };
        *dragged = true;
        if delta != Vec2::ZERO {
            if ctx.input(|i| i.modifiers.shift) {
                // Share Orbit's pan speed, viewport scale, and pivot reset.
                // Read Shift on every movement so it can change mid-drag.
                self.navigate(ctx, dv(delta), pivot);
            } else {
                let center = *pivot.get_or_insert_with(|| self.orbit_center());
                self.doc.camera.orbit_around(dv(delta), center);
                self.changed(ctx);
            }
            ctx.request_repaint();
        }
    }

    fn zoom_viewport(&mut self, ctx: &egui::Context) -> Option<Pos2> {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() <= 0.01 { return None; }
        let pos = ctx.input(|i| i.pointer.hover_pos())?;
        self.doc.camera.zoom(
            scroll as f64 * self.general_settings.zoom_speed
                * if self.general_settings.reverse_zoom { -1. } else { 1. },
            dv(pos - self.viewport.min),
            dv(self.viewport.size()),
        );
        self.changed(ctx);
        Some(pos)
    }

    fn viewport_input(&mut self, ctx: &egui::Context, response: &egui::Response) {
        if self.fastener_handle_under_pointer(ctx,response.rect) { return; }
        #[cfg(not(target_arch = "wasm32"))]
        if self.updates.open {
            return;
        }

        if self.project.blocked() || self.screenshot.open {
            return;
        }
        if self.settings_open
            || self.help
            || self.about
            || self.release_notes.open
            || self.object_creation.is_some()
            || self.object_rename.is_some()
            || self.viewports.renaming.is_some()
            || self.text_editor.is_some()
        {
            self.drawing_orbit_down = false;
            self.drawing_orbit_pivot = None;
            self.viewport_pointer = None;
            if self.tool == Tool::Freehand {
                self.drawing = None;
            }
            self.gesture = None;
            self.section_drag = None;
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
            self.section_drag = None;
            self.hover = None;
            return;
        }
        if response.enabled() && self.joint_marker_under_pointer(ctx, response.rect) {
            self.zoom_viewport(ctx);
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
            if self.section_plane_event(ctx, &event, response) {
                refresh_pointer = false;
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
                    if self.compass_contains(pos) {
                        if self.gesture.is_none() {
                            self.gesture = Some(Gesture {
                                button,
                                start: pos,
                                last: pos,
                                kind: GestureKind::Compass {
                                    axis: (button == egui::PointerButton::Primary)
                                        .then(|| self.compass_axis_at(pos))
                                        .flatten(),
                                    pivot: None,
                                    dragged: false,
                                },
                            });
                            self.hover = None;
                            self.flip_hover = None;
                            self.viewport_pointer = None;
                        }
                        continue;
                    }
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
                                | Tool::Rotate
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
                    let compass_drag = self
                        .gesture
                        .as_ref()
                        .is_some_and(|gesture| matches!(gesture.kind, GestureKind::Compass { .. }));
                    let in_viewport = self.viewport.contains(pos)
                        && response.contains_pointer()
                        && !self.compass_contains(pos)
                        && !compass_drag;
                    let mouse_navigation = self.gesture.as_ref().is_some_and(|gesture| {
                        matches!(
                            gesture.kind,
                            GestureKind::Navigate { .. } | GestureKind::Compass { .. }
                        )
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
                        self.update_compass_orbit(ctx, &mut gesture, pos);
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
                        let mut gesture = self.gesture.take().unwrap();
                        self.update_compass_orbit(ctx, &mut gesture, pos);
                        match gesture.kind {
                            GestureKind::Compass {
                                axis: Some(axis),
                                dragged: false,
                                ..
                            } if gesture.start.distance(pos) < 5.
                                && self.compass_axis_at(pos) == Some(axis) =>
                            {
                                self.doc.camera.set_standard_view(axis.view());
                                self.changed(ctx);
                                ctx.request_repaint();
                            }
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
        let over_compass = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| self.compass_contains(pos));
        let compass_drag = self
            .gesture
            .as_ref()
            .is_some_and(|gesture| matches!(gesture.kind, GestureKind::Compass { .. }));
        let over_section = self.section_drag.is_some() || ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| self.section_handle_at(pos));
        if !response.contains_pointer() || over_compass || compass_drag || over_section {
            self.hover = None;
            self.flip_hover = None;
        } else if response.hovered() && refresh_pointer {
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                self.update_pointer(pos);
            }
        }
        if response.hovered() && !over_compass && !compass_drag && !over_section {
            if let Some(pos) = self.zoom_viewport(ctx) {
                self.update_pointer(pos);
            }
        }
    }

    fn paint_tool_cursor(&self, ctx: &egui::Context, response: &egui::Response) {
        if response.enabled() && self.joint_marker_under_pointer(ctx, response.rect) {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
            return;
        }
        if !response.enabled() || !response.hovered() || !response.contains_pointer() {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if self.section_drag.is_some() || self.section_handle_at(pos) { return; }
        if self.compass_contains(pos)
            || self
                .gesture
                .as_ref()
                .is_some_and(|gesture| matches!(gesture.kind, GestureKind::Compass { .. }))
        {
            return;
        }
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
        // Keep tool icons clear of the snapped anchor, as for Dimensions.
        let offset = if tool.is_measuring()
            || matches!(tool, Tool::Move | Tool::Rotate | Tool::Scale | Tool::PushPull | Tool::Offset)
        {
            egui::vec2(15., 15.)
        } else {
            Vec2::ZERO
        };
        #[cfg(target_arch = "wasm32")]
        if crate::tool_cursor::show(ctx, tool, hotspot, offset) {
            ctx.set_cursor_icon(egui::CursorIcon::None);
            return;
        }
        // Let the tool cursor overlap adjacent panels at viewport edges.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("viewport_tool_cursor"),
        ));
        ctx.set_cursor_icon(egui::CursorIcon::None);
        icons::tool(
            &painter,
            Rect::from_center_size(pos - hotspot + offset, Vec2::splat(size)),
            tool,
        );
    }

    fn paint_scene(&mut self, ui: &egui::Ui, rect: Rect) {
        self.paint_scene_with_background(ui, rect, true);
    }

    fn paint_scene_with_background(&mut self, ui: &egui::Ui, rect: Rect, background: bool) {
        // All scene helpers see the same evaluated geometry while painting.
        // Restore source caches before handling input/inspector queries again.
        std::mem::swap(&mut self.face_cache, &mut self.render_face_cache);
        std::mem::swap(&mut self.entity_cache, &mut self.render_entity_cache);
        std::mem::swap(&mut self.picking_cache, &mut self.render_picking_cache);
        self.paint_evaluated_scene(ui, rect, background);
        std::mem::swap(&mut self.face_cache, &mut self.render_face_cache);
        std::mem::swap(&mut self.entity_cache, &mut self.render_entity_cache);
        std::mem::swap(&mut self.picking_cache, &mut self.render_picking_cache);
    }

    fn paint_evaluated_scene(&mut self, ui: &egui::Ui, rect: Rect, background: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = viewport_benchmark::scope("scene");
        let colors = Palette::for_ui(ui);
        let faces = FacePalette::for_ui(ui);
        // History previews affect rendering only; editing, saving and picking
        // continue to use the committed document.
        let model = self.history_preview.as_ref().unwrap_or_else(|| {
            self.modeling
                .as_ref()
                .and_then(|edit| edit.preview.as_ref())
                .unwrap_or(&self.doc.model)
        });
        {
            #[cfg(not(target_arch = "wasm32"))]
            let _profile = viewport_benchmark::scope("geometry_update");
            self.joinery_geometry.update_render(model,
                self.history_preview.is_none() && self.modeling.as_ref().is_some_and(|edit| edit.preview.is_some()),
                ui.ctx(), &self.doc.display, &self.shadow_cache);
        }
        let prepared_shadows = self.joinery_geometry.prepared_shadows.take();
        let model=&self.joinery_geometry.result.as_ref().unwrap().model;
        let model_revision = self.render_model.update(model);
        let empty_selection = BTreeSet::new();
        let selected = if self.history_preview.is_some() {
            &empty_selection
        } else {
            &self.selected
        };
        let hover = if self.history_preview.is_some() {
            None
        } else {
            self.hover
        };
        self.entity_cache.update(model);
        let entities = &self.entity_cache;
        let joinery_parts = self.joinery_edit_members().map(|members| members.map(|id|
            model.expand_entities(&BTreeSet::from([id]))));
        let joinery_color = |id: u64| joinery_parts.as_ref().and_then(|parts|
            parts.iter().position(|part| part.contains(&id)).map(Self::joinery_part_color));

        let emphasize_group = joinery_parts.is_some() || (self.doc.display.group_emphasis
            && model
                .edit_context
                .is_some_and(|id| model.contains_entity(&id)));
        let size = dv(rect.size());
        let camera = &self.doc.camera;
        let view = camera.view(size);
        let p = ui.painter().with_clip_rect(rect);
        let mut themed_display = display_scene::effective_display(&self.doc.display, model, ui.visuals().dark_mode);
        if joinery_parts.is_some() {
            if themed_display.face_style == ketchup_core::FaceStyle::Wireframe {
                themed_display.face_style = ketchup_core::FaceStyle::Shaded;
            }
            themed_display.x_ray = false;
            themed_display.shadows = false;
            themed_display.section_fill = false;
        }
        let display = &themed_display;
        if background {
            crate::environment::paint(&p, rect, camera, display, ui.visuals().dark_mode, self.doc.display.shadows);
        }
        // Transient previews and annotation layout use the normal render path.
        // Plain model redraws retain the complete CPU/GPU mesh until an input
        // affecting its appearance changes. Screen overlays below stay live.
        let key = (self.modeling.is_none()
            && self.drawing.is_none()
            && self.line_start.is_none()
            && self.paste_preview.is_none()
            && self.measurement.is_none()
            && self.text_editor.is_none()
            && !matches!(
                &self.gesture,
                Some(Gesture {
                    kind: GestureKind::Erase { .. },
                    ..
                })
            ))
        .then(|| renderer::SceneKey {
            joinery_members: self.joinery_edit_members(),
            joint_preview: self.joint_highlight_preview_key(),
            model_revision,
            camera: camera.clone(),
            size,
            pixels_per_point: ui.ctx().pixels_per_point(),
            display: display.clone(),
            dark: ui.visuals().dark_mode,
            tool: self.tool,
            selected: selected.clone(),
            hover,
            outliner_hover: self.outliner_hover.clone(),
        });
        if !key
            .as_ref()
            .is_some_and(|key| self.scene_cache.repaint(key, &p, rect))
        {
            let emphasized_entities = if let Some(parts) = &joinery_parts {
                parts.iter().flat_map(|part| part.iter().copied()).collect()
            } else if emphasize_group {
                model.expand_entities(&model.edit_context.into_iter().collect())
            } else {
                BTreeSet::new()
            };
            let selected_entities = if self.history_preview.is_some() {
                BTreeSet::new()
            } else if let Some(edit) = self
                .modeling
                .as_ref()
                .filter(|_| self.tool == Tool::PushPull)
            {
                let cap = model
                    .faces
                    .keys()
                    .filter(|id| {
                        !edit.base.faces.contains_key(id)
                            && model.parent(**id) == model.edit_context
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
                model.expand_entities(selected)
            };
            // Keep the moving selection solid while exposing surrounding geometry.
            // Derive this every frame so selecting after entering Move works too.
            let move_x_ray = self.tool == Tool::Move && !selected_entities.is_empty();
            let mut mesh = renderer::Mesh::cached(camera, size, &mut self.scene_cache);
            mesh.set_pixels_per_point(ui.ctx().pixels_per_point());
            mesh.set_display(display);
            mesh.enable_gpu_edge_occlusion();
            mesh.set_annotation_occlusion_enabled(
                !model.dimensions.is_empty()
                    || !model.texts.is_empty()
                    || self.measurement.is_some()
                    || self.text_editor.is_some()
                    || self.paste_preview.is_some(),
            );
            #[cfg(not(target_arch = "wasm32"))]
            let grid_profile = viewport_benchmark::scope("grid");
            if self.doc.display.grid {
                let colors = GridPalette::for_ui(ui);
                for line in grid::visible_lines(camera, &self.doc.display, size) {
                    let color = if line.major { colors.major } else { colors.minor };
                    mesh.background_grid_line(line.points, line.opacity, line.major, color);
                }
            }
            for axis in [Axis::X, Axis::Y, Axis::Z]
                .into_iter()
                .filter(|_| display.axes)
            {
                let v = axis.vector() * renderer::ReferenceFade::new(camera).axis_length();
                mesh.axis_line(
                    camera,
                    -v,
                    DVec3::ZERO,
                    0.85,
                    self.axis_color(axis).gamma_multiply(0.35),
                );
                mesh.axis_line(
                    camera,
                    DVec3::ZERO,
                    v,
                    1.1,
                    self.axis_color(axis).gamma_multiply(0.8),
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            drop(grid_profile);
            #[cfg(not(target_arch = "wasm32"))]
            let face_profile = viewport_benchmark::scope("faces");
            {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = viewport_benchmark::scope("face_cache");
                self.face_cache.update(model);
            }
            if let Some((prepared_display, mut shadows)) = prepared_shadows {
                // Adopt only the partitions computed with the current shadow
                // and visibility settings, paired with this completed model.
                if prepared_display.shadows == display.shadows
                    && (prepared_display.shadow_darkness > 0.) == (display.shadow_darkness > 0.)
                    && prepared_display.sun_angle == display.sun_angle
                    && prepared_display.sun_elevation == display.sun_elevation
                    && prepared_display.section_limits() == display.section_limits()
                    && prepared_display.hidden_objects == display.hidden_objects
                    && prepared_display.hidden_geometry == display.hidden_geometry
                    && (prepared_display.face_style == ketchup_core::FaceStyle::Wireframe)
                        == (display.face_style == ketchup_core::FaceStyle::Wireframe)
                {
                    shadows.adopt_revision(self.face_cache.revision());
                    self.shadow_cache = shadows;
                }
            }
            {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = viewport_benchmark::scope("shadow_update");
                self.shadow_cache.update(&self.face_cache, entities, display);
            }
            let mut visible_faces: Vec<_> = model
                .faces
                .values()
                .filter(|f| entities.get(f.id).displayed(display))
                .filter(|f| !emphasize_group || emphasized_entities.contains(&f.id))
                .map(|face| {
                    (
                        face,
                        if display.x_ray || move_x_ray {
                            view.view_distance(self.face_cache.get(face.id).center)
                        } else {
                            0.
                        },
                    )
                })
                .collect();
            if display.x_ray || move_x_ray {
                visible_faces.sort_by(|a, b| b.1.total_cmp(&a.1));
            }
            let lighting = display_scene::FaceLighting::new(camera);
            let erasing = match &self.gesture {
                Some(Gesture {
                    kind: GestureKind::Erase { ids },
                    ..
                }) => Some(ids),
                _ => None,
            };
            let mut face_draws = Vec::new();
            for (face, _) in visible_faces
                .into_iter()
                .filter(|_| display.face_style != ketchup_core::FaceStyle::Wireframe)
            {
                let entity = entities.get(face.id);
                let cached = self.face_cache.get(face.id);
                let normal = cached.normal;
                let tag_color = entity.tag_color;
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
                    && normal.dot(camera.toward_eye(model.vertices[&face.vertices[0]].position))
                        < 0.
                {
                    faces.back
                } else if let Some(rgb) = rgb {
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                } else {
                    faces.front
                };
                let outliner_hover = self.outliner_hover.contains(&face.id);
                let color = if entity.context.is_none() && !outliner_hover {
                    faces.inactive
                } else if erasing.is_some_and(|ids| {
                    entity
                        .context
                        .is_some_and(|id| ids.contains(&id))
                }) {
                    Color32::from_rgb(220, 91, 76)
                } else if self.tool == Tool::Paint
                    && model.face_colors.contains_key(&face.id)
                    && !outliner_hover
                {
                    material
                } else if let Some(color) = joinery_color(face.id) {
                    // Tint the material softly; edge colors retain full strength.
                    Color32::from_rgb(
                        ((material.r() as u16 * 3 + color.r() as u16) / 4) as u8,
                        ((material.g() as u16 * 3 + color.g() as u16) / 4) as u8,
                        ((material.b() as u16 * 3 + color.b() as u16) / 4) as u8,
                    )
                } else if selected_entities.contains(&face.id) {
                    faces.selected
                } else if hover.is_some() && hover == entity.context
                    || hover == Some(face.id)
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
                let color = if display.x_ray || entity.entity_hidden
                    || (move_x_ray && !selected_entities.contains(&face.id))
                {
                    color.gamma_multiply(0.18)
                } else {
                    color
                };
                face_draws.push(renderer::FaceDraw {
                    face, cached, color,
                    shadow: self.shadow_cache.get(face.id).map(|shadow|
                        (shadow, display_scene::shadow_color(color, display.shadow_darkness))),
                });
            }
            {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = viewport_benchmark::scope("face_emit");
                mesh.draw_faces(&face_draws);
            }
            mesh.set_entity(None);
            display_scene::paint_sections(
                &mut mesh,
                &mut self.section_cache,
                camera,
                model,
                &self.face_cache,
                entities,
                display,
                &faces,
            );
            #[cfg(not(target_arch = "wasm32"))]
            drop(face_profile);
            let profiles = {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = viewport_benchmark::scope("profile_edges");
                if display.profiles && !emphasize_group {
                display_scene::profile_edges(model, camera, &self.face_cache, entities, display)
            } else {
                BTreeSet::new()
                }
            };
            // Resolve ordinary edges in f64 before drawing selected strokes.
            // GPU depth alone can round nearby edges to the same value, letting
            // the wider selection color overwrite a nearer object's outline.
            for selected_pass in [false, true] {
                if selected_pass {
                    mesh.begin_selected_edges();
                    if selected_entities.is_empty() { continue; }
                }
                #[cfg(not(target_arch = "wasm32"))]
                let style_profile = viewport_benchmark::scope("edge_styles");
                let mut edge_draws = Vec::new();
                for e in model
                    .edges
                    .values()
                    .filter(|e| entities.get(e.id).displayed(display))
                    .filter(|e| selected_entities.contains(&e.id) == selected_pass)
                {
                    let entity = entities.get(e.id);
                    let selected = selected_entities.contains(&e.id);
                    let emphasized = emphasized_entities.contains(&e.id);
                    let root = entity.context;
                    let outliner_hover = self.outliner_hover.contains(&e.id);
                    let hover = outliner_hover || hover.is_some() && hover == root;
                    let pending =
                        erasing.is_some_and(|ids| root.is_some_and(|id| ids.contains(&id)));
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
                    } else if entity.locked {
                        Color32::from_rgb(90, 133, 176)
                    } else if let Some(color) = joinery_color(e.id) {
                        color
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
                            * (camera.distance / view.view_distance((a + b) * 0.5).max(0.01))
                                .clamp(0.35, 1.8)
                    } else {
                        width
                    };
                    let color =
                        if display.color_by_tag && !active && (!emphasize_group || emphasized) {
                            entity
                                .tag_color
                                .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
                                .unwrap_or(color)
                        } else {
                            color
                        };
                    edge_draws.push(renderer::EdgeDraw {
                        id: e.id,
                        key: (e.a.min(e.b), e.a.max(e.b)),
                        points: [a, b], width, color,
                        back: display.back_edges || entity.entity_hidden,
                        visible: !entity.entity_hidden && (display.edges || emphasize_group
                            || profiles.contains(&e.id) || active
                            || display.face_style == ketchup_core::FaceStyle::Wireframe),
                        overlay: display.x_ray || (move_x_ray && !selected)
                            || display.face_style == ketchup_core::FaceStyle::Wireframe,
                    });
                }
                #[cfg(not(target_arch = "wasm32"))]
                drop(style_profile);
                mesh.draw_edges(&edge_draws, display.extensions, display.endpoints);
            }
            if let (Some(start), Some(snap)) = (self.line_start, self.line_preview_snap()) {
                let color = self
                    .preview_alignment()
                    .or_else(|| self.active_drawing_guide().map(|g| g.axis))
                    .map(|axis| self.axis_color(axis))
                    .unwrap_or(colors.ink);
                mesh.world_line(camera, start, snap.point, 2.2, color);
            }
            for pair in self.drawing_points().windows(2) {
                mesh.world_line(
                    camera,
                    pair[0],
                    pair[1],
                    2.2,
                    self.lock
                        .map(|axis| self.axis_color(axis))
                        .unwrap_or(ACCENT),
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
            mesh.set_entity(None);
            self.paint_joint_highlights(&mut mesh,model,display,ui.visuals().dark_mode);
            let annotations = {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = viewport_benchmark::scope("annotations");
                mesh.occlude_annotations_with_edges();
                let mut shapes = self.dimension_shapes(&mut mesh, &p, rect, model);
                shapes.extend(self.text_shapes(&mut mesh, &p, rect, model));
                shapes
            };
            {
                #[cfg(not(target_arch = "wasm32"))]
                let _profile = viewport_benchmark::scope("mesh_prepare");
                mesh.paint_cached(&p, rect, &mut self.scene_cache);
            }
            p.extend(annotations.iter().cloned());
            self.scene_cache.remember_annotations(&p, rect, annotations);
            self.scene_cache.key = key;
        }
        self.paint_fasteners(&p,rect,model);
        self.paint_joinery(&p, rect);
        let screen = |point: DVec3| view.project(point).map(|v| rect.min + pv(v));
        self.picking_cache.set_display(display);
        self.picking_cache.update(&self.face_cache);
        if display.group_names {
            #[cfg(not(target_arch = "wasm32"))]
            let _profile = viewport_benchmark::scope("group_labels");
            let markers = self.group_marker_cache.update(model, model_revision);
            for marker in markers.iter() {
                let Some(object) = model.objects.get(&marker.id).filter(|object| {
                    matches!(object.kind, ketchup_core::ObjectKind::Group)
                        && entities.get(object.id).context == Some(object.id)
                        && !object.name.trim().is_empty()
                        && entities.get(object.id).displayed(display)
                }) else { continue; };
                if let Some(position) = self.picking_cache.marker_visible(
                    &self.face_cache, entities, camera, size, marker.point, &marker.owners,
                ).then(|| screen(marker.point)).flatten() {
                    if rect.contains(position) {
                        let is_selected = selected.contains(&object.id);
                        let foreground = if is_selected { Color32::WHITE } else { colors.ink };
                        let background = if is_selected { ACCENT } else { colors.paper };
                        let galley = p.layout_no_wrap(
                            object.name.clone(), egui::FontId::proportional(13.), foreground,
                        );
                        let label = egui::Rect::from_center_size(position, galley.size());
                        p.rect_filled(label.expand2(egui::vec2(5., 3.)), 3., background);
                        p.galley(label.min, galley, foreground);
                    }
                }
            }
        }

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
            if let Some(origin) = screen(DVec3::ZERO) {
                if self
                    .picking_cache
                    .visible(&self.face_cache, entities, camera, size, DVec3::ZERO)
                {
                    p.circle_filled(origin, 2.5, colors.muted);
                }
            }
        }
        for id in selected {
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
        if self.history_preview.is_some() {
            return;
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
                        Stroke::new(1., self.axis_color(axis).gamma_multiply(0.55)),
                    );
                }
            }
            if self.drawing_guide.is_some() || !combined.is_empty() {
                if let Some(point) = screen(origin) {
                    p.circle_filled(point, 3., colors.paper);
                    p.circle_stroke(point, 3., Stroke::new(1.5, self.axis_color(axis)));
                }
            }
        }
        if let Some((source, target)) = self.active_drawing_projection().filter(|_| display.guides)
        {
            let color = self
                .lock
                .map(|axis| self.axis_color(axis))
                .unwrap_or_else(|| self.inference_color());
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
        if matches!(self.tool, Tool::PushPull | Tool::Offset)
            && let Some(edit) = &self.modeling
            && let Some(snap) = edit.snap
        {
            let color = self.inference_color();
            if display.guides {
                let end = edit.anchor + edit.direction * edit.value;
                if let Some((a, b)) = camera.segment(snap.point, end, size) {
                    p.extend(egui::Shape::dashed_line(
                        &[rect.min + pv(a), rect.min + pv(b)],
                        Stroke::new(1.2, color),
                        4.,
                        4.,
                    ));
                }
            }
        }
        let modeling_anchor = matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale | Tool::Offset | Tool::PushPull)
            && self.modeling.is_none()
            && self.paste_preview.is_none()
            && !self.navigating()
            && p.ctx().input(|i| i.pointer.hover_pos()).is_some_and(|pos| rect.contains(pos));
        let modeling_start = self.modeling.as_ref()
            .filter(|_| matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale | Tool::Offset | Tool::PushPull))
            .map(|edit| edit.anchor);
        if self.tool.is_drawing() || self.tool == Tool::Text || modeling_anchor || modeling_start.is_some() {
            if let Some(start) = self.line_start.or(self.drawing.as_ref().map(|d| d.origin)).or_else(|| {
                (self.tool == Tool::Text).then(|| self.measurement.as_ref()?.points.first().copied()).flatten()
            }).or(modeling_start).filter(|_| !modeling_anchor) {
                if let Some(point) = screen(start) {
                    p.circle_filled(point, 3.5, colors.paper);
                    p.circle_stroke(point, 3.5, Stroke::new(1.5, colors.ink));
                }
            }
            let live_snap = if modeling_start.is_some() {
                self.modeling_preview_snap()
            } else {
                self.line_preview_snap()
            };
            if let Some(snap) = live_snap {
                if let Some(point) = screen(snap.point) {
                    let color = match snap.kind {
                        Inference::Axis(a) | Inference::OnAxis(a) => self.axis_color(a),
                        Inference::Midpoint => Color32::from_rgb(
                            self.doc.display.midpoint_color[0],
                            self.doc.display.midpoint_color[1],
                            self.doc.display.midpoint_color[2],
                        ),
                        _ => self.inference_color(),
                    };
                    let color = self
                        .lock
                        .filter(|_| self.tool != Tool::Line && !modeling_anchor)
                        .map(|axis| self.axis_color(axis))
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
                    let label = if modeling_anchor {
                        snap.kind.label().into()
                    } else if let Some(normal) = self.plane_lock() {
                        format!("{} plane · Locked", match normal {
                            Axis::X => "YZ", Axis::Y => "XZ", Axis::Z => "XY",
                        })
                    } else if let Some(axis) = self.lock {
                        if self.tool.is_drawing() && self.tool != Tool::Line {
                            format!("{} · Plane locked", axis.label())
                        } else {
                            format!("{} · Locked", axis.label())
                        }
                    } else {
                        snap.kind.label().into()
                    };
                    let galley = p.layout_no_wrap(label, FontId::proportional(11.), color);
                    let at = point + if modeling_anchor || modeling_start.is_some() {
                        egui::vec2(45., 18.)
                    } else {
                        egui::vec2(14., 16.)
                    };
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
                                self.doc
                                    .display
                                    .units
                                    .format_length(start.distance(snap.point)),
                                FontId::monospace(11.),
                                colors.ink,
                            );
                        }
                    }
                }
            } else if modeling_anchor
                && matches!(self.tool, Tool::PushPull | Tool::Offset)
                && let Some(point) = p.ctx().input(|i| i.pointer.hover_pos())
            {
                // Above the drawing-plane horizon there is no world target,
                // but the cursor's anchor marker must remain visible.
                p.circle_stroke(point, 4., Stroke::new(1.5, self.inference_color()));
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
                let value = precision::modeling_value_with_units(
                    self.tool,
                    &self.length_input,
                    self.doc.display.units,
                )
                .unwrap_or(edit.value);
                if self.tool == Tool::Rotate {
                    if edit.rotation.as_ref().is_some_and(|rotation| rotation.reference.is_none()) {
                        "Click a reference point away from the pivot · Esc to cancel".into()
                    } else {
                        format!("{} · Click to confirm · Esc to cancel", self.doc.display.units.format_angle(value))
                    }
                } else if self.tool == Tool::Scale {
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
                        self.doc.display.units.format_length(value)
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
            if self.tool == Tool::Rotate {
                self.paint_rotation(&p, rect);
            }
            if matches!(self.tool, Tool::Move | Tool::Scale) {
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
                                    Stroke::new(1.5, self.axis_color(guide.axis)),
                                );
                            }
                            p.circle_stroke(a, 3., Stroke::new(1., self.axis_color(guide.axis)));
                        }
                    }
                }
                if let (Some(a), Some(b)) =
                    (screen(edit.anchor), screen(edit.anchor + edit.direction))
                {
                    p.line_segment(
                        [a, b],
                        Stroke::new(
                            1.5,
                            self.lock
                                .map(|axis| self.axis_color(axis))
                                .unwrap_or_else(|| self.inference_color()),
                        ),
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
        ui.horizontal_centered(|ui| {
            if path.is_empty() {
                ui.text_label(RichText::new("Model").size(13.).strong());
                ui.text_label(RichText::new("/").color(colors.border));
                ui.text_label(RichText::new("No group open").size(12.).color(colors.muted));
            } else {
                if ui.small_button("Model").clicked() {
                    destination = Some(None);
                }
                for id in &path {
                    ui.text_label(RichText::new("›").color(colors.muted));
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
        if let Some(context) = destination {
            self.navigate_context(context, ui.ctx());
        }
    }
    fn drawing_lock_buttons(&mut self, ui: &mut egui::Ui) {
        let enabled = self.tool.is_drawing()
            || matches!(self.tool, Tool::Move | Tool::Rotate | Tool::Scale | Tool::PushPull | Tool::Offset)
            || self.tool.is_measuring();
        let colors = Palette::for_ui(ui);
        // The viewport toolbar flows right to left: show X, Y, then Z.
        for (axis, label) in [
            (Axis::Z, "Toggle Z axis lock (↑). Select up to two axes to lock their plane; ↓ clears all locks. Shapes and Rotate use a single lock as their plane normal."),
            (Axis::Y, "Toggle Y axis lock (←). Select up to two axes to lock their plane; ↓ clears all locks. Shapes and Rotate use a single lock as their plane normal."),
            (Axis::X, "Toggle X axis lock (→). Select up to two axes to lock their plane; ↓ clears all locks. Shapes and Rotate use a single lock as their plane normal."),
        ] {
            let active = self.lock == Some(axis) || self.second_lock == Some(axis);
            let available = enabled && (active || self.second_lock.is_none());
            let response = ui.add_enabled(
                available,
                egui::Button::new("")
                    .min_size(egui::vec2(32., 32.))
                    .selected(active),
            );
            let opacity = if available { 1. } else { 0.3 };
            icons::axis_lock(
                ui.painter(),
                response.rect.center(),
                axis,
                active,
                self.axis_color(axis).gamma_multiply(opacity),
                colors.muted.gamma_multiply(opacity),
            );
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::Button, available, active, label)
            });
            if response.on_hover_text(label).clicked() {
                self.toggle_axis_lock(axis, ui.ctx());
            }
        }
    }
    fn snapping_menu(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let response = ui
            .add(egui::Button::new("").min_size(egui::vec2(48., 32.)))
            .on_hover_text("Configure edge, face, face center, axis, and grid snapping");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Snapping"));
        icons::line_icon(
            ui.painter(),
            egui::pos2(response.rect.left() + 16., response.rect.center().y),
            "magnet",
            Color32::from_rgb(67, 125, 208),
        );
        icons::dropdown_chevron(
            ui.painter(),
            egui::pos2(response.rect.left() + 36., response.rect.center().y),
            colors.muted,
        );
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                Self::menu_style(ui.style_mut());
                let edges_changed = Self::menu_checkbox(ui, &mut self.snap_to_edge, "Snap to edge")
                    .on_hover_text("Snap to object edges, including their endpoints and midpoints.")
                    .changed();
                let faces_changed = Self::menu_checkbox(ui, &mut self.snap_to_face, "Snap to face")
                    .on_hover_text("Snap to object surfaces, independently of edge and face center snapping.")
                    .changed();
                let face_centers_changed = Self::menu_checkbox(ui, &mut self.snap_to_face_center, "Snap to face center")
                    .on_hover_text("Snap to the area center of a visible face, independently of edge and surface snapping.")
                    .changed();
                let axes_changed = Self::menu_checkbox(ui, &mut self.doc.snap_to_axes, "Snap to axes")
                    .on_hover_text("Automatically align drawing with the red, green, and blue axes. Hover an endpoint, midpoint, or edge to acquire axes through that position. Arrow keys can still lock an axis when this is off.")
                    .changed();
                let grid_changed = Self::menu_checkbox(ui, &mut self.snap_to_grid, "Snap to grid")
                    .on_hover_text("Snap points and moves to the grid spacing at the current zoom. Object targets and axis locks take precedence.")
                    .changed();
                if edges_changed || faces_changed || face_centers_changed {
                    self.sync_geometry_snapping();
                }
                if axes_changed || grid_changed || edges_changed || faces_changed || face_centers_changed {
                    self.drawing_reference = None;
                    self.drawing_axis_guides.clear();
                    self.snap = None;
                    if axes_changed {
                        self.changed(ui.ctx());
                    }
                    if self.tool == Tool::Scale {
                        self.refresh_axis_lock(ui.ctx());
                    }
                }
            });
    }
    fn group_emphasis_button(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let response = ui.add(
            egui::Button::new("")
                .min_size(egui::vec2(32., 32.))
                .selected(self.doc.display.group_emphasis),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::Button,
                ui.is_enabled(),
                self.doc.display.group_emphasis,
                "Emphasize group",
            )
        });
        icons::group_emphasis(
            ui.painter(),
            response.rect.center(),
            if self.doc.display.group_emphasis {
                colors.accent
            } else {
                colors.muted
            },
            colors.muted,
        );
        if response.on_hover_text("Emphasize group: emphasize the group/component currently being edited and show everything else in muted wireframe. Has no visual effect outside a group/component.").clicked() {
            self.doc.display.group_emphasis = !self.doc.display.group_emphasis;
            ui.ctx().request_repaint();
        }
    }
    fn compass_center(rect: Rect) -> Option<Pos2> {
        (rect.width() >= 90. * COMPASS_SCALE && rect.height() >= 90. * COMPASS_SCALE)
            .then(|| rect.right_bottom() - egui::vec2(26. + COMPASS_RADIUS, 23. + COMPASS_RADIUS))
    }

    fn compass_contains(&self, pos: Pos2) -> bool {
        Self::compass_center(self.viewport)
            .is_some_and(|center| center.distance(pos) <= COMPASS_RADIUS)
    }

    fn compass_axes(&self, center: Pos2) -> [(CompassAxis, Pos2); 6] {
        let camera = &self.doc.camera;
        // The camera's analytic basis also works at exact top and bottom views.
        let right = camera.right();
        let up = camera.up();
        let toward_eye = (camera.eye() - camera.target).normalize();
        let mut axes: [CompassAxis; 6] = std::array::from_fn(|index| CompassAxis {
            axis: [Axis::X, Axis::Y, Axis::Z][index % 3],
            negative: index >= 3,
        });
        axes.sort_by(|a, b| {
            a.direction()
                .dot(toward_eye)
                .total_cmp(&b.direction().dot(toward_eye))
        });
        axes.map(|axis| {
            let direction = axis.direction();
            let end = center
                + egui::vec2(
                    direction.dot(right) as f32 * 23. * COMPASS_SCALE,
                    -direction.dot(up) as f32 * 23. * COMPASS_SCALE,
                );
            (axis, end)
        })
    }

    fn compass_axis_at(&self, pos: Pos2) -> Option<CompassAxis> {
        let center = Self::compass_center(self.viewport)?;
        // Positive tips win overlaps; ties keep the frontmost painted tip.
        self.compass_axes(center)
            .into_iter()
            .rev()
            .filter(|(_, end)| end.distance(pos) <= COMPASS_TIP_RADIUS)
            .min_by_key(|(axis, _)| axis.negative)
            .map(|(axis, _)| axis)
    }

    fn paint_compass(&self, ui: &egui::Ui, response: &egui::Response) {
        let Some(center) = Self::compass_center(self.viewport) else {
            return;
        };
        let p = ui.painter().with_clip_rect(self.viewport);
        let colors = Palette::for_ui(ui);
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|_| response.enabled() && response.contains_pointer());
        if pointer.is_some_and(|pos| self.compass_contains(pos)) {
            p.circle_filled(center, COMPASS_RADIUS, colors.paper.gamma_multiply(0.45));
        }
        let hovered_axis = pointer.and_then(|pos| self.compass_axis_at(pos));
        if self.gesture.as_ref().is_some_and(|gesture| {
            matches!(gesture.kind, GestureKind::Compass { dragged: true, .. })
        }) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if hovered_axis.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if pointer.is_some_and(|pos| self.compass_contains(pos)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let axes = self.compass_axes(center);
        for (axis, end) in axes {
            if !axis.negative {
                p.line_segment(
                    [center, end],
                    Stroke::new(1.6 * COMPASS_SCALE, self.axis_color(axis.axis)),
                );
            }
        }
        for (axis, end) in axes {
            let color = self.axis_color(axis.axis);
            let hovered = hovered_axis == Some(axis);
            p.circle(
                end,
                COMPASS_TIP_RADIUS,
                if axis.negative && !hovered {
                    color.gamma_multiply(0.2)
                } else {
                    color
                },
                if axis.negative {
                    Stroke::new(COMPASS_SCALE, color.gamma_multiply(0.7))
                } else {
                    Stroke::NONE
                },
            );
            if hovered {
                p.circle_stroke(
                    end,
                    8.5 * COMPASS_SCALE,
                    Stroke::new(COMPASS_SCALE, colors.ink),
                );
            }
            if !axis.negative || hovered {
                p.text(
                    end,
                    Align2::CENTER_CENTER,
                    axis.label(),
                    FontId::proportional(9. * COMPASS_SCALE),
                    PAPER,
                );
            }
        }
    }

    fn viewport_panel(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let ctx = ui.ctx().clone();
        let [r, g, b] =
            crate::environment::for_theme(&self.doc.display, ui.visuals().dark_mode).background;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(r, g, b)))
            .show(ui, |ui| {
                egui::Panel::top("view_options")
                    .exact_size(47.)
                    .frame(
                        egui::Frame::new()
                            .fill(colors.paper)
                            .inner_margin(egui::Margin::symmetric(18, 7)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if Self::icon_button(
                                        ui,
                                        "panel",
                                        "Toggle model inspector (Tab)",
                                        true,
                                        colors.muted,
                                    ) {
                                        self.inspector = !self.inspector;
                                    }
                                    let dark = ctx.theme() == egui::Theme::Dark;
                                    if Self::icon_button(
                                        ui,
                                        if dark { "sun" } else { "moon" },
                                        if dark {
                                            "Switch to light mode"
                                        } else {
                                            "Switch to dark mode"
                                        },
                                        true,
                                        colors.muted,
                                    ) {
                                        ctx.set_theme(if dark {
                                            egui::Theme::Light
                                        } else {
                                            egui::Theme::Dark
                                        });
                                        ctx.request_repaint();
                                    }
                                    if Self::icon_button(
                                        ui,
                                        "fit",
                                        "Fit model to view",
                                        true,
                                        colors.muted,
                                    ) {
                                        self.fit(&ctx);
                                    }
                                    if Self::icon_button(
                                        ui,
                                        "origin",
                                        "Center view on axis origin (0, 0, 0)",
                                        true,
                                        colors.muted,
                                    ) {
                                        self.fit_origin(&ctx);
                                    }
                                    ui.separator();
                                    self.drawing_lock_buttons(ui);
                                    ui.separator();
                                    self.snapping_menu(ui);
                                    self.group_emphasis_button(ui);
                                    // This row flows right to left: Sections sits left of this separator.
                                    ui.separator();
                                    self.sections_menu(ui);
                                    self.paint_menu(ui);
                                    ui.allocate_ui_with_layout(
                                        ui.available_size(),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            self.viewport_tabs(ui);
                                        },
                                    );
                                },
                            );
                        });
                    });
                self.viewport_workspace(ui);
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
                        ui.text_heading("About KetchUp");
                        ui.text_label(
                            RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                                .color(colors.muted),
                        );
                    });
                });
            });
            modal.body(|ui| {
                let colors = Palette::for_ui(ui);
                ui.text_label("KetchUp is a 3D modeling app for turning ideas into shapes. Draw precise geometry, shape surfaces, and organize your model with groups and components.");
                ui.add_space(8.);
                ui.text_label(
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

    fn restore_document_selection(doc: &mut Document) -> BTreeSet<u64> {
        doc.selection_context = doc.selection_context.filter(|id| {
            doc.model.objects.contains_key(id)
                && !doc.model.is_locked(*id)
                && !doc.model.is_hidden(*id)
        });
        doc.model.edit_context = doc.selection_context;
        doc.selection.retain(|id| {
            doc.model.entity_in_context(*id) == Some(*id) && !doc.model.is_hidden(*id)
        });
        doc.selection.clone()
    }

    /// Keep project-specific selection out of the reusable inspector preferences.
    fn sync_selection(&mut self) -> bool {
        let desired = self.shared_selection_to_remember().unwrap_or(&self.selected);
        if self.doc.selection == *desired
            && self.doc.selection_context == self.doc.model.edit_context
        {
            return false;
        }
        let selection: BTreeSet<_> = desired
            .iter()
            .copied()
            .filter(|id| {
                self.doc.model.entity_in_context(*id) == Some(*id) && !self.doc.model.is_hidden(*id)
            })
            .collect();
        let changed = self.doc.selection != selection
            || self.doc.selection_context != self.doc.model.edit_context;
        self.doc.selection = selection;
        self.doc.selection_context = self.doc.model.edit_context;
        changed
    }

    fn persist(&mut self) {
        self.sync_selection();
        self.sync_viewport_cameras();
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
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(mut benchmark) = self.viewport_benchmark.take() {
            benchmark.tick(self, &ctx, frame);
            self.viewport_benchmark = Some(benchmark);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _profile = viewport_benchmark::scope("ui");
        // Rebuilt by visible Outliner rows; closing the inspector clears the highlight too.
        self.outliner_hover.clear();
        self.history_preview = None;
        self.account.poll(&ctx);
        self.poll_project(&ctx);
        self.poll_sharing(&ctx);
        self.sharing_before_frame(&ctx);
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
        if self.project.blocked() || self.screenshot.open {
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
        self.connection_sidebar_selection(&ctx);
        self.rename_viewport_window(&ctx);
        self.text_editor_window(&ctx);
        if self.help {
            self.help_window(&ctx);
        }
        if self.settings_open {
            self.settings_window(&ctx);
        }
        self.account.show(&ctx);
        self.share_dialog(&ctx);
        self.material_removal_window(&ctx);
        if self.about {
            self.about_window(&ctx);
        }
        crate::system_info::show(&ctx, frame);
        self.sync_stock_materials(&ctx);
        self.release_notes.show(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.update_window(&ctx);
        self.project_dialogs(&ctx);
        self.screenshot_dialog(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        Self::window_resize_handles(ui);
        self.sharing_after_frame(&ctx);
        self.persist_inspector_changes(
            &ctx,
            frame.storage_mut(),
            inspector_order_before != self.inspector_order,
        );
        #[cfg(target_arch = "wasm32")]
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            let title = format!("{} - KetchUp", self.doc.name);
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
        if self.sync_selection() || self.dirty_since.is_some() {
            self.persist();
        }
    }
    fn on_exit(&mut self) {
        if self.sync_selection() || self.dirty_since.is_some() {
            self.persist();
        }
        let ctx = egui::Context::default();
        self.release_shared_selection(&ctx);
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

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "tool_rail_tests.rs"]
mod tool_rail_tests;
