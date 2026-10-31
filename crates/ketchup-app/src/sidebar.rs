use super::*;
use ketchup_core::{
    AngleUnit, AreaUnit, DisplaySettings, FaceStyle, LengthUnit, Projection, VolumeUnit,
};

#[path = "view_controls.rs"]
mod view_controls;

#[path = "outliner.rs"]
mod outliner;
pub(super) use outliner::OutlinerCache;

#[path = "inspector_docking.rs"]
mod docking;
pub(super) use docking::InspectorLayout;

const ORDER_PREFERENCE_KEY: &str = "ketchup.inspector-order.v1";

const PREFERENCE_KEY: &str = "ketchup.inspector.v1";
const OUTLINER_SEARCH_ID: &str = "outliner_search";

// Persist the whole settings object: new DisplaySettings fields automatically
// participate, while serde defaults keep older preferences compatible.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct InspectorPreferences {
    general: settings::GeneralSettings,
    visible: bool,
    width: f32,
    docking: Option<docking::DockLayout>,
    component_filter: String,
    outliner_filter: String,
    shortcut_filter: String,
    paint_color: [u8; 3],
    paint_target: PaintTarget,
    tools: ToolPreferences,
    snapping: Option<SnappingPreferences>,
    cutting_plan: CuttingPlanPreferences,
    time_estimates: time_estimates::Settings,
    finish_product_filters: std::collections::BTreeMap<String, String>,
    stock_material_filter: String,
    last_seen_release_notes: Option<String>,
    display: Option<DisplaySettings>,
    camera: Option<CameraPreferences>,
    viewports: viewports::Preferences,
    screenshot: crate::screenshots::Settings,
    sections: std::collections::BTreeMap<String, bool>,
    scroll_positions: std::collections::BTreeMap<String, [f32; 2]>,
}

impl Default for InspectorPreferences {
    fn default() -> Self {
        Self {
            general: Default::default(),
            visible: true,
            width: 300.,
            docking: None,
            component_filter: String::new(),
            outliner_filter: String::new(),
            shortcut_filter: String::new(),
            paint_color: [195, 96, 67],
            paint_target: PaintTarget::default(),
            tools: ToolPreferences::default(),
            snapping: None,
            cutting_plan: CuttingPlanPreferences::default(),
            time_estimates: Default::default(),
            finish_product_filters: Default::default(),
            stock_material_filter: String::new(),
            last_seen_release_notes: None,
            display: None,
            camera: None,
            viewports: viewports::Preferences::default(),
            screenshot: Default::default(),
            sections: Default::default(),
            scroll_positions: Default::default(),
        }
    }
}

// Remember tool choices and reusable options, never an in-progress operation or
// model-specific selection. Defaults also apply to partially saved settings.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct ToolPreferences {
    active: Tool,
    drawing: Tool,
    measurement: Tool,
    solid: Tool,
    polygon_sides: usize,
    move_copy: bool,
    axis_lock: Option<Axis>,
}

impl Default for ToolPreferences {
    fn default() -> Self {
        Self {
            active: Tool::Select,
            drawing: Tool::Line,
            measurement: Tool::Dimensions,
            solid: Tool::Union,
            polygon_sides: 6,
            move_copy: false,
            axis_lock: None,
        }
    }
}

impl ToolPreferences {
    fn capture(app: &KetchupApp) -> Self {
        Self {
            active: app.tool,
            drawing: app.drawing_tool,
            measurement: app.measurement_tool,
            solid: app.solid_tool,
            polygon_sides: app.polygon_sides,
            move_copy: app.move_copy,
            axis_lock: app.lock,
        }
    }

    fn restore(&self, app: &mut KetchupApp) {
        let defaults = Self::default();
        app.drawing_tool = if self.drawing.is_drawing() {
            self.drawing
        } else {
            defaults.drawing
        };
        app.measurement_tool = if self.measurement.is_measuring() {
            self.measurement
        } else {
            defaults.measurement
        };
        app.solid_tool = if self.solid.is_solid() {
            self.solid
        } else {
            defaults.solid
        };
        app.polygon_sides = self.polygon_sides.clamp(3, 64);
        // This resets transient state without executing the tool (solid tools
        // can otherwise act immediately on selected geometry).
        app.set_tool(self.active);
        app.move_copy = self.move_copy;
        app.lock = self.axis_lock;
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct SnappingPreferences {
    geometry: bool,
    edges: bool,
    faces: bool,
    axes: bool,
    grid: bool,
}

impl Default for SnappingPreferences {
    fn default() -> Self {
        Self {
            geometry: true,
            edges: true,
            faces: true,
            axes: true,
            grid: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CuttingPlanPreferences {
    layout: crate::cutting_layout::Settings,
    selection: Option<bool>,
}

impl Default for CuttingPlanPreferences {
    fn default() -> Self {
        Self {
            layout: Default::default(),
            selection: None,
        }
    }
}

// Keep framing (target and distance) in the document, so opening another
// project never inherits the previous model's position or scale.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct CameraPreferences {
    projection: Projection,
    fov: f64,
    yaw: f64,
    pitch: f64,
}

impl Default for CameraPreferences {
    fn default() -> Self {
        Self::capture(&Camera::default())
    }
}

impl CameraPreferences {
    pub(super) fn capture(camera: &Camera) -> Self {
        Self {
            projection: camera.projection,
            fov: camera.fov,
            yaw: camera.yaw,
            pitch: camera.pitch,
        }
    }

    pub(super) fn restore(&self, camera: &mut Camera) {
        let candidate = Camera {
            projection: self.projection,
            fov: self.fov,
            yaw: self.yaw,
            pitch: self.pitch,
            ..camera.clone()
        };
        if candidate.valid() {
            *camera = candidate;
        }
    }
}

#[derive(Clone, Default)]
struct SectionMemory(
    std::collections::BTreeMap<String, bool>,
    std::collections::BTreeMap<String, egui::Id>,
);

#[derive(Clone, Default)]
struct ScrollMemory {
    positions: std::collections::BTreeMap<String, [f32; 2]>,
    scopes: std::collections::BTreeMap<String, egui::Id>,
}

fn scroll_memory_id() -> egui::Id {
    egui::Id::unique((PREFERENCE_KEY, "scroll_positions"))
}

fn valid_scroll_position(position: [f32; 2]) -> [f32; 2] {
    position.map(|value| if value.is_finite() { value.max(0.) } else { 0. })
}

// Restore only when entering a new UI scope, so normal wheel input, dragging,
// and kinetic scrolling remain owned by egui. Keys are independent of layout.
pub(super) fn restored_scroll_area(
    ui: &egui::Ui,
    key: &str,
    area: egui::ScrollArea,
) -> egui::ScrollArea {
    let position = ui.ctx().data_mut(|data| {
        let memory = data.get_temp_mut_or_default::<ScrollMemory>(scroll_memory_id());
        let previous_scope = memory.scopes.insert(key.to_owned(), ui.scope_id());
        (previous_scope != Some(ui.scope_id()))
            .then(|| memory.positions.get(key).copied())
            .flatten()
    });
    if let Some(position) = position {
        area.scroll_offset(egui::vec2(position[0], position[1]))
    } else {
        area
    }
}

pub(super) fn remember_scroll(ctx: &egui::Context, key: &str, offset: Vec2) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<ScrollMemory>(scroll_memory_id())
            .positions
            .insert(key.to_owned(), valid_scroll_position([offset.x, offset.y]));
    });
}

fn section_memory_id() -> egui::Id {
    egui::Id::unique(PREFERENCE_KEY)
}

// All inspector disclosures (including future ones) use these helpers, with a
// stable key independent of their position, title, and docked/floating layout.
pub(super) fn restore_section(ctx: &egui::Context, id: egui::Id, key: &str, default_open: bool) {
    let open = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
            .0
            .get(key)
            .copied()
    });
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, default_open);
    if let Some(open) = open {
        state.set_open(open);
    }
    state.store(ctx);
}

// CollapsingHeader owns its internal UI scope. Force the remembered value only
// when entering a different scope, then let egui handle clicks normally.
pub(super) fn restored_header_open(ui: &egui::Ui, key: &str, default_open: bool) -> Option<bool> {
    ui.ctx().data_mut(|data| {
        let memory = data.get_temp_mut_or_default::<SectionMemory>(section_memory_id());
        let previous_scope = memory.1.insert(key.to_owned(), ui.scope_id());
        (previous_scope != Some(ui.scope_id()))
            .then(|| memory.0.get(key).copied().unwrap_or(default_open))
    })
}

pub(super) fn remember_section(ctx: &egui::Context, id: egui::Id, key: &str) {
    if let Some(state) = egui::collapsing_header::CollapsingState::load(ctx, id) {
        ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
                .0
                .insert(key.to_owned(), state.is_open());
        });
    }
}

fn set_section_open(ctx: &egui::Context, key: &str, open: bool) {
    ctx.data_mut(|data| {
        let memory = data.get_temp_mut_or_default::<SectionMemory>(section_memory_id());
        memory.0.insert(key.to_owned(), open);
        // Apply the new value even to a CollapsingHeader already in this scope.
        memory.1.remove(key);
    });
}

