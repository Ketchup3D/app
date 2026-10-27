use super::*;
use eframe::App;

#[path = "context_menu_tests.rs"]
mod context_menus;
#[path = "drawing_inference_tests.rs"]
mod drawing_inference;
#[path = "feature_audit_tests.rs"]
mod feature_audit;
#[path = "outliner_drag_tests.rs"]
mod outliner_drag;
#[path = "precision_tests.rs"]
mod precision_input;
#[path = "release_notes_tests.rs"]
mod release_notes;

struct Harness {
    ctx: egui::Context,
    app: KetchupApp,
    time: f64,
    size: Vec2,
    maximized: bool,
}
impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut harness = Self {
            ctx,
            app: KetchupApp::with_document(Document::default(), None, true),
            time: 0.,
            size: egui::vec2(1440., 900.),
            maximized: false,
        };
        harness.frame(vec![], egui::Modifiers::NONE);
        harness.frame(vec![], egui::Modifiers::NONE);
        harness
    }
    fn frame(
        &mut self,
        mut events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        self.time += 0.01;
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .maximized = Some(self.maximized);
        let app = &mut self.app;
        let mut output = self
            .ctx
            .run_ui(input, |ui| app.ui(ui, &mut eframe::Frame::_new_kittest()));
        output.textures_delta.clear();
        output
    }
    fn pointer(
        &mut self,
        pos: Pos2,
        button: egui::PointerButton,
        pressed: bool,
        shift: bool,
    ) -> egui::FullOutput {
        let modifiers = egui::Modifiers {
            shift,
            ..Default::default()
        };
        self.pointer_with_modifiers(pos, button, pressed, modifiers)
    }
    fn pointer_with_modifiers(
        &mut self,
        pos: Pos2,
        button: egui::PointerButton,
        pressed: bool,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        self.frame(
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                },
            ],
            modifiers,
        )
    }
    fn click(&mut self, pos: Pos2) -> egui::FullOutput {
        self.pointer(pos, egui::PointerButton::Primary, true, false);
        self.pointer(pos, egui::PointerButton::Primary, false, false)
    }
    fn at(&self, p: DVec3) -> Pos2 {
        self.app.viewport.min
            + pv(self
                .app
                .doc
                .camera
                .project(p, dv(self.app.viewport.size()))
                .unwrap())
    }
    fn key(&mut self, key: Key) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![event(true)], egui::Modifiers::NONE);
        self.frame(vec![event(false)], egui::Modifiers::NONE);
    }
}

fn window_commands(output: &egui::FullOutput) -> &[egui::ViewportCommand] {
    &output.viewport_output[&egui::ViewportId::ROOT].commands
}

fn native_motion_burst(from: Pos2, to: Pos2, count: usize) -> Vec<egui::Event> {
    (1..=count)
        .flat_map(|i| {
            [
                egui::Event::PointerMoved(from.lerp(to, i as f32 / count as f32)),
                egui::Event::MouseMoved(egui::vec2(1., -1.)),
            ]
        })
        .collect()
}

#[test]
fn native_mouse_burst_batches_snapping_and_preserves_the_first_line_click() {
    let mut h = Harness::new();
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.app.set_tool(Tool::Line);
    let start = h.at(DVec3::ZERO);
    let end = h.at(DVec3::new(2., 1., 0.));
    let mut events = native_motion_burst(start - egui::vec2(30., 20.), start, 400);
    for pressed in [true, false] {
        events.push(egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    events.extend(native_motion_burst(start, end, 400));
    h.app.picking_preparations = 0;
    h.frame(events, egui::Modifiers::NONE);
    assert!(h.app.line_start.unwrap().length() < 1e-6);
    assert!(h.app.snap.unwrap().point.distance(DVec3::new(2., 1., 0.)) < 1e-6);
    assert!(h.app.doc.model.edges.is_empty());
    assert!(
        h.app.picking_preparations <= 4,
        "A motion burst must not cause a snap query per native mouse event: {}",
        h.app.picking_preparations
    );
    h.click(end);
    assert_eq!(h.app.doc.model.edges.len(), 1);
    let edge = h.app.doc.model.edges.values().next().unwrap();
    let (a, b) = h.app.doc.model.points(edge);
    assert!(a.length() < 1e-6);
    assert!(b.distance(DVec3::new(2., 1., 0.)) < 1e-6);
}

#[test]
fn selection_rectangle_tracks_native_motion_without_hover_picking() {
    let mut h = Harness::new();
    let edge = h.app.doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
    let from = h.app.viewport.min + Vec2::splat(10.);
    let to = h.app.viewport.max - Vec2::splat(10.);
    h.pointer(from, egui::PointerButton::Primary, true, false);
    h.app.hover = Some(edge);
    h.app.picking_preparations = 0;
    h.frame(native_motion_burst(from, to, 400), egui::Modifiers::NONE);
    assert_eq!(h.app.picking_preparations, 0);
    assert_eq!(h.app.gesture.as_ref().unwrap().last, to);
    assert_eq!(h.app.hover, None);
    assert!(h.app.selected.is_empty());
    h.pointer(to, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.selected, BTreeSet::from([edge]));
}

#[test]
fn native_mouse_deltas_do_not_discard_freehand_samples() {
    let mut h = Harness::new();
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    h.app.set_tool(Tool::Freehand);
    let start = h.app.viewport.center();
    let end = start + egui::vec2(180., -60.);
    h.pointer(start, egui::PointerButton::Primary, true, false);
    h.frame(native_motion_burst(start, end, 20), egui::Modifiers::NONE);
    assert!(h.app.drawing.as_ref().unwrap().points.len() > 5);
    h.pointer(end, egui::PointerButton::Primary, false, false);
    assert!(h.app.doc.model.edges.len() > 5);
}

#[test]
fn erase_and_freehand_hover_batch_motion_until_a_stroke_starts() {
    for tool in [Tool::Erase, Tool::Freehand] {
        let mut h = Harness::new();
        let edge = h.app.doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
        let before = h.app.doc.model.clone();
        h.app.set_tool(tool);
        let target = h.at(DVec3::ZERO);
        h.app.picking_preparations = 0;
        h.frame(
            native_motion_burst(target - egui::vec2(100., 50.), target, 400),
            egui::Modifiers::NONE,
        );
        assert_eq!(h.app.picking_preparations, 1, "{tool:?}");
        assert_eq!(h.app.hover, Some(edge), "{tool:?}");
        assert!(h.app.gesture.is_none());
        assert!(h.app.drawing.is_none());
        assert_eq!(h.app.doc.model, before);
    }
}

#[test]
fn erase_stroke_keeps_intermediate_motion_when_the_pointer_returns_to_its_start() {
    let mut h = Harness::new();
    let edge = h.app.doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
    let before = h.app.doc.model.clone();
    h.app.set_tool(Tool::Erase);
    let start = h.at(-DVec3::Y * 2.);
    let across = h.at(DVec3::Y * 2.);
    h.pointer(start, egui::PointerButton::Primary, true, false);
    let mut events = native_motion_burst(start, across, 10);
    events.extend(native_motion_burst(across, start, 10));
    h.frame(events, egui::Modifiers::NONE);
    let GestureKind::Erase { ids } = &h.app.gesture.as_ref().unwrap().kind else {
        panic!("The erase stroke should remain active until release");
    };
    assert_eq!(ids, &BTreeSet::from([edge]));
    assert_eq!(h.app.doc.model, before);
    h.pointer(start, egui::PointerButton::Primary, false, false);
    assert!(h.app.doc.model.edges.is_empty());
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn toolbar_window_actions_remain_accessible_at_minimum_width() {
    for width in [1440., 800.] {
        for (offset, maximized, command) in [
            (34., false, egui::ViewportCommand::Close),
            (74., false, egui::ViewportCommand::Maximized(true)),
            (74., true, egui::ViewportCommand::Maximized(false)),
            (114., false, egui::ViewportCommand::Minimized(true)),
        ] {
            let mut h = Harness::new();
            h.size.x = width;
            h.maximized = maximized;
            h.frame(vec![], egui::Modifiers::NONE);
            h.frame(vec![], egui::Modifiers::NONE);
            let output = h.click(egui::pos2(width - offset, 32.));
            assert_eq!(window_commands(&output), &[command], "width {width}");
            assert!(h.app.doc.model.edges.is_empty());
        }
    }
}

#[test]
fn toolbar_background_drags_and_double_click_toggles_maximize() {
    let mut h = Harness::new();
    let pos = egui::pos2(800., 32.);
    h.pointer(pos, egui::PointerButton::Primary, true, false);
    let output = h.frame(
        vec![egui::Event::PointerMoved(pos + egui::vec2(20., 0.))],
        egui::Modifiers::NONE,
    );
    assert!(window_commands(&output).contains(&egui::ViewportCommand::StartDrag));

    for maximized in [false, true] {
        let mut h = Harness::new();
        h.maximized = maximized;
        h.click(pos);
        let output = h.click(pos);
        assert_eq!(
            window_commands(&output),
            &[egui::ViewportCommand::Maximized(!maximized)]
        );
    }
}

#[test]
fn undecorated_window_keeps_resize_grips_and_disables_them_when_maximized() {
    let mut h = Harness::new();
    let pos = egui::pos2(1438., 898.);
    let output = h.pointer(pos, egui::PointerButton::Primary, true, false);
    assert_eq!(
        window_commands(&output),
        &[egui::ViewportCommand::BeginResize(
            egui::ResizeDirection::SouthEast
        )]
    );
    h.pointer(pos, egui::PointerButton::Primary, false, false);
    h.maximized = true;
    h.frame(vec![], egui::Modifiers::NONE);
    h.frame(vec![], egui::Modifiers::NONE);
    let output = h.pointer(pos, egui::PointerButton::Primary, true, false);
    assert!(window_commands(&output).is_empty());
}

#[test]
fn shape_tools_create_faces_and_undo_as_one_operation() {
    for (key, tool, edges) in [
        (Key::R, Tool::Rectangle, 4),
        (Key::C, Tool::Circle, 48),
        (Key::G, Tool::Polygon, 6),
    ] {
        let mut h = Harness::new();
        h.key(key);
        assert_eq!(h.app.tool, tool);
        h.click(h.at(DVec3::ZERO));
        let end = h.at(DVec3::new(2., 1.5, 0.));
        h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
        assert!(
            h.app.doc.model.edges.is_empty(),
            "preview must not edit geometry"
        );
        h.click(end);
        assert_eq!(h.app.doc.model.edges.len(), edges, "{tool:?}");
        assert_eq!(h.app.doc.model.faces.len(), 1, "{tool:?}");
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert!(h.app.doc.model.edges.is_empty());
        h.app.undo(&h.ctx, true);
        assert_eq!(h.app.doc.model.edges.len(), edges);
        assert_eq!(h.app.doc.model.faces.len(), 1);
    }
}

#[test]
fn drawing_menu_switches_tools_and_remembers_the_choice() {
    let mut h = Harness::new();
    let rail = egui::pos2(38., 169.);
    for tool in Tool::DRAWING {
        h.click(rail);
        let output = h.frame(vec![], egui::Modifiers::NONE);
        for option in Tool::DRAWING {
            assert_menu_icon(&output, option.label());
        }
        let pos = menu_text_position(&output, tool.label());
        h.click(pos);
        assert_eq!(h.app.tool, tool);
        assert_eq!(h.app.drawing_tool, tool);
        assert!(h.app.doc.model.edges.is_empty());
        h.key(Key::Space);
        assert_eq!(h.app.drawing_tool, tool);
    }
}

#[test]
fn toolbar_order_and_measurement_flyout_defaults_and_remembers_the_choice() {
    let mut h = Harness::new();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let labels = [
        "Select",
        "Line",
        "Erase",
        "Orbit",
        "Push/Pull",
        "Scale",
        "Offset",
        "Move",
        "Paint",
        "Dimensions",
        "Union",
    ];
    let positions: Vec<_> = labels
        .iter()
        .map(|label| text_position(&output, label))
        .collect();
    assert!(positions.iter().all(|pos| pos.x < 76.));
    assert!(positions.windows(2).all(|pair| pair[0].y < pair[1].y));
    assert_eq!(h.app.measurement_tool, Tool::Dimensions);
    assert_eq!(h.app.solid_tool, Tool::Union);
    let rail = positions[9];
    for tool in Tool::MEASURING {
        h.click(rail);
        let menu = h.frame(vec![], egui::Modifiers::NONE);
        for option in Tool::MEASURING {
            assert_menu_icon(&menu, option.label());
        }
        h.click(menu_text_position(&menu, tool.label()));
        assert_eq!(h.app.tool, tool);
        assert_eq!(h.app.measurement_tool, tool);
        assert!(!egui::Popup::is_any_open(&h.ctx));
        h.key(Key::Space);
        assert_eq!(h.app.measurement_tool, tool);
    }
    h.key(Key::I);
    assert_eq!(h.app.measurement_tool, Tool::Dimensions);
    h.key(Key::T);
    assert_eq!(h.app.measurement_tool, Tool::TapeMeasure);
    h.key(Key::A);
    assert_eq!(h.app.measurement_tool, Tool::Protractor);
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn freehand_drag_commits_a_stroke_and_escape_cancels_it() {
    let mut h = Harness::new();
    h.key(Key::D);
    let start = h.at(DVec3::ZERO);
    h.pointer(start, egui::PointerButton::Primary, true, false);
    for point in [
        DVec3::new(1., 0.5, 0.),
        DVec3::new(2., 0.3, 0.),
        DVec3::new(3., 1., 0.),
    ] {
        let pos = h.at(point);
        h.frame(vec![egui::Event::PointerMoved(pos)], egui::Modifiers::NONE);
    }
    assert!(h.app.doc.model.edges.is_empty());
    let end = h.at(DVec3::new(3., 1., 0.));
    h.pointer(end, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.doc.model.edges.len(), 3);
    h.app.doc.model.validate().unwrap();
    h.app.undo(&h.ctx, false);
    assert!(h.app.doc.model.edges.is_empty());
    h.pointer(start, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    h.key(Key::Escape);
    h.pointer(end, egui::PointerButton::Primary, false, false);
    assert!(h.app.doc.model.edges.is_empty());
    assert!(h.app.drawing.is_none());
}

#[test]
fn shape_previews_cancel_and_degenerate_rectangles_do_not_commit() {
    let mut h = Harness::new();
    h.key(Key::R);
    h.click(h.at(DVec3::ZERO));
    h.click(h.at(DVec3::X * 2.));
    assert!(h.app.doc.model.edges.is_empty());
    assert!(h.app.drawing.is_some());
    h.key(Key::Escape);
    assert!(h.app.drawing.is_none());
    h.click(h.at(DVec3::ZERO));
    h.key(Key::C);
    assert!(h.app.drawing.is_none());
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn shapes_stay_on_the_starting_face_plane() {
    let mut h = Harness::new();
    let corners = [
        DVec3::new(-4., 0., 0.),
        DVec3::new(4., 0., 0.),
        DVec3::new(4., 0., 5.),
        DVec3::new(-4., 0., 5.),
    ];
    for i in 0..4 {
        h.app
            .doc
            .model
            .add_edge(corners[i], corners[(i + 1) % 4])
            .unwrap();
    }
    h.key(Key::R);
    h.click(h.at(DVec3::new(-1., 0., 1.)));
    assert!(h.app.drawing.as_ref().unwrap().normal.dot(DVec3::Y).abs() > 0.99);
    let end = h.at(DVec3::new(1., 0., 3.));
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    let preview = h.app.drawing_points();
    assert!(preview.iter().all(|point| point.y.abs() < 1e-6));
    h.click(end);
    assert_eq!(h.app.doc.model.edges.len(), 8);
    h.app.doc.model.validate().unwrap();
}

#[test]
fn click_line_precision_and_axis_lock_through_real_ui() {
    let mut h = Harness::new();
    h.key(Key::L);
    let start = h.at(DVec3::ZERO);
    h.click(start);
    assert_eq!(h.app.line_start, Some(DVec3::ZERO));
    h.key(Key::ArrowUp);
    assert_eq!(h.app.lock, Some(Axis::Z));
    let end = h.at(DVec3::Z * 3.);
    h.frame(
        vec![
            egui::Event::PointerMoved(end),
            egui::Event::Text("2.5".into()),
        ],
        egui::Modifiers::NONE,
    );
    h.key(Key::Enter);
    assert_eq!(h.app.doc.model.edges.len(), 1);
    let e = h.app.doc.model.edges.values().next().unwrap();
    let (a, b) = h.app.doc.model.points(e);
    assert!(a.distance(DVec3::ZERO) < 1e-7);
    assert!(b.distance(DVec3::Z * 2.5) < 1e-5);
    h.key(Key::Escape);
    assert!(h.app.line_start.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 1);
}

#[test]
fn live_preview_and_click_snap_every_segment_in_a_line_chain() {
    let mut h = Harness::new();
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    for (index, axis) in [Axis::X, Axis::Y, Axis::Z].into_iter().enumerate() {
        let start = h.app.line_start.unwrap();
        let screen_start = h.at(start);
        let direction = (h.at(start + axis.vector()) - screen_start).normalized();
        // Include a short second segment inside the old 16-pixel dead zone.
        let distance = if index == 1 { 9. } else { 75. };
        let cursor =
            screen_start + direction * distance + egui::vec2(-direction.y, direction.x) * 0.5;
        h.frame(
            vec![egui::Event::PointerMoved(cursor)],
            egui::Modifiers::NONE,
        );
        let preview = h.app.snap.unwrap();
        assert_eq!(preview.kind, Inference::Axis(axis));
        assert!((preview.point - start).cross(axis.vector()).length() < 1e-6);
        assert!(preview.point.distance(start) > 1e-3);
        assert_eq!(
            h.app.doc.model.edges.len(),
            index,
            "Moving the preview must not create an edge"
        );
        h.click(cursor);
        assert!(
            h.app.line_start.unwrap().distance(preview.point) < 1e-6,
            "Click must apply the preview's snapped point"
        );
        assert_eq!(h.app.doc.model.edges.len(), index + 1);
    }
}

#[test]
fn perpendicular_preview_and_click_acquire_nearby_existing_edge() {
    let mut h = Harness::new();
    let start = DVec3::X * 4.;
    h.app.doc.model.add_edge(DVec3::ZERO, start).unwrap();
    h.app
        .doc
        .model
        .add_edge(DVec3::new(4.15, 1., 0.), DVec3::new(4.15, 7., 0.))
        .unwrap();
    h.key(Key::L);
    h.click(h.at(start));
    let target = DVec3::new(4.15, 3., 0.);
    let cursor = h.at(target);
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    let preview = h.app.snap.unwrap();
    assert_eq!(preview.kind, Inference::OnEdge);
    assert!(preview.point.distance(target) < 1e-5);
    h.click(cursor);
    assert!(h.app.line_start.unwrap().distance(preview.point) < 1e-6);
    assert_eq!(h.app.doc.model.edges.len(), 4);
    h.app.doc.model.validate().unwrap();
}

#[test]
fn precision_commit_uses_current_pointer_even_when_it_moved_in_the_same_frame() {
    let mut h = Harness::new();
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    let start = h.app.line_start.unwrap();
    h.frame(
        vec![egui::Event::PointerMoved(h.at(start + DVec3::X * 3.))],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.snap.unwrap().kind, Inference::Axis(Axis::X));
    let current = h.at(start + DVec3::Y * 3.);
    h.frame(
        vec![
            egui::Event::PointerMoved(current),
            egui::Event::Text("2".into()),
            egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.doc.model.edges.len(), 1);
    assert!(h.app.line_start.unwrap().distance(start + DVec3::Y * 2.) < 1e-6);
}

#[test]
fn navigation_clears_hover_and_freezes_tool_inference_until_release() {
    for tool in [
        Tool::Select,
        Tool::Line,
        Tool::Freehand,
        Tool::Erase,
        Tool::Paint,
    ] {
        let mut h = Harness::new();
        let edge = h.app.doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
        h.app.set_tool(tool);
        let start = h.at(DVec3::ZERO);
        h.frame(
            vec![egui::Event::PointerMoved(start)],
            egui::Modifiers::NONE,
        );
        h.app.hover = Some(edge);
        h.pointer(start, egui::PointerButton::Middle, true, false);
        assert!(h.app.hover.is_none(), "{tool:?}");
        let snap = h.app.snap.map(|s| s.point);
        let line_start = h.app.line_start;
        let before = h.app.doc.camera.clone();
        let end = start + egui::vec2(12., 8.);
        h.frame(
            vec![
                egui::Event::PointerMoved(start + egui::vec2(4., 2.)),
                egui::Event::PointerMoved(end),
            ],
            egui::Modifiers::NONE,
        );
        let mut expected = before;
        expected.orbit_around(DVec2::new(12., 8.), DVec3::ZERO);
        assert_eq!(h.app.doc.camera, expected);
        h.frame(vec![], egui::Modifiers::NONE);
        assert!(h.app.hover.is_none(), "{tool:?}");
        assert!(h.app.flip_hover.is_none());
        assert_eq!(h.app.snap.map(|s| s.point), snap);
        assert_eq!(h.app.line_start, line_start);
        h.pointer(end, egui::PointerButton::Middle, false, false);
        assert_eq!(h.app.tool, tool);
        assert_eq!(h.app.doc.model.edges.len(), 1);

        h.app.set_tool(Tool::Select);
        h.frame(
            vec![egui::Event::PointerMoved(h.at(DVec3::ZERO))],
            egui::Modifiers::NONE,
        );
        assert_eq!(h.app.hover, Some(edge));
        h.key(Key::O);
        h.frame(
            vec![egui::Event::PointerMoved(h.at(DVec3::ZERO))],
            egui::Modifiers::NONE,
        );
        assert!(h.app.hover.is_none());
    }
}

#[test]
fn drawing_modifier_orbits_without_mouse_buttons_and_preserves_pending_points() {
    for modifiers in [
        egui::Modifiers::CTRL,
        egui::Modifiers::MAC_CMD,
        egui::Modifiers::COMMAND,
    ] {
        for tool in Tool::DRAWING {
            let mut h = Harness::new();
            h.app.set_tool(tool);
            let start = h.at(DVec3::ZERO);
            if tool != Tool::Freehand {
                h.click(start);
            }
            h.frame(
                vec![egui::Event::PointerMoved(start)],
                egui::Modifiers::NONE,
            );
            let line_start = h.app.line_start;
            let points = h.app.drawing_points();
            let snap = h.app.snap.map(|s| s.point);
            let before = h.app.doc.camera.clone();
            h.frame(vec![], modifiers);
            assert!(h.app.navigating(), "{tool:?}: {modifiers:?}");
            assert_eq!(h.app.doc.camera, before);
            let end = start + egui::vec2(12., 8.);
            h.frame(
                vec![
                    egui::Event::PointerMoved(start + egui::vec2(4., 2.)),
                    egui::Event::MouseMoved(egui::vec2(4., 2.)),
                    egui::Event::PointerMoved(end),
                ],
                modifiers,
            );
            let mut expected = before;
            expected.orbit_around(DVec2::new(12., 8.), DVec3::ZERO);
            assert_eq!(h.app.doc.camera, expected);
            assert!(h.app.gesture.is_none());
            assert_eq!(h.app.snap.map(|s| s.point), snap);
            assert_eq!(h.app.tool, tool);
            assert_eq!(h.app.line_start, line_start);
            assert_eq!(h.app.drawing_points(), points);
            assert!(h.app.doc.model.edges.is_empty());
            h.frame(vec![], egui::Modifiers::NONE);
            assert!(!h.app.navigating());
            h.frame(
                vec![egui::Event::PointerMoved(end + egui::vec2(6., 4.))],
                egui::Modifiers::NONE,
            );
            assert_eq!(h.app.doc.camera, expected);

            // The next normal drawing gesture uses the preserved starting point.
            let target = h.at(DVec3::new(2., 3., 0.));
            if tool == Tool::Freehand {
                h.pointer(start, egui::PointerButton::Primary, true, false);
                h.frame(
                    vec![egui::Event::PointerMoved(target)],
                    egui::Modifiers::NONE,
                );
                h.pointer(target, egui::PointerButton::Primary, false, false);
            } else {
                h.click(target);
            }
            assert!(!h.app.doc.model.edges.is_empty(), "{tool:?}: {modifiers:?}");
            h.app.doc.model.validate().unwrap();
        }
    }
}

#[test]
fn drawing_modifier_moves_camera_once_with_either_mouse_button_held() {
    for button in [egui::PointerButton::Primary, egui::PointerButton::Middle] {
        let mut h = Harness::new();
        h.key(Key::L);
        let start = h.app.viewport.center();
        h.pointer_with_modifiers(start, button, true, egui::Modifiers::CTRL);
        let before = h.app.doc.camera.clone();
        let end = start + egui::vec2(12., 8.);
        h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::CTRL);
        let mut expected = before;
        expected.orbit_around(DVec2::new(12., 8.), DVec3::ZERO);
        assert_eq!(h.app.doc.camera, expected);
        h.pointer_with_modifiers(end, button, false, egui::Modifiers::CTRL);
        assert_eq!(h.app.doc.camera, expected);
        assert!(h.app.doc.model.edges.is_empty());
    }
}

#[test]
fn drawing_modifier_does_not_orbit_over_panels_or_jump_on_reentry() {
    let mut h = Harness::new();
    h.key(Key::L);
    let start = h.app.viewport.center();
    h.frame(
        vec![egui::Event::PointerMoved(start)],
        egui::Modifiers::NONE,
    );
    let end = start + egui::vec2(12., 8.);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::CTRL);
    let before = h.app.doc.camera.clone();
    let panel = egui::pos2(490., 32.);
    h.frame(
        vec![egui::Event::PointerMoved(panel)],
        egui::Modifiers::CTRL,
    );
    assert_eq!(h.app.doc.camera, before);
    h.frame(
        vec![egui::Event::PointerMoved(start)],
        egui::Modifiers::CTRL,
    );
    assert_eq!(h.app.doc.camera, before);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::CTRL);
    let mut expected = before;
    expected.orbit_around(DVec2::new(12., 8.), DVec3::ZERO);
    assert_eq!(h.app.doc.camera, expected);
    h.frame(vec![egui::Event::PointerGone], egui::Modifiers::CTRL);
    h.frame(
        vec![egui::Event::PointerMoved(start)],
        egui::Modifiers::CTRL,
    );
    assert_eq!(h.app.doc.camera, expected);
}

#[test]
fn drawing_modifier_release_stops_orbit_without_placing_a_point() {
    for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::MAC_CMD] {
        let mut h = Harness::new();
        h.key(Key::L);
        h.click(h.at(DVec3::ZERO));
        h.key(Key::ArrowUp);
        h.app.length_input = "2.5".into();
        let start = h.app.viewport.center();
        h.pointer_with_modifiers(start, egui::PointerButton::Primary, true, modifiers);
        let end = start + egui::vec2(2., 1.);
        h.frame(vec![egui::Event::PointerMoved(end)], modifiers);
        let camera = h.app.doc.camera.clone();
        h.frame(vec![], egui::Modifiers::NONE);
        assert!(!h.app.navigating());
        let release = end + egui::vec2(1., 0.);
        h.frame(
            vec![egui::Event::PointerMoved(release)],
            egui::Modifiers::NONE,
        );
        h.pointer(release, egui::PointerButton::Primary, false, false);
        assert_eq!(h.app.doc.camera, camera);
        assert!(h.app.doc.model.edges.is_empty());
        assert_eq!(h.app.line_start, Some(DVec3::ZERO));
        assert_eq!(h.app.lock, Some(Axis::Z));
        assert_eq!(h.app.length_input, "2.5");
    }
}

#[test]
fn pressing_drawing_modifier_during_a_point_click_cancels_placement() {
    for tool in [Tool::Line, Tool::Rectangle, Tool::Circle, Tool::Polygon] {
        for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::MAC_CMD] {
            let mut h = Harness::new();
            h.app.set_tool(tool);
            h.click(h.at(DVec3::ZERO));
            let start = h.at(DVec3::new(2., 3., 0.));
            h.pointer(start, egui::PointerButton::Primary, true, false);
            let before = h.app.doc.camera.clone();
            let end = start + egui::vec2(2., 1.);
            h.frame(vec![egui::Event::PointerMoved(end)], modifiers);
            assert_ne!(h.app.doc.camera, before);
            h.frame(vec![], egui::Modifiers::NONE);
            h.pointer(end, egui::PointerButton::Primary, false, false);
            assert!(h.app.doc.model.edges.is_empty());
            assert!(h.app.line_start.is_some() || h.app.drawing.is_some());
        }
    }
}

#[test]
fn drawing_modifier_pauses_freehand_sampling_and_resumes_on_release() {
    for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::MAC_CMD] {
        let mut h = Harness::new();
        h.key(Key::D);
        h.pointer(h.at(DVec3::ZERO), egui::PointerButton::Primary, true, false);
        let start = h.at(DVec3::X);
        h.frame(
            vec![egui::Event::PointerMoved(start)],
            egui::Modifiers::NONE,
        );
        let points = h.app.drawing.as_ref().unwrap().points.clone();
        let snap = h.app.snap.map(|s| s.point);
        let before = h.app.doc.camera.clone();
        let end = start + egui::vec2(12., 8.);
        h.frame(vec![egui::Event::PointerMoved(end)], modifiers);
        assert_ne!(h.app.doc.camera, before);
        assert_eq!(h.app.drawing.as_ref().unwrap().points, points);
        assert_eq!(h.app.snap.map(|s| s.point), snap);
        assert!(h.app.doc.model.edges.is_empty());
        h.frame(vec![], egui::Modifiers::NONE);
        let camera = h.app.doc.camera.clone();
        let target = h.at(DVec3::new(2., 3., 0.));
        h.frame(
            vec![egui::Event::PointerMoved(target)],
            egui::Modifiers::NONE,
        );
        assert!(h.app.drawing.as_ref().unwrap().points.len() > points.len());
        assert_eq!(h.app.doc.camera, camera);
        h.pointer(target, egui::PointerButton::Primary, false, false);
        assert!(!h.app.doc.model.edges.is_empty());
        h.app.doc.model.validate().unwrap();
    }
}

#[test]
fn finishing_freehand_after_orbit_does_not_add_the_orbit_endpoint() {
    for release_modifier_first in [false, true] {
        let mut h = Harness::new();
        h.key(Key::D);
        h.pointer(h.at(DVec3::ZERO), egui::PointerButton::Primary, true, false);
        let start = h.at(DVec3::X);
        h.frame(
            vec![egui::Event::PointerMoved(start)],
            egui::Modifiers::NONE,
        );
        let points = h.app.drawing.as_ref().unwrap().points.clone();
        assert_eq!(points.len(), 2);
        let end = start + egui::vec2(60., 30.);
        h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::CTRL);
        let modifiers = if release_modifier_first {
            h.frame(vec![], egui::Modifiers::NONE);
            egui::Modifiers::NONE
        } else {
            egui::Modifiers::CTRL
        };
        h.pointer_with_modifiers(end, egui::PointerButton::Primary, false, modifiers);
        assert_eq!(h.app.doc.model.edges.len(), 1);
        let edge = h.app.doc.model.edges.values().next().unwrap();
        let (a, b) = h.app.doc.model.points(edge);
        assert!(a.distance(points[0]) < 1e-6);
        assert!(b.distance(points[1]) < 1e-6);
    }
}

