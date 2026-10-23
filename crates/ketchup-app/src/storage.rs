use ketchup_core::Document;

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Result<std::path::PathBuf, String> {
    // An override makes automated/manual testing independent of personal models.
    if let Some(path) = std::env::var_os("KETCHUP_DATA_DIR") {
        return Ok(std::path::PathBuf::from(path).join("model.json"));
    }
    directories::ProjectDirs::from("", "Ketchup", "Ketchup")
        .map(|d| d.data_local_dir().join("model.json"))
        .ok_or_else(|| "Cannot locate the application data directory".into())
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
