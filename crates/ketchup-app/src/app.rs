use crate::{icons, renderer, storage, theme::*};
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
#[path = "sidebar.rs"]
mod sidebar;
#[path = "solid_tools.rs"]
mod solid_tools;
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
#[derive(Clone, Copy)]
enum ContextAction {
    EditObject,
    CloseContext,
    Group,
    Component,
    Ungroup,
    Explode,
    Cut,
    Copy,
    Paste,
    PasteInPlace,
    Delete,
    Hide,
    Lock,
    Unlock,
    UnhideAll,
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
    normal: DVec3,
    u: DVec3,
    v: DVec3,
    points: Vec<DVec3>,
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
    hover: Option<u64>,
    line_start: Option<DVec3>,
    snap: Option<Snap>,
    drawing_reference: Option<drawing_inference::DrawingReference>,
    drawing_guide: Option<AxisGuide>,
    drawing_projection: Option<(DVec3, DVec3)>,
    lock: Option<Axis>,
    length_input: String,
    modeling: Option<Modeling>,
    move_copy: bool,
    move_modifier_down: bool,
    last_move_copy: Option<MoveCopy>,
    measurement: Option<measurement_tools::Measurement>,
    paint_color: [u8; 3],
    gesture: Option<Gesture>,
    drawing_orbit_down: bool,
    drawing_orbit_pivot: Option<DVec3>,
    viewport_pointer: Option<Pos2>,
    viewport: Rect,
    inspector: bool,
    component_filter: String,
    new_tag_name: String,
    tag_rename: Option<(u64, String)>,
    help: bool,
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
    component_name: Option<String>,
}