#[test]
fn drawing_modifier_shift_pans_and_middle_navigation_still_works() {
    let mut h = Harness::new();
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    let before = h.app.doc.camera.clone();
    let start = h.app.viewport.center();
    let modifiers = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
    h.frame(
        vec![egui::Event::PointerMoved(start)],
        egui::Modifiers::NONE,
    );
    h.frame(vec![], modifiers);
    let end = start + egui::vec2(60., 30.);
    h.frame(vec![egui::Event::PointerMoved(end)], modifiers);
    assert_eq!(h.app.doc.camera.yaw, before.yaw);
    assert_eq!(h.app.doc.camera.pitch, before.pitch);
    assert_ne!(h.app.doc.camera.target, before.target);
    assert!(h.app.gesture.is_none());
    h.frame(vec![], egui::Modifiers::NONE);
    h.pointer(end, egui::PointerButton::Middle, true, false);
    let before = h.app.doc.camera.clone();
    let end = end + egui::vec2(12., 8.);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    assert_ne!(h.app.doc.camera.yaw, before.yaw);
    h.pointer(end, egui::PointerButton::Middle, false, false);
    assert_eq!(h.app.line_start, Some(DVec3::ZERO));
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn shift_orbit_pans_and_middle_navigation_preserves_pending_line() {
    let mut h = Harness::new();
    h.key(Key::L);
    let origin = h.at(DVec3::ZERO);
    h.click(origin);
    let before = h.app.doc.camera.clone();
    let target = h.app.line_start;
    let pos = h.app.viewport.center();
    h.pointer(pos, egui::PointerButton::Middle, true, true);
    let next = pos + egui::vec2(60., 30.);
    h.frame(
        vec![egui::Event::PointerMoved(next)],
        egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(h.app.doc.camera.yaw, before.yaw);
    assert_eq!(h.app.doc.camera.pitch, before.pitch);
    assert!(h.app.doc.camera.target.distance(before.target) > 0.1);
    let pivot = DVec3::ZERO;
    let pivot_screen = h.at(pivot);
    let radius = h.app.doc.camera.eye().distance(pivot);
    h.frame(
        vec![egui::Event::PointerMoved(next + egui::vec2(20., 0.))],
        egui::Modifiers::NONE,
    );
    assert_ne!(h.app.doc.camera.yaw, before.yaw);
    assert!(h.at(pivot).distance(pivot_screen) < 0.001);
    assert!((h.app.doc.camera.eye().distance(pivot) - radius).abs() < 1e-6);
    h.pointer(
        next + egui::vec2(20., 0.),
        egui::PointerButton::Middle,
        false,
        false,
    );
    assert_eq!(h.app.tool, Tool::Line);
    assert_eq!(h.app.line_start, target);
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn orbit_tool_shift_drag_only_translates_camera() {
    let mut h = Harness::new();
    h.key(Key::O);
    let before = h.app.doc.camera.clone();
    let p = h.app.viewport.center();
    h.pointer(p, egui::PointerButton::Primary, true, true);
    h.frame(
        vec![egui::Event::PointerMoved(p + egui::vec2(-80., 40.))],
        egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    h.pointer(
        p + egui::vec2(-80., 40.),
        egui::PointerButton::Primary,
        false,
        true,
    );
    assert_eq!(h.app.doc.camera.yaw, before.yaw);
    assert_eq!(h.app.doc.camera.pitch, before.pitch);
    assert_ne!(h.app.doc.camera.target, before.target);
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn orbit_uses_all_objects_center_after_pan_and_zoom() {
    for button in [egui::PointerButton::Primary, egui::PointerButton::Middle] {
        for hidden in [false, true] {
            let mut h = Harness::new();
            if button == egui::PointerButton::Primary {
                h.key(Key::O);
            }
            let corners = [
                DVec3::new(-8., -6., 2.),
                DVec3::new(-2., -6., 2.),
                DVec3::new(-2., 0., 2.),
                DVec3::new(-8., 0., 2.),
            ];
            for i in 0..4 {
                h.app
                    .doc
                    .model
                    .add_edge(corners[i], corners[(i + 1) % 4])
                    .unwrap();
            }
            let group = h
                .app
                .doc
                .model
                .make_object(&h.app.doc.model.root_entities(), false, "Selected panel")
                .unwrap();
            let edge = h
                .app
                .doc
                .model
                .add_edge(DVec3::new(12., 4., 4.), DVec3::new(18., 8., 10.))
                .unwrap();
            let component = h
                .app
                .doc
                .model
                .make_object(&BTreeSet::from([edge]), true, "Other object")
                .unwrap();
            h.app.selected = BTreeSet::from([group]);
            h.app.doc.model.locked.insert(component);
            if hidden {
                h.app.doc.model.hidden.insert(component);
            }
            let pivot = DVec3::new(5., 1., 6.);
            let size = dv(h.app.viewport.size());
            h.app.doc.camera.pan(DVec2::new(150., 90.), size.y);
            h.app.doc.camera.zoom(180., size * 0.7, size);
            let before = h.app.doc.camera.clone();
            assert!(pivot.distance(before.target) > 0.1);
            let pivot_screen = h.at(pivot);
            let radius = before.eye().distance(pivot);
            assert!(pivot_screen.distance(h.app.viewport.center()) > 1.);

            // Off-center pointer positions and the current selection do not set the pivot.
            let start = h.app.viewport.center() + egui::vec2(100., 70.);
            h.pointer(start, button, true, false);
            assert!(h.app.doc.camera.eye().distance(before.eye()) < 1e-6);
            assert!(h.at(pivot).distance(pivot_screen) < 0.001);
            for delta in [
                egui::vec2(0.01, 0.),
                egui::vec2(40., -35.),
                egui::vec2(70., -65.),
            ] {
                h.frame(
                    vec![egui::Event::PointerMoved(start + delta)],
                    egui::Modifiers::NONE,
                );
                assert!(h.at(pivot).distance(pivot_screen) < 0.001);
                assert!((h.app.doc.camera.eye().distance(pivot) - radius).abs() < 1e-6);
                assert_eq!(h.app.doc.camera.distance, before.distance);
                if delta.x < 1. {
                    assert!(h.app.doc.camera.eye().distance(before.eye()) < 0.01);
                }
            }
            assert_ne!(h.app.doc.camera.yaw, before.yaw);
            assert_ne!(h.app.doc.camera.pitch, before.pitch);

            // Releasing Shift must preserve the new framing without a recentering jump.
            let panned = start + egui::vec2(120., -40.);
            h.frame(
                vec![egui::Event::PointerMoved(panned)],
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            );
            let panned_screen = h.at(pivot);
            let panned_radius = h.app.doc.camera.eye().distance(pivot);
            assert!(panned_screen.distance(pivot_screen) > 1.);
            let end = panned + egui::vec2(20., 0.);
            h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
            assert!(h.at(pivot).distance(panned_screen) < 0.001);
            assert!((h.app.doc.camera.eye().distance(pivot) - panned_radius).abs() < 1e-6);
            assert_eq!(h.app.doc.camera.distance, before.distance);
            h.pointer(end, button, false, false);
        }
    }
}

#[test]
fn orbit_center_tracks_geometry_changes_and_uses_origin_for_empty_models() {
    let mut h = Harness::new();
    h.app.doc.camera.pitch = -0.4;
    h.app.doc.camera.target = DVec3::new(3., 4., 20.);
    let before = h.app.doc.camera.clone();
    assert_eq!(h.app.orbit_center(), DVec3::ZERO);
    assert_eq!(h.app.doc.camera.target, before.target);
    assert_eq!(h.app.doc.camera.distance, before.distance);
    assert_eq!(h.app.doc.camera.yaw, before.yaw);
    assert_eq!(h.app.doc.camera.pitch, before.pitch);

    let first = h
        .app
        .doc
        .model
        .add_edge(DVec3::new(2., 4., 6.), DVec3::new(4., 8., 10.))
        .unwrap();
    assert_eq!(h.app.orbit_center(), DVec3::new(3., 6., 8.));
    h.app
        .doc
        .model
        .add_edge(DVec3::new(10., 12., 14.), DVec3::new(12., 16., 18.))
        .unwrap();
    assert_eq!(h.app.orbit_center(), DVec3::new(7., 10., 12.));
    h.app.doc.model.erase(&BTreeSet::from([first]));
    assert_eq!(h.app.orbit_center(), DVec3::new(11., 14., 16.));
    assert_eq!(h.app.doc.camera.target, before.target);
}

#[test]
fn eraser_sweep_is_one_undo_step_and_ui_panels_never_draw() {
    let mut h = Harness::new();
    h.app
        .doc
        .model
        .add_edge(-DVec3::X * 2., DVec3::X * 2.)
        .unwrap();
    h.app
        .doc
        .model
        .add_edge(DVec3::Y - DVec3::X * 2., DVec3::Y + DVec3::X * 2.)
        .unwrap();
    h.key(Key::E);
    let a = h.at(-DVec3::Y);
    let b = h.at(DVec3::Y * 2.);
    h.pointer(a, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(b)], egui::Modifiers::NONE);
    h.pointer(b, egui::PointerButton::Primary, false, false);
    assert!(h.app.doc.model.edges.is_empty());
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edges.len(), 2);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edges.len(), 2);
    h.app.set_tool(Tool::Line);
    h.click(egui::pos2(1300., 500.));
    assert!(h.app.line_start.is_none());
}

#[test]
fn model_name_is_read_only_and_does_not_capture_tool_shortcuts() {
    let mut h = Harness::new();
    let name = h.app.doc.name.clone();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, &name));
    assert!(!h.ctx.egui_wants_keyboard_input());
    h.key(Key::L);
    assert_eq!(h.app.tool, Tool::Line);
    h.frame(
        vec![egui::Event::Text("Sketch".into())],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.doc.name, name);
    assert_eq!(h.app.tool, Tool::Line);
}

