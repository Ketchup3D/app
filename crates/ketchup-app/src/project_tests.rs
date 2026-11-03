#[cfg(test)]
use crate::test_ui::ContextExt as _;
use super::*;
use crate::project_files::Outcome;

fn deliver(app: &mut KetchupApp, result: Result<Outcome, String>, ctx: &egui::Context) {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.project.pending = Some(receiver);
    sender.send(result).unwrap();
    app.poll_project(ctx);
}

#[test]
fn selection_restores_for_its_project_and_ignores_missing_or_hidden_entities() {
    let ctx = egui::Context::default();
    let mut doc = Document::default();
    let visible = doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let hidden = doc.model.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
    doc.model.hidden.insert(hidden);
    doc.selection = BTreeSet::from([visible, hidden, u64::MAX]);
    let mut app = KetchupApp::with_document(doc, None, true);
    assert_eq!(app.selected, BTreeSet::from([visible]));
    assert_eq!(app.doc.selection, app.selected);

    // Integer IDs start over in another project; its own selection must win.
    let mut other = Document::default();
    assert_eq!(
        other.model.add_edge(DVec3::ZERO, DVec3::Z).unwrap(),
        visible
    );
    let selected = other.model.add_edge(DVec3::Y, DVec3::Y + DVec3::Z).unwrap();
    other.selection.insert(selected);
    app.replace_project(other, None, &ctx);
    assert_eq!(app.selected, BTreeSet::from([selected]));
    assert!(!app.project.modified);
    app.new_model(&ctx);
    assert!(app.selected.is_empty());
    assert!(app.doc.selection.is_empty());
    assert_eq!(app.doc.selection_context, None);
}

