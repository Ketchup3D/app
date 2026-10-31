use super::*;

fn render(app: &mut KetchupApp, ctx: &egui::Context, rect: Rect) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200., 900.))),
            ..Default::default()
        },
        |ui| app.paint_scene(ui, rect),
    );
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