impl KetchupApp {
    pub(super) fn request_outliner_search(&mut self, ctx: &egui::Context) {
        self.inspector = true;
        self.focus_outliner_search = true;
        set_section_open(ctx, "outliner", true);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::unique(OUTLINER_SEARCH_ID)));
        ctx.request_repaint();
    }

    pub(super) fn restore_inspector_preferences(
        &mut self,
        ctx: &egui::Context,
        storage: Option<&dyn eframe::Storage>,
    ) {
        self.inspector_order = restore_order(storage);
        let mut preferences = storage
            .and_then(|storage| eframe::get_value::<InspectorPreferences>(storage, PREFERENCE_KEY))
            .unwrap_or_default();
        self.inspector_layout =
            InspectorLayout::restore(preferences.docking.as_ref(), &self.inspector_order);
        self.inspector_order = self.inspector_layout.order();
        for position in preferences.scroll_positions.values_mut() {
            *position = valid_scroll_position(*position);
        }
        self.general_settings = preferences.general.clone();
        self.general_settings.normalize();
        preferences.tools.restore(self);
        self.time_estimates = preferences.time_estimates.clone();
        self.time_estimates.sanitize();
        self.finish_product_filters = preferences.finish_product_filters.clone();
        self.stock_material_filter = preferences.stock_material_filter.clone();
        self.inspector = preferences.visible;
        self.inspector_width = if preferences.width.is_finite() {
            preferences.width.clamp(260., 2000.)
        } else {
            300.
        };
        self.component_filter = preferences.component_filter.clone();
        self.outliner_filter = preferences.outliner_filter.clone();
        self.shortcut_filter = preferences.shortcut_filter.clone();
        self.paint_color = preferences.paint_color;
        self.paint_target = preferences.paint_target;
        self.screenshot.settings = preferences.screenshot.clone();
        self.project.cutting_plan_layout = if preferences.cutting_plan.layout.validate().is_ok() {
            preferences.cutting_plan.layout
        } else {
            Default::default()
        };
        self.project.cutting_plan_selection_preference = preferences.cutting_plan.selection;
        if let Some(snapping) = &preferences.snapping {
            self.doc.snap_to_geometry = snapping.geometry;
            self.doc.snap_to_axes = snapping.axes;
        }
        self.snap_to_edge = preferences.snapping.as_ref().is_none_or(|s| s.edges);
        self.snap_to_face = preferences.snapping.as_ref().is_none_or(|s| s.faces);
        self.snap_to_grid = preferences
            .snapping
            .as_ref()
            .is_some_and(|snapping| snapping.grid);
        self.release_notes.last_seen_version = preferences.last_seen_release_notes.clone();
        if let Some(display) = &preferences.display
            && display.valid()
        {
            self.doc.display = display.clone();
        }
        if let Some(camera) = &preferences.camera {
            camera.restore(&mut self.doc.camera);
        }
        self.viewports = viewports::Workspace::restore(
            &preferences.viewports,
            &mut self.doc.camera,
            &self.doc.viewport_cameras,
        );
        ctx.data_mut(|data| {
            data.insert_temp(
                scroll_memory_id(),
                ScrollMemory {
                    positions: preferences.scroll_positions.clone(),
                    scopes: Default::default(),
                },
            );
            data.insert_temp(
                section_memory_id(),
                SectionMemory(preferences.sections.clone(), Default::default()),
            )
        });
        self.inspector_preferences = preferences;
        self.sync_stock_materials(ctx);
    }

    fn current_inspector_preferences(&self, ctx: &egui::Context) -> InspectorPreferences {
        InspectorPreferences {
            general: self.general_settings.clone(),
            visible: self.inspector,
            width: self.inspector_width,
            // Capture the effective layout even before the inspector first renders.
            docking: InspectorLayout::restore(
                self.inspector_layout.dock.as_ref(),
                &self.inspector_order,
            )
            .dock,
            component_filter: self.component_filter.clone(),
            outliner_filter: self.outliner_filter.clone(),
            shortcut_filter: self.shortcut_filter.clone(),
            paint_color: self.paint_color,
            paint_target: self.paint_target,
            tools: ToolPreferences::capture(self),
            snapping: Some(SnappingPreferences {
                geometry: self.doc.snap_to_geometry,
                edges: self.snap_to_edge,
                faces: self.snap_to_face,
                axes: self.doc.snap_to_axes,
                grid: self.snap_to_grid,
            }),
            cutting_plan: CuttingPlanPreferences {
                layout: self.project.cutting_plan_layout,
                selection: self.project.cutting_plan_selection_preference,
            },
            time_estimates: self.time_estimates.clone(),
            finish_product_filters: self.finish_product_filters.clone(),
            stock_material_filter: self.stock_material_filter.clone(),
            last_seen_release_notes: self.release_notes.last_seen_version.clone(),
            display: Some(self.doc.display.clone()),
            camera: Some(CameraPreferences::capture(&self.doc.camera)),
            viewports: self.viewports.capture(&self.doc.camera),
            screenshot: self.screenshot.settings.clone(),
            sections: ctx.data_mut(|data| {
                data.get_temp_mut_or_default::<SectionMemory>(section_memory_id())
                    .0
                    .clone()
            }),
            scroll_positions: ctx.data_mut(|data| {
                data.get_temp_mut_or_default::<ScrollMemory>(scroll_memory_id())
                    .positions
                    .clone()
            }),
        }
    }

    pub(super) fn save_inspector_preferences(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREFERENCE_KEY, &self.inspector_preferences);
        save_order(&self.inspector_order, storage);
    }

    pub(super) fn persist_inspector_changes<'storage>(
        &mut self,
        ctx: &egui::Context,
        storage: Option<&mut (dyn eframe::Storage + 'storage)>,
        order_changed: bool,
    ) {
        let preferences = self.current_inspector_preferences(ctx);
        let changed = preferences != self.inspector_preferences;
        self.inspector_preferences = preferences;
        if (changed || order_changed)
            && let Some(storage) = storage
        {
            self.save_inspector_preferences(storage);
        }
        // Selection alone must survive an immediate reload, without marking
        // the model modified or waiting for the document autosave delay.
        if self.sync_selection() {
            self.persist();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum InspectorSection {
    Outliner,
    EntityInfo,
    Components,
    Tags,
    Display,
    View,
    Units,
    TimeCostEstimates,
    Materials,
}

pub(super) fn restore_order(
    storage: Option<&dyn eframe::Storage>,
) -> [InspectorSection; InspectorSection::COUNT] {
    let Some(saved) = storage.and_then(|storage| {
        eframe::get_value::<Vec<InspectorSection>>(storage, ORDER_PREFERENCE_KEY)
            // The original preference used a fixed five-element RON tuple.
            .or_else(|| {
                eframe::get_value::<[InspectorSection; 5]>(storage, ORDER_PREFERENCE_KEY)
                    .map(Vec::from)
            })
    }) else {
        return InspectorSection::ALL;
    };
    let mut order = Vec::new();
    for section in saved {
        if order.contains(&section) {
            return InspectorSection::ALL;
        }
        order.push(section);
    }
    // Append missing sections in introduction order, regardless of their default position.
    for section in InspectorSection::ALL
        .into_iter()
        .filter(|section| *section != InspectorSection::Units)
        .filter(|section| *section != InspectorSection::TimeCostEstimates)
        .filter(|section| *section != InspectorSection::Materials)
        .chain([
            InspectorSection::Units,
            InspectorSection::TimeCostEstimates,
            InspectorSection::Materials,
        ])
    {
        if !order.contains(&section) {
            order.push(section);
        }
    }
    order.try_into().unwrap_or(InspectorSection::ALL)
}

pub(super) fn save_order(
    order: &[InspectorSection; InspectorSection::COUNT],
    storage: &mut dyn eframe::Storage,
) {
    eframe::set_value(storage, ORDER_PREFERENCE_KEY, &order.to_vec());
    storage.flush();
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    use eframe::Storage;

    #[derive(Default)]
    struct SavedPreferences {
        values: std::collections::BTreeMap<String, String>,
        flushed: bool,
    }

    impl Storage for SavedPreferences {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.values.remove(key);
        }

        fn flush(&mut self) {
            self.flushed = true;
        }
    }

    fn scroll_frame(
        ctx: &egui::Context,
        key: &str,
        scope: &str,
        requested: Vec2,
        content: Vec2,
    ) -> Vec2 {
        let mut offset = Vec2::ZERO;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(500., 400.))),
                ..Default::default()
            },
            |ui| {
                ui.push_id(scope, |ui| {
                    let scroll = restored_scroll_area(
                        ui,
                        key,
                        egui::ScrollArea::both()
                            .id_salt("test_scroll")
                            .max_width(120.)
                            .max_height(80.)
                            .auto_shrink([false, false])
                            .scroll_offset(requested),
                    )
                    .show(ui, |ui| {
                        ui.allocate_exact_size(content, Sense::hover());
                    });
                    offset = scroll.state.clamped_offset();
                    remember_scroll(ui.ctx(), key, offset);
                });
            },
        );
        offset
    }

    #[test]
    fn sticky_headers_follow_their_own_area_and_keep_disclosures_persistent() {
        #[derive(Clone, Copy)]
        struct SectionFrame {
            viewport: Rect,
            section: Rect,
            header: Rect,
        }
        fn render(
            ctx: &egui::Context,
            offsets: [f32; 2],
            events: Vec<egui::Event>,
        ) -> [SectionFrame; 2] {
            let mut frames = [SectionFrame {
                viewport: Rect::NOTHING,
                section: Rect::NOTHING,
                header: Rect::NOTHING,
            }; 2];
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(700., 400.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.columns(2, |columns| {
                        for (index, column) in columns.iter_mut().enumerate() {
                            egui::ScrollArea::vertical()
                                .id_salt(("sticky_test", index))
                                .max_height(200.)
                                .auto_shrink([false, false])
                                .vertical_scroll_offset(offsets[index])
                                .show(column, |ui| {
                                    let viewport = ui.clip_rect();
                                    let id = ["sticky_left", "sticky_right"][index];
                                    let section = inspector_section_sized(
                                        ui,
                                        "First",
                                        id,
                                        true,
                                        None,
                                        Some(viewport),
                                        |ui| {
                                            ui.allocate_space(egui::vec2(180., 500.));
                                        },
                                    );
                                    frames[index] = SectionFrame {
                                        viewport,
                                        section: section.response.rect,
                                        header: section.inner.rect,
                                    };
                                    inspector_section_sized(
                                        ui,
                                        "Next",
                                        ["sticky_left_next", "sticky_right_next"][index],
                                        true,
                                        None,
                                        Some(viewport),
                                        |ui| {
                                            ui.allocate_space(egui::vec2(180., 300.));
                                        },
                                    );
                                });
                        }
                    });
                },
            );
            frames
        }

        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        render(&ctx, [0., 0.], vec![]);
        let initial = render(&ctx, [0., 0.], vec![]);
        let scrolled = render(&ctx, [100., 0.], vec![]);
        assert_eq!(scrolled[0].header.top(), scrolled[0].viewport.top());
        assert!(scrolled[0].section.top() < scrolled[0].viewport.top());
        assert_eq!(scrolled[1].header, initial[1].header);
        let offset = initial[0].section.bottom()
            - initial[0].viewport.top()
            - initial[0].header.height() * 0.5;
        let end = render(&ctx, [offset, 0.], vec![]);
        assert!(end[0].header.top() < end[0].viewport.top());
        assert!(end[0].header.bottom() <= end[0].section.bottom());

        // Clicking the visible pinned header must update the shared disclosure
        // preference immediately, without any model edit.
        render(&ctx, [100., 0.], vec![]);
        let pinned = render(&ctx, [100., 0.], vec![])[0].header.center();
        for pressed in [true, false] {
            render(
                &ctx,
                [100., 0.],
                vec![
                    egui::Event::PointerMoved(pinned),
                    egui::Event::PointerButton {
                        pos: pinned,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.inspector_preferences.sections["sticky_left"]);
        assert!(app.inspector_preferences.sections["sticky_right"]);
        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert!(!restored.inspector_preferences.sections["sticky_left"]);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
    }

    #[test]
    fn sidebar_and_outliner_scroll_positions_save_immediately_and_restore_independently() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        let positions = [
            ("sidebar_left", egui::vec2(0., 110.)),
            ("sidebar_right", egui::vec2(0., 260.)),
            ("outliner_tree", egui::vec2(70., 0.)),
            ("outliner_geometry_rows/None", egui::vec2(0., 180.)),
            ("outliner_geometry_rows/Some(42)", egui::vec2(0., 340.)),
            (currency::RESULTS_SCROLL_KEY, egui::vec2(0., 120.)),
        ];
        let content = Vec2::splat(1500.);
        for (key, position) in positions {
            assert_eq!(scroll_frame(&ctx, key, key, position, content), position);
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed, "{key}");
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());
        }

        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        for (key, position) in positions {
            // Each area has a new egui scope, just as after an app restart or
            // when moving the inspector between docked and floating layouts.
            assert_eq!(
                scroll_frame(&restored_ctx, key, "floating", Vec2::ZERO, content),
                position
            );
            assert_eq!(
                scroll_frame(&restored_ctx, key, "docked", Vec2::ZERO, content),
                position
            );
        }
        storage.flushed = false;
        restored.persist_inspector_changes(&restored_ctx, Some(&mut storage), false);
        assert!(
            !storage.flushed,
            "unchanged positions must not save every frame"
        );
    }

    #[test]
    fn scroll_restoration_allows_further_scrolling_and_clamps_shorter_content() {
        let ctx = egui::Context::default();
        let content = Vec2::splat(1500.);
        let key = "sidebar_right";
        remember_scroll(&ctx, key, egui::vec2(0., 400.));
        assert_eq!(
            scroll_frame(&ctx, key, "docked", Vec2::ZERO, content).y,
            400.
        );
        // Restoration must not keep forcing the saved offset in an existing
        // scope: egui must remain free to process subsequent scrolling.
        assert_eq!(
            scroll_frame(&ctx, key, "docked", egui::vec2(0., 200.), content).y,
            200.
        );
        assert_eq!(
            scroll_frame(&ctx, key, "floating", Vec2::ZERO, content).y,
            200.
        );
        let shorter = scroll_frame(
            &ctx,
            key,
            "floating",
            egui::vec2(0., 200.),
            Vec2::splat(20.),
        );
        assert_eq!(shorter, Vec2::ZERO);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        let restored_ctx = egui::Context::default();
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert_eq!(
            scroll_frame(&restored_ctx, key, "docked", Vec2::ZERO, content),
            Vec2::ZERO
        );
    }

    #[test]
    fn scroll_positions_default_for_older_preferences_and_reject_invalid_offsets() {
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.scroll_positions.is_empty());
        assert_eq!(valid_scroll_position([f32::NAN, f32::INFINITY]), [0., 0.]);
        assert_eq!(valid_scroll_position([-15., f32::NEG_INFINITY]), [0., 0.]);
        let ctx = egui::Context::default();
        let mut preferences = InspectorPreferences::default();
        preferences
            .scroll_positions
            .insert("sidebar_left".into(), [-10., 75.]);
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(
            restored
                .current_inspector_preferences(&ctx)
                .scroll_positions["sidebar_left"],
            [0., 75.]
        );
        assert_eq!(
            scroll_frame(&ctx, "sidebar_left", "left", Vec2::ZERO, Vec2::splat(1500.)),
            egui::vec2(0., 75.)
        );
    }

    #[test]
    fn outliner_virtualized_geometry_lists_restore_their_own_scroll_positions() {
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        let mut doc = Document::default();
        let mut grouped = BTreeSet::new();
        for index in 0..100 {
            let point = DVec3::Y * index as f64 * 2.;
            let edge = doc.model.add_edge(point, point + DVec3::X).unwrap();
            if index < 50 {
                grouped.insert(edge);
            }
        }
        let group = doc.model.make_object(&grouped, false, "Group").unwrap();
        let child_key = format!("outliner_geometry_rows/{:?}", Some(group));
        set_section_open(&ctx, "outliner_geometry/None", true);
        set_section_open(&ctx, &format!("outliner_object/{group}"), true);
        set_section_open(&ctx, &format!("outliner_geometry/{:?}", Some(group)), true);
        remember_scroll(&ctx, "outliner_geometry_rows/None", egui::vec2(0., 120.));
        remember_scroll(&ctx, &child_key, egui::vec2(0., 240.));
        let mut app = KetchupApp::with_document(doc.clone(), None, true);
        let render = |ctx: &egui::Context, app: &mut KetchupApp| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(300., 900.))),
                    ..Default::default()
                },
                |ui| app.outliner_contents(ui),
            );
        };
        render(&ctx, &mut app);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        let restored_ctx = egui::Context::default();
        restored_ctx.style_mut_of(restored_ctx.theme(), |style| style.animation_time = 0.);
        let mut restored = KetchupApp::with_document(doc, None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        render(&restored_ctx, &mut restored);
        let positions = restored
            .current_inspector_preferences(&restored_ctx)
            .scroll_positions;
        assert_eq!(positions["outliner_geometry_rows/None"], [0., 120.]);
        assert_eq!(positions[&child_key], [0., 240.]);
        assert!(restored.dirty_since.is_none());
    }

    #[test]
    fn lone_sections_fill_the_area_stay_open_and_preserve_the_saved_disclosure() {
        fn render(
            ctx: &egui::Context,
            size: Vec2,
            fill: bool,
            events: Vec<egui::Event>,
        ) -> (bool, Rect, Rect) {
            let mut body_visible = false;
            let mut panel = Rect::NOTHING;
            let mut header = Rect::NOTHING;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 900.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let mut section_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .scope_id(egui::Id::unique("lone_section_test"))
                            .max_rect(Rect::from_min_size(egui::pos2(24., 24.), size)),
                    );
                    let response = inspector_section_sized(
                        &mut section_ui,
                        "Components",
                        "components",
                        true,
                        fill.then_some(size),
                        Some(Rect::from_min_size(Pos2::ZERO, size + Vec2::splat(48.))),
                        |ui| {
                            body_visible = true;
                            ui.label("Contents");
                        },
                    );
                    panel = response.response.rect;
                    header = response.inner.rect;
                },
            );
            (body_visible, panel, header)
        }
        let ctx = egui::Context::default();
        set_section_open(&ctx, "components", false);
        let size = egui::vec2(320., 400.);
        assert!(!render(&ctx, size, false, vec![]).0);
        let (body_visible, panel, header) = render(&ctx, size, true, vec![]);
        assert!(body_visible);
        assert!((panel.size() - size).length() < 0.5, "{panel:?}");
        for pressed in [true, false] {
            assert!(
                render(
                    &ctx,
                    size,
                    true,
                    vec![
                        egui::Event::PointerMoved(header.center()),
                        egui::Event::PointerButton {
                            pos: header.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                )
                .0
            );
        }
        let resized = egui::vec2(260., 600.);
        let (body_visible, panel, _) = render(&ctx, resized, true, vec![]);
        assert!(body_visible);
        assert!((panel.size() - resized).length() < 0.5, "{panel:?}");
        assert!(!render(&ctx, resized, false, vec![]).0);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert!(!render(&restored_ctx, resized, false, vec![]).0);
        assert!(!app.project.modified);
    }

    #[test]
    fn saved_order_survives_recreating_the_app() {
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.inspector_order.rotate_left(2);
        let order = app.inspector_order;
        eframe::App::save(&mut app, &mut storage);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        drop(app);

        let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
        refreshed.inspector_order = restore_order(Some(&storage));
        assert_eq!(refreshed.inspector_order, order);
    }

    #[test]
    fn every_active_tool_persists_immediately_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        // Select also needs to save after another tool, rather than only being
        // restored by the default for an empty preference store.
        for tool in Tool::ALL.into_iter().rev() {
            storage.flushed = false;
            app.set_tool(tool);
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed, "{tool:?}");
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(restored.tool, tool);
            assert_eq!(
                ToolPreferences::capture(&restored),
                ToolPreferences::capture(&app)
            );
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());
            assert!(!restored.project.modified);
            assert!(restored.dirty_since.is_none());
        }
    }

    #[test]
    fn sidebar_tab_survives_immediate_reload_without_changing_group_emphasis() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        for enabled in [false, true] {
            let _ = ctx.run_ui(
                egui::RawInput {
                    events: [true, false]
                        .map(|pressed| egui::Event::Key {
                            key: Key::Tab,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        })
                        .to_vec(),
                    ..Default::default()
                },
                |_| app.keyboard(&ctx),
            );
            assert_eq!(app.inspector, enabled);
            assert!(app.doc.display.group_emphasis);
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(restored.inspector, enabled);
            assert!(restored.doc.display.group_emphasis);
        }
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.visible);
    }

    #[test]
    fn group_emphasis_survives_immediate_reload_and_project_changes() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        for enabled in [false, true] {
            app.doc.display.group_emphasis = enabled;
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(restored.doc.display.group_emphasis, enabled);
            restored.new_model(&ctx);
            assert_eq!(restored.doc.display.group_emphasis, enabled);
            restored.replace_project(Document::default(), None, &ctx);
            assert_eq!(restored.doc.display.group_emphasis, enabled);
        }
        let old: DisplaySettings = serde_json::from_str("{}").unwrap();
        assert!(old.group_emphasis);
    }

    #[test]
    fn each_additional_option_saves_independently_without_a_model_edit() {
        let options: [(&str, fn(&mut KetchupApp)); 14] = [
            ("object snapping", |app| app.doc.snap_to_geometry = false),
            ("axis snapping", |app| app.doc.snap_to_axes = false),
            ("edge snapping", |app| app.snap_to_edge = false),
            ("face snapping", |app| app.snap_to_face = false),
            ("grid snapping", |app| app.snap_to_grid = true),
            ("axis lock", |app| app.lock = Some(Axis::Y)),
            ("shortcut filter", |app| app.shortcut_filter = "Move".into()),
            ("stock rotation", |app| {
                app.project.cutting_plan_layout.allow_rotation = false
            }),
            ("sheet width", |app| {
                app.project.cutting_plan_layout.sheet_width_mm = 600.
            }),
            ("sheet length", |app| {
                app.project.cutting_plan_layout.sheet_length_mm = 1200.
            }),
            ("board length", |app| {
                app.project.cutting_plan_layout.board_length_mm = 3600.
            }),
            ("saw kerf", |app| {
                app.project.cutting_plan_layout.kerf_mm = 4.5
            }),
            ("whole project scope", |app| {
                app.project.cutting_plan_selection_preference = Some(false)
            }),
            ("selected parts scope", |app| {
                app.project.cutting_plan_selection_preference = Some(true)
            }),
        ];
        for (name, change) in options {
            let ctx = egui::Context::default();
            let mut storage = SavedPreferences::default();
            let mut app = KetchupApp::with_document(Document::default(), None, true);
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            storage.flushed = false;
            change(&mut app);
            let expected = app.current_inspector_preferences(&ctx);
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed, "{name}");
            assert!(!app.project.modified, "{name}");
            assert!(app.dirty_since.is_none(), "{name}");
            let restored_ctx = egui::Context::default();
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
            assert_eq!(
                restored.current_inspector_preferences(&restored_ctx),
                expected,
                "{name}"
            );
            assert!(!restored.project.modified, "{name}");
            assert!(
                restored.project.cutting_plan_selection.is_none(),
                "restoring scope must not open a modal"
            );
            storage.flushed = false;
            restored.persist_inspector_changes(&restored_ctx, Some(&mut storage), false);
            assert!(
                !storage.flushed,
                "{name}: unchanged settings must not write every frame"
            );
        }
    }

    #[test]
    fn grid_snapping_defaults_off_for_new_and_older_preferences() {
        for json in ["{}", r#"{"snapping":{"geometry":false,"axes":true}}"#] {
            let preferences: InspectorPreferences = serde_json::from_str(json).unwrap();
            let mut storage = SavedPreferences::default();
            eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
            let mut app = KetchupApp::with_document(Document::default(), None, true);
            assert!(!app.snap_to_grid);
            assert!(app.snap_to_edge);
            assert!(app.snap_to_face);
            app.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert!(!app.snap_to_grid);
            assert!(app.snap_to_edge);
            assert!(app.snap_to_face);
        }
    }

    #[test]
    fn older_and_invalid_export_preferences_keep_safe_defaults() {
        let ctx = egui::Context::default();
        for json in [
            "{}",
            r#"{"cutting_plan":{"layout":{"kerf_mm":5.0}}}"#,
            r#"{"cutting_plan":{"group_stock":false,"layout":{"kerf_mm":4.5}}}"#,
        ] {
            let preferences: InspectorPreferences = serde_json::from_str(json).unwrap();
            assert!(preferences.snapping.is_none());
            assert!(preferences.shortcut_filter.is_empty());
            assert!(preferences.cutting_plan.selection.is_none());
            assert!(preferences.cutting_plan.layout.validate().is_ok());
            let mut storage = SavedPreferences::default();
            eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
            let mut doc = Document::default();
            doc.snap_to_geometry = false;
            doc.snap_to_axes = false;
            let mut restored = KetchupApp::with_document(doc, None, true);
            restored.restore_inspector_preferences(&ctx, Some(&storage));
            assert!(!restored.doc.snap_to_geometry);
            assert!(!restored.doc.snap_to_axes);
            assert_eq!(
                restored.project.cutting_plan_layout,
                preferences.cutting_plan.layout
            );
        }
        let mut preferences = InspectorPreferences::default();
        preferences.cutting_plan.layout.sheet_width_mm = -1.;
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(
            restored.project.cutting_plan_layout,
            crate::cutting_layout::Settings::default()
        );
    }

    #[test]
    fn closing_shortcuts_keeps_the_saved_search_filter() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.help = true;
        app.shortcut_filter = "Move".into();
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |_| app.keyboard(&ctx),
        );
        assert!(!app.help);
        assert_eq!(app.shortcut_filter, "Move");
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.shortcut_filter, "Move");
    }

    #[test]
    fn tool_menu_choices_and_options_restore_without_transient_state() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.set_tool(Tool::Polygon);
        app.set_tool(Tool::Text);
        app.set_tool(Tool::Trim);
        app.set_tool(Tool::Move);
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        for sides in [3, 17, 64] {
            storage.flushed = false;
            app.polygon_sides = sides;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(restored.polygon_sides, sides);
            assert_eq!(restored.drawing_tool, Tool::Polygon);
            assert_eq!(restored.measurement_tool, Tool::Text);
            assert_eq!(restored.solid_tool, Tool::Trim);
        }
        app.move_copy = true;
        app.paint_color = [10, 20, 30];
        app.paint_target = PaintTarget::Group;
        app.line_start = Some(DVec3::X);
        app.solid_first = Some(123);
        app.selected.insert(123);
        app.length_input = "2m".into();
        storage.flushed = false;
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.tool, Tool::Move);
        assert!(restored.move_copy);
        assert_eq!(restored.paint_color, [10, 20, 30]);
        assert_eq!(restored.paint_target, PaintTarget::Group);
        assert!(restored.line_start.is_none());
        assert!(restored.solid_first.is_none());
        assert!(restored.selected.is_empty());
        assert!(restored.length_input.is_empty());
        assert_eq!(
            serde_json::to_value(&restored.doc).unwrap(),
            serde_json::to_value(Document::default()).unwrap()
        );

        // Turning an option back off must save just as promptly as enabling it.
        storage.flushed = false;
        app.move_copy = false;
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert!(!restored.move_copy);
    }

    #[test]
    fn older_tool_preferences_keep_defaults_and_invalid_options_are_normalized() {
        for json in ["{}", r#"{"tools":{}}"#] {
            let preferences: InspectorPreferences = serde_json::from_str(json).unwrap();
            let mut storage = SavedPreferences::default();
            eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
            let mut app = KetchupApp::with_document(Document::default(), None, true);
            app.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(ToolPreferences::capture(&app), ToolPreferences::default());
        }
        for (sides, expected) in [(0, 3), (usize::MAX, 64)] {
            let mut preferences = InspectorPreferences::default();
            preferences.tools = ToolPreferences {
                active: Tool::Polygon,
                drawing: Tool::Paint,
                measurement: Tool::Line,
                solid: Tool::Select,
                polygon_sides: sides,
                ..Default::default()
            };
            let mut storage = SavedPreferences::default();
            eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
            let mut app = KetchupApp::with_document(Document::default(), None, true);
            app.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(app.tool, Tool::Polygon);
            assert_eq!(app.drawing_tool, Tool::Polygon);
            assert_eq!(app.measurement_tool, Tool::Dimensions);
            assert_eq!(app.solid_tool, Tool::Union);
            assert_eq!(app.polygon_sides, expected);
        }
    }

    #[test]
    fn paint_target_persists_immediately_and_older_preferences_default_to_face() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        assert_eq!(app.paint_target, PaintTarget::Face);
        for target in [PaintTarget::Group, PaintTarget::Face] {
            storage.flushed = false;
            app.paint_target = target;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert!(!app.project.modified);
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
            assert_eq!(restored.paint_target, target);
        }
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(old.paint_target, PaintTarget::Face);
    }

    #[test]
    fn screenshot_options_persist_immediately_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let settings = crate::screenshots::Settings {
            views: [true, false, false, true, true, false, false, false],
            combined: false,
            paper: crate::screenshots::Paper::Tabloid,
            landscape: false,
            format: crate::screenshots::Format::Pdf,
            dpi: 300,
        };
        app.screenshot.settings = settings.clone();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(!app.project.modified);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.screenshot.settings, settings);
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(old.screenshot, crate::screenshots::Settings::default());
    }

    #[test]
    fn outliner_search_persists_immediately_and_defaults_to_empty() {
        let ctx = egui::Context::default();
        let mut storage = SavedPreferences::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.outliner_filter = "Handle".into();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);

        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.outliner_filter, "Handle");
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.outliner_filter.is_empty());

        restored.outliner_filter.clear();
        restored.persist_inspector_changes(&ctx, Some(&mut storage), false);
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert!(app.outliner_filter.is_empty());
    }

    #[test]
    fn find_shortcut_reveals_and_focuses_outliner_and_persists_expansion() {
        for width in [1440., 800.] {
            for modifiers in [
                egui::Modifiers::CTRL,
                egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
                egui::Modifiers {
                    mac_cmd: true,
                    command: true,
                    ..Default::default()
                },
            ] {
                let ctx = egui::Context::default();
                ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
                let mut app = KetchupApp::with_document(Document::default(), None, true);
                app.outliner_filter = "Cabinet".into();
                set_section_open(&ctx, "outliner", false);
                let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 900.)));
                let mut other_text = String::new();
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect,
                        ..Default::default()
                    },
                    |ui| {
                        app.inspector_panel(ui);
                        ui.add(egui::TextEdit::singleline(&mut other_text))
                            .request_focus();
                    },
                );
                assert!(ctx.egui_wants_keyboard_input());
                app.inspector = false;
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect,
                        events: vec![
                            egui::Event::ModifiersChanged(modifiers),
                            egui::Event::Key {
                                key: Key::F,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers,
                            },
                        ],
                        ..Default::default()
                    },
                    |ui| {
                        app.keyboard(&ctx);
                        assert!(!ctx.input(|input| input.key_pressed(Key::F)));
                        app.inspector_panel(ui);
                    },
                );
                // Allow newly opened windows and disclosures to finish their layout.
                for _ in 0..2 {
                    let _ = ctx.run_ui(
                        egui::RawInput {
                            screen_rect,
                            ..Default::default()
                        },
                        |ui| {
                            app.inspector_panel(ui);
                        },
                    );
                }
                assert!(app.inspector);
                assert!(!app.focus_outliner_search);
                assert_eq!(
                    ctx.memory(|memory| memory.focused()),
                    Some(egui::Id::unique(OUTLINER_SEARCH_ID))
                );
                assert_eq!(app.outliner_filter, "Cabinet");
                assert_eq!(app.tool, Tool::Select);

                let mut storage = SavedPreferences::default();
                app.persist_inspector_changes(&ctx, Some(&mut storage), false);
                assert!(!app.project.modified);
                let restored_ctx = egui::Context::default();
                let mut restored = KetchupApp::with_document(Document::default(), None, true);
                restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
                assert!(restored.inspector);
                assert!(
                    restored
                        .current_inspector_preferences(&restored_ctx)
                        .sections["outliner"]
                );
            }
        }
    }

    #[test]
    fn unavailable_or_invalid_saved_order_uses_all_default_sections() {
        assert_eq!(restore_order(None), InspectorSection::ALL);
        let mut storage = SavedPreferences::default();
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
        storage.set_string(ORDER_PREFERENCE_KEY, "corrupt preferences".into());
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
        eframe::set_value(
            &mut storage,
            ORDER_PREFERENCE_KEY,
            &[InspectorSection::Tags; 5],
        );
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
        eframe::set_value(
            &mut storage,
            ORDER_PREFERENCE_KEY,
            &InspectorSection::ALL[..4].to_vec(),
        );
        assert_eq!(restore_order(Some(&storage)), InspectorSection::ALL);
    }

    #[test]
    fn general_settings_save_immediately_and_restore_without_model_edits() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.general_settings.section = settings::Section::StockPricing;
        app.general_settings.orbit_speed = 2.;
        app.general_settings.pan_speed = 0.5;
        app.general_settings.zoom_speed = 3.;
        app.general_settings.reverse_zoom = true;
        app.general_settings.default_units.length = LengthUnit::Inches;
        app.general_settings.tool_keys.swap(1, 2);
        app.general_settings.stock.currency = "CAD".into();
        app.general_settings.stock.units = LengthUnit::Inches;
        app.general_settings.stock.wood.push(stock_settings::Stock {
            id: ketchup_core::MaterialId::new_v4(),
            name: "Pine board".into(),
            thickness_mm: 19.,
            width_mm: 89.,
            length_mm: 2438.4,
            price: 8.25,
        });
        set_section_open(&ctx, "settings/stock/wood", false);
        app.doc.display.grid_auto_size = false;
        app.doc.display.grid_spacing = 0.0254;
        app.doc.display.axis_colors = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        app.doc.display.inference_color = [10, 11, 12];
        app.doc.display.midpoint_color = [13, 14, 15];
        let expected = app.current_inspector_preferences(&ctx);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert_eq!(
            restored.current_inspector_preferences(&restored_ctx),
            expected
        );
        restored.new_model(&restored_ctx);
        assert_eq!(restored.doc.display.units.length, LengthUnit::Inches);
        let mut opened = Document::default();
        opened.display.units.length = LengthUnit::Millimeters;
        restored.replace_project(opened, None, &restored_ctx);
        assert_eq!(restored.doc.display.units.length, LengthUnit::Millimeters);
        assert_eq!(
            restored.general_settings.default_units.length,
            LengthUnit::Inches
        );
    }

    #[test]
    fn currency_changes_save_immediately_and_apply_after_switching_projects() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(old.general.stock.currency, currency::DEFAULT);

        for code in ["CAD", "EUR", "JPY"] {
            app.general_settings.stock.currency = code.into();
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());

            let restored_ctx = egui::Context::default();
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
            assert_eq!(restored.general_settings.stock.currency, code);
            restored.replace_project(Document::default(), None, &restored_ctx);
            assert_eq!(restored.general_settings.stock.currency, code);
            assert!(!restored.project.modified);
        }
    }

    #[test]
    fn currency_search_saves_immediately_and_restores_without_model_edits() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.general.stock.currency_filter.is_empty());

        for query in ["Canadian", "JPY", ""] {
            app.general_settings.stock.currency_filter = query.into();
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());

            let restored_ctx = egui::Context::default();
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
            assert_eq!(restored.general_settings.stock.currency_filter, query);
            assert_eq!(restored.general_settings.stock.currency, currency::DEFAULT);
            restored.replace_project(Document::default(), None, &restored_ctx);
            assert_eq!(restored.general_settings.stock.currency_filter, query);
            assert!(!restored.project.modified);
        }
    }

    #[test]
    fn stock_material_search_saves_immediately_without_model_edits() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let model = app.doc.model.clone();
        let mut storage = SavedPreferences::default();
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.stock_material_filter.is_empty());

        for query in ["Oak 19", "project material", ""] {
            app.stock_material_filter = query.into();
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert_eq!(app.doc.model, model);
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());

            let restored_ctx = egui::Context::default();
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
            assert_eq!(restored.stock_material_filter, query);
            restored.replace_project(Document::default(), None, &restored_ctx);
            assert_eq!(restored.stock_material_filter, query);
            assert!(restored.doc.model.stock_materials.is_empty());
            assert!(!restored.project.modified);
        }
    }

    #[test]
    fn legacy_currency_upgrade_is_saved_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut old = InspectorPreferences::default();
        old.general.stock.currency = "$".into();
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, PREFERENCE_KEY, &old);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(app.general_settings.stock.currency, "USD");
        storage.flushed = false;
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.general_settings.stock.currency, "USD");
    }

    #[test]
    fn finishing_products_and_disclosure_save_immediately_without_model_edits() {
        use finish_settings::{ContainerUnit, Finish, FinishType};

        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.general_settings.section = settings::Section::StockPricing;
        app.general_settings.stock.finishes = vec![
            Finish {
                name: "Pre-stain conditioner".into(),
                finish_type: FinishType::PreStain,
                container_size: 946.,
                container_unit: ContainerUnit::Milliliters,
                price: 18.75,
                ..Default::default()
            },
            Finish {
                name: "Clear varnish".into(),
                finish_type: FinishType::Varnish,
                container_size: 1.,
                container_unit: ContainerUnit::UsQuarts,
                price: 25.50,
                ..Default::default()
            },
        ];
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);

        // Editing existing products and closing the section are preferences,
        // so neither operation needs a document edit to be written to storage.
        app.general_settings.stock.finishes[1].finish_type = FinishType::Paint;
        app.general_settings.stock.finishes[1].name = "White paint".into();
        app.general_settings.stock.finishes[1].price = 30.;
        app.general_settings.stock.finishes[1].coverage_m2_per_container = Some(12.5);
        set_section_open(&ctx, "settings/stock/finishes", false);
        storage.flushed = false;
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());

        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert_eq!(restored.general_settings, app.general_settings);
        let _ = restored_ctx.run_ui(Default::default(), |ui| {
            assert_eq!(
                restored_header_open(ui, "settings/stock/finishes", true),
                Some(false)
            );
        });

        // Catalog edits and section expansion also survive a different project.
        restored.replace_project(Document::default(), None, &restored_ctx);
        assert_eq!(
            restored.general_settings.stock.finishes,
            app.general_settings.stock.finishes
        );
        assert!(restored.doc.model.stock_materials.is_empty());
        restored.general_settings.stock.finishes.remove(0);
        set_section_open(&restored_ctx, "settings/stock/finishes", true);
        storage.flushed = false;
        restored.persist_inspector_changes(&restored_ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!restored.project.modified);
        let final_ctx = egui::Context::default();
        app.restore_inspector_preferences(&final_ctx, Some(&storage));
        assert_eq!(
            app.general_settings.stock.finishes,
            restored.general_settings.stock.finishes
        );
        let _ = final_ctx.run_ui(Default::default(), |ui| {
            assert_eq!(
                restored_header_open(ui, "settings/stock/finishes", false),
                Some(true)
            );
        });
    }

    #[test]
    fn material_project_usage_persists_across_restarts() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let stock = stock_settings::Stock::default();
        let id = stock.id;
        app.general_settings.stock.wood.push(stock);
        let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        let object = app
            .doc
            .model
            .make_object(&BTreeSet::from([edge]), false, "Board")
            .unwrap();
        app.doc
            .model
            .objects
            .get_mut(&object)
            .unwrap()
            .stock_material_id = Some(id);
        app.doc.name = "Desk".into();
        app.project.path = Some("desk.ketchup".into());
        app.sync_stock_materials(&ctx);
        let project_id = app.doc.project_id;
        app.project.path = Some("desk-copy.ketchup".into());
        app.sync_stock_materials(&ctx);
        assert_eq!(
            app.general_settings.stock.projects.len(),
            2,
            "Save As retains the original file's last-known usage"
        );
        assert_eq!(app.doc.project_id, project_id);
        app.replace_project(Document::default(), None, &ctx);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(
            restored.general_settings.stock.projects,
            app.general_settings.stock.projects
        );
        assert!(
            restored
                .general_settings
                .stock
                .projects
                .values()
                .all(|usage| usage.materials.get(&id) == Some(&1))
        );
    }

    #[test]
    fn old_catalog_ids_are_saved_immediately_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut old = InspectorPreferences::default();
        old.general.stock.wood.push(stock_settings::Stock {
            id: ketchup_core::MaterialId::nil(),
            ..Default::default()
        });
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, PREFERENCE_KEY, &old);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.restore_inspector_preferences(&ctx, Some(&storage));
        let id = app.general_settings.stock.wood[0].id;
        assert!(!id.is_nil());
        storage.flushed = false;
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert_eq!(restored.general_settings.stock.wood[0].id, id);
    }

    #[test]
    fn removing_stock_persists_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        app.general_settings
            .stock
            .wood
            .push(stock_settings::Stock::default());
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        app.general_settings.stock.wood.clear();
        storage.flushed = false;
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        assert!(restored.general_settings.stock.wood.is_empty());
    }

    #[test]
    fn older_general_preferences_keep_defaults() {
        let preferences: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(preferences.general, settings::GeneralSettings::default());
    }

    #[test]
    fn sidebar_preferences_save_immediately_and_restore_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.inspector = false;
        app.inspector_width = 420.;
        app.component_filter = "Cabinet".into();
        app.paint_color = [12, 34, 56];
        let mut display = serde_json::to_value(DisplaySettings::default()).unwrap();
        // Exercise every toggle, including fields added to DisplaySettings later.
        for value in display.as_object_mut().unwrap().values_mut() {
            if let Some(enabled) = value.as_bool() {
                *value = serde_json::Value::Bool(!enabled);
            }
        }
        app.doc.display = serde_json::from_value(display).unwrap();
        app.doc.display.face_style = FaceStyle::Wireframe;
        app.doc.display.profile_width = 7.;
        app.doc.display.sun_angle = 280.;
        app.doc.display.sky_color = [3, 4, 5];
        app.doc.camera.projection = Projection::Parallel;
        app.doc.camera.fov = 65_f64.to_radians();
        app.doc
            .camera
            .set_standard_view(ketchup_core::StandardView::Bottom);
        for (key, open) in [
            ("display", false),
            ("fog", true),
            ("view", false),
            ("view_camera", true),
            ("view_standard_views", true),
            ("outliner_object/42", true),
        ] {
            let id = egui::Id::unique(key);
            let mut state =
                egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, id, false);
            state.set_open(open);
            state.store(&ctx);
            remember_section(&ctx, id, key);
        }
        let expected = app.current_inspector_preferences(&ctx);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());

        let refreshed_ctx = egui::Context::default();
        let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
        refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
        assert_eq!(
            refreshed.current_inspector_preferences(&refreshed_ctx),
            expected
        );
        assert!(!refreshed.project.modified);

        storage.flushed = false;
        refreshed.persist_inspector_changes(&refreshed_ctx, Some(&mut storage), false);
        assert!(
            !storage.flushed,
            "unchanged preferences must not write every frame"
        );
    }

    #[test]
    fn viewed_release_notes_save_immediately_and_restore_for_other_projects() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        // Establish saved preferences first, so only the read state changes.
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        storage.flushed = false;
        assert!(app.release_notes.has_unread());
        app.release_notes.open_if_unread();
        assert!(app.release_notes.open);
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 700.))),
                ..Default::default()
            },
            |_| app.release_notes.show(&ctx),
        );
        assert!(!app.release_notes.has_unread());
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());

        let mut other_doc = Document::default();
        other_doc.name = "Another project".into();
        let mut refreshed = KetchupApp::with_document(other_doc, None, true);
        let refreshed_ctx = egui::Context::default();
        refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
        refreshed.release_notes.open_if_unread();
        assert!(!refreshed.release_notes.has_unread());
        assert!(!refreshed.release_notes.open);
        assert_eq!(refreshed.doc.name, "Another project");
        storage.flushed = false;
        refreshed.persist_inspector_changes(&refreshed_ctx, Some(&mut storage), false);
        assert!(!storage.flushed);

        // An older saved read version must allow the next release to open.
        let mut older = refreshed.current_inspector_preferences(&refreshed_ctx);
        older.last_seen_release_notes = Some("0.0.0".into());
        eframe::set_value(&mut storage, PREFERENCE_KEY, &older);
        let mut updated = KetchupApp::with_document(Document::default(), None, true);
        updated.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
        updated.release_notes.open_if_unread();
        assert!(updated.release_notes.open);
        assert!(updated.release_notes.has_unread());
    }

    #[test]
    fn bulk_expansion_overrides_existing_headers_and_restores_after_immediate_save() {
        fn render(ctx: &egui::Context, moved: bool) -> (bool, bool) {
            let mut visible = (false, false);
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.push_id(if moved { "floating" } else { "docked" }, |ui| {
                    let id = ui.make_persistent_id(("outliner_object", 42_u64));
                    restore_section(ctx, id, "outliner_object/42", false);
                    let state = egui::collapsing_header::CollapsingState::load(ctx, id).unwrap();
                    visible.0 = state.is_open();
                    remember_section(ctx, id, "outliner_object/42");
                    let key = "outliner_geometry/Some(42)";
                    let open = restored_header_open(ui, key, false);
                    let header = egui::CollapsingHeader::new("Geometry (1)")
                        .id_salt(("outliner_geometry", Some(42_u64)))
                        .open(open)
                        .show(ui, |ui| {
                            visible.1 = true;
                            ui.label("Edge");
                        });
                    remember_section(ctx, header.header_response.id, key);
                });
            });
            visible
        }

        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        assert_eq!(render(&ctx, false), (false, false));
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        for open in [true, false] {
            for key in ["outliner_object/42", "outliner_geometry/Some(42)"] {
                set_section_open(&ctx, key, open);
            }
            // Save before another render, just as an immediate reload would.
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert_eq!(render(&ctx, false), (open, open));
            assert_eq!(render(&ctx, false), (open, open));

            let refreshed_ctx = egui::Context::default();
            refreshed_ctx.style_mut_of(refreshed_ctx.theme(), |style| style.animation_time = 0.);
            let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
            refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
            assert_eq!(render(&refreshed_ctx, true), (open, open));
            assert!(!refreshed.project.modified);
        }
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
    }

    #[test]
    fn missing_corrupt_or_invalid_preferences_preserve_document_display() {
        let ctx = egui::Context::default();
        let mut doc = Document::default();
        doc.display.axes = false;
        let mut app = KetchupApp::with_document(doc.clone(), None, true);
        app.restore_inspector_preferences(&ctx, None);
        assert_eq!(app.doc.display, doc.display);
        let mut storage = SavedPreferences::default();
        storage.set_string(PREFERENCE_KEY, "corrupt preferences".into());
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(app.doc.display, doc.display);
        assert!(app.inspector);
        assert_eq!(app.inspector_width, 300.);

        let mut preferences = InspectorPreferences::default();
        let mut display = DisplaySettings::default();
        display.fog_end = display.fog_start;
        preferences.display = Some(display);
        preferences.width = -20.;
        eframe::set_value(&mut storage, PREFERENCE_KEY, &preferences);
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(app.doc.display, doc.display);
        assert_eq!(app.inspector_width, 260.);

        let older: InspectorPreferences =
            serde_json::from_str(r#"{"display":{"axes":false}}"#).unwrap();
        assert!(older.visible);
        assert!(older.sections.is_empty());
        assert!(older.camera.is_none());
        assert!(older.last_seen_release_notes.is_none());
        let display = older.display.unwrap();
        assert!(!display.axes);
        assert_eq!(display.sky, DisplaySettings::default().sky);
        assert!(display.valid());
    }

    #[test]
    fn time_estimate_controls_save_immediately_and_restore_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let model = app.doc.model.clone();
        app.general_settings.section = settings::Section::SurfaceTreatment;
        app.general_settings.stock.finishes =
            (0..3).map(|_| finish_settings::Finish::default()).collect();
        for (i, step) in app.time_estimates.sanding.iter_mut().enumerate() {
            step.enabled = i % 2 == 0;
            step.minutes_per_m2 = 7. + i as f64;
        }
        app.time_estimates.sanding_after_varnish = [false, true, false, true, false, true];
        for (step, product) in [
            &mut app.time_estimates.pre_stain,
            &mut app.time_estimates.stain,
            &mut app.time_estimates.varnish,
        ]
        .into_iter()
        .zip(&app.general_settings.stock.finishes)
        {
            step.enabled = true;
            step.product_id = Some(product.id);
            step.minutes_per_m2 = 12.5;
            step.coats = 2;
            step.drying_hours = 3.25;
            step.final_drying_hours = Some(12.5);
        }
        set_section_open(&ctx, "time_cost_estimates", false);
        let subsections = [
            ("settings/treatment/sanding", false),
            ("entity/treatment/sanding", true),
            ("settings/treatment/finishing", true),
            ("entity/treatment/finishing", false),
            ("time_estimates_pre_stain", false),
            ("time_estimates_stain", true),
            ("time_estimates_varnish", false),
            ("time_estimates_material_cost", true),
        ];
        for (key, open) in subsections {
            set_section_open(&ctx, key, open);
        }
        app.inspector_order.rotate_right(1);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert_eq!(app.doc.model, model);
        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert_eq!(restored.time_estimates, app.time_estimates);
        assert_eq!(
            restored.general_settings.stock.finishes,
            app.general_settings.stock.finishes
        );
        assert_eq!(
            restored.general_settings.section,
            settings::Section::SurfaceTreatment
        );
        assert_eq!(restored.inspector_order, app.inspector_order);
        assert!(restored.selected.is_empty());
        assert!(!restored.project.modified);
        let _ = restored_ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.push_id("floating_estimator", |ui| {
                assert_eq!(
                    restored_header_open(ui, "time_cost_estimates", true),
                    Some(false)
                );
                for (key, open) in subsections {
                    assert_eq!(restored_header_open(ui, key, false), Some(open), "{key}");
                }
            });
        });
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(old.time_estimates, time_estimates::Settings::default());
    }

    #[test]
    fn finishing_product_searches_save_immediately_without_model_edits() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let model = app.doc.model.clone();
        let old: InspectorPreferences = serde_json::from_str("{}").unwrap();
        assert!(old.finish_product_filters.is_empty());
        for key in [
            "settings/treatment/pre_stain",
            "settings/treatment/stain",
            "settings/treatment/varnish",
            "entity/treatment/pre_stain",
            "entity/treatment/stain",
            "entity/treatment/varnish",
        ] {
            app.finish_product_filters
                .insert(key.into(), format!("Search for {key}"));
        }
        let mut storage = SavedPreferences::default();
        for query in ["oak stain", ""] {
            app.finish_product_filters
                .insert("entity/treatment/stain".into(), query.into());
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert_eq!(app.doc.model, model);
            assert!(!app.project.modified);
            assert!(app.dirty_since.is_none());
            let restored_ctx = egui::Context::default();
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
            assert_eq!(restored.finish_product_filters, app.finish_product_filters);
            restored.replace_project(Document::default(), None, &restored_ctx);
            assert_eq!(restored.finish_product_filters, app.finish_product_filters);
            assert!(!restored.project.modified);
        }
    }

    #[test]
    fn expanded_estimate_panels_render_all_finish_grids_without_id_clashes() {
        fn collect_text(shape: &egui::Shape, labels: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => labels.push(text.galley.job.text.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, labels);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        ctx.options_mut(|options| options.warn_on_id_clash = true);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.time_estimates.pre_stain.enabled = true;
        app.time_estimates.stain.enabled = true;
        app.time_estimates.varnish.enabled = true;
        for key in [
            "settings/treatment/sanding",
            "settings/treatment/finishing",
            "settings/treatment/pre_stain",
            "settings/treatment/stain",
            "settings/treatment/varnish",
        ] {
            set_section_open(&ctx, key, true);
        }
        for frame in 0..3 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 2000.))),
                    time: Some(frame as f64 / 60.),
                    ..Default::default()
                },
                |ui| {
                    ui.set_max_width(320.);
                    time_estimates::treatment_controls(
                        ui,
                        &mut app.time_estimates,
                        true,
                        &app.general_settings.stock.finishes,
                        &mut app.finish_product_filters,
                    );
                },
            );
            let mut labels = Vec::new();
            for clipped in &output.shapes {
                collect_text(&clipped.shape, &mut labels);
            }
            assert_eq!(
                labels
                    .iter()
                    .filter(|text| text.as_str() == "Minutes / m²")
                    .count(),
                3
            );
            assert!(
                !labels.iter().any(|text| text.contains("Grid ID")),
                "{labels:?}"
            );
        }
    }

    #[test]
    fn estimator_appends_to_previously_complete_orders() {
        let previous = vec![
            InspectorSection::View,
            InspectorSection::Units,
            InspectorSection::Display,
            InspectorSection::Tags,
            InspectorSection::Components,
            InspectorSection::EntityInfo,
            InspectorSection::Outliner,
        ];
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, ORDER_PREFERENCE_KEY, &previous);
        let restored = restore_order(Some(&storage));
        assert_eq!(&restored[..previous.len()], previous.as_slice());
        assert_eq!(
            restored[restored.len() - 2],
            InspectorSection::TimeCostEstimates
        );
        assert_eq!(restored.last(), Some(&InspectorSection::Materials));
    }

    #[test]
    fn original_fixed_array_order_migrates_without_losing_layout() {
        let original = [
            InspectorSection::Tags,
            InspectorSection::Display,
            InspectorSection::Outliner,
            InspectorSection::EntityInfo,
            InspectorSection::Components,
        ];
        let expected = [
            InspectorSection::Tags,
            InspectorSection::Display,
            InspectorSection::Outliner,
            InspectorSection::EntityInfo,
            InspectorSection::Components,
            InspectorSection::View,
            InspectorSection::Units,
            InspectorSection::TimeCostEstimates,
            InspectorSection::Materials,
        ];
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, ORDER_PREFERENCE_KEY, &original);
        assert_eq!(restore_order(Some(&storage)), expected);
        save_order(&expected, &mut storage);
        assert_eq!(restore_order(Some(&storage)), expected);
    }

    #[test]
    fn units_default_below_entity_info_and_append_to_existing_layouts() {
        let defaults = restore_order(None);
        let entity = defaults
            .iter()
            .position(|s| *s == InspectorSection::EntityInfo)
            .unwrap();
        assert_eq!(defaults[entity + 1], InspectorSection::Units);

        let previous = vec![
            InspectorSection::View,
            InspectorSection::Tags,
            InspectorSection::Display,
            InspectorSection::Outliner,
            InspectorSection::EntityInfo,
            InspectorSection::Components,
        ];
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, ORDER_PREFERENCE_KEY, &previous);
        let restored = restore_order(Some(&storage));
        assert_eq!(&restored[..previous.len()], previous.as_slice());
        assert_eq!(restored[previous.len()], InspectorSection::Units);
        assert_eq!(
            restored[restored.len() - 2],
            InspectorSection::TimeCostEstimates
        );
        assert_eq!(restored.last(), Some(&InspectorSection::Materials));
    }

    #[test]
    fn unit_choices_and_disclosure_save_immediately_without_a_model_edit() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let model = app.doc.model.clone();
        let units = ketchup_core::UnitSettings {
            length: LengthUnit::Inches,
            area: AreaUnit::SquareFeet,
            volume: VolumeUnit::Liters,
            angle: AngleUnit::Radians,
            ..Default::default()
        };
        app.doc.display.units = units;
        set_section_open(&ctx, "units", false);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        assert_eq!(app.doc.model, model);

        let restored_ctx = egui::Context::default();
        let mut restored = KetchupApp::with_document(Document::default(), None, true);
        restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
        assert_eq!(restored.doc.display.units, units);
        let id = egui::Id::unique("restored_units");
        restore_section(&restored_ctx, id, "units", true);
        assert!(
            !egui::collapsing_header::CollapsingState::load(&restored_ctx, id)
                .unwrap()
                .is_open()
        );
        set_section_open(&restored_ctx, "units", true);
        restored.persist_inspector_changes(&restored_ctx, Some(&mut storage), false);
        let reloaded_ctx = egui::Context::default();
        restored.restore_inspector_preferences(&reloaded_ctx, Some(&storage));
        restore_section(&reloaded_ctx, id, "units", false);
        assert!(
            egui::collapsing_header::CollapsingState::load(&reloaded_ctx, id)
                .unwrap()
                .is_open()
        );

        let older: InspectorPreferences =
            serde_json::from_str(r#"{"display":{"axes":false}}"#).unwrap();
        assert_eq!(
            older.display.unwrap().units,
            ketchup_core::UnitSettings::default()
        );
    }

    #[test]
    fn precision_preferences_save_without_a_model_edit_and_restore_each_control() {
        use ketchup_core::UnitPrecision;

        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let model = app.doc.model.clone();
        let mut storage = SavedPreferences::default();
        for precision in [
            UnitPrecision {
                length: Some(6),
                ..Default::default()
            },
            UnitPrecision {
                area: Some(1),
                ..Default::default()
            },
            UnitPrecision {
                volume: Some(0),
                ..Default::default()
            },
            UnitPrecision {
                angle: Some(5),
                ..Default::default()
            },
        ] {
            app.doc.display.units.precision = precision;
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert!(!app.project.modified);
            assert_eq!(app.doc.model, model);

            let restored_ctx = egui::Context::default();
            let mut restored = KetchupApp::with_document(Document::default(), None, true);
            restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
            assert_eq!(restored.doc.display.units.precision, precision);
            assert!(!restored.project.modified);
        }
    }

    #[test]
    fn older_section_orders_keep_their_layout_and_append_new_sections() {
        let mut storage = SavedPreferences::default();
        eframe::set_value(
            &mut storage,
            ORDER_PREFERENCE_KEY,
            &vec![InspectorSection::Tags, InspectorSection::Display],
        );
        assert_eq!(
            restore_order(Some(&storage)),
            [
                InspectorSection::Tags,
                InspectorSection::Display,
                InspectorSection::Outliner,
                InspectorSection::EntityInfo,
                InspectorSection::Components,
                InspectorSection::View,
                InspectorSection::Units,
                InspectorSection::TimeCostEstimates,
                InspectorSection::Materials,
            ]
        );
    }

    #[test]
    fn materials_append_to_existing_orders_and_save_without_model_edits() {
        let previous = vec![
            InspectorSection::View,
            InspectorSection::Tags,
            InspectorSection::Units,
            InspectorSection::Display,
            InspectorSection::Outliner,
            InspectorSection::EntityInfo,
            InspectorSection::TimeCostEstimates,
            InspectorSection::Components,
        ];
        let mut storage = SavedPreferences::default();
        eframe::set_value(&mut storage, ORDER_PREFERENCE_KEY, &previous);
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.restore_inspector_preferences(&ctx, Some(&storage));
        assert_eq!(&app.inspector_order[..previous.len()], previous.as_slice());
        assert_eq!(
            app.inspector_order.last(),
            Some(&InspectorSection::Materials)
        );

        // Older preferences have no disclosure entry, so Materials starts open.
        let id = egui::Id::unique("materials_default");
        restore_section(&ctx, id, "materials", true);
        assert!(
            egui::collapsing_header::CollapsingState::load(&ctx, id)
                .unwrap()
                .is_open()
        );
        let model = app.doc.model.clone();
        for open in [false, true] {
            set_section_open(&ctx, "materials", open);
            storage.flushed = false;
            app.persist_inspector_changes(&ctx, Some(&mut storage), false);
            assert!(storage.flushed);
            assert!(!app.project.modified);
            assert_eq!(app.doc.model, model);

            let reloaded_ctx = egui::Context::default();
            let mut reloaded = KetchupApp::with_document(Document::default(), None, true);
            reloaded.restore_inspector_preferences(&reloaded_ctx, Some(&storage));
            assert_eq!(reloaded.inspector_order, app.inspector_order);
            let _ = reloaded_ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.push_id("floating_materials", |ui| {
                    assert_eq!(restored_header_open(ui, "materials", true), Some(open));
                });
            });
            assert_eq!(reloaded.doc.model, model);
            assert!(!reloaded.project.modified);
        }
    }

    #[test]
    fn new_sections_restore_after_click_save_reload_and_layout_change() {
        fn render(ctx: &egui::Context, events: Vec<egui::Event>, moved: bool) -> (Rect, bool) {
            let mut rect = Rect::NOTHING;
            let mut body_visible = false;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 900.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.push_id(if moved { "floating" } else { "docked" }, |ui| {
                        rect =
                            inspector_section(ui, "Future option", "future_option", false, |ui| {
                                body_visible = true;
                                ui.label("Future controls");
                            })
                            .inner
                            .rect;
                    });
                },
            );
            (rect, body_visible)
        }
        let ctx = egui::Context::default();
        ctx.style_mut_of(ctx.theme(), |style| style.animation_time = 0.);
        assert!(!render(&ctx, vec![], false).1);
        let (rect, _) = render(&ctx, vec![], false);
        for pressed in [true, false] {
            render(
                &ctx,
                vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::PointerButton {
                        pos: rect.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                false,
            );
        }
        assert!(render(&ctx, vec![], false).1);
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        let new_ctx = egui::Context::default();
        new_ctx.style_mut_of(new_ctx.theme(), |style| style.animation_time = 0.);
        let mut refreshed = KetchupApp::with_document(Document::default(), None, true);
        refreshed.restore_inspector_preferences(&new_ctx, Some(&storage));
        assert!(render(&new_ctx, vec![], true).1);
    }

    #[test]
    fn view_preferences_restore_groups_without_copying_another_models_framing() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.camera.projection = Projection::Parallel;
        app.doc.camera.fov = 70_f64.to_radians();
        app.doc
            .camera
            .set_standard_view(ketchup_core::StandardView::Top);
        app.doc.camera.target = DVec3::splat(500.);
        app.doc.camera.distance = 300.;
        for (key, open) in [
            ("view", false),
            ("view_camera", true),
            ("view_standard_views", true),
        ] {
            set_section_open(&ctx, key, open);
        }
        let mut storage = SavedPreferences::default();
        app.persist_inspector_changes(&ctx, Some(&mut storage), false);
        assert!(storage.flushed);
        assert!(!app.project.modified);
        let mut doc = Document::default();
        doc.camera.target = DVec3::new(1., 2., 3.);
        doc.camera.distance = 42.;
        let refreshed_ctx = egui::Context::default();
        let mut refreshed = KetchupApp::with_document(doc.clone(), None, true);
        refreshed.restore_inspector_preferences(&refreshed_ctx, Some(&storage));
        assert_eq!(refreshed.doc.camera.target, doc.camera.target);
        assert_eq!(refreshed.doc.camera.distance, doc.camera.distance);
        assert_eq!(
            CameraPreferences::capture(&refreshed.doc.camera),
            CameraPreferences::capture(&app.doc.camera)
        );
        let _ = refreshed_ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.push_id("floating_view", |ui| {
                assert_eq!(restored_header_open(ui, "view", true), Some(false));
                assert_eq!(restored_header_open(ui, "view_camera", false), Some(true));
                assert_eq!(
                    restored_header_open(ui, "view_standard_views", false),
                    Some(true)
                );
            });
        });
        let mut preferences = refreshed.current_inspector_preferences(&refreshed_ctx);
        preferences.camera.as_mut().unwrap().fov = f64::NAN;
        // Exercise validation directly: JSON/RON need not preserve non-finite values.
        let before = refreshed.doc.camera.clone();
        preferences
            .camera
            .unwrap()
            .restore(&mut refreshed.doc.camera);
        assert_eq!(refreshed.doc.camera, before);
    }

    #[test]
    fn new_projects_keep_preferences_and_opened_projects_use_their_own_display() {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.display.axes = false;
        app.doc.display.fog = true;
        app.component_filter = "Cabinet".into();
        app.doc.camera.projection = Projection::Parallel;
        app.doc.camera.fov = 60_f64.to_radians();
        app.doc
            .camera
            .set_standard_view(ketchup_core::StandardView::Top);
        app.doc.camera.target = DVec3::splat(100.);
        app.doc.camera.distance = 200.;
        let expected_camera = Camera {
            projection: app.doc.camera.projection,
            fov: app.doc.camera.fov,
            ..Camera::default()
        };
        app.new_model(&ctx);
        assert!(!app.doc.display.axes);
        assert!(app.doc.display.fog);
        assert_eq!(app.component_filter, "Cabinet");
        assert_eq!(app.doc.camera, expected_camera);
        assert!(!app.project.modified);
        app.replace_project(Document::default(), Some("opened.ketchup".into()), &ctx);
        assert_eq!(app.doc.display, DisplaySettings::default());
        assert_eq!(app.doc.camera, Camera::default());
        assert_eq!(app.component_filter, "Cabinet");
    }
}