#[test]
fn nested_selection_restores_its_editing_context_and_discards_invalid_contexts() {
    let mut doc = Document::default();
    let edge = doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let inner = doc
        .model
        .make_object(&BTreeSet::from([edge]), true, "Inner")
        .unwrap();
    let outer = doc
        .model
        .make_object(&BTreeSet::from([inner]), false, "Outer")
        .unwrap();
    let inner = *doc.model.objects[&outer].entities.first().unwrap();
    let edge = *doc.model.objects[&inner].entities.first().unwrap();
    doc.selection = BTreeSet::from([edge, outer]);
    doc.selection_context = Some(inner);
    let loaded = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    let app = KetchupApp::with_document(loaded, None, true);
    assert_eq!(app.doc.model.edit_path(), vec![outer, inner]);
    assert_eq!(app.selected, BTreeSet::from([edge]));

    for context in [inner, u64::MAX] {
        let mut invalid = doc.clone();
        invalid.selection_context = Some(context);
        invalid.model.locked.insert(outer);
        let app = KetchupApp::with_document(invalid, None, true);
        assert_eq!(app.doc.model.edit_context, None);
        assert_eq!(app.selected, BTreeSet::from([outer]));
    }
    doc.model.hidden.insert(outer);
    let app = KetchupApp::with_document(doc, None, true);
    assert_eq!(app.doc.model.edit_context, None);
    assert!(app.selected.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn selection_changes_save_immediately_without_model_edits_or_preference_storage() {
    const CHILD: &str = "KETCHUP_SELECTION_AUDIT_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let directory = tempfile::tempdir().unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "app::project_tests::selection_changes_save_immediately_without_model_edits_or_preference_storage", "--nocapture"])
            .env(CHILD, "1")
            .env("KETCHUP_DATA_DIR", directory.path())
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }

    let ctx = egui::Context::default();
    let mut doc = Document::default();
    let a = doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let b = doc.model.add_edge(DVec3::Y, DVec3::Y + DVec3::X).unwrap();
    let c = doc.model.add_edge(DVec3::Z, DVec3::Z + DVec3::X).unwrap();
    let group = doc
        .model
        .make_object(&BTreeSet::from([c]), false, "Group")
        .unwrap();
    let child = *doc.model.objects[&group].entities.first().unwrap();
    let mut app = KetchupApp::with_document(doc, None, true);
    app.project.modified = false;
    let model = app.doc.model.clone();
    for selection in [
        BTreeSet::from([a]),
        BTreeSet::from([b]),
        BTreeSet::from([a, b]),
        BTreeSet::new(),
    ] {
        app.selected = selection.clone();
        app.persist_inspector_changes(&ctx, None, false);
        assert!(app.save_error.is_none());
        assert!(!app.project.modified);
        assert!(app.dirty_since.is_none());
        assert_eq!(app.doc.model, model);
        let saved = storage::load().unwrap().unwrap();
        assert_eq!(saved.selection, selection);
        let restarted = KetchupApp::with_document(saved, None, true);
        assert_eq!(restarted.selected, selection);
    }

    // An unchanged frame must not rewrite the document.
    let file = storage::data_dir().unwrap().join("model.json");
    let contents = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    app.persist_inspector_changes(&ctx, None, false);
    assert!(!file.exists());
    std::fs::write(&file, contents).unwrap();

    // Both entering an empty editing scope and selecting within it are durable.
    app.enter_object(group, &ctx);
    app.persist_inspector_changes(&ctx, None, false);
    let saved = storage::load().unwrap().unwrap();
    assert_eq!(saved.selection_context, Some(group));
    assert!(saved.selection.is_empty());
    app.selected.insert(child);
    app.persist_inspector_changes(&ctx, None, false);
    let restarted = KetchupApp::with_document(storage::load().unwrap().unwrap(), None, true);
    assert_eq!(restarted.doc.model.edit_context, Some(group));
    assert_eq!(restarted.selected, BTreeSet::from([child]));
    app.close_edit_context(&ctx);
    app.persist_inspector_changes(&ctx, None, false);
    assert_eq!(storage::load().unwrap().unwrap().selection_context, None);
    assert!(!app.project.modified);

    app.selected.insert(a);
    eframe::App::on_exit(&mut app);
    assert_eq!(
        storage::load().unwrap().unwrap().selection,
        BTreeSet::from([a])
    );
}

#[test]
fn new_project_clears_geometry_history_and_previous_file() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let before = app.doc.model.clone();
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.doc.history.record("Line", before, &app.doc.model);
    app.selected.insert(edge);
    app.project.path = Some("old.ketchup".into());
    app.doc.snap_to_axes = false;
    app.doc.camera.yaw += 1.;
    app.doc.camera.pitch -= 0.4;
    app.doc.camera.target = DVec3::splat(100.);
    app.doc.camera.distance = 200.;
    app.line_start = Some(DVec3::X);
    app.new_model(&ctx);
    assert_eq!(app.doc.model, Model::default());
    assert_eq!(app.doc.camera, Camera::default());
    assert_eq!(app.doc.name, Document::default().name);
    assert!(!app.doc.snap_to_axes);
    assert!(app.selected.is_empty());
    assert!(app.doc.history.undo_label().is_none());
    assert!(app.project.path.is_none());
    assert!(!app.project.modified);
    assert!(app.line_start.is_none());
}

#[test]
fn opening_a_saved_project_restores_undo_and_redo_in_the_app() {
    let ctx = egui::Context::default();
    let mut doc = Document::default();
    for (y, label) in [(0., "First line"), (2., "Second line")] {
        let before = doc.model.clone();
        doc.model.add_edge(DVec3::Y * y, DVec3::Y * y + DVec3::X).unwrap();
        doc.history.record(label, before, &doc.model);
    }
    assert!(doc.history.undo(&mut doc.model));
    let bytes = serde_json::to_vec(&doc).unwrap();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    app.replace_project(crate::project_files::decode(&bytes).unwrap(), None, &ctx);
    assert_eq!(app.doc.history.undo_label(), Some("First line"));
    assert_eq!(app.doc.history.redo_label(), Some("Second line"));
    app.undo(&ctx, false);
    assert!(app.doc.model.edges.is_empty());
    app.undo(&ctx, true);
    app.undo(&ctx, true);
    assert_eq!(app.doc.model.edges.len(), 2);
}