impl KetchupApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply(&cc.egui_ctx);
        restore_preference(&cc.egui_ctx, cc.storage);
        let (doc, error) = match storage::load() {
            Ok(doc) => (doc.unwrap_or_default(), None),
            Err(e) => (Document::default(), Some(e)),
        };
        let app = Self::with_document(doc, error, renderer::install(cc));
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
            hover: None,
            line_start: None,
            snap: None,
            drawing_reference: None,
            drawing_guide: None,
            drawing_projection: None,
            lock: None,
            length_input: String::new(),
            modeling: None,
            move_copy: false,
            move_modifier_down: false,
            last_move_copy: None,
            measurement: None,
            paint_color: [195, 96, 67],
            gesture: None,
            drawing_orbit_down: false,
            drawing_orbit_pivot: None,
            viewport_pointer: None,
            viewport: Rect::NOTHING,
            inspector: true,
            component_filter: String::new(),
            new_tag_name: String::new(),
            tag_rename: None,
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
            component_name: None,
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
    fn context_action(&mut self, ctx: &egui::Context, action: ContextAction) {
        use ContextAction::*;
        let locked = self.doc.model.selection_protected(&self.selected);
        if !matches!(action, ContextAction::Copy) {
            self.last_move_copy = None;
            self.length_input.clear();
            self.measurement = None;
            self.drawing = None;
            self.modeling = None;
            self.gesture = None;
            self.line_start = None;
            self.snap = None;
        }
        match action {
            EditObject => {
                if self.selected.len() == 1 {
                    self.enter_object(*self.selected.first().unwrap(), ctx);
                }
            }
            CloseContext => self.close_edit_context(ctx),
            Copy => self.copy_selection(ctx),
            Cut if !locked => {
                self.copy_selection(ctx);
                self.erase_selected(ctx);
            }
            Delete if !locked => self.erase_selected(ctx),
            Group if !locked => self.create_object(ctx, false, "Group"),
            Component if !locked && !self.selected.is_empty() => {
                self.component_name = Some("Component".into());
                self.gesture = None;
            }
            Paste | PasteInPlace => {
                let Some(clipboard) = self.clipboard.clone() else {
                    return;
                };
                self.set_tool(Tool::Select);
                if matches!(action, PasteInPlace) {
                    self.paste_at(ctx, &clipboard, DVec3::ZERO);
                } else {
                    let pos = self
                        .context_position
                        .take()
                        .or_else(|| ctx.input(|i| i.pointer.hover_pos()))
                        .filter(|p| self.viewport.contains(*p))
                        .unwrap_or(self.viewport.center());
                    self.prepare_picking();
                    let point = self
                        .infer_snap(pos, None, None, self.doc.snap_to_axes)
                        .map(|s| s.point)
                        .unwrap_or(clipboard.anchor);
                    self.paste_preview = Some(PastePreview { clipboard, point });
                    self.say(ctx, "Click to place the copy · Escape to cancel");
                }
            }
            Ungroup | Explode if !locked => {
                let before = self.doc.model.clone();
                self.selected = self
                    .doc
                    .model
                    .dissolve_objects(&self.selected, matches!(action, Ungroup));
                self.finish_edit(
                    if matches!(action, Ungroup) {
                        "Ungroup"
                    } else {
                        "Explode"
                    },
                    before,
                    ctx,
                );
            }
            Hide | Lock
                if !locked
                    || (matches!(action, Lock)
                        && !self.doc.model.selection_locked(&self.selected)) =>
            {
                let before = self.doc.model.clone();
                if matches!(action, Hide) {
                    self.doc.model.hidden.extend(&self.selected);
                    self.selected.clear();
                } else {
                    self.doc.model.locked.extend(&self.selected);
                }
                self.finish_context_state_edit(
                    if matches!(action, Hide) {
                        "Hide"
                    } else {
                        "Lock"
                    },
                    before,
                    ctx,
                );
            }
            Unlock => {
                let before = self.doc.model.clone();
                let ids = self.doc.model.expand_entities(&self.selected);
                self.doc.model.locked.retain(|id| !ids.contains(id));
                self.finish_context_state_edit("Unlock", before, ctx);
            }
            UnhideAll => {
                let before = self.doc.model.clone();
                if let Some(context) = self.doc.model.edit_context {
                    let ids = self
                        .doc
                        .model
                        .expand_entities(&self.doc.model.objects[&context].entities);
                    self.doc.model.hidden.retain(|id| !ids.contains(id));
                } else {
                    self.doc.model.hidden.clear();
                }
                self.finish_context_state_edit("Unhide all", before, ctx);
            }
            _ => {}
        }
    }
    fn context_menu(&mut self, response: &egui::Response) {
        let mut action = None;
        response.context_menu(|ui| {
            ui.set_min_width(230.);
            ui.spacing_mut().item_spacing.y = 2.;
            ui.spacing_mut().interact_size.y = 24.;
            ui.spacing_mut().button_padding = egui::vec2(8., 3.);
            let selected = !self.selected.is_empty();
            let editable = selected && !self.doc.model.selection_protected(&self.selected);
            let objects = self
                .selected
                .iter()
                .any(|id| self.doc.model.objects.contains_key(id));
            let groups = self.selected.iter().any(|id| {
                self.doc
                    .model
                    .objects
                    .get(id)
                    .is_some_and(|o| o.kind == ObjectKind::Group)
            });
            let mut item = |ui: &mut egui::Ui, label: &str, enabled, command| {
                let shortcut = match command {
                    ContextAction::Cut => Some((egui::Modifiers::COMMAND, Key::X)),
                    ContextAction::Copy => Some((egui::Modifiers::COMMAND, Key::C)),
                    ContextAction::Paste => Some((egui::Modifiers::COMMAND, Key::V)),
                    ContextAction::PasteInPlace => {
                        Some((egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::V))
                    }
                    ContextAction::Delete => Some((egui::Modifiers::NONE, Key::Delete)),
                    _ => None,
                };
                let mut button = egui::Button::new(label);
                if let Some((modifiers, key)) = shortcut {
                    button = button.shortcut_text(
                        ui.ctx()
                            .format_shortcut(&egui::KeyboardShortcut::new(modifiers, key)),
                    );
                }
                if ui.add_enabled(enabled, button).clicked() {
                    action = Some(command);
                    ui.close();
                }
            };
            if self.selected.len() == 1 && objects {
                let object = &self.doc.model.objects[self.selected.first().unwrap()];
                let label = if object.kind == ObjectKind::Group {
                    "Edit Group"
                } else {
                    "Edit Component"
                };
                item(
                    ui,
                    label,
                    !self.doc.model.is_locked(object.id),
                    ContextAction::EditObject,
                );
                ui.separator();
            }
            if self.doc.model.edit_context.is_some() {
                item(
                    ui,
                    "Close Editing Context",
                    true,
                    ContextAction::CloseContext,
                );
                ui.separator();
            }
            item(ui, "Make Group", editable, ContextAction::Group);
            item(ui, "Create Component…", editable, ContextAction::Component);
            item(ui, "Ungroup", editable && groups, ContextAction::Ungroup);
            item(ui, "Explode", editable && objects, ContextAction::Explode);
            ui.separator();
            item(ui, "Cut", editable, ContextAction::Cut);
            item(ui, "Copy", selected, ContextAction::Copy);
            item(ui, "Paste", self.clipboard.is_some(), ContextAction::Paste);
            item(
                ui,
                "Paste in Place",
                self.clipboard.is_some(),
                ContextAction::PasteInPlace,
            );
            item(ui, "Delete", editable, ContextAction::Delete);
            ui.separator();
            item(ui, "Hide", editable, ContextAction::Hide);
            if self.doc.model.selection_locked(&self.selected) {
                item(ui, "Unlock", selected, ContextAction::Unlock);
            } else {
                item(ui, "Lock", selected, ContextAction::Lock);
            }
            item(
                ui,
                "Unhide All",
                !self.doc.model.hidden.is_empty(),
                ContextAction::UnhideAll,
            );
        });
        if let Some(action) = action {
            self.context_action(&response.ctx, action);
            self.context_position = None;
        }
    }
    fn component_window(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.component_name.take() else {
            return;
        };
        let mut open = true;
        let mut create = false;
        let mut cancel = false;
        egui::Window::new("Create Component")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut name);
                if name.len() > 256 {
                    name.truncate(name.floor_char_boundary(256));
                }
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    create = ui
                        .add_enabled(!name.trim().is_empty(), egui::Button::new("Create"))
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if create {
            self.create_object(ctx, true, &name);
        } else if open && !cancel {
            self.component_name = Some(name);
        }
    }
    fn new_model(&mut self, ctx: &egui::Context) {
        self.replace_project(Document::default(), None, ctx);
        self.say(ctx, "New project ready");
    }
    fn fit(&mut self, ctx: &egui::Context) {
        self.doc.camera.fit(
            self.doc.model.bounds(),
            dv(self.viewport.size().max(Vec2::splat(1.))),
        );
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
        self.drawing = Some(Drawing {
            origin: snap.point,
            normal,
            u,
            v: normal.cross(u),
            points: vec![snap.point],
        });
        self.update_pointer(pos);
    }
    fn drawing_snap(&self, pos: Pos2) -> Option<Snap> {
        let drawing = self.drawing.as_ref()?;
        let size = dv(self.viewport.size());
        let pixel = dv(pos - self.viewport.min);
        // Keep the whole operation on the plane chosen by the first click.
        // Directional axis inference would flatten a rectangle near its corners.
        let inferred = self.infer_snap(pos, None, None, false);
        if let Some(snap) = inferred.filter(|snap| {
            snap.kind != Inference::Plane
                && (snap.point - drawing.origin).dot(drawing.normal).abs() < 1e-6
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
            if let Some((face, _, _)) = self.doc.model.nearest_editable_face(ray) {
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
        if self.project.blocked() {
            return;
        }
        // File commands also work while the project name field has focus.
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
                || self.component_name.is_some()
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
            self.component_name = None;
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        if ctx.egui_wants_keyboard_input() || self.help || self.component_name.is_some() {
            let modifiers = ctx.input(|i| i.modifiers);
            self.move_modifier_down = if ctx.os() == egui::os::OperatingSystem::Mac {
                modifiers.alt
            } else {
                modifiers.ctrl
            };
            return;
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
                    // Letter and space keys belong to an in-progress dimension,
                    // including unit suffixes, rather than tool shortcuts.
                    if self.precision_available()
                        && !self.length_input.is_empty()
                        && ((Key::A..=Key::Z).contains(&key) || key == Key::Space)
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
                        Key::Q => self.set_tool(Tool::Offset),
                        Key::M => self.set_tool(Tool::Move),
                        Key::B => self.set_tool(Tool::Paint),
                        Key::T => self.set_tool(Tool::TapeMeasure),
                        Key::A => self.set_tool(Tool::Protractor),
                        Key::I => self.set_tool(Tool::Dimensions),
                        Key::F => self.fit(ctx),
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
                            if matches!(self.tool, Tool::Line | Tool::Move)
                                || self.tool.is_measuring() =>
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
        // The toolbar flows right to left, placing Close at the outer edge.
        if Self::icon_button(ui, "close", "Close", true, PAPER) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if Self::icon_button(
            ui,
            if maximized { "restore" } else { "maximize" },
            if maximized { "Restore" } else { "Maximize" },
            true,
            PAPER,
        ) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if Self::icon_button(ui, "minimize", "Minimize", true, PAPER) {
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

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::for_ui(ui);
        let ctx = ui.ctx().clone();
        egui::Panel::top("top_bar")
            .exact_size(64.)
            .frame(
                egui::Frame::new()
                    .fill(if ui.visuals().dark_mode { colors.paper } else { INK })
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
                ui.visuals_mut().override_text_color = Some(PAPER);
                ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(1., PAPER);
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().widgets.hovered.weak_bg_fill = Color32::from_rgb(65, 77, 68);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    Self::window_controls(ui);
                    #[cfg(not(target_arch = "wasm32"))]
                    if self.updates.ready() && ui.button("Update ready").clicked() {
                        self.updates.open = true;
                    }
                    if !compact {
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
                            ui.label(RichText::new("KetchUp").size(23.).strong().color(PAPER));
                        }
                        ui.add_space(18.);
                        ui.menu_button(RichText::new("File").color(PAPER), |ui| {
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
                            ui.menu_button("Import/Export", |ui| {
                                if ui.button("Import SketchUp (.skp)…")
                                    .on_hover_text("Add SketchUp geometry as editable groups. Keeps placement, face holes, and solid colors. Textures and annotations are omitted. Maximum file size: 64 MiB.")
                                    .clicked() {
                                    self.import_model(&ctx);
                                    ui.close();
                                }
                                if ui.button("Export Wavefront OBJ (.obj)…")
                                    .on_hover_text("Export visible edges and triangulated faces in metres, with Z up.")
                                    .clicked() {
                                    self.export_model(&ctx);
                                    ui.close();
                                }
                            });
                            ui.separator();
                            if ui.button("Save locally now").clicked() {
                                self.persist();
                                ui.close();
                            }
                            if ui.button("Keyboard shortcuts").clicked() {
                                self.help = true;
                                ui.close();
                            }
                        });
                        ui.menu_button(RichText::new("Environment").color(PAPER), |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            if crate::environment::controls(ui, &mut self.doc.display) {
                                self.changed(&ctx);
                            }
                        });
                        ui.menu_button(RichText::new("Help").color(PAPER), |ui| {
                            ui.visuals_mut().override_text_color = Some(colors.ink);
                            ui.label(format!("KetchUp {}", env!("CARGO_PKG_VERSION")));
                            if ui.button("Keyboard shortcuts").clicked() {
                                self.help = true;
                                ui.close();
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            if ui.button("Check for updates…").clicked() {
                                self.updates.open = true;
                                self.updates.check(&ctx, false);
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
                                    .desired_width((ui.available_width() - 16.).clamp(80., 180.))
                                    .font(egui::TextStyle::Body)
                                    .text_color(PAPER)
                                    .margin(egui::vec2(8., 7.)),
                            );
                            if response.changed() {
                                self.changed(&ctx);
                            }
                            response.on_hover_text("Rename your model");
                        });
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
                ui.horizontal_centered(|ui| {
                    dot(ui, ACCENT);
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
                    ui.label(RichText::new(message).size(12.).color(colors.muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                        ui.label(
                            RichText::new(if self.tool == Tool::Scale { "×" } else { "" })
                                .color(colors.muted),
                        );
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
                });
            });
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
        let color = if active { ACCENT } else { colors.ink };
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
        response
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
                                    ACCENT,
                                );
                            }
                            let color = if active { ACCENT } else { colors.ink };
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
                            if response.on_hover_text(tooltip).clicked() && !nested {
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
            } else if dimensions > 0 {
                let mut parts = vec![];
                if edges > 0 {
                    parts.push(noun(edges, "edge"));
                }
                if faces > 0 {
                    parts.push(noun(faces, "face"));
                }
                parts.push(noun(dimensions, "dimension"));
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
        egui::CollapsingHeader::new("This model")
            .id_salt("model_summary")
            .show(ui, |ui| {
                Self::property(ui, "Edges", &self.doc.model.edges.len().to_string());
                Self::property(ui, "Faces", &self.doc.model.faces.len().to_string());
                Self::property(
                    ui,
                    "Dimensions",
                    &self.doc.model.dimensions.len().to_string(),
                );
                Self::property(ui, "Vertices", &self.doc.model.vertices.len().to_string());
                Self::property(ui, "Units", "Meters");
                ui.add_space(20.);
                ui.horizontal(|ui| {
                    for (label, axis) in [("X", Axis::X), ("Y", Axis::Y), ("Z", Axis::Z)] {
                        dot(ui, axis_color(axis));
                        ui.label(RichText::new(label).size(11.).color(colors.muted));
                        ui.add_space(7.);
                    }
                });
            });
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
            egui::Panel::right("inspector")
                .exact_size(300.)
                .resizable(false)
                .frame(egui::Frame::new().fill(colors.paper))
                .show(ui, |ui| {
                    ui.spacing_mut().scroll.bar_outer_margin = 0.;
                    egui::ScrollArea::vertical()
                        .content_margin(18)
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.inspector_contents(ui));
                });
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

    fn sweep_erase(&self, from: Pos2, to: Pos2) -> BTreeSet<u64> {
        let a = dv(from - self.viewport.min);
        let b = dv(to - self.viewport.min);
        let size = dv(self.viewport.size());
        self.doc
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
            .collect()
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
        self.hover = if matches!(self.tool, Tool::PushPull | Tool::Offset | Tool::Paint) {
            self.prepare_picking();
            self.picking_cache
                .nearest(
                    &self.face_cache,
                    &self.entity_cache,
                    self.doc.camera.ray(local, size),
                    true,
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
            }
            self.update_drawing_inference(pos);
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
    fn orbit_center(&self) -> DVec3 {
        // Every entity contributes, including hidden and nested geometry. A
        // bounds scan avoids expanding the entire object hierarchy into sets
        // on the first movement of each orbit (and after switching from pan).
        let model = &self.doc.model;
        let mut points = model
            .edges
            .values()
            .flat_map(|edge| {
                let (a, b) = model.points(edge);
                [a, b]
            })
            .chain(model.faces.values().flat_map(|face| {
                face.vertices
                    .iter()
                    .chain(face.holes.iter().flatten())
                    .map(|id| model.vertices[id].position)
            }))
            .chain(model.dimensions.values().flat_map(|d| d.layout(model)));
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
        if self.project.blocked() {
            return;
        }
        if self.help || self.component_name.is_some() {
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
                        if !hit.is_some_and(|id| self.selected.contains(&id)) {
                            self.selected = hit.into_iter().collect();
                        }
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
                                Tool::TapeMeasure | Tool::Protractor | Tool::Dimensions => {
                                    GestureKind::Measure
                                }
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
                                self.finish_edit("Erase edges", before, ctx);
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
        if response.hovered() && refresh_pointer {
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
            self.tool
        };
        let size = 28.;
        // Keep the arrow tip and pencil point at the actual interaction position.
        // Symmetric tool icons use their center as the hotspot.
        let hotspot = match tool {
            Tool::Select => egui::vec2(-0.55, -0.95),
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
        let model = self
            .modeling
            .as_ref()
            .and_then(|edit| edit.preview.as_ref())
            .unwrap_or(&self.doc.model);
        self.entity_cache.update(model);
        let entities = &self.entity_cache;
        let size = dv(rect.size());
        let camera = &self.doc.camera;
        let p = ui.painter().with_clip_rect(rect);
        let themed_display =
            crate::environment::for_theme(&self.doc.display, ui.visuals().dark_mode);
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
        self.face_cache.update(model);
        self.shadow_cache
            .update(&self.face_cache, entities, display);
        let mut visible_faces: Vec<_> = model
            .faces
            .values()
            .filter(|f| entities.get(f.id).displayed(display))
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
                center(b)
                    .distance_squared(camera.eye())
                    .total_cmp(&center(a).distance_squared(camera.eye()))
            });
        }
        let lighting = display_scene::FaceLighting::new(camera);
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
                && normal.dot(camera.eye() - model.vertices[&face.vertices[0]].position) < 0.
            {
                Color32::from_rgb(150, 172, 185)
            } else if let Some(rgb) = rgb {
                Color32::from_rgb(rgb[0], rgb[1], rgb[2])
            } else {
                Color32::from_rgb(244, 247, 239)
            };
            let color = if entities.get(face.id).context.is_none() {
                Color32::from_rgb(231, 234, 225)
            } else if self.tool == Tool::Paint && model.face_colors.contains_key(&face.id) {
                material
            } else if selected_entities.contains(&face.id) {
                Color32::from_rgb(244, 202, 181)
            } else if self.hover.is_some() && self.hover == entities.get(face.id).context
                || self.hover == Some(face.id)
            {
                Color32::from_rgb(208, 227, 202)
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
                for &triangle in &shadow.lit {
                    mesh.world_triangle(camera, triangle, color);
                }
                let shadow_color = display_scene::shadow_color(color, display.shadow_darkness);
                for &triangle in &shadow.shaded {
                    mesh.world_triangle(camera, triangle, shadow_color);
                }
            } else {
                for &triangle in &cached.triangles {
                    mesh.world_triangle(camera, triangle, color);
                }
            }
        }
        let erasing = match &self.gesture {
            Some(Gesture {
                kind: GestureKind::Erase { ids },
                ..
            }) => Some(ids),
            _ => None,
        };
        display_scene::paint_sections(
            &mut mesh,
            camera,
            model,
            &self.face_cache,
            entities,
            display,
        );
        let profiles = if display.profiles {
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
            let root = entities.get(e.id).context;
            let hover = self.hover.is_some() && self.hover == root;
            let pending = erasing.is_some_and(|ids| root.is_some_and(|id| ids.contains(&id)));
            let color = if root.is_none() {
                Color32::from_rgb(184, 193, 181)
            } else if pending {
                Color32::from_rgb(220, 91, 76)
            } else if entities.get(e.id).locked {
                Color32::from_rgb(90, 133, 176)
            } else if selected {
                ACCENT
            } else if hover && self.tool == Tool::Erase {
                ACCENT
            } else if hover {
                GREEN
            } else {
                INK
            };
            let (a, b) = model.points(e);
            let active = selected || hover || pending;
            let width = if active {
                3.
            } else if profiles.contains(&e.id) {
                display.profile_width
            } else {
                1.8
            };
            let width = if display.depth_cue && !active {
                width
                    * (camera.distance / ((a + b) * 0.5).distance(camera.eye()).max(0.01))
                        .clamp(0.35, 1.8)
            } else {
                width
            };
            let color = if display.color_by_tag && !active {
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
                .unwrap_or(INK);
            mesh.world_line(camera, start, snap.point, 2.2, color);
        }
        for pair in self.drawing_points().windows(2) {
            mesh.world_line(camera, pair[0], pair[1], 2.2, ACCENT);
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
                    mesh.world_triangle(
                        camera,
                        triangle.map(|p| p + offset),
                        Color32::from_rgb(218, 232, 207),
                    );
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
                mesh.world_line(camera, a + offset, b + offset, 2.4, GREEN);
            }
        }
        mesh.paint_cached(&p, rect, &mut self.scene_cache);
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
                            Stroke::new(1., MUTED),
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
                    p.circle_filled(origin, 2.5, MUTED);
                }
            }
        }
        for id in &self.selected {
            if let Some(e) = model.edges.get(id) {
                let (a, b) = model.points(e);
                for pt in [a, b] {
                    if let Some(point) = screen(pt) {
                        p.circle_filled(point, 3., PAPER);
                        p.circle_stroke(point, 3., Stroke::new(1.3, ACCENT));
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
                    _ => self
                        .preview_alignment()
                        .zip(self.line_start)
                        .map(|(axis, start)| (start, axis)),
                }
            });
        if let Some((origin, axis)) = guide.filter(|_| display.guides) {
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
            if self.drawing_guide.is_some() {
                if let Some(point) = screen(origin) {
                    p.circle_filled(point, 3., PAPER);
                    p.circle_stroke(point, 3., Stroke::new(1.5, axis_color(axis)));
                }
            }
        }
        if let Some((source, target)) = self.active_drawing_projection().filter(|_| display.guides)
        {
            let color = self.lock.map(axis_color).unwrap_or(GREEN);
            if let Some((a, b)) = camera.segment(source, target, size) {
                p.extend(egui::Shape::dashed_line(
                    &[rect.min + pv(a), rect.min + pv(b)],
                    Stroke::new(1.2, color),
                    4.,
                    4.,
                ));
            }
            if let Some(point) = screen(source) {
                p.circle_filled(point, 3., PAPER);
                p.circle_stroke(point, 3., Stroke::new(1.5, color));
            }
        }
        if self.tool.is_drawing() {
            if let Some(start) = self.line_start.or(self.drawing.as_ref().map(|d| d.origin)) {
                if let Some(point) = screen(start) {
                    p.circle_filled(point, 3.5, PAPER);
                    p.circle_stroke(point, 3.5, Stroke::new(1.5, INK));
                }
            }
            if let Some(snap) = self.line_preview_snap() {
                if let Some(point) = screen(snap.point) {
                    let color = match snap.kind {
                        Inference::Axis(a) | Inference::OnAxis(a) => axis_color(a),
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
                if edit.error.is_some() { ACCENT } else { INK },
            );
            let at = rect.left_bottom() + egui::vec2(20., -40.);
            p.rect_filled(
                Rect::from_min_size(
                    at - egui::vec2(8., 6.),
                    galley.size() + egui::vec2(16., 12.),
                ),
                5,
                PAPER,
            );
            p.galley(at, galley, INK);
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
                            p.circle_filled(point, 4., PAPER);
                            p.circle_stroke(point, 4., Stroke::new(1.5, GREEN));
                        }
                    }
                }
                if let (Some(a), Some(b)) =
                    (screen(edit.anchor), screen(edit.anchor + edit.direction))
                {
                    p.line_segment(
                        [a, b],
                        Stroke::new(1.5, self.lock.map(axis_color).unwrap_or(GREEN)),
                    );
                }
            }
        }
        self.paint_solid_tools(&p, rect);
        self.paint_measurements(&p, rect, model);
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
                        if Self::icon_button(ui,"fit","Fit model to view (F)",true,colors.muted){self.fit(&ctx);}
                        ui.separator();
                        if ui.checkbox(&mut self.doc.snap_to_geometry,RichText::new("Snap").size(12.).color(colors.muted)).on_hover_text("Snap drawing to existing endpoints, midpoints, edges, and faces").changed() {self.snap=None;self.changed(&ctx);}
                        if ui.checkbox(&mut self.doc.snap_to_axes,RichText::new("Snap to axes").size(12.).color(colors.muted)).on_hover_text("Automatically align drawing with the red, green, and blue axes. Hover an endpoint, midpoint, or edge to acquire axes through that position. Arrow keys can still lock an axis when this is off.").changed() {self.snap=None;self.changed(&ctx);}
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
    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.help;
        egui::Window::new("Make yourself at home").open(&mut open).collapsible(false).resizable(false).vscroll(true).max_height((ctx.content_rect().height()-80.).max(180.)).default_width(440.).anchor(Align2::CENTER_CENTER,Vec2::ZERO).show(ctx,|ui|{
            let colors = Palette::for_ui(ui);
            ui.add_space(8.);ui.label(RichText::new("Shape your next idea.").size(20.).strong());ui.add_space(8.);
            ui.label("Build linework in three dimensions. Your model is saved automatically on this device.");ui.add_space(16.);
            egui::Grid::new("shortcuts").spacing(egui::vec2(30.,12.)).show(ui,|ui|{
                for (key,label) in [("Ctrl / ⌘ N","New project"),("Ctrl / ⌘ O","Open .ketchup project"),("Ctrl / ⌘ S","Save project"),("Ctrl / ⌘ Shift S","Save project as"),("Space","Select"),("L","Draw connected lines"),("R","Rectangle"),("C","Circle"),("G","Polygon"),("D","Freehand"),("P","Push/Pull a face"),("Union flyout","Flip, Outer Shell, Union, Intersect, Trim, Split"),("S","Scale geometry"),("Q","Offset a face outline"),("M","Move geometry"),(Self::move_copy_key(ctx),"Toggle Move / Copy while using Move"),("/N or Nx + Enter","After placing a copy: divide the span or repeat at the same spacing"),("B","Paint faces"),("T","Tape Measure"),("A","Protractor"),("I","Dimensions"),("E","Erase edges"),("O","Orbit"),("Shift + orbit drag","Pan the camera"),("Hold Ctrl / ⌘ + move cursor","Orbit temporarily while drawing"),("Middle mouse drag","Orbit temporarily from any tool"),("Shift + middle drag","Pan temporarily from any tool"),("Scroll","Zoom toward the pointer"),("Hover geometry","Acquire X/Y/Z reference axes through a point or edge"),("Axis lock + hover","Match the hovered geometry’s coordinate on the locked axis"),("→ / ← / ↑","Lock X / Y / Z while drawing"),("↓","Clear the axis lock"),("Length or formula + Enter","Set length or radius, such as 60cm - 89mm"),("Width,height + Enter","Rectangle dimensions, such as 5m,50mm"),("Escape","Cancel the unfinished operation"),("Shift + click","Toggle entity selection"),("Right click","Object and clipboard actions"),("Double-click object","Edit its children, including nested objects"),("Escape (while editing)","Close the current group/component level"),("Ctrl / ⌘ X / C / V","Cut / Copy / Paste"),("Ctrl / ⌘ Shift V","Paste in place"),("Delete / Backspace","Delete selection"),("Ctrl / ⌘ Z","Undo"),("Ctrl / ⌘ Shift Z","Redo"),("F","Fit model in the viewport")] {
                    ui.label(RichText::new(key).monospace().size(12.).color(colors.green));ui.label(label);ui.end_row();
                }
            });ui.add_space(14.);ui.label(RichText::new("Close a flat outline to fill it. Select its interior to delete only the face; select a boundary edge to delete that edge and its dependent faces. Snap is enabled by default.").small().color(colors.muted));
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
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
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
            ui.disable();
        }
        self.top_bar(ui);
        self.status_bar(ui);
        self.tool_rail(ui);
        self.inspector_panel(ui);
        let theme_before = ctx.options(|options| options.theme_preference);
        self.viewport_panel(ui);
        if theme_before != ctx.options(|options| options.theme_preference)
            && let Some(storage) = frame.storage_mut()
        {
            save_preference(&ctx, storage);
        }
        self.component_window(&ctx);
        if self.help {
            self.help_window(&ctx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.update_window(&ctx);
        self.project_dialogs(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        Self::window_resize_handles(ui);
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
