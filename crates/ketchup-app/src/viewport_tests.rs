use super::*;
use eframe::Storage;

#[derive(Default)]
struct MemoryStorage {
    values: BTreeMap<String, String>,
    flushes: usize,
}
impl Storage for MemoryStorage {
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
        self.flushes += 1;
    }
}

fn tab_ids(layout: &Layout) -> Vec<u64> {
    let mut ids = Vec::new();
    layout.tabs(&mut ids);
    ids
}

struct UiHarness {
    app: KetchupApp,
    ctx: egui::Context,
    time: f64,
}
impl UiHarness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut harness = Self {
            app: KetchupApp::with_document(Document::default(), None, true),
            ctx,
            time: 0.,
        };
        harness.frame(vec![]);
        harness.frame(vec![]);
        harness
    }
    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 0.01;
        self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 700.))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| self.app.viewport_panel(ui),
        )
    }
    fn pointer(&mut self, pos: Pos2, button: egui::PointerButton, pressed: bool) {
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
    }
    fn click(&mut self, pos: Pos2) {
        self.pointer(pos, egui::PointerButton::Primary, true);
        self.pointer(pos, egui::PointerButton::Primary, false);
    }
    fn label(&mut self, label: &str) -> Pos2 {
        self.frame(vec![])
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label}"))
    }
}

#[test]
fn pointer_creates_and_docks_a_tab_then_orbits_only_its_camera() {
    let mut harness = UiHarness::new();
    let current = harness.label("Current");
    let plus = harness.label("+");
    let model = harness.label("Model");
    assert!(current.x < plus.x && plus.x < model.x);
    harness.click(plus);
    let tab = harness.label("View 1");
    harness.pointer(tab, egui::PointerButton::Primary, true);
    let drop = egui::pos2(990., 350.);
    harness.frame(vec![egui::Event::PointerMoved(drop)]);
    harness.pointer(drop, egui::PointerButton::Primary, false);
    harness.frame(vec![]);
    assert!(matches!(
        harness.app.viewports.layout,
        Layout::Split {
            horizontal: true,
            ..
        }
    ));
    let right = harness.app.viewport;
    assert!(right.left() > 400.);
    let original_left = harness.app.viewports.cameras[&0].clone();
    let original_right = harness.app.viewports.cameras[&1].clone();
    let start = right.center();
    harness.pointer(start, egui::PointerButton::Middle, true);
    // Cross the divider while dragging: input must remain captured by
    // the right-hand camera until release.
    let end = egui::pos2(250., start.y + 30.);
    harness.frame(vec![egui::Event::PointerMoved(end)]);
    harness.pointer(end, egui::PointerButton::Middle, false);
    assert_eq!(harness.app.viewports.cameras[&0], original_left);
    assert_ne!(harness.app.viewports.cameras[&1], original_right);
    assert!(harness.app.gesture.is_none());
    assert!(harness.app.doc.model.edges.is_empty());
}

#[test]
fn tabs_share_the_existing_toolbar_and_close_only_from_the_context_menu() {
    let mut harness = UiHarness::new();
    let current = harness.label("Current");
    let plus = harness.label("+");
    let model = harness.label("Model");
    assert!((current.y - plus.y).abs() < 3.);
    assert!((current.y - model.y).abs() < 3.);
    assert!(current.y < 40., "tabs belong in the original toolbar");
    assert!(
        harness.app.viewport.top() <= 55.,
        "no additional toolbar row"
    );
    harness.click(plus);
    let output = harness.frame(vec![]);
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "×" || text.galley.job.text == "Viewports")));
    assert_eq!(tab_ids(&harness.app.viewports.layout), vec![0, 1]);
    let tab = harness.label("View 1");
    harness.pointer(tab, egui::PointerButton::Secondary, true);
    harness.pointer(tab, egui::PointerButton::Secondary, false);
    // Opening the context menu alone leaves the view intact.
    assert_eq!(tab_ids(&harness.app.viewports.layout), vec![0, 1]);
    let close = harness.label("Close");
    harness.click(close);
    assert_eq!(harness.app.viewports.layout, Layout::default());
    assert_eq!(harness.app.viewports.active, 0);
    assert!(!harness.app.viewports.cameras.contains_key(&1));
    assert!(!harness.app.project.modified);
    assert!(harness.app.dirty_since.is_none());

    let mut storage = MemoryStorage::default();
    harness
        .app
        .persist_inspector_changes(&harness.ctx, Some(&mut storage), false);
    let mut restored = KetchupApp::with_document(Document::default(), None, true);
    restored.restore_inspector_preferences(&egui::Context::default(), Some(&storage));
    assert_eq!(restored.viewports.layout, Layout::default());

    let current = harness.label("Current");
    harness.pointer(current, egui::PointerButton::Secondary, true);
    harness.pointer(current, egui::PointerButton::Secondary, false);
    let close = harness.label("Close");
    harness.click(close);
    assert_eq!(
        harness.app.viewports.layout,
        Layout::default(),
        "keep the final view"
    );
}