impl InspectorSection {
    pub(super) const COUNT: usize = 9;

    pub(super) const ALL: [Self; Self::COUNT] = [
        Self::Outliner,
        Self::EntityInfo,
        Self::Units,
        Self::Components,
        Self::Tags,
        Self::Display,
        Self::View,
        Self::TimeCostEstimates,
        Self::Materials,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Outliner => "Outliner",
            Self::EntityInfo => "Entity Info",
            Self::Components => "Components",
            Self::Tags => "Tags",
            Self::Display => "Display",
            Self::View => "View",
            Self::Units => "Units",
            Self::TimeCostEstimates => "Time & Cost Estimates",
            Self::Materials => "Materials",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Outliner => "outliner",
            Self::EntityInfo => "entity_info",
            Self::Components => "components",
            Self::Tags => "tags",
            Self::Display => "display",
            Self::View => "view",
            Self::Units => "units",
            Self::TimeCostEstimates => "time_cost_estimates",
            Self::Materials => "materials",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

fn face_style_preview(
    painter: &egui::Painter,
    rect: Rect,
    style: FaceStyle,
    colors: Palette,
    faces: FacePalette,
) {
    let center = rect.center();
    let scale = rect.width().min(rect.height()) * 0.46;
    let point = |x, y| center + egui::vec2(x * scale, y * scale);
    let top = point(0., -1.);
    let left = point(-0.9, -0.55);
    let front = point(0., -0.1);
    let right = point(0.9, -0.55);
    let bottom_left = point(-0.9, 0.55);
    let bottom = point(0., 1.);
    let bottom_right = point(0.9, 0.55);
    let outline = Stroke::new(1.2, colors.ink);
    if style == FaceStyle::Wireframe {
        let back = point(0., 0.1);
        for end in [top, bottom_left, bottom_right] {
            painter.line_segment([back, end], Stroke::new(1., colors.muted));
        }
    } else {
        let fills = match style {
            FaceStyle::HiddenLine => [colors.paper; 3],
            FaceStyle::Monochrome => [faces.front, faces.side, faces.back],
            FaceStyle::Materials => faces.materials,
            FaceStyle::Shaded => [faces.front, faces.side, faces.shaded_side],
            FaceStyle::Wireframe => unreachable!(),
        };
        for (points, fill) in [
            ([top, right, front, left], fills[0]),
            ([left, front, bottom, bottom_left], fills[1]),
            ([front, right, bottom_right, bottom], fills[2]),
        ] {
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                fill,
                Stroke::NONE,
            ));
        }
    }
    painter.add(egui::Shape::closed_line(
        vec![top, right, bottom_right, bottom, bottom_left, left],
        outline,
    ));
    for end in [left, right, bottom] {
        painter.line_segment([front, end], outline);
    }
}

