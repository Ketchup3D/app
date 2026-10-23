use super::*;
use ketchup_core::DVec3;

#[test]
fn ketchup_files_round_trip_complete_projects_and_replace_existing_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("house.ketchup");
    let mut doc = Document {
        name: "House".into(),
        snap_to_axes: false,
        ..Default::default()
    };
    doc.model.add_edge(DVec3::ZERO, DVec3::X).unwrap();
    doc.camera.yaw += 0.4;
    let json = serde_json::to_vec(&doc).unwrap();
    write(&path, &json).unwrap();
    let restored = decode(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(restored.name, doc.name);
    assert_eq!(restored.model, doc.model);
    assert_eq!(restored.snap_to_axes, doc.snap_to_axes);
    assert_eq!(restored.camera.yaw, doc.camera.yaw);
    doc.name = "Renamed house".into();
    write(&path, &serde_json::to_vec(&doc).unwrap()).unwrap();
    assert_eq!(
        decode(&std::fs::read(&path).unwrap()).unwrap().name,
        doc.name
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn invalid_project_files_are_rejected() {
    assert!(decode(b"not json").is_err());
    assert!(decode(&[0xff]).is_err());
    let mut json = serde_json::to_value(Document::default()).unwrap();
    json["version"] = 999.into();
    assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
    json["version"] = 2.into();
    json["camera"]["distance"] = (-1).into();
    assert!(decode(&serde_json::to_vec(&json).unwrap()).is_err());
}

#[test]
fn failed_save_keeps_existing_file_intact() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("house.ketchup");
    std::fs::write(&path, b"original").unwrap();
    assert!(write(&path.join("missing/project.ketchup"), b"new").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
}

#[test]
fn save_to_opened_path_writes_without_showing_a_picker() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved.ketchup");
    let doc = Document::default();
    let pending = save(
        &eframe::egui::Context::default(),
        Some(path.clone()),
        doc.name.clone(),
        serde_json::to_string(&doc).unwrap(),
    );
    match pending
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap()
    {
        Outcome::Saved(Some(destination)) => assert_eq!(destination, path),
        outcome => panic!("Unexpected save result: {outcome:?}"),
    }
    assert_eq!(
        decode(&std::fs::read(&path).unwrap()).unwrap().name,
        doc.name
    );
}

#[test]
fn project_file_names_and_extensions_are_portable() {
    assert_eq!(file_name(" My house "), "My house.ketchup");
    assert_eq!(file_name("House.KETCHUP"), "House.KETCHUP");
    assert_eq!(file_name("../floor:1"), "_floor_1.ketchup");
    assert_eq!(file_name(" . "), "Untitled model.ketchup");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("house");
    assert_eq!(
        ketchup_path(path.clone()).unwrap(),
        path.with_extension("ketchup")
    );
    let explicit = path.with_extension("KETCHUP");
    assert_eq!(ketchup_path(explicit.clone()).unwrap(), explicit);
    std::fs::write(path.with_extension("ketchup"), b"original").unwrap();
    assert!(
        ketchup_path(path).is_err(),
        "Appending an extension must not bypass overwrite confirmation"
    );
}