#[test]
fn defaults_and_older_preferences_open_a_single_current_view() {
    let preferences: Preferences = serde_json::from_str("{}").unwrap();
    assert_eq!(preferences, Preferences::default());
    let mut camera = Camera {
        target: DVec3::new(15., 20., 3.),
        distance: 70.,
        ..Default::default()
    };
    let original = camera.clone();
    let workspace = Workspace::restore(&preferences, &mut camera, &BTreeMap::new());
    assert_eq!(workspace.layout, Layout::default());
    assert_eq!(workspace.active, 0);
    assert_eq!(camera, original);
}

#[test]
fn edge_drops_split_in_all_directions_and_center_drops_collapse_empty_panes() {
    for side in [
        DropSide::Left,
        DropSide::Right,
        DropSide::Top,
        DropSide::Bottom,
    ] {
        let mut workspace = Workspace::default();
        let tab = workspace.add(0, &Camera::default());
        workspace.dock(tab, 0, side);
        let Layout::Split {
            horizontal,
            first,
            second,
            ..
        } = &workspace.layout
        else {
            panic!("expected split")
        };
        assert_eq!(
            *horizontal,
            matches!(side, DropSide::Left | DropSide::Right)
        );
        let (fresh, original) = if matches!(side, DropSide::Left | DropSide::Top) {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(tab_ids(fresh), vec![tab]);
        assert_eq!(tab_ids(original), vec![0]);
        workspace.dock(tab, 0, DropSide::Center);
        assert_eq!(
            workspace.layout,
            Layout::Pane {
                id: 0,
                tabs: vec![0, tab],
                selected: tab
            }
        );
    }
}

#[test]
fn nested_splits_moves_and_closes_keep_each_tab_exactly_once() {
    let mut workspace = Workspace::default();
    let a = workspace.add(0, &Camera::default());
    let b = workspace.add(0, &Camera::default());
    workspace.dock(a, 0, DropSide::Right);
    workspace.dock(b, 0, DropSide::Bottom);
    assert_eq!(
        tab_ids(&workspace.layout)
            .into_iter()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, a, b])
    );
    // Moving the last tab of a source pane to an existing different pane
    // preserves the target ID while collapsing its old sibling branch.
    workspace.dock(a, 0, DropSide::Left);
    let mut ids = BTreeSet::new();
    assert!(workspace.layout.valid(0, &mut ids, &mut BTreeSet::new()));
    assert_eq!(ids, BTreeSet::from([0, a, b]));
    workspace.layout.remove(a);
    workspace.layout.remove(b);
    assert_eq!(workspace.layout, Layout::default());
    workspace.dock(0, 0, DropSide::Right);
    assert_eq!(
        workspace.layout,
        Layout::default(),
        "a sole tab cannot split itself"
    );
}

#[test]
fn camera_switches_share_model_and_history_but_not_orientation_or_zoom() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let original_camera = app.doc.camera.clone();
    let other = app.viewports.add(0, &original_camera);
    app.activate_viewport(other);
    app.doc
        .camera
        .orbit_around(DVec2::new(70., -20.), DVec3::ZERO);
    app.doc.camera.distance = 42.;
    let other_camera = app.doc.camera.clone();
    let before = app.doc.model.clone();
    app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.finish_edit("Draw in other view", before, &ctx);
    app.activate_viewport(0);
    assert_eq!(app.doc.camera, original_camera);
    assert_eq!(app.doc.model.edges.len(), 1);
    app.undo(&ctx, false);
    assert!(app.doc.model.edges.is_empty());
    app.activate_viewport(other);
    assert_eq!(app.doc.camera, other_camera);
    app.undo(&ctx, true);
    assert_eq!(app.doc.model.edges.len(), 1);
}