fn face_style_picker(ui: &mut egui::Ui, current: &mut FaceStyle) {
    let colors = Palette::for_ui(ui);
    let faces = FacePalette::for_ui(ui);
    let spacing = ui.spacing().item_spacing.x;
    let columns = ((ui.available_width() + spacing) / (96. + spacing))
        .floor()
        .clamp(1., FaceStyle::ALL.len() as f32) as usize;
    let width = (ui.available_width() - spacing * (columns - 1) as f32) / columns as f32;
    ui.push_id("face_style", |ui| {
        for row in FaceStyle::ALL.chunks(columns) {
            ui.horizontal(|ui| {
                for &style in row {
                    let selected = *current == style;
                    let mut response = ui
                        .add_sized(
                            egui::vec2(width, 100.),
                            egui::Button::new("").selected(selected).corner_radius(5),
                        )
                        .on_hover_text(style.label());
                    if response.clicked() && !selected {
                        *current = style;
                        response.mark_changed();
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::RadioButton,
                            ui.is_enabled(),
                            *current == style,
                            style.label(),
                        )
                    });
                    if ui.is_rect_visible(response.rect) {
                        let rect = response.rect;
                        face_style_preview(
                            ui.painter(),
                            Rect::from_center_size(
                                egui::pos2(rect.center().x, rect.top() + 33.),
                                egui::vec2(width - 16., 50.),
                            ),
                            style,
                            colors,
                            faces,
                        );
                        let mut label = egui::text::LayoutJob::simple(
                            style.label().to_owned(),
                            FontId::proportional(11.),
                            colors.ink,
                            width - 12.,
                        );
                        label.halign = egui::Align::Center;
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(label));
                        let pos = egui::pos2(
                            rect.center().x,
                            rect.bottom() - 21. - galley.size().y * 0.5,
                        );
                        ui.painter().galley(pos, galley, colors.ink);
                    }
                }
            });
        }
    });
}