#[test]
fn closing_shortcuts_preserves_unfinished_drawing_and_blocks_file_commands() {
    let mut h = Harness::new();
    h.app.set_tool(Tool::Line);
    h.app.line_start = Some(DVec3::X);
    h.app.lock = Some(Axis::Y);
    h.app.length_input = "2m".into();
    h.key(Key::F1);
    assert!(h.app.help);
    h.frame(
        vec![egui::Event::Key {
            key: Key::N,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        egui::Modifiers::COMMAND,
    );
    assert!(h.app.project.confirm.is_none());
    assert_eq!(h.app.line_start, Some(DVec3::X));
    h.key(Key::Escape);
    assert!(!h.app.help);
    assert_eq!(h.app.line_start, Some(DVec3::X));
    assert_eq!(h.app.lock, Some(Axis::Y));
    assert_eq!(h.app.length_input, "2m");
}

#[test]
fn window_focus_loss_cancels_gesture_without_erasing_geometry() {
    let mut h = Harness::new();
    h.app.doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
    h.key(Key::E);
    let p = h.at(DVec3::ZERO);
    h.pointer(p, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerGone], egui::Modifiers::NONE);
    assert!(h.app.gesture.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 1);
}

#[test]
#[ignore = "Requires a GPU adapter; writes target/qa/*.png for visual inspection"]
fn gpu_renders_empty_and_modeled_workspace() {
    render_snapshots(gpu_test_state(), false);
}

#[test]
#[ignore = "Requires a GPU adapter; renders and compares every display setting independently"]
fn gpu_renders_individual_display_options() {
    render_snapshots(gpu_test_state(), true);
}

fn gpu_test_state() -> eframe::egui_wgpu::RenderState {
    use eframe::{egui_wgpu, wgpu};
    use std::sync::Arc;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU adapter");
    eprintln!("GPU test adapter: {:?}", adapter.get_info());
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = egui_wgpu::Renderer::new(
        &device,
        format,
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(wgpu::TextureFormat::Depth32Float),
            ..Default::default()
        },
    );
    egui_wgpu::RenderState {
        adapter,
        available_adapters: vec![],
        instance,
        device,
        queue,
        target_format: format,
        renderer: Arc::new(egui::epaint::mutex::RwLock::new(renderer)),
    }
}

#[test]
fn close_snapped_outline_then_select_and_delete_face_or_edge() {
    let mut h = Harness::new();
    assert!(h.app.doc.snap_to_geometry);
    h.key(Key::L);
    for point in [
        DVec3::ZERO,
        DVec3::new(4., 0., 0.),
        DVec3::new(4., 3., 0.),
        DVec3::new(0., 3., 0.),
    ] {
        h.click(h.at(point));
    }
    h.click(h.at(DVec3::ZERO) + egui::vec2(3., 2.));
    assert_eq!(h.app.doc.model.faces.len(), 1);
    assert_eq!(h.app.doc.model.edges.len(), 4);
    let face = *h.app.doc.model.faces.keys().next().unwrap();
    h.key(Key::Space);
    h.click(h.at(DVec3::new(2., 1.5, 0.)));
    assert_eq!(h.app.selected, BTreeSet::from([face]));
    h.key(Key::Delete);
    assert!(h.app.doc.model.faces.is_empty());
    assert_eq!(h.app.doc.model.edges.len(), 4);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.faces.len(), 1);
    h.click(h.at(DVec3::new(2., 0., 0.)));
    assert_eq!(h.app.selected.len(), 1);
    assert!(
        h.app
            .doc
            .model
            .edges
            .contains_key(h.app.selected.first().unwrap())
    );
    h.key(Key::Delete);
    assert_eq!(h.app.doc.model.edges.len(), 3);
    assert!(h.app.doc.model.faces.is_empty());
}

#[test]
fn snap_checkbox_starts_checked_and_disables_geometry_magnet() {
    let mut h = Harness::new();
    assert!(h.app.doc.snap_to_geometry);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let checkbox = text_position(&output, "Snap to objects");
    h.click(checkbox);
    assert!(
        !h.app.doc.snap_to_geometry,
        "Snap to objects checkbox should toggle off at {checkbox:?}"
    );
    h.app
        .doc
        .model
        .add_edge(DVec3::new(0., 0., 2.), DVec3::new(6., 0., 2.))
        .unwrap();
    h.key(Key::L);
    let pointer = h.at(DVec3::new(1.5, 0., 2.));
    h.frame(
        vec![egui::Event::PointerMoved(pointer)],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.snap.unwrap().kind, Inference::Plane);
    h.click(checkbox);
    assert!(h.app.doc.snap_to_geometry);
    h.frame(
        vec![egui::Event::PointerMoved(pointer)],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.snap.unwrap().kind, Inference::OnEdge);
}

#[test]
fn eraser_hovers_and_erases_face_interiors_without_removing_boundary_edges() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    let before = h.app.doc.model.clone();
    h.key(Key::E);
    let cursor = h.at(DVec3::new(2., 2., 0.));
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.hover, Some(face));
    h.click(cursor);
    assert!(!h.app.doc.model.faces.contains_key(&face));
    assert_eq!(h.app.doc.model.edges, before.edges);
    assert_eq!(h.app.doc.model.vertices, before.vertices);
    assert_eq!(h.app.history.undo_label(), Some("Erase geometry"));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    h.app.undo(&h.ctx, true);
    assert!(!h.app.doc.model.faces.contains_key(&face));
    assert_eq!(h.app.doc.model.edges, before.edges);
}

#[test]
fn eraser_stroke_within_a_face_previews_and_commits_one_undo_step() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    let before = h.app.doc.model.clone();
    h.key(Key::E);
    let start = h.at(DVec3::new(1., 2., 0.));
    let end = h.at(DVec3::new(3., 2., 0.));
    h.pointer(start, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    let GestureKind::Erase { ids } = &h.app.gesture.as_ref().unwrap().kind else {
        panic!("The erase stroke should remain active until release");
    };
    assert_eq!(ids, &BTreeSet::from([face]));
    assert_eq!(h.app.doc.model, before);
    h.pointer(end, egui::PointerButton::Primary, false, false);
    assert!(h.app.doc.model.faces.is_empty());
    assert_eq!(h.app.doc.model.edges, before.edges);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.history.undo_label().is_none());
}

#[test]
fn eraser_face_targets_respect_hidden_and_locked_objects() {
    for grouped in [false, true] {
        for hidden in [false, true] {
            let mut h = Harness::new();
            let face = modeling_rectangle(&mut h);
            let target = if grouped {
                h.app
                    .doc
                    .model
                    .make_object(&BTreeSet::from([face]), false, "Group")
                    .unwrap()
            } else {
                face
            };
            if hidden {
                h.app.doc.model.hidden.insert(target);
            } else {
                h.app.doc.model.locked.insert(target);
            }
            let before = h.app.doc.model.clone();
            h.key(Key::E);
            let cursor = h.at(DVec3::new(2., 2., 0.));
            h.frame(
                vec![egui::Event::PointerMoved(cursor)],
                egui::Modifiers::NONE,
            );
            assert_eq!(h.app.hover, None);
            h.click(cursor);
            assert_eq!(h.app.doc.model, before);
            assert!(h.app.history.undo_label().is_none());
        }
    }
}