#[test]
fn opening_project_replaces_state_and_resets_history() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let before = app.doc.model.clone();
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.doc.history.record("Line", before, &app.doc.model);
    app.selected.insert(edge);
    app.set_tool(Tool::Line);
    app.line_start = Some(DVec3::ZERO);
    let mut doc = Document {
        name: "Opened project".into(),
        snap_to_geometry: false,
        ..Default::default()
    };
    doc.model.add_edge(DVec3::ZERO, DVec3::Y * 5.).unwrap();
    let model = doc.model.clone();
    let path = std::path::PathBuf::from("opened.ketchup");
    deliver(
        &mut app,
        Ok(Outcome::Opened(Box::new(doc), Some(path.clone()))),
        &ctx,
    );
    assert_eq!(app.doc.model, model);
    assert_eq!(app.doc.name, "opened");
    assert!(app.doc.snap_to_geometry);
    assert!(app.snap_to_edge);
    assert!(app.snap_to_face);
    assert!(app.snap_to_face_center);
    assert_eq!(app.project.path, Some(path));
    assert!(!app.project.modified);
    assert!(app.doc.history.undo_label().is_none());
    assert!(app.selected.is_empty());
    assert!(app.line_start.is_none());
    assert_eq!(app.tool, Tool::Select);
}

#[test]
fn opening_legacy_projects_and_creating_projects_preserve_geometry_target_preferences() {
    let ctx = egui::Context::default();
    for (edges, faces, centers) in [
        (false, true, false),
        (false, false, true),
        (false, false, false),
    ] {
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.snap_to_edge = edges;
        app.snap_to_face = faces;
        app.snap_to_face_center = centers;
        app.sync_geometry_snapping();
        for legacy_geometry in [false, true] {
            let doc = Document {
                snap_to_geometry: legacy_geometry,
                ..Default::default()
            };
            app.replace_project(doc, None, &ctx);
            assert_eq!(app.doc.snap_to_geometry, edges || faces || centers);
            app.new_model(&ctx);
            assert_eq!(app.doc.snap_to_geometry, edges || faces || centers);
            assert_eq!(app.snap_to_edge, edges);
            assert_eq!(app.snap_to_face, faces);
            assert_eq!(app.snap_to_face_center, centers);
            assert!(!app.project.modified);
        }
    }
}

#[test]
fn failed_or_cancelled_file_operations_keep_current_work_and_cancel_queued_new() {
    for result in [Ok(Outcome::Cancelled), Err("Invalid project".into())] {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        app.changed(&ctx);
        let model = app.doc.model.clone();
        app.project.path = Some("original.ketchup".into());
        app.project.after_save = Some(projects::Action::New);
        deliver(&mut app, result, &ctx);
        assert_eq!(app.doc.model, model);
        assert_eq!(
            app.project.path.as_deref(),
            Some(std::path::Path::new("original.ketchup"))
        );
        assert!(app.project.modified);
        assert!(app.project.after_save.is_none());
        assert!(app.project.recent.is_empty());
        assert!(app.project.pending.is_none());
    }
}

#[test]
fn successful_save_continues_new_only_when_all_current_changes_were_saved() {
    for edited_during_save in [false, true] {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
        app.project.snapshot = Some(serde_json::to_string(&app.doc).unwrap());
        app.project.after_save = Some(projects::Action::New);
        if edited_during_save {
            app.doc
                .model
                .add_edge(DVec3::Y, DVec3::Y + DVec3::X)
                .unwrap();
        }
        deliver(
            &mut app,
            Ok(Outcome::Saved(Some("saved.ketchup".into()))),
            &ctx,
        );
        assert_eq!(app.doc.model.edges.is_empty(), !edited_during_save);
        assert_eq!(app.project.modified, edited_during_save);
        assert!(app.project.after_save.is_none());
    }
}