fn inspector_icon_color(ui: &egui::Ui, id: &str) -> Color32 {
    // Keep each section's hue consistent, with lighter strokes on dark surfaces.
    let (light, dark) = match icons::section_icon(id) {
        "outliner" => ([48, 111, 173], [131, 185, 239]),
        "entity_info" => ([17, 129, 133], [103, 207, 211]),
        "units" => ([67, 131, 68], [143, 205, 144]),
        "time_cost_estimates" => ([183, 92, 19], [242, 170, 100]),
        "materials" => ([160, 111, 22], [230, 190, 104]),
        "time_estimates_sanding" => ([160, 111, 22], [230, 190, 104]),
        "time_estimates_finishing" | "settings/stock/finishes" => ([126, 83, 183], [188, 153, 239]),
        "time_estimates_pre_stain" => ([17, 129, 133], [103, 207, 211]),
        "time_estimates_stain" => ([183, 92, 19], [242, 170, 100]),
        "time_estimates_varnish" => ([67, 131, 68], [143, 205, 144]),
        "components" => ([126, 83, 183], [188, 153, 239]),
        "tags" => ([160, 111, 22], [230, 190, 104]),
        "display" => ([198, 76, 58], [246, 142, 125]),
        "view" | "view_camera" => ([48, 111, 173], [131, 185, 239]),
        "view_standard_views" => ([17, 129, 133], [103, 207, 211]),
        "edge_style" => ([88, 109, 135], [159, 181, 209]),
        "sections" => ([176, 67, 113], [237, 142, 181]),
        "shadows" => ([183, 92, 19], [242, 170, 100]),
        "fog" => ([117, 105, 155], [181, 169, 219]),
        "display_environment" => ([67, 131, 68], [143, 205, 144]),
        _ => return Palette::for_ui(ui).green,
    };
    let [r, g, b] = if ui.visuals().dark_mode { dark } else { light };
    Color32::from_rgb(r, g, b)
}