#[test]
fn eraser_does_not_delete_an_edge_hidden_behind_a_filled_face() {
    let mut h = Harness::new();
    let square = [
        DVec3::new(-3., -3., 2.),
        DVec3::new(3., -3., 2.),
        DVec3::new(3., 3., 2.),
        DVec3::new(-3., 3., 2.),
    ];
    for i in 0..4 {
        h.app
            .doc
            .model
            .add_edge(square[i], square[(i + 1) % 4])
            .unwrap();
    }
    let cursor = h.at(DVec3::Z * 2.);
    let hidden = h
        .app
        .doc
        .camera
        .ray(dv(cursor - h.app.viewport.min), dv(h.app.viewport.size()))
        .plane_z(0.)
        .unwrap();
    let hidden_edge = h
        .app
        .doc
        .model
        .add_edge(hidden - DVec3::X, hidden + DVec3::X)
        .unwrap();
    let before = h.app.doc.model.clone();
    h.key(Key::E);
    h.click(cursor);
    assert!(h.app.doc.model.faces.is_empty());
    assert!(h.app.doc.model.edges.contains_key(&hidden_edge));
    assert_eq!(h.app.doc.model.edges, before.edges);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

fn render_snapshots(state: eframe::egui_wgpu::RenderState, audit: bool) {
    use eframe::wgpu;
    let ctx = egui::Context::default();
    apply(&ctx);
    let mut cc = eframe::CreationContext::_new_kittest(ctx.clone());
    cc.wgpu_render_state = Some(state.clone());
    assert!(renderer::install(&cc));
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/qa");
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut time = 0.;
    let snapshots: Vec<String> = if audit {
        feature_audit::render_cases()
            .into_iter()
            .map(|c| c.name)
            .collect()
    } else {
        [
            "empty",
            "modeled",
            "face-selected",
            "axis-preview",
            "push-pull-preview",
            "painted",
            "context-menu",
            "component-dialog",
            "paste-preview",
            "group-edit",
            "nested-edit",
            "move-alignment",
            "rectangle-preview",
            "drawing-menu",
            "measure-tape",
            "measure-angle",
            "dimensions",
            "measurement-menu",
            "solid-menu",
            "solid-union",
            "solid-split",
            "flip-planes",
            "sidebar",
            "display-section",
            "display-xray",
            "compact",
            "axis-in-front",
            "axis-behind",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    };
    let mut previous_audit: Option<(String, Vec<u8>)> = None;
    for snapshot in snapshots {
        let snapshot = snapshot.as_str();
        if audit {
            app = feature_audit::render_workspace(snapshot);
        }
        if matches!(snapshot, "axis-in-front" | "axis-behind") {
            app = KetchupApp::with_document(Document::default(), None, true);
            app.doc.display.grid = false;
            app.doc.camera.target = DVec3::Y * 3.;
            let toward_eye = (app.doc.camera.eye() - app.doc.camera.target).normalize();
            let right = DVec3::Z.cross(toward_eye).normalize();
            let up = toward_eye.cross(right);
            // Cover the same screen point with a face either behind or in
            // front of the Y axis. Both occlusion directions must work.
            let center = app.doc.camera.target
                + toward_eye * if snapshot == "axis-in-front" { -1. } else { 1. };
            let corners = [
                center - right - up,
                center + right - up,
                center + right + up,
                center - right + up,
            ];
            for i in 0..4 {
                app.doc
                    .model
                    .add_edge(corners[i], corners[(i + 1) % 4])
                    .unwrap();
            }
        }
        if snapshot == "modeled" {
            app.doc.name = "Courtyard study".into();
            let corners = [
                DVec3::new(-3., -2., 0.),
                DVec3::new(3., -2., 0.),
                DVec3::new(3., 2., 0.),
                DVec3::new(-3., 2., 0.),
            ];
            for i in 0..4 {
                let a = corners[i];
                let b = corners[(i + 1) % 4];
                app.doc.model.add_edge(a, b).unwrap();
                app.doc
                    .model
                    .add_edge(a + DVec3::Z * 3., b + DVec3::Z * 3.)
                    .unwrap();
                app.doc.model.add_edge(a, a + DVec3::Z * 3.).unwrap();
            }
            let ridge_a = DVec3::new(-3., 0., 5.);
            let ridge_b = DVec3::new(3., 0., 5.);
            app.doc.model.add_edge(ridge_a, ridge_b).unwrap();
            for (i, ridge) in [(0, ridge_a), (3, ridge_a), (1, ridge_b), (2, ridge_b)] {
                app.doc
                    .model
                    .add_edge(corners[i] + DVec3::Z * 3., ridge)
                    .unwrap();
            }
            app.selected
                .insert(*app.doc.model.edges.keys().next().unwrap());
            app.doc.camera.target = DVec3::new(0., 0., 2.);
            app.doc.camera.distance = 19.;
        }
        if snapshot == "face-selected" {
            let point = app
                .doc
                .camera
                .project(DVec3::new(0., -2., 1.5), dv(app.viewport.size()))
                .unwrap();
            let (id, _, _) = app
                .doc
                .model
                .nearest_face(app.doc.camera.ray(point, dv(app.viewport.size())))
                .unwrap();
            app.selected = BTreeSet::from([id]);
        }
        if snapshot == "axis-preview" {
            app.set_tool(Tool::Line);
            app.selected.clear();
            let start = DVec3::new(3., -2., 3.);
            app.line_start = Some(start);
            let cursor = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(start + DVec3::Z * 1.8, dv(app.viewport.size()))
                    .unwrap())
                + egui::vec2(2., 0.);
            app.update_pointer(cursor);
            assert_eq!(app.preview_alignment(), Some(Axis::Z));
        }
        if snapshot == "push-pull-preview" {
            app.doc = Document::default();
            let corners = [
                DVec3::new(-2., -2., 0.),
                DVec3::new(2., -2., 0.),
                DVec3::new(2., 2., 0.),
                DVec3::new(-2., 2., 0.),
            ];
            for i in 0..4 {
                app.doc
                    .model
                    .add_edge(corners[i], corners[(i + 1) % 4])
                    .unwrap();
            }
            app.set_tool(Tool::PushPull);
            app.selected.clear();
            let at = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(DVec3::ZERO, dv(app.viewport.size()))
                    .unwrap());
            app.begin_modeling(at, &ctx);
            let end = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(DVec3::Z * 2., dv(app.viewport.size()))
                    .unwrap());
            app.update_modeling(end);
            assert_eq!(
                app.modeling
                    .as_ref()
                    .unwrap()
                    .preview
                    .as_ref()
                    .unwrap()
                    .faces
                    .len(),
                6
            );
        }
        if snapshot == "painted" {
            let preview = app.modeling.take().unwrap().preview.unwrap();
            app.doc.model = preview;
            app.set_tool(Tool::Paint);
            app.selected.clear();
            for id in app.doc.model.faces.keys().copied().collect::<Vec<_>>() {
                app.doc.model.paint_face(id, [195, 96, 67]).unwrap();
            }
        }
        if snapshot == "context-menu" {
            app.set_tool(Tool::Select);
            app.selected = app.doc.model.selectable_entities();
            app.create_object(&ctx, false, "House");
        }
        if snapshot == "component-dialog" {
            egui::Popup::close_all(&ctx);
            app.object_creation = Some(ObjectCreation {
                component: true,
                name: "House component".into(),
            });
        }
        if snapshot == "paste-preview" {
            app.object_creation = None;
            app.context_action(&ctx, ContextAction::Copy);
            app.context_action(&ctx, ContextAction::Paste);
        }
        if snapshot == "group-edit" {
            app.doc = Document::default();
            app.doc.name = "Nested assemblies".into();
            app.set_tool(Tool::Select);
            let corners = [
                DVec3::new(-2., -2., 0.),
                DVec3::new(2., -2., 0.),
                DVec3::new(2., 2., 0.),
                DVec3::new(-2., 2., 0.),
            ];
            for i in 0..4 {
                app.doc
                    .model
                    .add_edge(corners[i], corners[(i + 1) % 4])
                    .unwrap();
            }
            let panel = app
                .doc
                .model
                .make_object(&app.doc.model.selectable_entities(), false, "Panel")
                .unwrap();
            let assembly = app
                .doc
                .model
                .make_object(&BTreeSet::from([panel]), true, "Assembly")
                .unwrap();
            let copy = app.doc.model.copy_entities(&BTreeSet::from([assembly]));
            app.doc.model.paste_entities(&copy, DVec3::X * 6.);
            app.doc.camera.target = DVec3::X * 2.;
            app.doc.camera.distance = 21.;
            app.enter_object(assembly, &ctx);
        }
        if snapshot == "nested-edit" {
            let assembly = app.doc.model.edit_context.unwrap();
            let panel = *app.doc.model.objects[&assembly]
                .entities
                .iter()
                .find(|id| app.doc.model.objects.contains_key(id))
                .unwrap();
            app.enter_object(panel, &ctx);
            let face = *app
                .doc
                .model
                .object_contents(panel)
                .unwrap()
                .faces
                .keys()
                .next()
                .unwrap();
            app.selected = BTreeSet::from([face]);
        }
        if snapshot == "move-alignment" {
            app.doc = Document::default();
            app.doc.camera.target = DVec3::new(0., 1., 2.);
            let start = DVec3::new(-4., -3., 2.);
            let moving = app
                .doc
                .model
                .add_edge(start, start + DVec3::X * 0.5)
                .unwrap();
            for reference in [DVec3::new(0., 4., 2.), DVec3::new(3., 0., 2.)] {
                app.doc
                    .model
                    .add_edge(reference, reference + DVec3::new(0.31, 0.27, 0.19))
                    .unwrap();
            }
            app.selected = BTreeSet::from([moving]);
            app.set_tool(Tool::Move);
            let anchor = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(start, dv(app.viewport.size()))
                    .unwrap());
            app.begin_modeling(anchor, &ctx);
        }
        if snapshot == "rectangle-preview" {
            app.doc = Document::default();
            app.set_tool(Tool::Rectangle);
            app.notice = None;
            app.selected.clear();
            let start = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(DVec3::new(-2., -1.5, 0.), dv(app.viewport.size()))
                    .unwrap());
            app.begin_drawing(start);
            let end = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(DVec3::new(2., 1.5, 0.), dv(app.viewport.size()))
                    .unwrap());
            app.update_pointer(end);
        }
        if snapshot == "drawing-menu" {
            app.drawing = None;
        }
        if snapshot == "measurement-menu" {
            egui::Popup::close_all(&ctx);
            app.set_tool(Tool::Dimensions);
        }
        if snapshot.starts_with("measure-") || snapshot == "dimensions" {
            egui::Popup::close_all(&ctx);
            app.doc = Document::default();
            app.doc.name = "Measured study".into();
            app.doc.camera.target = DVec3::new(2., 1., 0.);
            app.inspector = true;
            app.notice = None;
            app.selected.clear();
            let points = [
                DVec3::ZERO,
                DVec3::X * 4.,
                DVec3::new(4., 3., 0.),
                DVec3::Y * 3.,
            ];
            for i in 0..4 {
                app.doc
                    .model
                    .add_edge(points[i], points[(i + 1) % 4])
                    .unwrap();
            }
            let at = |point| {
                app.viewport.min
                    + pv(app
                        .doc
                        .camera
                        .project(point, dv(app.viewport.size()))
                        .unwrap())
            };
            let clicks = if snapshot == "measure-tape" {
                vec![at(points[0]), at(points[2])]
            } else {
                vec![at(points[0]), at(points[1]), at(points[3])]
            };
            if snapshot == "dimensions" {
                app.set_tool(Tool::Dimensions);
                let id = app
                    .doc
                    .model
                    .add_dimension(points[0], points[1], DVec3::new(0., -1.2, 0.))
                    .unwrap();
                app.doc
                    .model
                    .add_dimension(points[1], points[2], DVec3::X * 1.2)
                    .unwrap();
                app.selected = BTreeSet::from([id]);
            } else {
                app.set_tool(if snapshot == "measure-tape" {
                    Tool::TapeMeasure
                } else {
                    Tool::Protractor
                });
                for click in clicks {
                    app.measurement_click(click, &ctx);
                }
            }
        }
        if snapshot.starts_with("solid-") || snapshot == "flip-planes" {
            egui::Popup::close_all(&ctx);
            app.doc = Document::default();
            app.doc.name = "Solid tools study".into();
            app.doc.camera.target = DVec3::new(2., 1., 1.);
            app.doc.camera.distance = 16.;
            app.inspector = true;
            app.notice = None;
            let a = ui_solid_box(&mut app.doc.model, DVec3::ZERO, DVec3::new(3., 3., 2.));
            let b = ui_solid_box(
                &mut app.doc.model,
                DVec3::new(1.5, -1., 0.),
                DVec3::new(4.5, 2., 3.),
            );
            for (id, color) in [(a, [190, 85, 62]), (b, [84, 133, 160])] {
                let faces: Vec<_> = app
                    .doc
                    .model
                    .expand_entities(&BTreeSet::from([id]))
                    .into_iter()
                    .filter(|id| app.doc.model.faces.contains_key(id))
                    .collect();
                for face in faces {
                    app.doc.model.paint_face(face, color).unwrap();
                }
            }
            app.selected = BTreeSet::from([a, b]);
            if snapshot == "solid-menu" {
                app.set_tool(Tool::Union);
            } else if snapshot == "flip-planes" {
                app.selected = BTreeSet::from([a]);
                app.start_solid_tool(Tool::Flip, &ctx);
                app.flip_hover = Some(Axis::X);
            } else {
                app.start_solid_tool(
                    if snapshot == "solid-split" {
                        Tool::Split
                    } else {
                        Tool::Union
                    },
                    &ctx,
                );
                if snapshot == "solid-split" {
                    for id in app.selected.clone() {
                        let offset = match app.doc.model.objects[&id].name.as_str() {
                            "Split A" => DVec3::new(-1., 0., 0.),
                            "Split B" => DVec3::new(1., 0., 0.),
                            _ => DVec3::new(0., -1., 0.),
                        };
                        app.doc
                            .model
                            .move_selection(&BTreeSet::from([id]), offset)
                            .unwrap();
                    }
                }
                assert_eq!(
                    app.selected.len(),
                    if snapshot == "solid-split" { 3 } else { 1 }
                );
                app.selected.clear();
            }
        }
        if matches!(snapshot, "sidebar" | "display-section" | "display-xray") {
            egui::Popup::close_all(&ctx);
            app.set_tool(Tool::Select);
            app.inspector = true;
            app.selected.clear();
            app.notice = None;
            if snapshot == "sidebar" {
                app.doc = Document::default();
                app.doc.name = "Component collection".into();
                let object = ui_solid_box(&mut app.doc.model, DVec3::ZERO, DVec3::new(2., 2., 3.));
                let component = app
                    .doc
                    .model
                    .make_object(&BTreeSet::from([object]), true, "Cabinet")
                    .unwrap();
                let copy = app.doc.model.copy_entities(&BTreeSet::from([component]));
                let second = app.doc.model.paste_entities(&copy, DVec3::X * 4.);
                let tag = app.doc.model.add_tag("Furniture").unwrap();
                app.doc
                    .model
                    .assign_tag(&BTreeSet::from([component]), Some(tag))
                    .unwrap();
                app.doc.model.assign_tag(&second, Some(tag)).unwrap();
                app.doc.model.add_tag("Architecture").unwrap();
                app.doc.camera.target = DVec3::new(3., 1., 1.);
                app.doc.camera.distance = 16.;
            }
            app.doc.display.shadows = true;
            app.doc.display.section_planes = snapshot == "display-section";
            app.doc.display.section_cuts = snapshot == "display-section";
            app.doc.display.section_height = 1.5;
            app.doc.display.x_ray = snapshot == "display-xray";
            app.doc.display.back_edges = snapshot == "display-xray";
        }
        if snapshot == "compact" {
            app.set_tool(Tool::Select);
            app.inspector = false;
        }
        let (width, height) = if snapshot == "compact" {
            (900_u32, 600_u32)
        } else {
            (1440_u32, 900_u32)
        };
        let device = &state.device;
        let queue = &state.queue;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("KetchUp QA screenshot"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("QA depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        for frame in 0..4 {
            time += 0.1;
            app.dirty_since = None; // Snapshot edits must not write a user's saved model.
            let events = if snapshot == "drawing-menu" && frame < 2 {
                let point = egui::pos2(38., 169.);
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 0,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else if matches!(snapshot, "solid-menu" | "measurement-menu") && frame < 2 {
                let point = egui::pos2(38., if snapshot == "solid-menu" { 781. } else { 713. });
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 0,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else if matches!(snapshot, "solid-union" | "solid-split" | "flip-planes") {
                vec![egui::Event::PointerMoved(egui::pos2(1100., 90.))]
            } else if snapshot == "context-menu" && frame < 2 {
                let point = app.viewport.min
                    + pv(app
                        .doc
                        .camera
                        .project(DVec3::new(0., -2., 1.5), dv(app.viewport.size()))
                        .unwrap());
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Secondary,
                        pressed: frame == 0,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else if snapshot == "paste-preview" {
                let point = app.viewport.min
                    + pv(app
                        .doc
                        .camera
                        .project(DVec3::new(6., 0., 0.), dv(app.viewport.size()))
                        .unwrap());
                vec![egui::Event::PointerMoved(point)]
            } else if snapshot == "move-alignment" {
                let cursor = app.viewport.min
                    + pv(app
                        .doc
                        .camera
                        .project(DVec3::new(3., 4., 2.), dv(app.viewport.size()))
                        .unwrap())
                    + egui::vec2(2., -2.);
                vec![egui::Event::PointerMoved(cursor)]
            } else {
                vec![]
            };
            let cpu_start = std::time::Instant::now();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(width as f32, height as f32),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| app.ui(ui, &mut eframe::Frame::_new_kittest()),
            );
            if audit && frame == 3 {
                eprintln!(
                    "DISPLAY_CPU {snapshot} {:.3} ms",
                    cpu_start.elapsed().as_secs_f64() * 1000.
                );
            }
            let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
            let screen = eframe::egui_wgpu::ScreenDescriptor {
                size_in_pixels: [width, height],
                pixels_per_point: output.pixels_per_point,
            };
            let mut renderer = state.renderer.write();
            for (id, deltas) in &output.textures_delta.set {
                for delta in deltas {
                    renderer.update_texture(device, queue, *id, delta);
                }
            }
            let mut encoder = device.create_command_encoder(&Default::default());
            let buffers = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
            {
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("QA scene"),
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
            }
            queue.submit(buffers.into_iter().chain([encoder.finish()]));
            for id in &output.textures_delta.free {
                renderer.free_texture(id);
            }
            output.textures_delta.clear();
        }
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("QA readback"),
            size: stride as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap()
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })
            .unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        let rgba: Vec<u8> = mapped
            .chunks(stride as usize)
            .flat_map(|row| row[..width as usize * 4].iter().copied())
            .collect();
        if audit {
            let (pair, index) = snapshot.rsplit_once('-').unwrap();
            if index == "0" {
                previous_audit = Some((pair.into(), rgba.clone()));
            } else {
                let (previous_pair, before) = previous_audit.take().unwrap();
                assert_eq!(previous_pair, pair);
                let mut changed = 0;
                for y in app.viewport.top().ceil() as usize..app.viewport.bottom().floor() as usize
                {
                    for x in
                        app.viewport.left().ceil() as usize..app.viewport.right().floor() as usize
                    {
                        let offset = (y * width as usize + x) * 4;
                        changed +=
                            usize::from(rgba[offset..offset + 3] != before[offset..offset + 3]);
                    }
                }
                assert!(changed > 0, "{pair} did not change any viewport pixels");
                eprintln!("DISPLAY_RENDER {pair} changed={changed}");
            }
        }
        if snapshot == "face-selected" {
            let point = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(DVec3::new(0., -2., 1.5), dv(app.viewport.size()))
                    .unwrap());
            let offset = (point.y as usize * width as usize + point.x as usize) * 4;
            assert_eq!(
                &rgba[offset..offset + 3],
                // The selected highlight is shaded by the view's key light.
                &[192, 159, 142],
                "Selected face must render a filled interior"
            );
        }
        if matches!(snapshot, "axis-in-front" | "axis-behind") {
            let point = app.viewport.min
                + pv(app
                    .doc
                    .camera
                    .project(DVec3::Y * 3., dv(app.viewport.size()))
                    .unwrap());
            let green_axis = (-2..=2).any(|dy| {
                (-2..=2).any(|dx| {
                    let x = point.x.round() as i32 + dx;
                    let y = point.y.round() as i32 + dy;
                    let offset = (y as usize * width as usize + x as usize) * 4;
                    let [r, g, b] =
                        [rgba[offset], rgba[offset + 1], rgba[offset + 2]].map(i32::from);
                    g > r + 12 && g > b + 12
                })
            });
            assert_eq!(
                green_axis,
                snapshot == "axis-in-front",
                "{snapshot}: Y axis must respect face depth"
            );
        }
        assert!(
            rgba.chunks(4)
                .any(|pixel| pixel[0] < 100 && pixel[1] < 100 && pixel[2] < 100),
            "UI should contain visible dark content"
        );
        let file = std::fs::File::create(out_dir.join(format!("{snapshot}.png"))).unwrap();
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
        drop(mapped);
        buffer.unmap();
    }
}

fn text_position(output: &egui::FullOutput, label: &str) -> Pos2 {
    fn locate(shape: &egui::Shape, label: &str) -> Option<Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| locate(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| locate(&shape.shape, label))
        .unwrap_or_else(|| panic!("Missing UI text: {label}"))
}

fn menu_text_position(output: &egui::FullOutput, label: &str) -> Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.job.text == label && (76.0..400.0).contains(&text.pos.x) =>
            {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing submenu text: {label}"))
}

fn assert_menu_icon(output: &egui::FullOutput, label: &str) {
    let center = menu_text_position(output, label);
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.job.text == label
                    && (text.pos.y - center.y).abs() < 16.
                    && text.pos.x > 76. =>
            {
                Some(text)
            }
            _ => None,
        })
        .expect("Menu label must be painted");
    let icon_rect =
        Rect::from_center_size(egui::pos2(text.pos.x - 20., center.y), egui::vec2(30., 30.));
    assert!(
        output.shapes.iter().any(|shape| {
            !matches!(shape.shape, egui::Shape::Text(_) | egui::Shape::Noop)
                && icon_rect.contains_rect(shape.shape.visual_bounding_rect())
                && shape.shape.visual_bounding_rect().is_positive()
        }),
        "Missing icon beside {label}"
    );
}

fn open_context_menu(h: &mut Harness, point: Pos2) -> egui::FullOutput {
    // Let egui retire the hit-test layer of a previously closed popup/window.
    h.frame(vec![], egui::Modifiers::NONE);
    h.frame(vec![], egui::Modifiers::NONE);
    h.pointer(point, egui::PointerButton::Secondary, true, false);
    h.pointer(point, egui::PointerButton::Secondary, false, false);
    h.frame(vec![], egui::Modifiers::NONE)
}

#[test]
fn menus_block_background_hover_and_restore_it_after_closing() {
    for context_menu in [true, false] {
        let mut h = Harness::new();
        let menu = if context_menu {
            let point = h.app.viewport.center();
            open_context_menu(&mut h, point)
        } else {
            h.click(egui::pos2(38., 169.));
            h.frame(vec![], egui::Modifiers::NONE)
        };
        let target = if context_menu {
            text_position(&menu, "Delete")
        } else {
            menu_text_position(&menu, Tool::Circle.label())
        };
        assert!(h.app.viewport.contains(target));
        let point = h
            .app
            .doc
            .camera
            .ray(dv(target - h.app.viewport.min), dv(h.app.viewport.size()))
            .plane_z(0.)
            .unwrap();
        let edge = h
            .app
            .doc
            .model
            .add_edge(point - DVec3::X, point + DVec3::X)
            .unwrap();
        h.app.picking_preparations = 0;
        h.frame(
            vec![egui::Event::PointerMoved(target)],
            egui::Modifiers::NONE,
        );
        assert_eq!(h.app.hover, None);
        assert_eq!(h.app.picking_preparations, 0);

        // Covering an existing highlight must also clear it without mouse motion.
        h.app.hover = Some(edge);
        h.app.flip_hover = Some(Axis::X);
        h.frame(vec![], egui::Modifiers::NONE);
        assert_eq!(h.app.hover, None);
        assert_eq!(h.app.flip_hover, None);

        h.key(Key::Escape);
        h.frame(vec![], egui::Modifiers::NONE);
        h.frame(
            vec![egui::Event::PointerMoved(target)],
            egui::Modifiers::NONE,
        );
        assert_eq!(h.app.hover, Some(edge));
    }
}

