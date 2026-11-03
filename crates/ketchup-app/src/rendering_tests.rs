use super::*;

fn render(app: &mut KetchupApp, ctx: &egui::Context, rect: Rect) {
    let _ = ctx.run_test_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200., 900.))),
            ..Default::default()
        },
        |ui| app.paint_scene(ui, rect),
    );
}

#[test]
fn source_picking_and_preview_markers_do_not_invalidate_render_or_shadow_geometry() {
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let first = ui_solid_box(&mut app.doc.model, DVec3::ZERO, DVec3::new(0.3, 0.1, 0.04));
    let second = ui_solid_box(&mut app.doc.model, DVec3::new(0.25, 0., 0.), DVec3::new(0.55, 0.1, 0.04));
    app.doc.model.add_connection(ketchup_core::connections::Connection {
        members: [first, second], entry_face: None,
        operation: ketchup_core::connections::ConnectionOperation::Joinery(Default::default()),
    }).unwrap();
    let ctx = egui::Context::default();
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1064., 748.));
    app.doc.display.shadows = true;
    app.doc.display.wood_joinery = true;
    render(&mut app, &ctx, rect);
    let revision = app.render_face_cache.revision();
    let shadow_builds = app.shadow_cache.builds;
    for i in 0..4 {
        app.prepare_picking();
        app.doc.camera.yaw += 0.01;
        render(&mut app, &ctx, rect);
        assert_eq!(app.render_face_cache.revision(), revision);
        assert_eq!(app.shadow_cache.builds, shadow_builds, "hover/orbit must reuse shadows");
        // A preview replaces topology before the next paint. Marker queries
        // must keep their hierarchy paired with its matching face snapshot.
        if i == 3 {
            let mut preview = app.doc.model.clone();
            let member = preview.connections.values().next().unwrap().members[0];
            preview.move_selection(&BTreeSet::from([member]), DVec3::X * 2.).unwrap();
            app.joinery_geometry.update(&preview);
            let _ = ctx.run_test_ui(egui::RawInput {
                events: vec![egui::Event::PointerMoved(rect.center())],
                ..Default::default()
            }, |ui| { app.joint_marker_under_pointer(ui.ctx(), rect); });
        }
    }
}

#[test]
fn unchanged_views_skip_mesh_builds_and_appearance_changes_invalidate_them() {
    let ctx = egui::Context::default();
    ctx.set_theme(egui::Theme::Light);
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let object = ui_solid_box(&mut app.doc.model, DVec3::ZERO, DVec3::splat(2.));
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.));
    render(&mut app, &ctx, rect);
    render(&mut app, &ctx, rect);
    // Moving a pane changes its scissor/viewport, not the normalized mesh.
    render(&mut app, &ctx, rect.translate(egui::vec2(10., 20.)));
    assert_eq!(app.scene_cache.mesh_builds, 1);

    let changes: &[fn(&mut KetchupApp)] = &[
        |app| app.hover = Some(*app.doc.model.objects.keys().next().unwrap()),
        |app| {
            app.selected
                .insert(*app.doc.model.objects.keys().next().unwrap());
        },
        |app| {
            app.outliner_hover
                .insert(*app.doc.model.faces.keys().next().unwrap());
        },
        |app| app.doc.camera.yaw += 0.2,
        |app| app.doc.display.x_ray = !app.doc.display.x_ray,
        |app| {
            app.doc
                .model
                .vertices
                .values_mut()
                .next()
                .unwrap()
                .position
                .x += 0.1
        },
        |app| app.doc.model.faces.clear(),
    ];
    for change in changes {
        let previous = app.scene_cache.mesh_builds;
        change(&mut app);
        render(&mut app, &ctx, rect);
        render(&mut app, &ctx, rect);
        assert_eq!(app.scene_cache.mesh_builds, previous + 1);
    }
    assert!(app.selected.contains(&object));
    let previous = app.scene_cache.mesh_builds;
    ctx.set_theme(egui::Theme::Dark);
    render(&mut app, &ctx, rect);
    assert_eq!(app.scene_cache.mesh_builds, previous + 1);
    render(&mut app, &ctx, rect.shrink(10.));
    assert_eq!(app.scene_cache.mesh_builds, previous + 2);
    ctx.set_pixels_per_point(1.5);
    render(&mut app, &ctx, rect.shrink(10.));
    assert_eq!(app.scene_cache.mesh_builds, previous + 3);

    // A different project can reuse entity IDs while changing the geometry.
    app.doc.model = Model::default();
    let replacement = ui_solid_box(&mut app.doc.model, DVec3::ZERO, DVec3::splat(3.));
    assert_eq!(replacement, object);
    render(&mut app, &ctx, rect.shrink(10.));
    assert_eq!(app.scene_cache.mesh_builds, previous + 4);
}

#[test]
fn transient_previews_bypass_reuse_and_return_to_the_static_path_after_cancel() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    ui_solid_box(&mut app.doc.model, DVec3::ZERO, DVec3::splat(2.));
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.));
    render(&mut app, &ctx, rect);
    app.line_start = Some(DVec3::ZERO);
    render(&mut app, &ctx, rect);
    render(&mut app, &ctx, rect);
    assert_eq!(app.scene_cache.mesh_builds, 3);
    assert!(app.scene_cache.key.is_none());
    app.line_start = None;
    render(&mut app, &ctx, rect);
    render(&mut app, &ctx, rect);
    assert_eq!(app.scene_cache.mesh_builds, 4);
}

#[test]
fn joinery_and_annotations_remain_retained_while_the_outliner_is_open() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let members = [
        ui_solid_box(&mut app.doc.model, DVec3::ZERO, DVec3::new(0.3, 0.1, 0.04)),
        ui_solid_box(&mut app.doc.model, DVec3::new(0.2, 0., 0.), DVec3::new(0.5, 0.1, 0.04)),
    ];
    app.selected = BTreeSet::from(members);
    app.doc.model.add_connection(ketchup_core::connections::Connection {
        members, entry_face: None,
        operation: ketchup_core::connections::ConnectionOperation::Joinery(Default::default()),
    }).unwrap();
    app.doc.display.wood_joinery = true;
    app.doc.model.add_dimension(DVec3::ZERO, DVec3::X, DVec3::Y).unwrap();
    let text = app.doc.model.add_text(DVec3::ZERO, DVec3::Y, "Board").unwrap();
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800., 600.));
    let render_with_outliner = |app: &mut KetchupApp| {
        let _ = ctx.run_test_ui(egui::RawInput::default(), |ui| {
            app.outliner_contents(ui);
            app.paint_scene(ui, rect);
        });
    };
    render_with_outliner(&mut app);
    // Allow an initial font-atlas resize to settle before checking retention.
    render_with_outliner(&mut app);
    let builds = app.scene_cache.mesh_builds;
    for _ in 0..4 { render_with_outliner(&mut app); }
    assert_eq!(app.scene_cache.mesh_builds, builds);
    app.doc.model.texts.get_mut(&text).unwrap().text = "Changed".into();
    render_with_outliner(&mut app);
    assert_eq!(app.scene_cache.mesh_builds, builds + 1);
}
