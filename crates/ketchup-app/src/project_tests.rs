use super::*;
use crate::project_files::Outcome;

fn deliver(app: &mut KetchupApp, result: Result<Outcome, String>, ctx: &egui::Context) {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.project.pending = Some(receiver);
    sender.send(result).unwrap();
    app.poll_project(ctx);
}

#[test]
fn new_project_clears_geometry_history_and_previous_file() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let before = app.doc.model.clone();
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.history.record("Line", before, &app.doc.model);
    app.selected.insert(edge);
    app.project.path = Some("old.ketchup".into());
    app.doc.snap_to_axes = false;
    app.doc.camera.yaw += 1.;
    app.line_start = Some(DVec3::X);
    app.new_model(&ctx);
    assert_eq!(app.doc.model, Model::default());
    assert_eq!(app.doc.camera, Camera::default());
    assert_eq!(app.doc.name, Document::default().name);
    assert!(app.doc.snap_to_axes);
    assert!(app.selected.is_empty());
    assert!(app.history.undo_label().is_none());
    assert!(app.project.path.is_none());
    assert!(!app.project.modified);
    assert!(app.line_start.is_none());
}

#[test]
fn opening_project_replaces_state_and_resets_history() {
    let ctx = egui::Context::default();
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let before = app.doc.model.clone();
    let edge = app.doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    app.history.record("Line", before, &app.doc.model);
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
    assert_eq!(app.doc.name, "Opened project");
    assert!(!app.doc.snap_to_geometry);
    assert_eq!(app.project.path, Some(path));
    assert!(!app.project.modified);
    assert!(app.history.undo_label().is_none());
    assert!(app.selected.is_empty());
    assert!(app.line_start.is_none());
    assert_eq!(app.tool, Tool::Select);
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
            app.doc.name = "Later change".into();
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
        let mut output = ctx.run_ui(input, |_ui| {
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
    assert_eq!(app.history.undo_label(), Some("Import SketchUp"));
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
    assert!(app.history.undo_label().is_none());
    assert!(app.project.error.is_some());
}