#[test]
fn context_menu_preserves_multiselection_groups_and_ungroups_through_pointer_input() {
    let mut h = Harness::new();
    let a = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    let b = h
        .app
        .doc
        .model
        .add_edge(DVec3::Y * 2., DVec3::new(2., 2., 0.))
        .unwrap();
    h.app.selected = BTreeSet::from([a, b]);
    let point = h.at(DVec3::X);
    let menu = open_context_menu(&mut h, point);
    assert_eq!(h.app.selected, BTreeSet::from([a, b]));
    for label in [
        "Make Group…",
        "Create Component…",
        "Ungroup",
        "Explode",
        "Cut",
        "Copy",
        "Paste",
        "Paste in Place",
        "Delete",
        "Hide",
        "Lock",
    ] {
        text_position(&menu, label);
    }
    h.click(text_position(&menu, "Make Group…"));
    assert!(h.app.doc.model.objects.is_empty());
    assert_eq!(h.app.selected, BTreeSet::from([a, b]));
    let creation = h.app.object_creation.as_mut().unwrap();
    assert!(!creation.component);
    h.frame(
        vec![egui::Event::Text("Assembly".into())],
        egui::Modifiers::NONE,
    );
    let dialog = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&dialog, "Make Group");
    h.click(text_position(&dialog, "Create"));
    assert_eq!(h.app.doc.model.objects.len(), 1);
    let group = *h.app.selected.first().unwrap();
    assert_eq!(h.app.doc.model.objects[&group].entities.len(), 2);
    assert_eq!(h.app.doc.model.objects[&group].name, "Assembly");
    let menu = open_context_menu(&mut h, point);
    h.click(text_position(&menu, "Ungroup"));
    assert!(h.app.doc.model.objects.is_empty());
    assert_eq!(h.app.selected.len(), 2);
    assert_eq!(h.app.history.undo_label(), Some("Ungroup"));
}

#[test]
fn context_menu_creates_named_component_and_explodes_it() {
    let mut h = Harness::new();
    h.app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    let point = h.at(DVec3::X);
    let menu = open_context_menu(&mut h, point);
    h.click(text_position(&menu, "Create Component…"));
    assert!(h.app.object_creation.is_some());
    assert!(h.app.doc.model.objects.is_empty());
    h.frame(
        vec![egui::Event::Text("Beam".into())],
        egui::Modifiers::NONE,
    );
    let dialog = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&dialog, "Create"));
    let id = *h.app.selected.first().unwrap();
    assert_eq!(h.app.doc.model.objects[&id].name, "Beam");
    assert!(matches!(
        h.app.doc.model.objects[&id].kind,
        ObjectKind::Component { .. }
    ));
    let menu = open_context_menu(&mut h, point);
    h.click(text_position(&menu, "Explode"));
    assert!(h.app.doc.model.objects.is_empty());
    h.app.doc.model.validate().unwrap();
}

#[test]
fn clipboard_events_support_in_place_paste_click_placement_cut_cancel_and_undo() {
    let mut h = Harness::new();
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    let id = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    h.app.selected.insert(id);
    h.frame(vec![egui::Event::Copy], egui::Modifiers::NONE);
    assert!(h.app.clipboard.is_some());
    h.frame(
        vec![egui::Event::Paste(String::new())],
        egui::Modifiers::SHIFT,
    );
    assert!(h.app.paste_preview.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 2);
    let edge = &h.app.doc.model.edges[h.app.selected.first().unwrap()];
    assert_eq!(h.app.doc.model.points(edge), (DVec3::ZERO, DVec3::X * 2.));
    h.frame(
        vec![egui::Event::Paste(String::new())],
        egui::Modifiers::NONE,
    );
    assert!(h.app.paste_preview.is_some());
    h.key(Key::Escape);
    assert!(h.app.paste_preview.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 2);
    h.frame(
        vec![egui::Event::Paste(String::new())],
        egui::Modifiers::NONE,
    );
    h.click(h.at(DVec3::new(4., 3., 0.)));
    assert!(h.app.paste_preview.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 3);
    let edge = &h.app.doc.model.edges[h.app.selected.first().unwrap()];
    let (a, b) = h.app.doc.model.points(edge);
    assert!(((a + b) * 0.5).distance(DVec3::new(4., 3., 0.)) < 1e-5);
    h.frame(vec![egui::Event::Cut], egui::Modifiers::NONE);
    assert_eq!(h.app.doc.model.edges.len(), 2);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edges.len(), 3);
    h.app.doc.model.validate().unwrap();
}

#[test]
fn context_menu_lock_protects_delete_and_hide_can_be_reversed_from_outliner() {
    let mut h = Harness::new();
    let id = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    let point = h.at(DVec3::X);
    let menu = open_context_menu(&mut h, point);
    h.click(text_position(&menu, "Lock"));
    assert!(h.app.doc.model.locked.contains(&id));
    h.key(Key::Delete);
    h.frame(vec![egui::Event::Cut], egui::Modifiers::NONE);
    assert_eq!(h.app.doc.model.edges.len(), 1);
    let menu = open_context_menu(&mut h, point);
    h.click(text_position(&menu, "Unlock"));
    assert!(h.app.doc.model.locked.is_empty());
    let menu = open_context_menu(&mut h, point);
    h.click(text_position(&menu, "Hide"));
    assert!(h.app.selected.is_empty());
    assert!(h.app.doc.model.hidden.contains(&id));
    let tree = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&tree, "Geometry (1)"));
    h.frame(vec![], egui::Modifiers::NONE);
    let tree = h.frame(vec![], egui::Modifiers::NONE);
    let row = text_position(&tree, &format!("#{id} · Hidden"));
    let menu = open_context_menu(&mut h, row);
    h.click(text_position(&menu, "Unhide"));
    assert!(h.app.doc.model.hidden.is_empty());
    h.click(point);
    assert_eq!(h.app.selected, BTreeSet::from([id]));
}

fn modeling_rectangle(h: &mut Harness) -> u64 {
    let points = [
        DVec3::ZERO,
        DVec3::new(4., 0., 0.),
        DVec3::new(4., 4., 0.),
        DVec3::new(0., 4., 0.),
    ];
    for i in 0..4 {
        h.app
            .doc
            .model
            .add_edge(points[i], points[(i + 1) % 4])
            .unwrap();
    }
    *h.app.doc.model.faces.keys().next().unwrap()
}

fn modeling_number(h: &mut Harness, value: &str) {
    h.frame(vec![egui::Event::Text(value.into())], egui::Modifiers::NONE);
    h.key(Key::Enter);
}

#[test]
fn push_pull_pointer_preview_numeric_commit_cancel_and_undo() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    let original = h.app.doc.model.clone();
    h.key(Key::P);
    let at = h.at(DVec3::new(2., 2., 0.));
    h.click(at);
    assert!(h.app.modeling.is_some());
    let end = h.at(DVec3::new(2., 2., 2.));
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    assert_eq!(
        h.app.doc.model, original,
        "Preview must not change saved geometry"
    );
    let edit = h.app.modeling.as_ref().unwrap();
    assert!(edit.preview.as_ref().unwrap().faces.len() > 1);
    modeling_number(&mut h, "2");
    assert!(h.app.modeling.is_none());
    assert_eq!(h.app.doc.model.faces.len(), 6);
    assert_eq!(h.app.history.undo_label(), Some("Push/Pull"));
    assert_ne!(h.app.selected, BTreeSet::from([face]));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, original);
    h.click(at);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    h.key(Key::Escape);
    assert!(h.app.modeling.is_none());
    assert_eq!(h.app.doc.model, original);
    assert!(h.app.history.undo_label().is_none());
}

#[test]
fn offset_then_push_down_previews_and_commits_an_open_recess() {
    let mut h = Harness::new();
    let rim = modeling_rectangle(&mut h);
    h.key(Key::Q);
    let offset_at = h.at(DVec3::new(2., 0.25, 0.));
    h.click(offset_at);
    modeling_number(&mut h, "0.5");
    let before = h.app.doc.model.clone();
    assert_eq!(before.faces[&rim].holes.len(), 1);

    h.key(Key::P);
    let at = h.at(DVec3::new(2., 2., 0.));
    h.click(at);
    let inner = h.app.modeling.as_ref().unwrap().face.unwrap();
    let end = h.at(DVec3::new(2., 2., -1.));
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    let preview = h.app.modeling.as_ref().unwrap().preview.as_ref().unwrap();
    assert!(!preview.faces.contains_key(&inner));
    let ray = ketchup_core::Ray {
        origin: DVec3::new(2., 2., 10.),
        direction: -DVec3::Z,
    };
    let (_, _, point) = preview.nearest_face(ray).unwrap();
    assert!(point.distance(DVec3::new(2., 2., -1.)) < 1e-6);
    assert_eq!(h.app.doc.model, before);
    h.key(Key::Escape);
    assert_eq!(h.app.doc.model, before);

    h.click(at);
    modeling_number(&mut h, "-1");
    assert!(h.app.modeling.is_none());
    assert!(!h.app.doc.model.faces.contains_key(&inner));
    assert_eq!(h.app.doc.model.faces.len(), 6);
    let (floor, _, point) = h.app.doc.model.nearest_face(ray).unwrap();
    assert!(point.distance(DVec3::new(2., 2., -1.)) < 1e-6);
    assert_eq!(h.app.selected, BTreeSet::from([floor]));
    assert_eq!(h.app.history.undo_label(), Some("Push/Pull"));
    h.app.doc.model.validate().unwrap();
    let after = h.app.doc.model.clone();
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model, after);
}

#[test]
fn move_and_scale_real_pointer_clicks_and_axis_locked_numeric_distance() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    h.app.selected = BTreeSet::from([face]);
    h.key(Key::M);
    let start = h.at(DVec3::new(2., 2., 0.));
    h.click(start);
    h.key(Key::ArrowUp);
    let end = h.at(DVec3::new(2., 2., 2.));
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    modeling_number(&mut h, "3");
    assert!(
        h.app
            .doc
            .model
            .vertices
            .values()
            .all(|v| (v.position.z - 3.).abs() < 1e-6)
    );
    assert_eq!(h.app.history.undo_label(), Some("Move"));
    h.key(Key::S);
    let reference = h.at(DVec3::new(3., 3., 3.));
    h.click(reference);
    modeling_number(&mut h, "0.5");
    assert!((h.app.doc.model.face_area(&h.app.doc.model.faces[&face]) - 4.).abs() < 1e-7);
    assert_eq!(h.app.history.undo_label(), Some("Scale"));
    h.key(Key::M);
    let start = h.at(DVec3::new(2., 2., 3.));
    h.click(start);
    let end = h.at(DVec3::new(4., 2., 3.));
    h.click(end);
    assert!(h.app.modeling.is_none());
    let center = h.app.doc.model.selection_center(&h.app.selected).unwrap();
    assert!((center - DVec3::new(4., 2., 3.)).length() < 1e-6);
}

#[test]
fn offset_and_paint_through_viewport_preserve_history_and_saved_color() {
    let mut h = Harness::new();
    let face = modeling_rectangle(&mut h);
    h.key(Key::Q);
    let at = h.at(DVec3::new(2., 0.25, 0.));
    h.click(at);
    modeling_number(&mut h, "0.5");
    assert!(h.app.modeling.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 8);
    assert_eq!(h.app.history.undo_label(), Some("Offset"));
    h.key(Key::B);
    assert!(h.app.inspector);
    h.app.paint_color = [45, 103, 188];
    let inside = h.at(DVec3::new(2., 2., 0.));
    h.click(inside);
    let colors = h.app.doc.model.face_colors.clone();
    assert_eq!(colors.len(), 1);
    assert_eq!(colors.values().next(), Some(&[45, 103, 188]));
    assert_eq!(h.app.history.undo_label(), Some("Paint face"));
    h.app.undo(&h.ctx, false);
    assert!(h.app.doc.model.face_colors.is_empty());
    h.app.undo(&h.ctx, true);
    assert_eq!(h.app.doc.model.face_colors, colors);
    h.app.doc.model.locked.insert(face);
    h.app.paint_color = [0, 0, 0];
    let border = h.at(DVec3::new(2., 0.25, 0.));
    h.click(border);
    assert!(
        !h.app
            .doc
            .model
            .face_colors
            .values()
            .any(|c| *c == [0, 0, 0])
    );
}

#[test]
fn system_clipboard_geometry_can_be_pasted_into_a_different_window() {
    let mut source = Harness::new();
    source
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    source.app.selected = source.app.doc.model.selectable_entities();
    let copied = source.frame(vec![egui::Event::Copy], egui::Modifiers::NONE);
    let text = copied
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    let mut destination = Harness::new();
    destination.frame(vec![egui::Event::Paste(text)], egui::Modifiers::SHIFT);
    assert_eq!(destination.app.doc.model.edges.len(), 1);
    destination.app.doc.model.validate().unwrap();
    let before = destination.app.doc.model.clone();
    destination.frame(
        vec![egui::Event::Paste("ketchup-geometry-v1\n{}".into())],
        egui::Modifiers::SHIFT,
    );
    assert_eq!(destination.app.doc.model, before);
}

#[test]
fn move_preview_and_commit_combine_two_reference_axes_and_undo_exactly() {
    let mut h = Harness::new();
    let start = DVec3::new(-4., -3., 2.);
    let target = DVec3::new(3., 4., 2.);
    let moving = h
        .app
        .doc
        .model
        .add_edge(start, start + DVec3::X * 0.5)
        .unwrap();
    let vertex = h.app.doc.model.edges[&moving].a;
    for reference in [DVec3::new(0., 4., 2.), DVec3::new(3., 0., 2.)] {
        h.app
            .doc
            .model
            .add_edge(reference, reference + DVec3::new(0.31, 0.27, 0.19))
            .unwrap();
    }
    h.app.selected = BTreeSet::from([moving]);
    let before = h.app.doc.model.clone();
    h.key(Key::M);
    let anchor = h.at(start);
    h.click(anchor);
    let cursor = h.at(target) + egui::vec2(2., -2.);
    h.frame(
        vec![egui::Event::PointerMoved(cursor)],
        egui::Modifiers::NONE,
    );
    let edit = h.app.modeling.as_ref().unwrap();
    assert_eq!(edit.guides.len(), 2);
    assert!(edit.guides.iter().any(|g| g.axis == Axis::X));
    assert!(edit.guides.iter().any(|g| g.axis == Axis::Y));
    assert!((edit.preview.as_ref().unwrap().vertices[&vertex].position - target).length() < 1e-7);
    assert_eq!(h.app.doc.model, before);
    h.click(cursor);
    assert!(h.app.modeling.is_none());
    assert!((h.app.doc.model.vertices[&vertex].position - target).length() < 1e-7);
    assert_eq!(h.app.history.undo_label(), Some("Move"));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

fn double_click_object(h: &mut Harness, world: DVec3) {
    h.time += 0.4;
    let point = h.at(world);
    h.click(point);
    h.click(point);
    h.frame(vec![], egui::Modifiers::NONE);
    h.frame(vec![], egui::Modifiers::NONE);
}

#[test]
fn double_click_enters_nested_objects_and_escape_and_breadcrumb_leave_levels() {
    for component in [false, true] {
        let mut h = Harness::new();
        let edge = h
            .app
            .doc
            .model
            .add_edge(DVec3::ZERO, DVec3::X * 2.)
            .unwrap();
        h.app
            .doc
            .model
            .make_object(&BTreeSet::from([edge]), !component, "Inner")
            .unwrap();
        let outer = h
            .app
            .doc
            .model
            .make_object(&h.app.doc.model.selectable_entities(), component, "Outer")
            .unwrap();
        let inner = *h.app.doc.model.objects[&outer]
            .entities
            .iter()
            .find(|id| h.app.doc.model.objects.contains_key(id))
            .unwrap();
        double_click_object(&mut h, DVec3::X);
        assert_eq!(h.app.doc.model.edit_context, Some(outer));
        assert!(h.app.selected.is_empty());
        h.click(h.at(DVec3::X));
        assert_eq!(h.app.selected, BTreeSet::from([inner]));
        double_click_object(&mut h, DVec3::X);
        assert_eq!(h.app.doc.model.edit_path(), vec![outer, inner]);
        let frame = h.frame(vec![], egui::Modifiers::NONE);
        text_position(&frame, "Outer");
        text_position(&frame, "Inner");
        h.key(Key::Escape);
        assert_eq!(h.app.doc.model.edit_context, Some(outer));
        let frame = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&frame, "Model"));
        assert!(h.app.doc.model.edit_context.is_none());
    }
}

#[test]
fn editing_group_children_supports_line_delete_clipboard_and_undo_without_affecting_outside() {
    let mut h = Harness::new();
    h.app.doc.snap_to_geometry = false;
    h.app.doc.snap_to_axes = false;
    let edge = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    let group = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), false, "Editable")
        .unwrap();
    let outside = h
        .app
        .doc
        .model
        .add_edge(DVec3::Y * 3., DVec3::new(2., 3., 0.))
        .unwrap();
    double_click_object(&mut h, DVec3::X);
    h.key(Key::L);
    h.click(h.at(DVec3::ZERO));
    h.key(Key::ArrowUp);
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::Z))],
        egui::Modifiers::NONE,
    );
    h.click(h.at(DVec3::Z));
    assert_eq!(
        h.app.doc.model.object_contents(group).unwrap().edges.len(),
        2
    );
    assert_eq!(h.app.doc.model.parent(outside), None);
    h.key(Key::Escape); // Cancel the unfinished chain first.
    assert_eq!(h.app.doc.model.edit_context, Some(group));
    h.key(Key::Space);
    h.time += 0.4;
    h.click(h.at(DVec3::Z * 0.5));
    assert_eq!(h.app.selected.len(), 1);
    assert!(!h.app.selected.contains(&group));
    h.frame(vec![egui::Event::Copy], egui::Modifiers::NONE);
    h.frame(
        vec![egui::Event::Paste(String::new())],
        egui::Modifiers::SHIFT,
    );
    assert_eq!(
        h.app.doc.model.object_contents(group).unwrap().edges.len(),
        3
    );
    h.key(Key::Delete);
    assert_eq!(
        h.app.doc.model.object_contents(group).unwrap().edges.len(),
        2
    );
    assert!(h.app.doc.model.edges.contains_key(&outside));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edit_context, Some(group));
    assert_eq!(
        h.app.doc.model.object_contents(group).unwrap().edges.len(),
        3
    );
    h.app.doc.model.validate().unwrap();
}