// Keep egui's disclosure behavior, animation, and accessibility while giving
// inspector groups a shared surface and a full-width click target.
pub(super) fn inspector_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    id: &'static str,
    primary: bool,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<egui::Response> {
    inspector_section_sized(ui, title, id, primary, None, None, contents)
}

// A lone docked section fills its area and has a fixed-open, draggable header.
// Its usual disclosure state is left untouched for when it rejoins a stack.
fn inspector_section_sized<R>(
    ui: &mut egui::Ui,
    title: &str,
    id: &'static str,
    primary: bool,
    fill_size: Option<Vec2>,
    sticky_viewport: Option<Rect>,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<egui::Response> {
    let colors = Palette::for_ui(ui);
    let icon_color = inspector_icon_color(ui, id);
    let body_style = ui.style().as_ref().clone();
    let frame = egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1., colors.border))
        .corner_radius(if primary { 8 } else { 5 })
        .inner_margin(1)
        .shadow(if primary {
            egui::epaint::Shadow {
                offset: [0, 2],
                blur: 5,
                spread: 0,
                color: Color32::from_black_alpha(12),
            }
        } else {
            egui::epaint::Shadow::NONE
        });
    let content_size = fill_size.map(|size| (size - frame.total_margin().sum()).max(Vec2::ZERO));
    let response = frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        if let Some(size) = content_size {
            ui.set_min_size(size);
        }
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(10., if primary { 10. } else { 7. });
            // Reclaim the disclosure slot for lone sections, keeping a little left padding.
            ui.spacing_mut().indent = if fill_size.is_some() { 34. } else { 52. };
            let visuals = ui.visuals_mut();
            visuals.collapsing_header_frame = true;
            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
            ] {
                widget.corner_radius = egui::CornerRadius::same(if primary { 7 } else { 4 });
                widget.bg_stroke = Stroke::NONE;
                widget.expansion = 0.;
            }
            visuals.widgets.inactive.weak_bg_fill =
                if primary { colors.canvas } else { colors.paper };
            visuals.widgets.hovered.weak_bg_fill = colors.hovered;
            visuals.widgets.active.weak_bg_fill = colors.selected;
            if let Some(viewport) = sticky_viewport {
                let header_style = ui.style().clone();
                let height = (ui.text_style_height(&egui::TextStyle::Button)
                    + ui.spacing().button_padding.y * 2.)
                    .max(ui.spacing().interact_size.y);
                let (_, natural_rect) = ui.allocate_space(egui::vec2(ui.available_width(), height));
                let header_id = ui.make_persistent_id(id);
                let mut state = fill_size.is_none().then(|| {
                    restore_section(ui.ctx(), header_id, id, primary);
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        header_id,
                        primary,
                    )
                });
                let openness = state.as_ref().map_or(1., |state| state.openness(ui.ctx()));
                *ui.style_mut() = body_style;
                let body = |ui: &mut egui::Ui| {
                    egui::Frame::new().inner_margin(10).show(ui, |ui| {
                        if fill_size.is_some() {
                            ui.set_min_size(ui.available_size());
                        }
                        contents(ui)
                    });
                };
                if let Some(state) = &mut state {
                    state.show_body_unindented(ui, body);
                } else {
                    body(ui);
                }
                // Constrain the pinned header to its own section, so the next
                // section pushes it out as the end of the body scrolls past.
                let top = natural_rect
                    .top()
                    .max(viewport.top())
                    .min(ui.min_rect().bottom() - height);
                let rect = natural_rect.translate(egui::vec2(0., top - natural_rect.top()));
                let layer = egui::LayerId::new(ui.layer_id().order, header_id.with("sticky"));
                ui.ctx().set_sublayer(ui.layer_id(), layer);
                let mut header_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("sticky_header")
                        .layer_id(layer)
                        .max_rect(rect),
                );
                header_ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
                header_ui.set_style(header_style);
                let mut header = header_ui.interact(rect, header_id, Sense::click_and_drag());
                if let Some(state) = &mut state {
                    if header.clicked() {
                        state.toggle(&header_ui);
                        header.mark_changed();
                    }
                    state.store(ui.ctx());
                    remember_section(ui.ctx(), header_id, id);
                    ui.ctx().accesskit_node_builder(header_id, |node| {
                        node.set_expanded(state.is_open());
                    });
                }
                header.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        if state.is_some() {
                            egui::Role::DisclosureTriangle
                        } else {
                            egui::Role::Group
                        },
                        ui.is_enabled(),
                        title,
                    )
                });
                let visuals = header_ui.style().interact(&header);
                header_ui
                    .painter()
                    .rect_filled(rect, 7, visuals.weak_bg_fill);
                let galley = egui::WidgetText::from(RichText::new(title).strong()).into_galley(
                    &header_ui,
                    Some(egui::TextWrapMode::Extend),
                    (rect.width() - header_ui.spacing().indent).max(0.),
                    egui::TextStyle::Button,
                );
                header_ui.painter().galley(
                    egui::pos2(
                        rect.left() + header_ui.spacing().indent,
                        rect.center().y - galley.size().y * 0.5,
                    ),
                    galley,
                    visuals.text_color(),
                );
                let center = egui::pos2(
                    rect.left() + header_ui.spacing().indent / 2. - 12.,
                    rect.center().y,
                );
                icons::line_icon(
                    header_ui.painter(),
                    egui::pos2(
                        rect.left() + header_ui.spacing().indent - 14.,
                        rect.center().y,
                    ),
                    id,
                    icon_color,
                );
                if state.is_some() {
                    let rotation =
                        egui::emath::Rot2::from_angle(openness * std::f32::consts::FRAC_PI_2);
                    let points = [
                        egui::vec2(-2., -4.),
                        egui::vec2(2., 0.),
                        egui::vec2(-2., 4.),
                    ]
                    .map(|point| center + rotation * point);
                    header_ui.painter().add(egui::Shape::line(
                        points.to_vec(),
                        Stroke::new(
                            1.6,
                            if openness > 0. {
                                colors.green
                            } else {
                                colors.muted
                            },
                        ),
                    ));
                }
                return header;
            }
            if fill_size.is_some() {
                let height = ui.text_style_height(&egui::TextStyle::Button)
                    + ui.spacing().button_padding.y * 2.;
                let (rect, header) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), height),
                    Sense::click_and_drag(),
                );
                header.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Group, ui.is_enabled(), title)
                });
                let visuals = ui.style().interact(&header);
                ui.painter().rect_filled(rect, 7, visuals.weak_bg_fill);
                let galley = egui::WidgetText::from(RichText::new(title).strong()).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    (rect.width() - ui.spacing().indent).max(0.),
                    egui::TextStyle::Button,
                );
                ui.painter().galley(
                    egui::pos2(
                        rect.left() + ui.spacing().indent,
                        rect.center().y - galley.size().y * 0.5,
                    ),
                    galley,
                    visuals.text_color(),
                );
                icons::line_icon(
                    ui.painter(),
                    egui::pos2(rect.left() + ui.spacing().indent - 14., rect.center().y),
                    id,
                    icon_color,
                );
                *ui.style_mut() = body_style;
                egui::Frame::new().inner_margin(10).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    contents(ui)
                });
                return header;
            }
            let open = restored_header_open(ui, id, primary);
            let header = egui::CollapsingHeader::new(RichText::new(title).strong())
                .id_salt(id)
                .default_open(primary)
                .open(open)
                .icon(move |ui, openness, response| {
                    let center = response.rect.center() - egui::vec2(12., 0.);
                    icons::line_icon(ui.painter(), center + egui::vec2(24., 0.), id, icon_color);
                    let rotation =
                        egui::emath::Rot2::from_angle(openness * std::f32::consts::FRAC_PI_2);
                    let points = [
                        egui::vec2(-2., -4.),
                        egui::vec2(2., 0.),
                        egui::vec2(-2., 4.),
                    ]
                    .map(|point| center + rotation * point);
                    ui.painter().add(egui::Shape::line(
                        points.to_vec(),
                        Stroke::new(
                            1.6,
                            if openness > 0. {
                                colors.green
                            } else {
                                colors.muted
                            },
                        ),
                    ));
                })
                .show_unindented(ui, |ui| {
                    *ui.style_mut() = body_style;
                    egui::Frame::new().inner_margin(10).show(ui, contents).inner
                })
                .header_response;
            remember_section(ui.ctx(), header.id, id);
            header
        })
        .inner
    });
    if fill_size.is_none() {
        ui.add_space(if primary { 4. } else { 1. });
    }
    response
}