#[test]
fn tabs_layout_active_view_and_orientations_save_without_a_model_edit() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let tab = app.viewports.add(0, &app.doc.camera);
    app.viewports.dock(tab, 0, DropSide::Right);
    app.activate_viewport(tab);
    app.doc.camera.yaw = 1.2;
    app.doc.camera.pitch = 0.3;
    if let Layout::Split { ratio, .. } = &mut app.viewports.layout {
        *ratio = 0.63;
    }
    let mut storage = MemoryStorage::default();
    app.persist_inspector_changes(&ctx, Some(&mut storage), false);
    assert_eq!(storage.flushes, 1);
    assert!(!app.project.modified);
    assert!(app.dirty_since.is_none());
    let expected = app.viewports.capture(&app.doc.camera);
    let restored_ctx = egui::Context::default();
    let mut restored = KetchupApp::with_document(Document::default(), None, true);
    restored.restore_inspector_preferences(&restored_ctx, Some(&storage));
    assert_eq!(restored.viewports.capture(&restored.doc.camera), expected);
    assert_eq!(restored.doc.camera.yaw, 1.2);
    assert_eq!(restored.doc.camera.pitch, 0.3);
    restored.persist_inspector_changes(&restored_ctx, Some(&mut storage), false);
    assert_eq!(
        storage.flushes, 1,
        "unchanged preferences should not flush again"
    );
}

#[test]
fn project_cameras_round_trip_without_leaking_framing_to_other_projects() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let tab = app.viewports.add(0, &app.doc.camera);
    app.activate_viewport(tab);
    app.doc.camera.target = DVec3::new(100., 200., 30.);
    app.doc.camera.distance = 150.;
    app.doc.camera.yaw = 2.;
    let other = app.doc.camera.clone();
    app.activate_viewport(0);
    app.sync_viewport_cameras();
    let saved = Document::from_json(&serde_json::to_string(&app.doc).unwrap()).unwrap();
    assert_eq!(saved.viewport_cameras[&tab], other);
    let preferences = app.viewports.capture(&app.doc.camera);
    let mut loaded = KetchupApp::with_document(saved, None, true);
    loaded.viewports = Workspace::restore(
        &preferences,
        &mut loaded.doc.camera,
        &loaded.doc.viewport_cameras,
    );
    loaded.activate_viewport(tab);
    assert_eq!(loaded.doc.camera, other);
    let fresh = Document::default();
    app.replace_project(fresh.clone(), None, &ctx);
    app.activate_viewport(tab);
    assert_eq!(app.doc.camera.target, fresh.camera.target);
    assert_eq!(app.doc.camera.distance, fresh.camera.distance);
    assert_eq!(app.doc.camera.yaw, other.yaw);
    assert!(app.doc.model.edges.is_empty());
}

#[test]
fn older_documents_default_viewport_cameras_and_invalid_saved_cameras_are_rejected() {
    let mut json = serde_json::to_value(Document::default()).unwrap();
    json.as_object_mut().unwrap().remove("viewport_cameras");
    let restored = Document::from_json(&json.to_string()).unwrap();
    assert!(restored.viewport_cameras.is_empty());
    let mut invalid = restored;
    invalid.viewport_cameras.insert(
        5,
        Camera {
            distance: -1.,
            ..Camera::default()
        },
    );
    assert!(Document::from_json(&serde_json::to_string(&invalid).unwrap()).is_err());
}

#[test]
fn invalid_saved_layout_falls_back_to_current() {
    let mut preferences = Preferences::default();
    preferences.layout = Layout::Pane {
        id: 0,
        tabs: vec![0, 0],
        selected: 0,
    };
    let mut camera = Camera::default();
    assert_eq!(
        Workspace::restore(&preferences, &mut camera, &BTreeMap::new()).layout,
        Layout::default()
    );
    preferences.layout = Layout::default();
    preferences.active = 99;
    assert_eq!(
        Workspace::restore(&preferences, &mut camera, &BTreeMap::new()).layout,
        Layout::default()
    );
}

#[test]
fn drop_preview_matches_the_requested_edge_or_center() {
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.));
    for (pos, expected) in [
        (egui::pos2(1., 300.), DropSide::Left),
        (egui::pos2(799., 300.), DropSide::Right),
        (egui::pos2(400., 1.), DropSide::Top),
        (egui::pos2(400., 599.), DropSide::Bottom),
        (rect.center(), DropSide::Center),
    ] {
        let (side, preview) = drop_target(rect, pos);
        assert_eq!(side, expected);
        assert!(rect.contains_rect(preview));
        assert_eq!(
            preview.area(),
            rect.area() * if side == DropSide::Center { 1. } else { 0.5 }
        );
    }
}