#[test]
fn double_click_component_allows_push_pull_and_updates_its_other_instance() {
    let mut h = Harness::new();
    let points = [
        DVec3::ZERO,
        DVec3::X * 2.,
        DVec3::new(2., 2., 0.),
        DVec3::Y * 2.,
    ];
    for i in 0..4 {
        h.app
            .doc
            .model
            .add_edge(points[i], points[(i + 1) % 4])
            .unwrap();
    }
    let component = h
        .app
        .doc
        .model
        .make_object(&h.app.doc.model.selectable_entities(), true, "Panel")
        .unwrap();
    let copy = h.app.doc.model.copy_entities(&BTreeSet::from([component]));
    let peer = *h
        .app
        .doc
        .model
        .paste_entities(&copy, DVec3::X * 5.)
        .first()
        .unwrap();
    double_click_object(&mut h, DVec3::new(1., 1., 0.));
    assert_eq!(h.app.doc.model.edit_context, Some(component));
    h.key(Key::P);
    h.click(h.at(DVec3::new(1., 1., 0.)));
    assert!(h.app.modeling.is_some());
    h.frame(
        vec![
            egui::Event::PointerMoved(h.at(DVec3::new(1., 1., 1.))),
            egui::Event::Text("1".into()),
        ],
        egui::Modifiers::NONE,
    );
    h.key(Key::Enter);
    assert_eq!(
        h.app
            .doc
            .model
            .object_contents(component)
            .unwrap()
            .faces
            .len(),
        6
    );
    assert_eq!(
        h.app.doc.model.object_contents(peer).unwrap().faces.len(),
        6
    );
    assert!(
        h.app
            .selected
            .iter()
            .all(|id| h.app.doc.model.parent(*id) == Some(component))
    );
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edit_context, Some(component));
    assert_eq!(
        h.app
            .doc
            .model
            .object_contents(component)
            .unwrap()
            .faces
            .len(),
        1
    );
    assert_eq!(
        h.app.doc.model.object_contents(peer).unwrap().faces.len(),
        1
    );
    h.app.doc.model.validate().unwrap();
}

#[test]
fn locked_objects_cannot_be_opened_and_single_click_outside_closes_the_current_level() {
    let mut h = Harness::new();
    let edge = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    let group = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), false, "Locked")
        .unwrap();
    h.app.doc.model.locked.insert(group);
    double_click_object(&mut h, DVec3::X);
    assert!(h.app.doc.model.edit_context.is_none());
    h.app.doc.model.locked.clear();
    double_click_object(&mut h, DVec3::X);
    assert_eq!(h.app.doc.model.edit_context, Some(group));
    h.time += 0.4;
    h.click(h.at(DVec3::Y * 4.));
    assert!(h.app.doc.model.edit_context.is_none());
}

#[test]
fn outside_click_leaves_one_nested_level_without_selecting_outside_geometry() {
    for component in [false, true] {
        let mut h = Harness::new();
        let edge = h
            .app
            .doc
            .model
            .add_edge(DVec3::ZERO, DVec3::X * 2.)
            .unwrap();
        let inner = h
            .app
            .doc
            .model
            .make_object(&BTreeSet::from([edge]), component, "Inner")
            .unwrap();
        let outer = h
            .app
            .doc
            .model
            .make_object(&BTreeSet::from([inner]), false, "Outer")
            .unwrap();
        let inner = *h.app.doc.model.objects[&outer]
            .entities
            .iter()
            .next()
            .unwrap();
        let outside_point = DVec3::Y * 4. + DVec3::X;
        h.app
            .doc
            .model
            .add_edge(DVec3::Y * 4., DVec3::Y * 4. + DVec3::X * 2.)
            .unwrap();
        double_click_object(&mut h, DVec3::X);
        double_click_object(&mut h, DVec3::X);
        assert_eq!(h.app.doc.model.edit_path(), vec![outer, inner]);
        h.time += 0.4;
        h.click(h.at(DVec3::X));
        assert_eq!(h.app.doc.model.edit_context, Some(inner));
        assert!(!h.app.selected.is_empty());
        h.time += 0.4;
        h.click(h.at(outside_point));
        assert_eq!(h.app.doc.model.edit_context, Some(outer));
        assert!(h.app.selected.is_empty());
        h.click(h.at(outside_point));
        assert!(h.app.doc.model.edit_context.is_none());
        assert!(h.app.selected.is_empty());
    }
}

#[test]
fn shift_click_and_box_selection_outside_keep_the_group_open() {
    let mut h = Harness::new();
    let edge = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 2.)
        .unwrap();
    let group = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), false, "Group")
        .unwrap();
    double_click_object(&mut h, DVec3::X);
    let outside = h.at(DVec3::Y * 4.);
    h.pointer(outside, egui::PointerButton::Primary, true, true);
    h.pointer(outside, egui::PointerButton::Primary, false, true);
    assert_eq!(h.app.doc.model.edit_context, Some(group));
    h.pointer(outside, egui::PointerButton::Primary, true, false);
    h.pointer(
        outside + egui::vec2(30., 30.),
        egui::PointerButton::Primary,
        false,
        false,
    );
    assert_eq!(h.app.doc.model.edit_context, Some(group));
}

#[test]
fn tape_measure_snaps_reads_distance_and_restarts_without_editing_geometry() {
    let mut h = Harness::new();
    let (a, b) = (DVec3::new(0., 0., 2.), DVec3::new(3., 4., 2.));
    h.app.doc.model.add_edge(a, b).unwrap();
    let before = h.app.doc.model.clone();
    h.key(Key::T);
    h.click(h.at(a));
    h.frame(
        vec![egui::Event::PointerMoved(h.at(b))],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.measurement_value().as_deref(), Some("5.000 m"));
    h.click(h.at(b));
    assert!(h.app.measurement.as_ref().unwrap().complete);
    h.frame(
        vec![egui::Event::PointerMoved(h.at(DVec3::X))],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.measurement_value().as_deref(), Some("5.000 m"));
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.history.undo_label().is_none());
    h.click(h.at(a));
    assert_eq!(h.app.measurement.as_ref().unwrap().points.len(), 1);
    h.key(Key::Escape);
    assert!(h.app.measurement.is_none());
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn protractor_uses_three_snapped_points_and_measures_in_three_dimensions() {
    let mut h = Harness::new();
    let center = DVec3::ZERO;
    let baseline = DVec3::Z * 3.;
    let target = DVec3::new(2., 0., 2.);
    h.app.doc.model.add_edge(center, baseline).unwrap();
    h.app.doc.model.add_edge(center, target).unwrap();
    let before = h.app.doc.model.clone();
    h.key(Key::A);
    h.click(h.at(center));
    h.click(h.at(baseline));
    h.frame(
        vec![egui::Event::PointerMoved(h.at(target))],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.measurement_value().as_deref(), Some("45.00°"));
    h.click(h.at(target));
    assert!(h.app.measurement.as_ref().unwrap().complete);
    assert_eq!(h.app.measurement_value().as_deref(), Some("45.00°"));
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.history.undo_label().is_none());
    h.key(Key::M);
    assert!(h.app.measurement.is_none());
}

#[test]
fn dimensions_commit_only_after_placement_and_support_selection_delete_and_undo() {
    let mut h = Harness::new();
    let edge = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 4.)
        .unwrap();
    h.key(Key::I);
    h.click(h.at(DVec3::ZERO));
    h.click(h.at(DVec3::X * 4.));
    assert_eq!(h.app.measurement.as_ref().unwrap().points.len(), 2);
    assert!(h.app.doc.model.dimensions.is_empty());
    let placement = h.at(DVec3::new(2., -1.5, 0.));
    h.click(placement);
    assert!(h.app.measurement.is_none());
    assert_eq!(h.app.doc.model.dimensions.len(), 1);
    assert_eq!(h.app.doc.model.edges.len(), 1);
    assert_eq!(h.app.history.undo_label(), Some("Add dimension"));
    let id = *h.app.doc.model.dimensions.keys().next().unwrap();
    let dimension = &h.app.doc.model.dimensions[&id];
    assert!((dimension.length(&h.app.doc.model) - 4.).abs() < 1e-7);
    assert!(dimension.offset.length() > 0.1);
    let [_, _, a, b] = dimension.layout(&h.app.doc.model);
    h.key(Key::Space);
    h.click(h.at((a + b) * 0.5));
    assert_eq!(h.app.selected, BTreeSet::from([id]));
    h.key(Key::Delete);
    assert!(h.app.doc.model.dimensions.is_empty());
    assert!(h.app.doc.model.edges.contains_key(&edge));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.dimensions.len(), 1);
    h.app
        .doc
        .model
        .scale_selection(&BTreeSet::from([edge]), DVec3::ZERO, 2.)
        .unwrap();
    assert!((h.app.doc.model.dimensions[&id].length(&h.app.doc.model) - 8.).abs() < 1e-7);
}

#[test]
fn measurement_cancellation_and_middle_navigation_preserve_pending_points_and_context() {
    let mut h = Harness::new();
    let edge = h
        .app
        .doc
        .model
        .add_edge(DVec3::ZERO, DVec3::X * 4.)
        .unwrap();
    let group = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), false, "Measured group")
        .unwrap();
    assert!(h.app.doc.model.enter_object(group));
    h.key(Key::I);
    h.click(h.at(DVec3::ZERO));
    let start = h.app.measurement.as_ref().unwrap().points.clone();
    let from = h.app.viewport.center();
    let to = from + egui::vec2(20., 15.);
    h.pointer(from, egui::PointerButton::Middle, true, false);
    h.frame(vec![egui::Event::PointerMoved(to)], egui::Modifiers::NONE);
    h.pointer(to, egui::PointerButton::Middle, false, false);
    assert_eq!(h.app.measurement.as_ref().unwrap().points, start);
    assert_eq!(h.app.tool, Tool::Dimensions);
    h.key(Key::Escape);
    assert!(h.app.measurement.is_none());
    assert_eq!(h.app.doc.model.edit_context, Some(group));
    assert!(h.app.doc.model.dimensions.is_empty());
}

/// Repeatable CPU workload; run explicitly to compare interaction costs.
#[test]
#[ignore]
fn pointer_workload_benchmark() {
    use std::time::Instant;
    for count in [10, 100] {
        let mut h = Harness::new();
        for i in 0..count {
            let x = (i % 10) as f64 * 1.2 - 6.;
            let y = (i / 10) as f64 * 1.2 - 6.;
            let points = [
                DVec3::new(x, y, 0.),
                DVec3::new(x + 1., y, 0.),
                DVec3::new(x + 1., y + 1., 0.),
                DVec3::new(x, y + 1., 0.),
            ];
            for j in 0..4 {
                h.app
                    .doc
                    .model
                    .add_edge(points[j], points[(j + 1) % 4])
                    .unwrap();
            }
        }
        h.app
            .doc
            .camera
            .fit(h.app.doc.model.bounds(), dv(h.app.viewport.size()));
        h.app.set_tool(Tool::Line);
        let positions: Vec<_> = (0..100)
            .map(|i| {
                let t = i as f32 / 100.;
                h.app.viewport.min + h.app.viewport.size() * egui::vec2(0.1 + t * 0.8, 0.37)
            })
            .collect();
        let start = Instant::now();
        for &pos in &positions {
            h.app.update_pointer(pos);
            std::hint::black_box(h.app.snap);
        }
        let pointer = start.elapsed();
        let start = Instant::now();
        for &pos in &positions {
            std::hint::black_box(
                h.frame(vec![egui::Event::PointerMoved(pos)], egui::Modifiers::NONE),
            );
        }
        eprintln!(
            "{count} faces / {} points: pointer {:?}/event, UI {:?}/frame",
            h.app.doc.model.vertices.len(),
            pointer / 100,
            start.elapsed() / 100
        );
    }
}

/// Exercise imported topology without timing its construction or triangulation.
#[test]
#[ignore = "Explicit large-model CPU benchmark; timings depend on hardware"]
fn imported_model_workload_benchmark() {
    use std::time::Instant;
    let mut h = Harness::new();
    h.app.doc.model = crate::picking::tests::triangle_grid(4096);
    h.app.fit(&h.ctx);
    h.frame(vec![], egui::Modifiers::NONE);
    for tool in [Tool::Select, Tool::Line, Tool::Paint, Tool::Orbit] {
        h.app.set_tool(tool);
        let start = Instant::now();
        for i in 0..20 {
            let point = h.app.viewport.min
                + h.app.viewport.size() * egui::vec2(0.2 + i as f32 * 0.025, 0.47);
            std::hint::black_box(h.frame(
                vec![egui::Event::PointerMoved(point)],
                egui::Modifiers::NONE,
            ));
        }
        eprintln!(
            "4096 imported faces, {tool:?}: {:?}/frame",
            start.elapsed() / 20
        );
    }
}

#[test]
#[ignore = "Explicit axes on/off comparison on a grouped import"]
fn axes_workload_benchmark() {
    use std::time::Instant;
    let mut h = Harness::new();
    let mut json = serde_json::to_value(crate::picking::tests::triangle_grid(4096)).unwrap();
    // Imported assemblies commonly have many groups; avoid constructing them
    // with interactive operations so setup is separate from measured frames.
    for i in 0..512u64 {
        let id = 30000 + i;
        let children: Vec<_> = (i * 8..i * 8 + 8)
            .flat_map(|triangle| (4..=7).map(move |entity| triangle * 7 + entity))
            .collect();
        json["objects"][id.to_string()] = serde_json::json!({
            "id": id, "name": "Tile", "kind": "Group", "entities": children
        });
    }
    json["next_id"] = 30512.into();
    h.app.doc.model = serde_json::from_value(json).unwrap();
    h.app.doc.model.validate().unwrap();
    h.app.set_tool(Tool::Orbit);
    h.app.fit(&h.ctx);
    for axes in [false, true] {
        h.app.doc.display.axes = axes;
        h.frame(vec![], egui::Modifiers::NONE);
        let start = Instant::now();
        for _ in 0..10 {
            std::hint::black_box(h.frame(vec![], egui::Modifiers::NONE));
        }
        eprintln!(
            "4096 faces / 512 groups, axes={axes}: {:?}/frame",
            start.elapsed() / 10
        );
    }
}

#[test]
fn every_tool_survives_interrupted_gestures_resize_and_focus_loss() {
    for populated in [false, true] {
        for tool in Tool::ALL {
            let mut h = Harness::new();
            if populated {
                ui_solid_pair(&mut h);
            }
            h.app.set_tool(tool);
            let a = h.app.viewport.center();
            let b = a + egui::vec2(31., -19.);
            h.pointer(a, egui::PointerButton::Primary, true, false);
            h.frame(vec![egui::Event::PointerMoved(b)], egui::Modifiers::NONE);
            h.frame(
                vec![egui::Event::WindowFocused(false)],
                egui::Modifiers::NONE,
            );
            h.frame(vec![egui::Event::PointerGone], egui::Modifiers::NONE);
            h.pointer(b, egui::PointerButton::Primary, false, false);
            h.key(Key::Escape);
            h.frame(
                vec![egui::Event::WindowFocused(true)],
                egui::Modifiers::NONE,
            );
            h.pointer(a, egui::PointerButton::Middle, true, false);
            h.frame(vec![egui::Event::PointerMoved(b)], egui::Modifiers::SHIFT);
            h.pointer(b, egui::PointerButton::Middle, false, false);
            for size in [egui::vec2(800., 600.), egui::vec2(1440., 900.)] {
                h.size = size;
                h.frame(vec![], egui::Modifiers::NONE);
            }
            h.app.set_tool(Tool::Select);
            h.app
                .doc
                .model
                .validate()
                .unwrap_or_else(|error| panic!("{tool:?}, populated={populated}: {error}"));
            assert!(h.app.doc.camera.valid(), "{tool:?}, populated={populated}");
            assert!(h.app.gesture.is_none(), "{tool:?}, populated={populated}");
            let json = serde_json::to_string(&h.app.doc).unwrap();
            Document::from_json(&json).unwrap();
        }
    }
}

#[test]
fn display_styles_and_effects_survive_camera_changes_and_round_trip() {
    let mut h = Harness::new();
    ui_solid_pair(&mut h);
    for style in ketchup_core::FaceStyle::ALL {
        for effects in [false, true] {
            let d = &mut h.app.doc.display;
            d.face_style = style;
            d.x_ray = effects;
            d.hidden_geometry = effects;
            d.hidden_objects = effects;
            d.back_edges = effects;
            d.profiles = effects;
            d.depth_cue = effects;
            d.endpoints = effects;
            d.extensions = effects;
            d.color_by_tag = effects;
            d.section_planes = effects;
            d.section_cuts = effects;
            d.shadows = effects;
            d.fog = effects;
            for pitch in [-1.5, 0., 1.5] {
                h.app.doc.camera.pitch = pitch;
                h.frame(vec![], egui::Modifiers::NONE);
                assert!(h.app.doc.camera.valid());
            }
            let restored =
                Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
            assert_eq!(restored.display, h.app.doc.display);
            restored.model.validate().unwrap();
        }
    }
}

/// Measure Move separately from drawing/picking; timings exclude model creation.
#[test]
#[ignore]
fn move_workload_benchmark() {
    use std::time::Instant;
    for count in [10, 100] {
        let mut h = Harness::new();
        for i in 0..count {
            let x = (i % 10) as f64 * 1.2 - 6.;
            let y = (i / 10) as f64 * 1.2 - 6.;
            let points = [
                DVec3::new(x, y, 0.),
                DVec3::new(x + 1., y, 0.),
                DVec3::new(x + 1., y + 1., 0.),
                DVec3::new(x, y + 1., 0.),
            ];
            let mut edges = BTreeSet::new();
            for j in 0..4 {
                edges.insert(
                    h.app
                        .doc
                        .model
                        .add_edge(points[j], points[(j + 1) % 4])
                        .unwrap(),
                );
            }
            h.app.doc.model.make_object(&edges, false, "Tile").unwrap();
        }
        h.app
            .doc
            .camera
            .fit(h.app.doc.model.bounds(), dv(h.app.viewport.size()));
        h.app.set_tool(Tool::Move);
        h.app.selected = h.app.doc.model.objects.keys().take(1).copied().collect();
        h.app.begin_modeling(h.at(DVec3::new(-6., -6., 0.)), &h.ctx);
        assert!(h.app.modeling.is_some());
        h.app.modeling.as_mut().unwrap().direction = DVec3::X;
        let positions: Vec<_> = (0..40)
            .map(|i| {
                h.app.viewport.min + h.app.viewport.size() * egui::vec2(0.15 + i as f32 / 60., 0.37)
            })
            .collect();
        let edit = h.app.modeling.as_ref().unwrap();
        let start = Instant::now();
        for &pos in &positions {
            std::hint::black_box(infer_move_with_snapping(
                &edit.base,
                &h.app.doc.camera,
                dv(h.app.viewport.size()),
                dv(pos - h.app.viewport.min),
                edit.anchor,
                None,
                true,
                true,
            ));
        }
        let inference = start.elapsed() / 40;
        let start = Instant::now();
        for i in 0..40 {
            std::hint::black_box(
                KetchupApp::modeling_result(Tool::Move, edit, 0.1 + i as f64 * 0.01).unwrap(),
            );
        }
        let transform = start.elapsed() / 40;
        let start = Instant::now();
        for &pos in &positions {
            h.app.update_modeling(pos);
        }
        eprintln!("Prepared Move update: {:?}/event", start.elapsed() / 40);
        let start = Instant::now();
        for &pos in &positions {
            std::hint::black_box(
                h.frame(vec![egui::Event::PointerMoved(pos)], egui::Modifiers::NONE),
            );
        }
        eprintln!(
            "Move {count} objects / {} points: uncached inference {inference:?}, validated transform {transform:?}, UI {:?}/frame",
            h.app.doc.model.vertices.len(),
            start.elapsed() / 40
        );
    }
}