impl KetchupApp {
    pub(super) fn begin_component_placement(&mut self, id: u64, ctx: &egui::Context) {
        if !self
            .doc
            .model
            .objects
            .get(&id)
            .is_some_and(|o| matches!(o.kind, ObjectKind::Component { .. }))
        {
            return;
        }
        let mut model = self.doc.model.copy_entities(&BTreeSet::from([id]));
        // An instance placed from the library starts visible and unlocked.
        model.hidden.remove(&id);
        model.locked.remove(&id);
        model.entity_tags.remove(&id);
        let points = model.vertices.values().map(|v| v.position).chain(
            model
                .dimensions
                .values()
                .flat_map(|dimension| dimension.layout(&model)),
        );
        let mut points = points.chain(model.texts.values().flat_map(|t| t.layout(&model)));
        let Some(first) = points.next() else {
            self.say(ctx, "This component is empty");
            return;
        };
        let anchor = points.fold(first, |lo, point| lo.min(point));
        let clipboard = GeometryClipboard { model, anchor };
        self.set_tool(Tool::Select);
        self.paste_preview = Some(PastePreview {
            clipboard,
            point: self.doc.camera.target,
        });
        self.say(ctx, "Click to place the component · Escape to cancel");
    }

    fn components_contents(&mut self, ui: &mut egui::Ui) {
        let mut definitions = std::collections::BTreeMap::new();
        for object in self.doc.model.objects.values() {
            if let ObjectKind::Component { definition } = object.kind {
                let entry =
                    definitions
                        .entry(definition)
                        .or_insert((object.id, object.name.clone(), 0));
                entry.2 += 1;
            }
        }
        if definitions.is_empty() {
            ui.label(RichText::new("No components yet").color(Palette::for_ui(ui).muted));
            ui.label("Select geometry and choose Make Component from the context menu.");
        } else {
            ui.add(
                egui::TextEdit::singleline(&mut self.component_filter)
                    .hint_text("Search components")
                    .desired_width(f32::INFINITY),
            );
            let query = self.component_filter.to_lowercase();
            let mut rows: Vec<_> = definitions
                .into_values()
                .filter(|(_, name, _)| name.to_lowercase().contains(&query))
                .collect();
            rows.sort_by(|a, b| {
                a.1.to_lowercase()
                    .cmp(&b.1.to_lowercase())
                    .then(a.0.cmp(&b.0))
            });
            if rows.is_empty() {
                ui.label(RichText::new("No matching components").color(Palette::for_ui(ui).muted));
            }
            for (id, name, count) in rows {
                ui.push_id(id, |ui| {
                    egui::Frame::new()
                        .fill(Palette::for_ui(ui).canvas)
                        .inner_margin(8)
                        .corner_radius(4)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(RichText::new(name).strong());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{count} instance{}",
                                        if count == 1 { "" } else { "s" }
                                    ))
                                    .small()
                                    .color(Palette::for_ui(ui).muted),
                                );
                                if ui.small_button("Place instance").clicked() {
                                    self.begin_component_placement(id, ui.ctx());
                                }
                            });
                        });
                });
            }
        }
        if ui
            .add_enabled(
                !self.selected.is_empty() && !self.doc.model.selection_protected(&self.selected),
                egui::Button::new("Make Component…"),
            )
            .clicked()
        {
            self.context_action(ui.ctx(), ContextAction::Component);
        }
    }

    fn tags_contents(&mut self, ui: &mut egui::Ui) {
        let mut before = None;
        let mut label = "Edit tags";
        ui.horizontal(|ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.new_tag_name)
                    .hint_text("New tag name")
                    .desired_width((ui.available_width() - 40.).max(80.)),
            );
            let enter = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui
                .add_enabled(!self.new_tag_name.trim().is_empty(), egui::Button::new("+"))
                .on_hover_text("Add tag")
                .clicked()
                || enter
            {
                before.get_or_insert_with(|| self.doc.model.clone());
                match self.doc.model.add_tag(&self.new_tag_name) {
                    Ok(_) => {
                        self.new_tag_name.clear();
                        label = "Add tag";
                    }
                    Err(error) => self.say(ui.ctx(), error),
                }
            }
        });
        ui.label(
            RichText::new("Visibility · Color · Name")
                .small()
                .color(Palette::for_ui(ui).muted),
        );
        ui.label("Untagged");
        let tags: Vec<_> = self.doc.model.tags.values().cloned().collect();
        for mut tag in tags {
            ui.push_id(tag.id, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut tag.visible, "")
                        .on_hover_text("Show or hide this tag and its contents")
                        .changed()
                    {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.tags.get_mut(&tag.id).unwrap().visible = tag.visible;
                    }
                    if ui.color_edit_button_srgb(&mut tag.color).changed() {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.tags.get_mut(&tag.id).unwrap().color = tag.color;
                    }
                    let name = ui
                        .add_sized(
                            egui::vec2((ui.available_width() - 80.).max(30.), 24.),
                            egui::Label::new(&tag.name).truncate(),
                        )
                        .on_hover_text("Double-click to rename");
                    if name.double_clicked() {
                        self.tag_rename = Some((tag.id, tag.name.clone()));
                        Self::request_name_input_focus(
                            ui.ctx(),
                            egui::Id::unique("rename_tag_name"),
                        );
                    }
                    if ui
                        .small_button("Edit")
                        .on_hover_text("Rename tag")
                        .clicked()
                    {
                        self.tag_rename = Some((tag.id, tag.name.clone()));
                        Self::request_name_input_focus(
                            ui.ctx(),
                            egui::Id::unique("rename_tag_name"),
                        );
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Delete tag; its entities become Untagged")
                        .clicked()
                    {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        self.doc.model.remove_tag(tag.id);
                        label = "Delete tag";
                    }
                });
            });
        }
        if let Some((id, mut name)) = self.tag_rename.take() {
            if self.doc.model.tags.contains_key(&id) {
                let mut keep = true;
                ui.horizontal(|ui| {
                    let input_id = egui::Id::unique("rename_tag_name");
                    ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .id(input_id)
                            .desired_width(115.),
                    );
                    Self::focus_name_input(ui, &name, input_id);
                    if ui.small_button("Save").clicked() {
                        before.get_or_insert_with(|| self.doc.model.clone());
                        match self.doc.model.rename_tag(id, &name) {
                            Ok(()) => {
                                keep = false;
                                label = "Rename tag";
                            }
                            Err(error) => self.say(ui.ctx(), error),
                        }
                    }
                    if ui.small_button("Cancel").clicked() {
                        keep = false;
                    }
                });
                if keep {
                    self.tag_rename = Some((id, name));
                }
            }
        }
        ui.separator();
        if self.selected.is_empty() {
            ui.label(
                RichText::new("Select entities to assign a tag.")
                    .small()
                    .color(Palette::for_ui(ui).muted),
            );
        } else {
            let first = self
                .selected
                .first()
                .and_then(|id| self.doc.model.entity_tags.get(id))
                .copied();
            let mixed = self
                .selected
                .iter()
                .any(|id| self.doc.model.entity_tags.get(id).copied() != first);
            let current = if mixed {
                "Mixed tags"
            } else {
                first
                    .and_then(|id| self.doc.model.tags.get(&id))
                    .map(|t| t.name.as_str())
                    .unwrap_or("Untagged")
            };
            let mut assign = None;
            ui.add_enabled_ui(!self.doc.model.selection_protected(&self.selected), |ui| {
                egui::ComboBox::from_id_salt("selection_tag")
                    .selected_text(current)
                    .width(ui.available_width() - 10.)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(!mixed && first.is_none(), "Untagged")
                            .clicked()
                        {
                            assign = Some(None);
                        }
                        for tag in self.doc.model.tags.values() {
                            if ui
                                .selectable_label(!mixed && first == Some(tag.id), &tag.name)
                                .clicked()
                            {
                                assign = Some(Some(tag.id));
                            }
                        }
                    });
            });
            if let Some(tag) = assign {
                before.get_or_insert_with(|| self.doc.model.clone());
                if let Err(error) = self.doc.model.assign_tag(&self.selected, tag) {
                    self.say(ui.ctx(), error);
                }
                label = "Assign tag";
            }
        }
        if let Some(before) = before.filter(|before| *before != self.doc.model) {
            // Tag visibility is global, while assignments inside definitions are shared.
            if label == "Assign tag" {
                self.finish_context_state_edit(label, before, ui.ctx());
            } else {
                self.finish_edit(label, before, ui.ctx());
            }
            if self
                .doc
                .model
                .edit_context
                .is_some_and(|id| self.doc.model.is_hidden(id))
            {
                self.navigate_context(None, ui.ctx());
            }
            self.hover = None;
            self.snap = None;
        }
    }

    fn units_contents(&mut self, ui: &mut egui::Ui) {
        Self::unit_settings_contents(ui, &mut self.doc.display.units);
    }

    pub(super) fn unit_settings_contents(
        ui: &mut egui::Ui,
        units: &mut ketchup_core::UnitSettings,
    ) {
        fn precision_picker(
            ui: &mut egui::Ui,
            id: &'static str,
            label: &str,
            precision: &mut Option<u8>,
            default: u8,
        ) {
            let name = |digits: u8| format!("{:.*}", digits as usize, 0.);
            let current = precision
                .unwrap_or(default)
                .min(ketchup_core::units::MAX_DECIMAL_PLACES);
            ui.horizontal(|ui| {
                let label = ui.label(label);
                egui::ComboBox::from_id_salt(id)
                    .selected_text(name(current))
                    .width(ui.available_width())
                    .truncate()
                    .show_ui(ui, |ui| {
                        for digits in 0..=ketchup_core::units::MAX_DECIMAL_PLACES {
                            if ui
                                .selectable_label(current == digits, name(digits))
                                .clicked()
                            {
                                *precision = Some(digits);
                            }
                        }
                    })
                    .response
                    .labelled_by(label.id);
            });
        }

        fn picker<T: Copy + PartialEq>(
            ui: &mut egui::Ui,
            id: &'static str,
            label: &str,
            selected: &mut T,
            choices: &[T],
            name: impl Fn(T) -> &'static str,
        ) {
            let label = ui.label(label);
            egui::ComboBox::from_id_salt(id)
                .selected_text(name(*selected))
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    for &choice in choices {
                        ui.selectable_value(selected, choice, name(choice));
                    }
                })
                .response
                .labelled_by(label.id);
        }

        picker(
            ui,
            "units_length",
            "Length",
            &mut units.length,
            LengthUnit::ALL,
            LengthUnit::label,
        );
        precision_picker(
            ui,
            "units_length_precision",
            "Length precision",
            &mut units.precision.length,
            3,
        );
        picker(
            ui,
            "units_area",
            "Area",
            &mut units.area,
            AreaUnit::ALL,
            AreaUnit::label,
        );
        precision_picker(
            ui,
            "units_area_precision",
            "Area precision",
            &mut units.precision.area,
            3,
        );
        picker(
            ui,
            "units_volume",
            "Volume",
            &mut units.volume,
            VolumeUnit::ALL,
            VolumeUnit::label,
        );
        precision_picker(
            ui,
            "units_volume_precision",
            "Volume precision",
            &mut units.precision.volume,
            3,
        );
        picker(
            ui,
            "units_angle",
            "Angle",
            &mut units.angle,
            AngleUnit::ALL,
            AngleUnit::label,
        );
        precision_picker(
            ui,
            "units_angle_precision",
            "Angle precision",
            &mut units.precision.angle,
            units.angle.default_precision(),
        );
        ui.add_space(4.);
        ui.label(
            RichText::new("Precision controls displayed decimal places. Lengths entered without a unit use the selected length unit.")
                .small()
                .color(Palette::for_ui(ui).muted),
        );
    }

    fn display_contents(&mut self, ui: &mut egui::Ui) {
        let before = self.doc.display.clone();
        let d = &mut self.doc.display;
        ui.label(RichText::new("Visibility").strong());
        for (value, label) in [
            (&mut d.axes, "Axes"),
            (&mut d.guides, "Guides"),
            (&mut d.grid, "Grid"),
            (&mut d.dimensions, "Dimensions"),
            (&mut d.hidden_geometry, "Hidden geometry"),
            (&mut d.hidden_objects, "Hidden objects"),
        ] {
            ui.checkbox(value, label);
        }
        ui.separator();
        ui.label(RichText::new("Face style").strong());
        face_style_picker(ui, &mut d.face_style);
        ui.checkbox(&mut d.x_ray, "X-Ray");
        ui.checkbox(&mut d.color_by_tag, "Color by tag");
        inspector_section(ui, "Edge style", "edge_style", false, |ui| {
            for (value, label) in [
                (&mut d.edges, "Edges"),
                (&mut d.back_edges, "Back edges"),
                (&mut d.profiles, "Profiles"),
                (&mut d.depth_cue, "Depth cue"),
                (&mut d.endpoints, "Endpoints"),
                (&mut d.extensions, "Extensions"),
            ] {
                ui.checkbox(value, label);
            }
            ui.add_enabled_ui(d.profiles, |ui| {
                // Keep the label above the slider so this nested section fits
                // even at the inspector's minimum width.
                let label = ui.label("Profile width");
                ui.add(egui::Slider::new(&mut d.profile_width, 1. ..=8.))
                    .labelled_by(label.id);
            });
        });
        inspector_section(ui, "Sections", "sections", false, |ui| {
            ui.checkbox(&mut d.section_planes, "Section planes");
            ui.checkbox(&mut d.section_cuts, "Section cuts");
            ui.checkbox(&mut d.section_fill, "Section fill");
            ui.horizontal(|ui| {
                ui.label("Height");
                ui.add(
                    egui::DragValue::new(&mut d.section_height)
                        .speed(0.05)
                        .range(-1e8..=1e8)
                        .suffix(" m"),
                );
            });
            ui.label(
                RichText::new("Horizontal section: keeps geometry below the plane.")
                    .small()
                    .color(Palette::for_ui(ui).muted),
            );
        });
        inspector_section(ui, "Shadows", "shadows", false, |ui| {
            ui.checkbox(&mut d.shadows, "Display shadows");
            ui.add_enabled_ui(d.shadows, |ui| {
                ui.add(
                    egui::Slider::new(&mut d.sun_angle, 0. ..=360.)
                        .text("Direction")
                        .suffix("°"),
                );
                ui.add(
                    egui::Slider::new(&mut d.sun_elevation, 5. ..=85.)
                        .text("Elevation")
                        .suffix("°"),
                );
                ui.add(egui::Slider::new(&mut d.shadow_darkness, 0. ..=1.).text("Darkness"));
            });
        });
        inspector_section(ui, "Fog", "fog", false, |ui| {
            ui.checkbox(&mut d.fog, "Display fog");
            ui.add_enabled_ui(d.fog, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Start");
                    ui.add(
                        egui::DragValue::new(&mut d.fog_start)
                            .speed(0.5)
                            .range(0. ..=1e8)
                            .suffix(" m"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("End");
                    ui.add(
                        egui::DragValue::new(&mut d.fog_end)
                            .speed(0.5)
                            .range(0.1..=1e9)
                            .suffix(" m"),
                    );
                });
                d.fog_end = d.fog_end.max(d.fog_start + 0.1);
            });
            ui.horizontal(|ui| {
                crate::environment::background_control(ui, d);
                ui.label("Background / fog color");
            });
        });
        inspector_section(ui, "Environment", "display_environment", false, |ui| {
            crate::environment::controls(ui, d);
        });
        if ui.button("Reset display").clicked() {
            *d = DisplaySettings {
                units: d.units,
                ..DisplaySettings::default()
            };
        }
        if before != self.doc.display {
            self.changed(ui.ctx());
        }
    }
}
