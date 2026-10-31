//! A shared document with independently framed cameras. Tools use the active
//! document camera while every view shares the model, selection and history.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum Layout {
    Pane {
        id: u64,
        tabs: Vec<u64>,
        selected: u64,
    },
    Split {
        id: u64,
        horizontal: bool,
        ratio: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

impl Default for Layout {
    fn default() -> Self {
        Self::Pane {
            id: 0,
            tabs: vec![0],
            selected: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    layout: Layout,
    active: u64,
    next_id: u64,
    cameras: BTreeMap<u64, sidebar::CameraPreferences>,
    names: BTreeMap<u64, String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            active: 0,
            next_id: 1,
            cameras: BTreeMap::new(),
            names: BTreeMap::new(),
        }
    }
}

pub(super) struct Workspace {
    layout: Layout,
    active: u64,
    next_id: u64,
    cameras: BTreeMap<u64, Camera>,
    scene_caches: BTreeMap<u64, renderer::SceneCache>,
    names: BTreeMap<u64, String>,
    dragging: Option<u64>,
    pub(super) renaming: Option<(u64, String)>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            layout: Layout::default(),
            active: 0,
            next_id: 1,
            cameras: BTreeMap::new(),
            scene_caches: BTreeMap::new(),
            names: BTreeMap::new(),
            dragging: None,
            renaming: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropSide {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

struct PaneRect {
    id: u64,
    selected: u64,
    rect: Rect,
}

impl Layout {
    fn pane_mut(&mut self, pane: u64) -> Option<&mut Self> {
        match self {
            Self::Pane { id, .. } if *id == pane => Some(self),
            Self::Split { first, second, .. } => {
                first.pane_mut(pane).or_else(|| second.pane_mut(pane))
            }
            _ => None,
        }
    }

    fn pane_for_tab(&self, tab: u64) -> Option<u64> {
        match self {
            Self::Pane { id, tabs, .. } if tabs.contains(&tab) => Some(*id),
            Self::Split { first, second, .. } => {
                first.pane_for_tab(tab).or_else(|| second.pane_for_tab(tab))
            }
            _ => None,
        }
    }

    fn select(&mut self, tab: u64) -> bool {
        match self {
            Self::Pane { tabs, selected, .. } if tabs.contains(&tab) => {
                *selected = tab;
                true
            }
            Self::Split { first, second, .. } => first.select(tab) || second.select(tab),
            _ => false,
        }
    }

    fn remove(&mut self, tab: u64) {
        match self {
            Self::Pane { tabs, selected, .. } => {
                tabs.retain(|id| *id != tab);
                if *selected == tab {
                    *selected = tabs.first().copied().unwrap_or(0);
                }
            }
            Self::Split { first, second, .. } => {
                first.remove(tab);
                second.remove(tab);
                if first.empty() {
                    *self = *second.clone();
                } else if second.empty() {
                    *self = *first.clone();
                }
            }
        }
    }

    fn empty(&self) -> bool {
        matches!(self, Self::Pane { tabs, .. } if tabs.is_empty())
    }

    fn tabs(&self, ids: &mut Vec<u64>) {
        match self {
            Self::Pane { tabs, .. } => ids.extend(tabs),
            Self::Split { first, second, .. } => {
                first.tabs(ids);
                second.tabs(ids);
            }
        }
    }

    // Validate saved trees before using their IDs or recursively laying them out.
    fn valid(&self, depth: usize, ids: &mut BTreeSet<u64>, panes: &mut BTreeSet<u64>) -> bool {
        if depth > 16 {
            return false;
        }
        match self {
            Self::Pane { id, tabs, selected } => {
                *id < u64::MAX - 1
                    && panes.insert(*id)
                    && !tabs.is_empty()
                    && tabs.contains(selected)
                    && tabs.iter().all(|id| *id < u64::MAX - 1 && ids.insert(*id))
            }
            Self::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                *id < u64::MAX - 1
                    && panes.insert(*id)
                    && ratio.is_finite()
                    && (0.1..=0.9).contains(ratio)
                    && first.valid(depth + 1, ids, panes)
                    && second.valid(depth + 1, ids, panes)
            }
        }
    }

    fn rectangles(&mut self, ui: &mut egui::Ui, rect: Rect, panes: &mut Vec<PaneRect>) {
        match self {
            Self::Pane { id, selected, .. } => panes.push(PaneRect {
                id: *id,
                selected: *selected,
                rect,
            }),
            Self::Split {
                id,
                horizontal,
                ratio,
                first,
                second,
            } => {
                let span = if *horizontal {
                    rect.width()
                } else {
                    rect.height()
                };
                let available = (span - 6.).max(0.);
                let offset = available * *ratio;
                let (a, divider, b) = if *horizontal {
                    let x = rect.left() + offset;
                    (
                        Rect::from_min_max(rect.min, egui::pos2(x, rect.bottom())),
                        Rect::from_min_max(
                            egui::pos2(x, rect.top()),
                            egui::pos2(x + 6., rect.bottom()),
                        ),
                        Rect::from_min_max(egui::pos2(x + 6., rect.top()), rect.max),
                    )
                } else {
                    let y = rect.top() + offset;
                    (
                        Rect::from_min_max(rect.min, egui::pos2(rect.right(), y)),
                        Rect::from_min_max(
                            egui::pos2(rect.left(), y),
                            egui::pos2(rect.right(), y + 6.),
                        ),
                        Rect::from_min_max(egui::pos2(rect.left(), y + 6.), rect.max),
                    )
                };
                let response = ui.interact(
                    divider,
                    ui.scope_id().with(("viewport-divider", *id)),
                    Sense::drag(),
                );
                if response.dragged() && available > 1. {
                    let delta = ui.input(|i| {
                        if *horizontal {
                            i.pointer.delta().x
                        } else {
                            i.pointer.delta().y
                        }
                    });
                    *ratio = (*ratio + delta / available).clamp(0.1, 0.9);
                }
                response.on_hover_cursor(if *horizontal {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
                ui.painter()
                    .rect_filled(divider, 0, Palette::for_ui(ui).border);
                first.rectangles(ui, a, panes);
                second.rectangles(ui, b, panes);
            }
        }
    }
}

impl Workspace {
    fn name(&self, id: u64) -> String {
        self.names.get(&id).cloned().unwrap_or_else(|| {
            if id == 0 {
                "Current".to_owned()
            } else {
                format!("View {id}")
            }
        })
    }

    pub(super) fn capture(&self, camera: &Camera) -> Preferences {
        let mut cameras: BTreeMap<_, _> = self
            .cameras
            .iter()
            .map(|(id, camera)| (*id, sidebar::CameraPreferences::capture(camera)))
            .collect();
        cameras.insert(self.active, sidebar::CameraPreferences::capture(camera));
        Preferences {
            layout: self.layout.clone(),
            active: self.active,
            next_id: self.next_id,
            cameras,
            names: self.names.clone(),
        }
    }

    pub(super) fn restore(
        preferences: &Preferences,
        camera: &mut Camera,
        saved: &BTreeMap<u64, Camera>,
    ) -> Self {
        let mut ids = BTreeSet::new();
        let mut panes = BTreeSet::new();
        if !preferences.layout.valid(0, &mut ids, &mut panes)
            || !ids.contains(&preferences.active)
            || panes.contains(&u64::MAX)
        {
            return Self::default();
        }
        let mut workspace = Self {
            layout: preferences.layout.clone(),
            active: preferences.active,
            next_id: preferences.next_id.max(
                ids.iter()
                    .chain(panes.iter())
                    .max()
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(1),
            ),
            names: preferences
                .names
                .iter()
                .filter(|(id, name)| ids.contains(*id) && !name.trim().is_empty())
                .map(|(id, name)| (*id, name.clone()))
                .collect(),
            cameras: ids
                .into_iter()
                .map(|id| {
                    let mut view = saved
                        .get(&id)
                        .filter(|view| view.valid())
                        .cloned()
                        .unwrap_or_else(|| camera.clone());
                    if let Some(preference) = preferences.cameras.get(&id) {
                        preference.restore(&mut view);
                    }
                    (id, view)
                })
                .collect(),
            scene_caches: BTreeMap::new(),
            dragging: None,
            renaming: None,
        };
        workspace.layout.select(workspace.active);
        *camera = workspace.cameras[&workspace.active].clone();
        workspace
    }

    pub(super) fn reframe_project(&mut self, camera: &Camera, saved: &BTreeMap<u64, Camera>) {
        // Retain reusable orientations, never a different project's target/scale.
        for (id, view) in &mut self.cameras {
            if let Some(framing) = saved.get(id).filter(|view| view.valid()) {
                *view = framing.clone();
            } else {
                view.target = camera.target;
                view.distance = camera.distance;
            }
        }
        self.cameras.insert(self.active, camera.clone());
        self.dragging = None;
    }

    fn add(&mut self, pane: u64, camera: &Camera) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(Layout::Pane { tabs, selected, .. }) = self.layout.pane_mut(pane) {
            tabs.push(id);
            *selected = id;
        }
        self.cameras.insert(id, camera.clone());
        id
    }

    fn dock(&mut self, tab: u64, pane: u64, side: DropSide) {
        let Some(Layout::Pane { tabs, .. }) = self.layout.pane_mut(pane) else {
            return;
        };
        if tabs.contains(&tab) && (tabs.len() == 1 || side == DropSide::Center) {
            return;
        }
        // A stack split leaves the original pane alive. Moving its last tab
        // to another pane collapses the empty source branch.
        if side == DropSide::Center {
            self.layout.remove(tab);
            if let Some(Layout::Pane { tabs, selected, .. }) = self.layout.pane_mut(pane) {
                tabs.push(tab);
                *selected = tab;
            }
        } else {
            let new_pane = self.next_id;
            self.next_id += 2;
            let target = self.layout.pane_mut(pane).unwrap();
            let mut original = target.clone();
            // Remove locally first so the moved tab is not duplicated.
            original.remove(tab);
            self.layout.remove(tab);
            let target = self.layout.pane_mut(pane).unwrap();
            let fresh = Layout::Pane {
                id: new_pane,
                tabs: vec![tab],
                selected: tab,
            };
            let before = matches!(side, DropSide::Left | DropSide::Top);
            let (first, second) = if before {
                (fresh, original)
            } else {
                (original, fresh)
            };
            *target = Layout::Split {
                id: new_pane + 1,
                horizontal: matches!(side, DropSide::Left | DropSide::Right),
                ratio: 0.5,
                first: Box::new(first),
                second: Box::new(second),
            };
        }
    }
}

fn drop_target(rect: Rect, pos: Pos2) -> (DropSide, Rect) {
    let x = (pos.x - rect.left()) / rect.width().max(1.);
    let y = (pos.y - rect.top()) / rect.height().max(1.);
    let distances = [
        (x, DropSide::Left),
        (1. - x, DropSide::Right),
        (y, DropSide::Top),
        (1. - y, DropSide::Bottom),
    ];
    let (distance, side) = distances
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
    let side = if distance < 0.25 {
        side
    } else {
        DropSide::Center
    };
    let mut preview = rect;
    match side {
        DropSide::Left => preview.max.x = rect.center().x,
        DropSide::Right => preview.min.x = rect.center().x,
        DropSide::Top => preview.max.y = rect.center().y,
        DropSide::Bottom => preview.min.y = rect.center().y,
        DropSide::Center => {}
    }
    (side, preview)
}

impl KetchupApp {
    pub(super) fn sync_viewport_cameras(&mut self) {
        self.viewports
            .cameras
            .insert(self.viewports.active, self.doc.camera.clone());
        self.doc.viewport_cameras = self.viewports.cameras.clone();
    }

    fn activate_viewport(&mut self, id: u64) {
        if self.viewports.active == id {
            return;
        }
        self.viewports
            .cameras
            .insert(self.viewports.active, self.doc.camera.clone());
        self.viewports.active = id;
        self.viewports.layout.select(id);
        self.doc.camera = self
            .viewports
            .cameras
            .get(&id)
            .cloned()
            .unwrap_or_else(|| self.doc.camera.clone());
        self.viewport_pointer = None;
        self.drawing_orbit_pivot = None;
        self.hover = None;
        self.flip_hover = None;
        self.snap = None;
    }

    fn close_viewport(&mut self, tab: u64, ctx: &egui::Context) {
        let mut tabs = Vec::new();
        self.viewports.layout.tabs(&mut tabs);
        if tabs.len() <= 1 || !tabs.contains(&tab) || self.gesture.is_some() {
            return;
        }
        self.viewports.layout.remove(tab);
        if self.viewports.active == tab {
            let mut remaining = Vec::new();
            self.viewports.layout.tabs(&mut remaining);
            self.activate_viewport(remaining[0]);
        }
        self.viewports.cameras.remove(&tab);
        self.viewports.names.remove(&tab);
        ctx.request_repaint();
    }

    pub(super) fn viewport_tabs(&mut self, ui: &mut egui::Ui) {
        let mut tabs = Vec::new();
        self.viewports.layout.tabs(&mut tabs);
        let mut activate = None;
        let mut close = None;
        let mut add = false;
        let key = "viewport-tabs";
        let scroll = sidebar::restored_scroll_area(
            ui, key, egui::ScrollArea::horizontal()
                .id_salt("viewport-tabs")
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden),
        ).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                for tab in &tabs {
                    ui.push_id(("viewport-tab", *tab), |ui| {
                        let label = self.viewports.name(*tab);
                        let response = ui.add(egui::Button::new(label)
                            .selected(*tab == self.viewports.active)
                            .sense(Sense::click_and_drag()));
                        if response.clicked() { activate = Some(*tab); }
                        if response.drag_started() { self.viewports.dragging = Some(*tab); }
                        response.context_menu(|ui| {
                            Self::menu_style(ui.style_mut());
                            if ui.add_enabled(self.gesture.is_none(), egui::Button::new("Rename…")).clicked() {
                                self.viewports.renaming = Some((*tab, self.viewports.name(*tab)));
                                Self::request_name_input_focus(ui.ctx(), egui::Id::unique("rename_viewport_name"));
                                ui.close();
                            }
                            if ui.add_enabled(tabs.len() > 1 && self.gesture.is_none(), egui::Button::new("Close")).clicked() {
                                close = Some(*tab);
                                ui.close();
                            }
                        });
                        response.on_hover_text("Drag to a viewport edge to split, or its center to join tabs. Right-click to rename or close.");
                    });
                }
                add = ui.button("+").on_hover_text("Create viewport").clicked();
                ui.separator();
                self.edit_context_bar(ui);
            });
        });
        sidebar::remember_scroll(ui.ctx(), key, scroll.state.clamped_offset());
        if self.gesture.is_none() {
            if add {
                if let Some(pane) = self.viewports.layout.pane_for_tab(self.viewports.active) {
                    activate = Some(self.viewports.add(pane, &self.doc.camera));
                }
            }
            if let Some(tab) = activate {
                self.activate_viewport(tab);
                ui.ctx().request_repaint();
            }
            if let Some(tab) = close {
                self.close_viewport(tab, ui.ctx());
            }
        }
    }

    pub(super) fn rename_viewport_window(&mut self, ctx: &egui::Context) {
        let Some((id, mut name)) = self.viewports.renaming.take() else {
            return;
        };
        if self.viewports.layout.pane_for_tab(id).is_none() {
            return;
        }
        let mut save = false;
        let mut cancel = false;
        let response = Modal::new("rename_viewport", 360.).show(ctx, |modal| {
            modal.header(|ui| {
                ui.heading("Rename view");
            });
            modal.body(|ui| {
                ui.label("Name");
                let input =
                    Self::name_input(ui, &mut name, egui::Id::unique("rename_viewport_name"));
                if name.len() > 1024 {
                    name.truncate(name.floor_char_boundary(1024));
                }
                save = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            });
            modal.footer(|ui| {
                cancel = ui.button("Cancel").clicked();
                let valid = !name.trim().is_empty();
                save = (primary_button(ui, "Save", valid).clicked() || save) && valid;
            });
        });
        if save {
            self.viewports.names.insert(id, name.trim().to_owned());
        } else if !cancel && !response.should_close() {
            self.viewports.renaming = Some((id, name));
        }
    }

    pub(super) fn viewport_workspace(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !ui.is_enabled() {
            self.viewports.dragging = None;
        }
        self.viewports
            .cameras
            .insert(self.viewports.active, self.doc.camera.clone());
        let mut panes = Vec::new();
        self.viewports
            .layout
            .rectangles(ui, ui.available_rect_before_wrap(), &mut panes);
        let mut activate = None;
        let mut views = Vec::new();
        // Build every pane first, route input exactly once, then paint every
        // camera from the updated shared model in this same frame.
        for pane in &panes {
            if pane.rect.width() <= 1. || pane.rect.height() <= 1. {
                continue;
            }
            let rect = pane.rect;
            let response = ui.interact(
                rect,
                ui.scope_id().with(("viewport-canvas", pane.id)),
                Sense::click_and_drag(),
            );
            // Activate on press so the first click/drag uses this camera.
            // Hover and wheel input leave the active camera unchanged.
            if self.gesture.is_none()
                && self.viewports.dragging.is_none()
                && response.enabled()
                && response.contains_pointer()
                && ctx.input(|input| input.pointer.any_pressed())
                && ctx
                    .input(|input| input.pointer.hover_pos())
                    .is_some_and(|pos| rect.contains(pos))
            {
                activate = Some(pane.selected);
            }
            views.push((pane.selected, rect, response));
        }
        if self.gesture.is_none() {
            if let Some(id) = activate {
                if self.viewports.active != id {
                    ctx.request_repaint();
                }
                self.activate_viewport(id);
            }
        }
        if self.viewports.dragging.is_none() {
            if let Some((_, rect, response)) =
                views.iter().find(|(id, _, _)| *id == self.viewports.active)
            {
                self.viewport = *rect;
                self.viewport_input(&ctx, response);
                if !ctx
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|pos| self.compass_contains(pos))
                {
                    self.context_menu(response);
                }
                self.viewports
                    .cameras
                    .insert(self.viewports.active, self.doc.camera.clone());
            }
        }
        let mut tabs = Vec::new();
        self.viewports.layout.tabs(&mut tabs);
        let show_names = tabs.len() > 1;
        let active_camera = self.doc.camera.clone();
        let active_rect = self.viewport;
        // Occlusion and GPU meshes depend on the camera. Sharing them across
        // panes would invalidate them on every pane, even in a static frame.
        self.viewports
            .scene_caches
            .retain(|id, _| views.iter().any(|(visible, _, _)| visible == id));
        for (id, rect, response) in &views {
            if rect.width() <= 1. || rect.height() <= 1. {
                continue;
            }
            self.doc.camera = self
                .viewports
                .cameras
                .get(id)
                .cloned()
                .unwrap_or_else(|| active_camera.clone());
            self.viewport = *rect;
            // Screen-space drag previews only belong to their input viewport.
            let inactive_gesture = if *id != self.viewports.active {
                self.gesture.take()
            } else {
                None
            };
            if self.gpu_ready {
                let mut cache = self.viewports.scene_caches.remove(id).unwrap_or_default();
                std::mem::swap(&mut cache, &mut self.scene_cache);
                self.paint_scene(ui, *rect);
                std::mem::swap(&mut cache, &mut self.scene_cache);
                self.viewports.scene_caches.insert(*id, cache);
            } else {
                ui.painter().with_clip_rect(*rect).text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "A compatible GPU renderer is required. Try updating your graphics driver.",
                    FontId::proportional(15.),
                    ACCENT,
                );
            }
            self.paint_compass(ui, response);
            if show_names {
                // Paint a passive label so clicks still reach the viewport.
                let colors = Palette::for_ui(ui);
                let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                let mut job = egui::text::LayoutJob::simple(
                    self.viewports.name(*id),
                    FontId::proportional(13.),
                    colors.ink,
                    (rect.width() - 28.).max(1.),
                );
                job.wrap.max_rows = 1;
                let galley = painter.layout_job(job);
                let badge = Rect::from_min_size(
                    rect.left_top() + egui::vec2(8., 8.),
                    galley.size() + egui::vec2(12., 8.),
                );
                painter.rect_filled(badge, 4, colors.paper.gamma_multiply(0.94));
                painter.galley(badge.min + egui::vec2(6., 4.), galley, colors.ink);
            }
            if *id == self.viewports.active {
                self.paint_tool_cursor(&ctx, response);
                ui.painter().rect_stroke(
                    *rect,
                    0,
                    Stroke::new(1., Palette::for_ui(ui).accent.gamma_multiply(0.5)),
                    egui::StrokeKind::Inside,
                );
            } else if inactive_gesture.is_some() {
                self.gesture = inactive_gesture;
            }
        }
        self.doc.camera = active_camera;
        self.viewport = active_rect;
        if let Some(tab) = self.viewports.dragging {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
                if let Some(pane) = panes.iter().find(|pane| pane.rect.contains(pos)) {
                    let (side, preview) = drop_target(pane.rect, pos);
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Tooltip,
                        egui::Id::unique("viewport-drop-preview"),
                    ));
                    let color = Palette::for_ui(ui).accent;
                    painter.rect_filled(preview.shrink(3.), 4, color.gamma_multiply(0.2));
                    painter.rect_stroke(
                        preview.shrink(3.),
                        4,
                        Stroke::new(2., color),
                        egui::StrokeKind::Inside,
                    );
                    if ctx.input(|i| i.pointer.any_released()) {
                        self.viewports.dock(tab, pane.id, side);
                        self.activate_viewport(tab);
                        ctx.request_repaint();
                    }
                }
            }
            if ctx.input(|i| !i.pointer.primary_down()) {
                self.viewports.dragging = None;
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "viewport_tests.rs"]
mod tests;