fn begin_move_copy(h: &mut Harness, mac: bool) -> Model {
    h.ctx.set_os(if mac {
        egui::os::OperatingSystem::Mac
    } else {
        egui::os::OperatingSystem::Windows
    });
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::Y).unwrap();
    h.app.selected = BTreeSet::from([edge]);
    let before = h.app.doc.model.clone();
    h.key(Key::M);
    let start = h.at(DVec3::ZERO);
    h.click(start);
    h.key(Key::ArrowRight);
    let end = h.at(DVec3::X * 6.);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    let modifier = if mac {
        egui::Modifiers::ALT
    } else {
        egui::Modifiers::CTRL
    };
    h.frame(vec![], modifier);
    assert!(h.app.move_copy);
    assert!(h.app.modeling.as_ref().unwrap().copy);
    h.frame(vec![], egui::Modifiers::NONE);
    assert!(
        h.app.move_copy,
        "releasing the modifier must keep Copy enabled"
    );
    before
}

fn assert_copy_positions(h: &Harness, expected: &[f64]) {
    let mut positions: Vec<_> = h
        .app
        .doc
        .model
        .edges
        .values()
        .map(|e| h.app.doc.model.vertices[&e.a].position.x)
        .collect();
    positions.sort_by(f64::total_cmp);
    assert_eq!(positions.len(), expected.len());
    for (actual, expected) in positions.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
    h.app.doc.model.validate().unwrap();
}

#[test]
fn move_copy_modifier_toggles_live_preview_on_windows_and_mac() {
    for mac in [false, true] {
        let mut h = Harness::new();
        let before = begin_move_copy(&mut h, mac);
        let modifier = if mac {
            egui::Modifiers::ALT
        } else {
            egui::Modifiers::CTRL
        };
        let preview = h.app.modeling.as_ref().unwrap().preview.as_ref().unwrap();
        assert_eq!(preview.edges.len(), 2);
        assert_eq!(h.app.doc.model, before);
        h.frame(vec![], modifier);
        assert!(!h.app.move_copy);
        assert_eq!(
            h.app
                .modeling
                .as_ref()
                .unwrap()
                .preview
                .as_ref()
                .unwrap()
                .edges
                .len(),
            1
        );
        h.frame(vec![], modifier);
        assert!(
            !h.app.move_copy,
            "holding the modifier must not toggle repeatedly"
        );
        h.frame(vec![], egui::Modifiers::NONE);
        h.frame(vec![], modifier);
        h.frame(vec![], egui::Modifiers::NONE);
        modeling_number(&mut h, "6");
        assert_copy_positions(&h, &[0., 6.]);
        assert_eq!(
            h.app.doc.model.edges[before.edges.keys().next().unwrap()],
            *before.edges.values().next().unwrap()
        );
        assert_eq!(h.app.selected.len(), 1);
        assert!(h.app.selected.iter().all(|id| !before.contains_entity(id)));
        assert_eq!(h.app.history.undo_label(), Some("Move copy"));
        h.app.undo(&h.ctx, false);
        assert_eq!(h.app.doc.model, before);
        h.app.undo(&h.ctx, true);
        assert_copy_positions(&h, &[0., 6.]);
    }
}

#[test]
fn move_copy_arrays_divide_repeat_revise_and_undo_as_one_operation() {
    let mut h = Harness::new();
    let before = begin_move_copy(&mut h, false);
    let end = h.at(DVec3::X * 6.);
    h.click(end);
    assert_copy_positions(&h, &[0., 6.]);
    for (input, positions) in [
        ("/3", vec![0., 2., 4., 6.]),
        ("/2", vec![0., 3., 6.]),
        ("3x", vec![0., 6., 12., 18.]),
        ("x2", vec![0., 6., 12.]),
        ("3/", vec![0., 2., 4., 6.]),
        ("2*", vec![0., 6., 12.]),
        ("*3", vec![0., 6., 12., 18.]),
        ("/1", vec![0., 6.]),
    ] {
        modeling_number(&mut h, input);
        assert_copy_positions(&h, &positions);
        assert!(h.app.length_input.is_empty());
    }
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    assert_eq!(h.app.history.undo_label(), None);
    h.app.undo(&h.ctx, true);
    assert_copy_positions(&h, &[0., 6.]);
    assert!(h.app.last_move_copy.is_none());
}

#[test]
fn move_copy_invalid_arrays_are_atomic_and_escape_keeps_placed_copies() {
    let mut h = Harness::new();
    begin_move_copy(&mut h, false);
    modeling_number(&mut h, "6");
    let placed = h.app.doc.model.clone();
    for input in ["/0", "/-2", "/2.5", "/1001", "/", "2x3", "1e3x", "6"] {
        h.app.length_input.clear();
        modeling_number(&mut h, input);
        assert_eq!(h.app.doc.model, placed, "invalid input {input}");
        assert!(h.app.last_move_copy.is_some());
    }
    h.key(Key::Escape);
    assert_eq!(h.app.doc.model, placed);
    assert!(h.app.last_move_copy.is_none());
    assert!(!h.app.move_copy);
    assert!(h.app.length_input.is_empty());
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edges.len(), 1);
}

#[test]
fn move_copy_cancel_tool_switch_and_other_edits_clear_array_state() {
    let mut h = Harness::new();
    let before = begin_move_copy(&mut h, false);
    h.key(Key::Escape);
    assert_eq!(h.app.doc.model, before);
    assert!(h.app.modeling.is_none());
    assert!(!h.app.move_copy);
    assert_eq!(h.app.history.undo_label(), None);

    let start = h.at(DVec3::ZERO);
    h.click(start);
    h.key(Key::ArrowRight);
    let end = h.at(DVec3::X * 6.);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    h.frame(vec![], egui::Modifiers::CTRL);
    h.frame(vec![], egui::Modifiers::NONE);
    modeling_number(&mut h, "6");
    assert!(h.app.last_move_copy.is_some());
    h.key(Key::Delete);
    assert!(h.app.last_move_copy.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 1);
    h.key(Key::Space);
    assert!(!h.app.move_copy);
}

#[test]
fn move_copy_uses_option_on_mac_and_preserves_clipboard_shortcuts() {
    let mut h = Harness::new();
    h.ctx.set_os(egui::os::OperatingSystem::Mac);
    h.key(Key::M);
    for modifier in [egui::Modifiers::CTRL, egui::Modifiers::MAC_CMD] {
        h.frame(vec![], modifier);
        assert!(!h.app.move_copy);
        h.frame(vec![], egui::Modifiers::NONE);
    }
    h.ctx.set_os(egui::os::OperatingSystem::Windows);
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::Y).unwrap();
    h.app.selected.insert(edge);
    h.frame(vec![egui::Event::Copy], egui::Modifiers::CTRL);
    assert!(!h.app.move_copy);
    assert!(h.app.clipboard.is_some());
}

#[test]
fn move_copy_arrays_preserve_nested_objects_colors_and_component_links() {
    for component in [false, true] {
        let mut h = Harness::new();
        let face = modeling_rectangle(&mut h);
        h.app.doc.model.face_colors.insert(face, [12, 34, 56]);
        let inner = h
            .app
            .doc
            .model
            .make_object(&h.app.doc.model.selectable_entities(), false, "Inner")
            .unwrap();
        let outer = h
            .app
            .doc
            .model
            .make_object(&BTreeSet::from([inner]), component, "Outer")
            .unwrap();
        let before = h.app.doc.model.clone();
        let kind = before.objects[&outer].kind.clone();
        h.app.selected = BTreeSet::from([outer]);
        h.key(Key::M);
        h.frame(vec![], egui::Modifiers::CTRL);
        h.frame(vec![], egui::Modifiers::NONE);
        let anchor = h.at(DVec3::ZERO);
        h.click(anchor);
        h.key(Key::ArrowUp);
        let end = h.at(DVec3::Z * 6.);
        h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
        modeling_number(&mut h, "6");
        modeling_number(&mut h, "/3");
        assert_eq!(h.app.doc.model.root_entities().len(), 4);
        assert_eq!(h.app.doc.model.objects.len(), 8);
        assert_eq!(h.app.doc.model.faces.len(), 4);
        assert!(
            h.app
                .doc
                .model
                .face_colors
                .values()
                .all(|color| *color == [12, 34, 56])
        );
        assert_eq!(h.app.doc.model.face_colors.len(), 4);
        let mut heights = Vec::new();
        for root in h.app.doc.model.root_entities() {
            assert_eq!(h.app.doc.model.objects[&root].kind, kind);
            let children = &h.app.doc.model.objects[&root].entities;
            assert_eq!(children.len(), 1);
            assert!(
                h.app
                    .doc
                    .model
                    .objects
                    .contains_key(children.first().unwrap())
            );
            heights.push(
                h.app
                    .doc
                    .model
                    .selection_center(&BTreeSet::from([root]))
                    .unwrap()
                    .z,
            );
        }
        heights.sort_by(f64::total_cmp);
        assert_eq!(heights, vec![0., 2., 4., 6.]);
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert_eq!(h.app.doc.model, before);
    }
}

#[test]
fn move_copy_arrays_inside_component_stay_in_context_and_update_peers() {
    let mut h = Harness::new();
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::Y).unwrap();
    let component = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), true, "Assembly")
        .unwrap();
    let source = h.app.doc.model.copy_entities(&BTreeSet::from([component]));
    let peer = *h
        .app
        .doc
        .model
        .paste_entities(&source, DVec3::Y * 4.)
        .first()
        .unwrap();
    assert!(h.app.doc.model.enter_object(component));
    h.app.selected = h.app.doc.model.selectable_entities();
    let before = h.app.doc.model.clone();
    h.key(Key::M);
    let anchor = h.at(DVec3::ZERO);
    h.click(anchor);
    h.key(Key::ArrowRight);
    let end = h.at(DVec3::X * 6.);
    h.frame(vec![egui::Event::PointerMoved(end)], egui::Modifiers::NONE);
    h.frame(vec![], egui::Modifiers::CTRL);
    h.frame(vec![], egui::Modifiers::NONE);
    modeling_number(&mut h, "6");
    modeling_number(&mut h, "/3");
    assert_eq!(h.app.doc.model.edit_context, Some(component));
    assert_eq!(h.app.doc.model.objects[&component].entities.len(), 4);
    assert_eq!(h.app.doc.model.objects[&peer].entities.len(), 4);
    assert!(
        h.app
            .selected
            .iter()
            .all(|id| h.app.doc.model.parent(*id) == Some(component))
    );
    assert_eq!(h.app.doc.model.root_entities().len(), 2);
    h.app.doc.model.validate().unwrap();
    h.key(Key::Escape);
    assert_eq!(h.app.doc.model.edit_context, Some(component));
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn move_copy_array_can_be_entered_in_the_measurements_field() {
    let mut h = Harness::new();
    begin_move_copy(&mut h, false);
    modeling_number(&mut h, "6");
    h.click(egui::pos2(h.size.x - 90., h.size.y - 21.));
    assert!(
        h.ctx.egui_wants_keyboard_input(),
        "click should focus the Measurements field"
    );
    modeling_number(&mut h, "/3");
    assert_copy_positions(&h, &[0., 2., 4., 6.]);
    assert!(h.app.length_input.is_empty());
}