#[test]
fn save_as_updates_the_name_without_marking_the_project_modified() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    app.project.path = Some("original.ketchup".into());
    app.project.snapshot = Some(serde_json::to_string(&app.doc).unwrap());
    deliver(
        &mut app,
        Ok(Outcome::Saved(Some("New house.ketchup".into()))),
        &ctx,
    );
    assert_eq!(app.doc.name, "New house");
    assert_eq!(app.project.path, Some("New house.ketchup".into()));
    assert!(!app.project.modified);
    assert!(app.dirty_since.is_some());
}

#[test]
fn new_shortcut_prompts_for_unsaved_changes_and_escape_cancels() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.changed(&ctx);
    let model = app.doc.model.clone();
    for (key, modifiers) in [
        (Key::N, egui::Modifiers::COMMAND),
        (Key::Escape, egui::Modifiers::NONE),
    ] {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_test_ui(input, |_ui| {
            app.keyboard(&ctx);
            app.project_dialogs(&ctx);
        });
        output.textures_delta.clear();
        if key == Key::N {
            assert!(app.project.confirm.is_some());
        }
    }
    assert!(app.project.confirm.is_none());
    assert_eq!(app.doc.model, model);
    assert!(app.project.modified);
}

#[test]
fn importing_adds_geometry_in_one_undo_step_and_keeps_project_destination() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    let before = app.doc.model.clone();
    let name = app.doc.name.clone();
    app.project.path = Some("original.ketchup".into());
    let mut imported = Model::default();
    imported.add_edge(DVec3::Y, DVec3::Y * 2.).unwrap();
    let result = crate::project_files::prepare_import(app.doc.model.clone(), &imported);
    deliver(&mut app, result, &ctx);
    assert_eq!(app.doc.model.edges.len(), 2);
    assert!(app.doc.model.edges.contains_key(&edge));
    assert_eq!(app.doc.name, name);
    assert_eq!(app.project.path, Some("original.ketchup".into()));
    assert!(app.project.modified);
    assert_eq!(app.doc.history.undo_label(), Some("Import SketchUp"));
    assert!(!app.selected.is_empty());
    let after = app.doc.model.clone();
    app.undo(&ctx, false);
    assert_eq!(app.doc.model, before);
    app.undo(&ctx, true);
    assert_eq!(app.doc.model, after);
}

#[test]
fn exporting_does_not_mark_saved_or_change_the_project_destination() {
    for modified in [false, true] {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.project.path = Some("original.ketchup".into());
        app.project.modified = modified;
        deliver(&mut app, Ok(Outcome::Exported), &ctx);
        assert_eq!(app.project.path, Some("original.ketchup".into()));
        assert_eq!(app.project.modified, modified);
    }
}

#[test]
fn invalid_import_keeps_current_geometry_selection_and_history() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.selected.insert(edge);
    let before = app.doc.model.clone();
    let mut invalid = Model::default();
    invalid.add_edge(DVec3::ZERO, DVec3::Y).unwrap();
    invalid.vertices.clear();
    let result = crate::project_files::prepare_import(app.doc.model.clone(), &invalid);
    deliver(&mut app, result, &ctx);
    assert_eq!(app.doc.model, before);
    assert!(app.selected.contains(&edge));
    assert!(app.doc.history.undo_label().is_none());
    assert!(app.project.error.is_some());
}

#[test]
fn cutting_plan_defaults_to_selection_and_blocks_other_file_actions_until_cancelled() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    assert!(app.project.cutting_plan_layout.allow_rotation);
    app.prepare_cutting_plan();
    assert_eq!(app.project.cutting_plan_selection, Some(false));
    assert!(app.project.blocked());
    app.project.cutting_plan_selection = None;
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.selected.insert(edge);
    app.prepare_cutting_plan();
    assert_eq!(app.project.cutting_plan_selection, Some(true));
    let before = app.doc.model.clone();
    app.request_project(projects::Action::New, &ctx);
    assert_eq!(app.doc.model, before);
    assert_eq!(app.project.cutting_plan_selection, Some(true));
}

