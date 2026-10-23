use ketchup_core::Document;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn data_dir() -> Result<std::path::PathBuf, String> {
    // An override makes automated/manual testing independent of personal models.
    if let Some(path) = std::env::var_os("KETCHUP_DATA_DIR") {
        return Ok(std::path::PathBuf::from(path));
    }
    directories::ProjectDirs::from("", "Ketchup", "Ketchup")
        .map(|d| d.data_local_dir().to_path_buf())
        .ok_or_else(|| "Cannot locate the application data directory".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Result<std::path::PathBuf, String> {
    Ok(data_dir()?.join("model.json"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load() -> Result<Option<Document>, String> {
    let file = path()?;
    match std::fs::read_to_string(file) {
        Ok(json) => Document::from_json(&json).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn save(doc: &Document) -> Result<(), String> {
    let file = path()?;
    std::fs::create_dir_all(file.parent().ok_or("Invalid storage path")?)
        .map_err(|e| e.to_string())?;
    let temporary = file.with_extension("json.tmp");
    let json = serde_json::to_vec(doc).map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut out = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        out.write_all(&json)
            .and_then(|_| out.sync_all())
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(temporary, file).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn browser_storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("No browser window")?
        .local_storage()
        .map_err(|_| "Local storage is blocked".to_string())?
        .ok_or_else(|| "Local storage is unavailable".into())
}
#[cfg(target_arch = "wasm32")]
pub fn load() -> Result<Option<Document>, String> {
    browser_storage()?
        .get_item("ketchup.document.v1")
        .map_err(|_| "Cannot read local storage".to_string())?
        .map(|s| Document::from_json(&s))
        .transpose()
}
#[cfg(target_arch = "wasm32")]
pub fn save(doc: &Document) -> Result<(), String> {
    let json = serde_json::to_string(doc).map_err(|e| e.to_string())?;
    browser_storage()?
        .set_item("ketchup.document.v1", &json)
        .map_err(|_| "Local storage is blocked or full".to_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn native_autosave_roundtrip_corruption_and_write_failure_recovery() {
        const CHILD: &str = "KETCHUP_AUTOSAVE_AUDIT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "storage::tests::native_autosave_roundtrip_corruption_and_write_failure_recovery", "--nocapture"])
                .env(CHILD, "1")
                .env("KETCHUP_DATA_DIR", directory.path().join("recovery"))
                .output().unwrap();
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        assert!(load().unwrap().is_none());
        let mut doc = Document::default();
        doc.name = "Autosave audit".into();
        doc.model
            .add_edge(ketchup_core::DVec3::ZERO, ketchup_core::DVec3::X)
            .unwrap();
        save(&doc).unwrap();
        assert_eq!(
            serde_json::to_value(load().unwrap().unwrap()).unwrap(),
            serde_json::to_value(&doc).unwrap()
        );
        let file = path().unwrap();
        std::fs::write(&file, b"corrupt recovery document").unwrap();
        assert!(load().is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"corrupt recovery document");
        save(&doc).unwrap();
        assert_eq!(load().unwrap().unwrap().name, doc.name);
        // A blocked storage directory must report failure and leave the live
        // document available for further edits and an explicit retry.
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(file.parent().unwrap()).unwrap();
        std::fs::write(file.parent().unwrap(), b"blocked").unwrap();
        assert!(save(&doc).is_err());
        doc.model
            .add_edge(ketchup_core::DVec3::X, ketchup_core::DVec3::Y)
            .unwrap();
        std::fs::remove_file(file.parent().unwrap()).unwrap();
        save(&doc).unwrap();
        assert_eq!(load().unwrap().unwrap().model, doc.model);
    }
}