fn ui_solid_box(model: &mut Model, min: DVec3, max: DVec3) -> u64 {
    let mut body = Model::default();
    let corners = [
        min,
        DVec3::new(max.x, min.y, min.z),
        DVec3::new(max.x, max.y, min.z),
        DVec3::new(min.x, max.y, min.z),
    ];
    for i in 0..4 {
        body.add_edge(corners[i], corners[(i + 1) % 4]).unwrap();
    }
    let face = *body.faces.keys().next().unwrap();
    let normal = ketchup_core::faces::plane(&body, &body.faces[&face].vertices)
        .unwrap()
        .normal;
    body.push_pull(face, (max.z - min.z) / normal.z).unwrap();
    let entities = model.paste_entities(&body, DVec3::ZERO);
    model.make_object(&entities, false, "Solid box").unwrap()
}
fn ui_solid_pair(h: &mut Harness) -> (u64, u64) {
    let a = ui_solid_box(&mut h.app.doc.model, DVec3::ZERO, DVec3::splat(2.));
    let b = ui_solid_box(
        &mut h.app.doc.model,
        DVec3::new(1., 0., 0.),
        DVec3::new(3., 2., 2.),
    );
    (a, b)
}
#[test]
fn solid_flyout_contains_all_six_tools_and_union_applies_to_selection_with_undo() {
    let mut h = Harness::new();
    let (a, b) = ui_solid_pair(&mut h);
    h.app.selected = BTreeSet::from([a, b]);
    let before = h.app.doc.model.clone();
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Union"));
    let menu = h.frame(vec![], egui::Modifiers::NONE);
    for label in ["Flip", "Outer Shell", "Union", "Intersect", "Trim", "Split"] {
        assert_menu_icon(&menu, label);
    }
    h.click(menu_text_position(&menu, "Union"));
    assert_eq!(h.app.tool, Tool::Union);
    assert!(
        !egui::Popup::is_any_open(&h.ctx),
        "Choosing an operation should close its flyout"
    );
    assert_eq!(h.app.solid_tool, Tool::Union);
    assert_eq!(h.app.doc.model.objects.len(), 1);
    assert_eq!(h.app.history.undo_label(), Some("Union"));
    let result = *h.app.selected.first().unwrap();
    assert!((h.app.doc.model.solid_volume(result).unwrap() - 12.).abs() < 1e-6);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Union"));
    let menu = h.frame(vec![], egui::Modifiers::NONE);
    h.click(menu_text_position(&menu, "Outer Shell"));
    assert_eq!(h.app.solid_tool, Tool::OuterShell);
}
#[test]
fn trim_picks_cutter_then_target_and_cancel_never_edits_geometry() {
    let mut h = Harness::new();
    let (a, b) = ui_solid_pair(&mut h);
    let before = h.app.doc.model.clone();
    h.app.start_solid_tool(Tool::Trim, &h.ctx);
    let cutter = h.at(DVec3::new(0.5, 0., 1.));
    h.click(cutter);
    assert_eq!(h.app.solid_first, Some(a));
    assert_eq!(h.app.doc.model, before);
    h.key(Key::Escape);
    assert!(h.app.solid_first.is_none());
    assert_eq!(h.app.doc.model, before);
    h.click(cutter);
    let target = h.at(DVec3::new(2.5, 0., 1.));
    h.click(target);
    assert!(h.app.doc.model.objects.contains_key(&a));
    assert!(!h.app.doc.model.objects.contains_key(&b));
    assert_eq!(h.app.history.undo_label(), Some("Trim"));
    let result = *h.app.selected.first().unwrap();
    assert!((h.app.doc.model.solid_volume(result).unwrap() - 4.).abs() < 1e-6);
}
#[test]
fn split_and_intersection_keep_the_expected_editable_bodies() {
    for (tool, count, volume) in [(Tool::Split, 3, 12.), (Tool::Intersect, 1, 4.)] {
        let mut h = Harness::new();
        let (a, b) = ui_solid_pair(&mut h);
        h.app.selected = BTreeSet::from([a, b]);
        h.app.start_solid_tool(tool, &h.ctx);
        assert_eq!(h.app.selected.len(), count);
        let actual: f64 = h
            .app
            .selected
            .iter()
            .map(|id| h.app.doc.model.solid_volume(*id).unwrap())
            .sum();
        assert!((actual - volume).abs() < 1e-6);
        h.app.doc.model.validate().unwrap();
    }
}
#[test]
fn intersect_accepts_a_preselected_face_and_group_or_a_face_as_the_first_click() {
    for preselect_both in [false, true] {
        let mut h = Harness::new();
        let object = ui_solid_box(&mut h.app.doc.model, DVec3::ZERO, DVec3::splat(2.));
        let mut sheet = Model::default();
        let points = [
            DVec3::new(-1., -1., 1.),
            DVec3::new(3., -1., 1.),
            DVec3::new(3., 3., 1.),
            DVec3::new(-1., 3., 1.),
        ];
        for i in 0..4 {
            sheet.add_edge(points[i], points[(i + 1) % 4]).unwrap();
        }
        let ids = h.app.doc.model.paste_entities(&sheet, DVec3::ZERO);
        let face = *ids
            .iter()
            .find(|id| h.app.doc.model.faces.contains_key(id))
            .unwrap();
        let before = h.app.doc.model.clone();
        if preselect_both {
            h.app.selected = BTreeSet::from([object, face]);
            h.app.start_solid_tool(Tool::Intersect, &h.ctx);
        } else {
            h.app.start_solid_tool(Tool::Intersect, &h.ctx);
            // Look straight down so the exposed sheet and solid are unambiguous.
            h.app.doc.camera.pitch = 1.55;
            h.app.doc.camera.target = DVec3::new(1., 1., 0.);
            h.click(h.at(DVec3::new(-0.5, 1., 1.)));
            assert_eq!(h.app.solid_first, Some(face));
            assert_eq!(h.app.doc.model, before);
            h.click(h.at(DVec3::new(1., 1., 2.)));
        }
        assert!(h.app.solid_first.is_none());
        assert_eq!(h.app.history.undo_label(), Some("Intersect"));
        let result = h.app.doc.model.copy_entities(&h.app.selected);
        assert_eq!(result.faces.len(), 1);
        assert!(
            (result
                .faces
                .values()
                .map(|f| result.face_area(f))
                .sum::<f64>()
                - 4.)
                .abs()
                < 1e-6
        );
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert_eq!(h.app.doc.model, before);
    }
}
#[test]
fn flip_uses_keyboard_inspector_and_clickable_planes_without_detaching_objects() {
    let mut h = Harness::new();
    let (a, _) = ui_solid_pair(&mut h);
    h.app.selected = BTreeSet::from([a]);
    h.app.start_solid_tool(Tool::Flip, &h.ctx);
    let vertices = h.app.doc.model.selection_vertices(&h.app.selected);
    let before = h.app.doc.model.clone();
    let center = h.app.doc.model.selection_center(&h.app.selected).unwrap();
    h.key(Key::ArrowRight);
    for &id in &vertices {
        let (p, q) = (
            before.vertices[&id].position,
            h.app.doc.model.vertices[&id].position,
        );
        assert!((q - DVec3::new(2. * center.x - p.x, p.y, p.z)).length() < 1e-7);
    }
    assert!(h.app.doc.model.objects.contains_key(&a));
    assert_eq!(h.app.history.undo_label(), Some("Flip X"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Y / green"));
    assert_eq!(h.app.history.undo_label(), Some("Flip Y"));
    let point = h.at(center + DVec3::new(0., 0.6, 0.4));
    let axis = h.app.flip_plane_at(point).unwrap();
    h.click(point);
    assert_eq!(
        h.app.history.undo_label(),
        Some(match axis {
            Axis::X => "Flip X",
            Axis::Y => "Flip Y",
            Axis::Z => "Flip Z",
        })
    );
    h.app.doc.model.validate().unwrap();
}

#[test]
fn dimensions_locked_during_selection_or_placement_keep_three_lines_and_a_label() {
    fn has_label(shape: &egui::Shape, label: &str, at: Pos2) -> bool {
        match shape {
            egui::Shape::Text(text) => {
                text.galley.job.text == label
                    && (text.pos + text.galley.size() * 0.5).distance(at) < 1.
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_label(shape, label, at)),
            _ => false,
        }
    }

    fn collect_lines(shape: &egui::Shape, color: Color32, lines: &mut Vec<[Pos2; 2]>) {
        match shape {
            egui::Shape::LineSegment { points, stroke }
                if stroke.width == 1. && stroke.color == color =>
            {
                lines.push(*points);
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_lines(shape, color, lines);
                }
            }
            _ => {}
        }
    }

    let a = DVec3::new(-1., -1., 1.);
    let diagonal = DVec3::new(2., 3., 3.);
    let placement = DVec3::new(3., -2., 3.);
    for lock_stage in [0, 1, 2] {
        for (key, axis, b, expected, placement) in [
            (Key::ArrowRight, Axis::X, diagonal, 3., placement),
            (Key::ArrowLeft, Axis::Y, diagonal, 4., placement),
            (Key::ArrowUp, Axis::Z, diagonal, 2., placement),
            (
                Key::ArrowRight,
                Axis::X,
                DVec3::new(-1., 2., 5.),
                5.,
                DVec3::new(1., 0.5, 3.),
            ),
            (
                Key::ArrowLeft,
                Axis::Y,
                DVec3::new(2., -1., 5.),
                5.,
                DVec3::new(0.5, 1., 3.),
            ),
            (
                Key::ArrowUp,
                Axis::Z,
                DVec3::new(2., 3., 1.),
                5.,
                DVec3::new(0.5, 1., 3.),
            ),
        ] {
            let mut h = Harness::new();
            h.app.doc.model.add_edge(a, b).unwrap();
            h.key(Key::I);
            if lock_stage == 0 {
                h.key(key);
            }
            h.click(h.at(a));
            if lock_stage == 1 {
                h.key(key);
            }
            let second = h.at(b);
            h.frame(
                vec![egui::Event::PointerMoved(second)],
                egui::Modifiers::NONE,
            );
            assert!(h.app.snap.unwrap().point.distance(b) < 1e-6);
            if lock_stage < 2 {
                assert_eq!(h.app.measurement_value(), Some(format_length(expected)));
            }
            h.click(second);
            if lock_stage == 2 {
                h.key(key);
            }
            assert_eq!(h.app.measurement_value(), Some(format_length(expected)));
            let points = &h.app.measurement.as_ref().unwrap().points;
            assert_eq!(points.len(), 2);
            assert!(points[0].distance(a) < 1e-6);
            assert!(points[1].distance(b) < 1e-6);

            let placement = h.at(placement);
            let preview = h.frame(
                vec![egui::Event::PointerMoved(placement)],
                egui::Modifiers::NONE,
            );
            let mut lines = vec![];
            for shape in &preview.shapes {
                collect_lines(&shape.shape, axis_color(axis), &mut lines);
            }
            assert_eq!(lines.len(), 3);
            assert!(
                lines
                    .iter()
                    .all(|[a, b]| a.is_finite() && b.is_finite() && a.distance(*b) > 1.)
            );
            let label_at = lines[2][0].lerp(lines[2][1], 0.5);
            assert!(preview.shapes.iter().any(|shape| has_label(
                &shape.shape,
                &format_length(expected),
                label_at
            )));

            let placed = h.click(placement);
            let dimension = h.app.doc.model.dimensions.values().next().unwrap();
            let (start, end) = dimension.points(&h.app.doc.model);
            assert!(start.distance(a) < 1e-6);
            assert!(end.distance(b) < 1e-6);
            assert_eq!(dimension.axis, Some(axis));
            assert_eq!(dimension.start.weights.len(), 1);
            assert_eq!(dimension.end.weights.len(), 1);
            assert!((dimension.length(&h.app.doc.model) - expected).abs() < 1e-6);
            if (b - a).dot(axis.vector()).abs() < 1e-7 {
                let [_, _, line_start, line_end] = dimension.layout(&h.app.doc.model);
                assert!((line_start - start).cross(axis.vector()).length() < 1e-6);
                assert!((line_end - end).cross(axis.vector()).length() < 1e-6);
                assert!((line_start - start).length() > 1.);
                assert!((line_end - end).length() > 1.);
            }
            let mut placed_lines = vec![];
            for shape in &placed.shapes {
                collect_lines(&shape.shape, ACCENT, &mut placed_lines);
            }
            assert_eq!(placed_lines, lines);
            assert!(placed.shapes.iter().any(|shape| has_label(
                &shape.shape,
                &format_length(expected),
                label_at
            )));
        }
    }
}

#[test]
fn dimensions_axis_lock_updates_preview_and_placement_without_moving_endpoints() {
    let (a, b) = (DVec3::new(-1., -1., 1.), DVec3::new(2., 3., 3.));
    for (key, axis, expected) in [
        (Key::ArrowRight, Axis::X, 3.),
        (Key::ArrowLeft, Axis::Y, 4.),
        (Key::ArrowUp, Axis::Z, 2.),
    ] {
        let mut h = Harness::new();
        h.app.doc.model.add_edge(a, b).unwrap();
        let before = h.app.doc.model.clone();
        h.key(Key::I);
        h.click(h.at(a));
        h.click(h.at(b));
        let placement = h.at(DVec3::new(3., -2., 3.));
        h.frame(
            vec![egui::Event::PointerMoved(placement)],
            egui::Modifiers::NONE,
        );
        let endpoints = h.app.measurement.as_ref().unwrap().points.clone();
        h.key(key);
        assert_eq!(h.app.lock, Some(axis));
        assert_eq!(h.app.measurement_value(), Some(format_length(expected)));
        assert_eq!(h.app.measurement.as_ref().unwrap().points, endpoints);
        h.click(placement);
        let dimension = h.app.doc.model.dimensions.values().next().unwrap();
        assert_eq!(dimension.axis, Some(axis));
        assert!((dimension.length(&h.app.doc.model) - expected).abs() < 1e-6);
        let [start, end, line_start, line_end] = dimension.layout(&h.app.doc.model);
        assert!((start - a).length() < 1e-6);
        assert!((end - b).length() < 1e-6);
        assert!((line_end - line_start).cross(axis.vector()).length() < 1e-6);
        assert!((line_start - start).dot(axis.vector()).abs() < 1e-6);
        assert!((line_end - end).dot(axis.vector()).abs() < 1e-6);
        assert_eq!(dimension.start.weights.len(), 1);
        assert_eq!(dimension.end.weights.len(), 1);
        h.app.doc.model.validate().unwrap();
        h.app.undo(&h.ctx, false);
        assert_eq!(h.app.doc.model, before);
        h.app.undo(&h.ctx, true);
        assert_eq!(
            h.app.doc.model.dimensions.values().next().unwrap().axis,
            Some(axis)
        );
    }
}

#[test]
fn dimensions_axis_lock_switches_and_clears_during_placement() {
    let mut h = Harness::new();
    let (a, b) = (DVec3::new(-1., -1., 1.), DVec3::new(2., 3., 3.));
    h.app.doc.model.add_edge(a, b).unwrap();
    h.key(Key::I);
    h.click(h.at(a));
    h.click(h.at(b));
    let aligned = h.app.measurement_value();
    h.key(Key::ArrowRight);
    assert_eq!(h.app.measurement_value().as_deref(), Some("3.000 m"));
    h.key(Key::ArrowLeft);
    assert_eq!(h.app.measurement_value().as_deref(), Some("4.000 m"));
    h.key(Key::ArrowUp);
    assert_eq!(h.app.measurement_value().as_deref(), Some("2.000 m"));
    h.key(Key::ArrowUp);
    assert_eq!(h.app.lock, None);
    assert_eq!(h.app.measurement_value(), aligned);
    h.key(Key::ArrowRight);
    h.key(Key::ArrowDown);
    assert_eq!(h.app.lock, None);
    assert_eq!(h.app.measurement_value(), aligned);
    h.click(h.at(DVec3::new(3., -2., 3.)));
    let dimension = h.app.doc.model.dimensions.values().next().unwrap();
    assert_eq!(dimension.axis, None);
    assert!((dimension.length(&h.app.doc.model) - a.distance(b)).abs() < 1e-6);
}

#[test]
fn sidebar_sections_collapse_and_display_controls_change_saved_settings() {
    let mut h = Harness::new();
    // Make all sections accessible in the viewport without scrolling.
    h.size.y = 1600.;
    let output = h.frame(vec![], egui::Modifiers::NONE);
    for title in ["Components", "Tags", "Display"] {
        text_position(&output, title);
    }
    h.click(text_position(&output, "Entity Info"));
    for _ in 0..30 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Axes"));
    assert!(!h.app.doc.display.axes);
    assert!(h.app.project.modified);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let tags_y = text_position(&output, "Tags").y;
    h.click(text_position(&output, "Components"));
    for _ in 0..30 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    // The next section moves up when Components is collapsed.
    assert!(text_position(&output, "Tags").y < tags_y - 30.);
    h.click(text_position(&output, "Components"));
    for _ in 0..30 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&output, "No components yet");
    let saved = Document::from_json(&serde_json::to_string(&h.app.doc).unwrap()).unwrap();
    assert!(!saved.display.axes);
}

#[test]
fn sidebar_headers_reorder_in_memory_and_keep_their_disclosure_state() {
    use sidebar::InspectorSection;

    let mut h = Harness::new();
    h.ctx.style_mut_of(h.ctx.theme(), |s| s.animation_time = 0.);
    h.size.y = 2000.;
    let output = h.frame(vec![], egui::Modifiers::NONE);
    h.click(text_position(&output, "Components"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let from = text_position(&output, "Components");
    let to = text_position(&output, "Outliner") - egui::vec2(0., 8.);
    h.pointer(from, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(to)], egui::Modifiers::NONE);
    h.pointer(to, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.inspector_order[0], InspectorSection::Components);
    assert!(!h.app.project.modified);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    assert!(text_position(&output, "Components").y < text_position(&output, "Outliner").y);
    // The moved section is still collapsed; expanding it still works normally.
    assert!(text_position(&output, "Outliner").y - text_position(&output, "Components").y < 80.);
    h.click(text_position(&output, "Components"));
    let output = h.frame(vec![], egui::Modifiers::NONE);
    text_position(&output, "No components yet");

    let from = text_position(&output, "Components");
    let to = text_position(&output, "Tags") + egui::vec2(0., 8.);
    h.pointer(from, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(to)], egui::Modifiers::NONE);
    h.pointer(to, egui::PointerButton::Primary, false, false);
    assert_eq!(
        h.app.inspector_order,
        [
            InspectorSection::Outliner,
            InspectorSection::EntityInfo,
            InspectorSection::Tags,
            InspectorSection::Components,
            InspectorSection::Display,
            InspectorSection::View,
        ]
    );
    let order = h.app.inspector_order;
    h.app.inspector = false;
    h.frame(vec![], egui::Modifiers::NONE);
    h.app.inspector = true;
    h.app.new_model(&h.ctx);
    h.frame(vec![], egui::Modifiers::NONE);
    assert_eq!(h.app.inspector_order, order);
    let fresh = KetchupApp::with_document(Document::default(), None, true);
    assert_eq!(fresh.inspector_order, InspectorSection::ALL);
}

#[test]
fn sidebar_drag_moves_the_entire_panel_and_animates_its_neighbors_and_drop() {
    let mut h = Harness::new();
    h.size.y = 2000.;
    h.ctx
        .style_mut_of(h.ctx.theme(), |s| s.animation_time = 0.2);
    for _ in 0..30 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let from = text_position(&output, "Components");
    let body_offset = text_position(&output, "No components yet") - from;
    let neighbor_start = text_position(&output, "Outliner").y;
    let to = text_position(&output, "Outliner") + egui::vec2(12., -8.);
    let viewport = h.app.viewport;
    let order = h.app.inspector_order;
    h.pointer(from, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(to)], egui::Modifiers::NONE);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    assert!(text_position(&output, "Components").distance(to) < 1.);
    assert!((text_position(&output, "No components yet") - to - body_offset).length() < 1.);
    let neighbor_first = text_position(&output, "Outliner").y;
    for _ in 0..8 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let neighbor_middle = text_position(&output, "Outliner").y;
    for _ in 0..25 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let neighbor_end = text_position(&output, "Outliner").y;
    assert!(neighbor_first >= neighbor_start);
    assert!(neighbor_middle > neighbor_first + 1.);
    assert!(neighbor_end > neighbor_middle + 1.);
    assert!(text_position(&output, "Components").distance(to) < 1.);
    assert_eq!(h.app.inspector_order, order);
    assert_eq!(h.app.viewport, viewport);
    h.pointer(to, egui::PointerButton::Primary, false, false);
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let dropping = text_position(&output, "Components");
    for _ in 0..30 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let dropped = text_position(&output, "Components");
    assert!(dropping.distance(dropped) > 1.);
    assert!(dropped.y < text_position(&output, "Outliner").y);
    assert!((dropped.x - from.x).abs() < 1.);
    assert!(!h.app.project.modified);
}

#[test]
fn sidebar_header_drag_cancels_on_escape_or_release_outside_the_inspector() {
    let mut h = Harness::new();
    h.size.y = 2000.;
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let from = text_position(&output, "Components");
    let to = text_position(&output, "Outliner") - egui::vec2(0., 8.);
    let order = h.app.inspector_order;
    h.pointer(from, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(to)], egui::Modifiers::NONE);
    h.key(Key::Escape);
    h.pointer(to, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.inspector_order, order);

    for _ in 0..30 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let output = h.frame(vec![], egui::Modifiers::NONE);
    let from = text_position(&output, "Components");
    let outside = h.app.viewport.center();
    h.pointer(from, egui::PointerButton::Primary, true, false);
    h.frame(
        vec![egui::Event::PointerMoved(outside)],
        egui::Modifiers::NONE,
    );
    assert!(egui::DragAndDrop::has_payload_of_type::<
        sidebar::InspectorSection,
    >(&h.ctx));
    h.pointer(outside, egui::PointerButton::Primary, false, false);
    assert_eq!(h.app.inspector_order, order);
}

#[test]
fn expanding_edge_style_keeps_the_inspector_width_stable() {
    let mut h = Harness::new();
    h.ctx.style_mut_of(h.ctx.theme(), |s| s.animation_time = 0.);
    h.size.y = 1600.;
    for _ in 0..3 {
        h.frame(vec![], egui::Modifiers::NONE);
    }
    let right = h.app.viewport.right();
    for _ in 0..2 {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&output, "Edge style"));
        for _ in 0..5 {
            h.frame(vec![], egui::Modifiers::NONE);
            assert!((h.app.viewport.right() - right).abs() < 0.1);
        }
    }
}

#[test]
fn library_placement_creates_linked_instance_supports_cancel_and_undo() {
    let mut h = Harness::new();
    let edge = h.app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let component = h
        .app
        .doc
        .model
        .make_object(&BTreeSet::from([edge]), true, "Beam")
        .unwrap();
    let before = h.app.doc.model.clone();
    h.app.begin_component_placement(component, &h.ctx);
    assert!(h.app.paste_preview.is_some());
    h.key(Key::Escape);
    assert!(h.app.paste_preview.is_none());
    assert_eq!(h.app.doc.model, before);
    h.app.begin_component_placement(component, &h.ctx);
    let at = h.at(DVec3::new(0., 3., 0.));
    h.click(at);
    assert_eq!(h.app.doc.model.objects.len(), 2);
    assert!(
        h.app
            .doc
            .model
            .objects
            .values()
            .all(|o| o.kind == before.objects[&component].kind)
    );
    h.app.doc.model.validate().unwrap();
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model, before);
}

#[test]
fn view_sidebar_buttons_set_projection_and_all_standard_views_without_resizing() {
    let mut h = Harness::new();
    h.ctx.style_mut_of(h.ctx.theme(), |s| s.animation_time = 0.);
    h.size.y = 2600.;
    h.frame(vec![], egui::Modifiers::NONE);
    let width = h.app.viewport.right();
    for title in ["Camera", "Standard views"] {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&output, title));
    }
    for (label, projection) in [
        ("Parallel", ketchup_core::Projection::Parallel),
        ("Perspective", ketchup_core::Projection::Perspective),
    ] {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&output, label));
        assert_eq!(h.app.doc.camera.projection, projection);
    }
    let target = h.app.doc.camera.target;
    let distance = h.app.doc.camera.distance;
    for view in ketchup_core::StandardView::ALL {
        let output = h.frame(vec![], egui::Modifiers::NONE);
        h.click(text_position(&output, view.label()));
        assert_eq!(h.app.doc.camera.standard_view(), Some(view));
        assert_eq!(h.app.doc.camera.target, target);
        assert_eq!(h.app.doc.camera.distance, distance);
        assert!((h.app.viewport.right() - width).abs() < 0.1);
    }
    assert!(h.app.project.modified);
}