#[test]
fn cutting_plan_remembers_scope_and_falls_back_when_no_parts_are_selected() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    app.project.cutting_plan_selection_preference = Some(true);
    app.prepare_cutting_plan();
    assert_eq!(app.project.cutting_plan_selection, Some(false));
    assert_eq!(app.project.cutting_plan_selection_preference, Some(true));
    app.project.cutting_plan_selection = None;
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.selected.insert(edge);
    app.prepare_cutting_plan();
    assert_eq!(app.project.cutting_plan_selection, Some(true));
    app.project.cutting_plan_selection = None;
    app.project.cutting_plan_selection_preference = Some(false);
    app.prepare_cutting_plan();
    assert_eq!(app.project.cutting_plan_selection, Some(false));
    app.project.cutting_plan_selection = None;
    app.new_model(&ctx);
    assert_eq!(app.project.cutting_plan_selection_preference, Some(false));
}

#[test]
fn cutting_plan_export_failure_preserves_project_selection_and_history() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.selected.insert(edge);
    app.project.path = Some("original.ketchup".into());
    app.project.modified = true;
    let before = app.doc.model.clone();
    app.project.operation = projects::Operation::CuttingPlan;
    app.project.pending = Some(crate::project_files::export_cutting_plan(
        &ctx,
        app.doc.name.clone(),
        app.doc.model.clone(),
        app.selected.clone(),
        crate::cutting_layout::Settings::default(),
    ));
    for _ in 0..100 {
        app.poll_project(&ctx);
        if app.project.pending.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.project.error.as_deref().is_some_and(|error| {
        error.contains("No visible three-dimensional parts with assigned stock materials")
    }));
    assert_eq!(app.doc.model, before);
    assert_eq!(app.selected, BTreeSet::from([edge]));
    assert_eq!(app.project.path, Some("original.ketchup".into()));
    assert!(app.project.modified);
    assert!(app.doc.history.undo_label().is_none());
}

#[test]
fn exchange_formats_use_atomic_import_and_undo_redo() {
    for (extension, bytes) in [
        (
            "stl",
            include_bytes!("../tests/fixtures/triangle-mm.stl").as_slice(),
        ),
        ("obj", b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".as_slice()),
        (
            "dxf",
            include_bytes!("../tests/fixtures/profile-mm.dxf").as_slice(),
        ),
    ] {
        let ctx = egui::Context::default();
        let mut app = KetchupApp::with_document(Document::default(), None, true);
        app.project.import_format = extension.into();
        app.project.path = Some("existing.ketchup".into());
        let before = app.doc.model.clone();
        let imported = crate::mesh_exchange::import(bytes, extension, "Part").unwrap();
        let result = crate::project_files::prepare_import(before.clone(), &imported);
        deliver(&mut app, result, &ctx);
        let after = app.doc.model.clone();
        assert!(!app.selected.is_empty());
        assert_eq!(app.doc.history.undo_label(), Some("Import geometry"));
        assert_eq!(app.project.path, Some("existing.ketchup".into()));
        app.undo(&ctx, false);
        assert_eq!(app.doc.model, before);
        app.undo(&ctx, true);
        assert_eq!(app.doc.model, after);
        let selection = app.selected.clone();
        deliver(
            &mut app,
            crate::mesh_exchange::import(b"bad", extension, "Bad")
                .and_then(|m| crate::project_files::prepare_import(after.clone(), &m)),
            &ctx,
        );
        assert_eq!(app.doc.model, after);
        assert_eq!(app.selected, selection);
    }
}

#[test]
fn recent_project_uses_unsaved_changes_protection() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    app.project.path = Some("example.ketchup".into());
    app.remember_recent_project(None);
    app.project.modified = true;
    app.request_project(projects::Action::Recent(0), &ctx);
    assert!(matches!(app.project.confirm, Some(projects::Action::Recent(0))));
    assert!(app.project.pending.is_none());
}
