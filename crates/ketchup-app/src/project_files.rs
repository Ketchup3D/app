//! Portable project files are separate from the local recovery/autosave document.
use ketchup_core::Document;
use std::{path::PathBuf, sync::mpsc};

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "project_file_tests.rs"]
mod tests;

#[derive(Debug)]
pub enum Outcome {
    Opened(Box<Document>, Option<PathBuf>),
    Saved(Option<PathBuf>),
    Imported(Box<ketchup_core::Model>, std::collections::BTreeSet<u64>),
    Exported,
    Cancelled,
}
pub type Pending = mpsc::Receiver<Result<Outcome, String>>;

pub fn file_name(name: &str) -> String {
    exchange_name(name, "ketchup")
}

pub fn set_document_name(doc: &mut Document, path: &std::path::Path) {
    if let Some(name) = path.file_stem() {
        doc.name = name.to_string_lossy().into_owned();
    }
}

fn exchange_name(name: &str, extension: &str) -> String {
    let name: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches([' ', '.']);
    let name = if name.is_empty() {
        "Untitled model"
    } else {
        name
    };
    if name
        .to_ascii_lowercase()
        .ends_with(&format!(".{extension}"))
    {
        name.to_owned()
    } else {
        format!("{name}.{extension}")
    }
}

pub fn decode(bytes: &[u8]) -> Result<Document, String> {
    let json = std::str::from_utf8(bytes).map_err(|_| "The project is not UTF-8 text")?;
    Document::from_json(json)
}

#[cfg(not(target_arch = "wasm32"))]
fn run(
    ctx: &eframe::egui::Context,
    action: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
) -> Pending {
    let (sender, receiver) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = sender.send(action());
        ctx.request_repaint();
    });
    receiver
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open(ctx: &eframe::egui::Context) -> Pending {
    run(ctx, || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open KetchUp project")
            .add_filter("KetchUp project", &["ketchup"])
            .pick_file()
        else {
            return Ok(Outcome::Cancelled);
        };
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        decode(&bytes).map(|doc| Outcome::Opened(Box::new(doc), Some(path)))
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write(path: &std::path::Path, contents: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary
        .write_all(contents)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save(
    ctx: &eframe::egui::Context,
    path: Option<PathBuf>,
    name: String,
    contents: String,
) -> Pending {
    run(ctx, move || {
        let path = if let Some(path) = path {
            path
        } else {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Save KetchUp project as")
                .add_filter("KetchUp project", &["ketchup"])
                .set_file_name(file_name(&name))
                .save_file()
            else {
                return Ok(Outcome::Cancelled);
            };
            ketchup_path(path)?
        };
        write(&path, contents.as_bytes())?;
        Ok(Outcome::Saved(Some(path)))
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn ketchup_path(path: PathBuf) -> Result<PathBuf, String> {
    extension_path(path, "ketchup")
}

#[cfg(not(target_arch = "wasm32"))]
fn extension_path(path: PathBuf, extension: &str) -> Result<PathBuf, String> {
    if path.extension().is_some_and(|ext| {
        ext.as_encoded_bytes()
            .eq_ignore_ascii_case(extension.as_bytes())
    }) {
        return Ok(path);
    }
    let mut name = path.into_os_string();
    name.push(format!(".{extension}"));
    let path = PathBuf::from(name);
    // The picker confirmed overwriting the selected path, not an appended path.
    if path.exists() {
        return Err(format!(
            "{} already exists. Select that .{extension} file in the save dialog to replace it.",
            path.display()
        ));
    }
    Ok(path)
}

#[cfg(target_arch = "wasm32")]
pub fn open(ctx: &eframe::egui::Context) -> Pending {
    let (sender, receiver) = mpsc::channel();
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = if let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("KetchUp project", &["ketchup"])
            .pick_file()
            .await
        {
            match wasm_bindgen_futures::JsFuture::from(file.inner().array_buffer()).await {
                Ok(buffer) => {
                    decode(&js_sys::Uint8Array::new(&buffer).to_vec()).map(|mut doc| {
                        set_document_name(&mut doc, std::path::Path::new(&file.file_name()));
                        Outcome::Opened(Box::new(doc), None)
                    })
                }
                Err(_) => Err("The browser could not read this project".into()),
            }
        } else {
            Ok(Outcome::Cancelled)
        };
        let _ = sender.send(result);
        ctx.request_repaint();
    });
    receiver
}

#[cfg(target_arch = "wasm32")]
pub fn save(
    ctx: &eframe::egui::Context,
    _path: Option<PathBuf>,
    name: String,
    contents: String,
) -> Pending {
    let (sender, receiver) = mpsc::channel();
    let result = download(&file_name(&name), &contents)
        .map(|_| Outcome::Saved(None))
        .map_err(|e| {
            e.as_string()
                .unwrap_or_else(|| "The browser could not download this project".into())
        });
    let _ = sender.send(result);
    ctx.request_repaint();
    receiver
}

#[cfg(target_arch = "wasm32")]
fn download(name: &str, contents: &str) -> Result<(), eframe::wasm_bindgen::JsValue> {
    use eframe::wasm_bindgen::{JsCast, JsValue, closure::Closure};
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("No browser window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("No browser document"))?;
    let body = document
        .body()
        .ok_or_else(|| JsValue::from_str("No browser document body"))?;
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(contents));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(if name.ends_with(".obj") {
        "text/plain"
    } else {
        "application/json"
    });
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)?;
    let anchor = document
        .create_element("a")?
        .dyn_into::<web_sys::HtmlAnchorElement>()?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    anchor.set_href(&url);
    anchor.set_download(name);
    if let Err(error) = body.append_child(&anchor) {
        let _ = web_sys::Url::revoke_object_url(&url);
        return Err(error);
    }
    anchor.click();
    anchor.remove();
    // Keep the URL alive long enough for the browser to start the download.
    let cleanup = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(cleanup.unchecked_ref(), 10_000);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn import(ctx: &eframe::egui::Context, model: ketchup_core::Model) -> Pending {
    run(ctx, move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import SketchUp model")
            .add_filter("SketchUp model", &["skp"])
            .pick_file()
        else {
            return Ok(Outcome::Cancelled);
        };
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len()
            > crate::interchange::MAX_SKP_BYTES as u64
        {
            return Err("SketchUp import is limited to 64 MiB per file".into());
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("SketchUp import");
        crate::interchange::import_skp(&bytes, name)
            .and_then(|imported| prepare_import(model, &imported))
    })
}

#[cfg(target_arch = "wasm32")]
pub fn import(ctx: &eframe::egui::Context, model: ketchup_core::Model) -> Pending {
    let (sender, receiver) = mpsc::channel();
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = if let Some(file) = rfd::AsyncFileDialog::new()
            .set_title("Import SketchUp model")
            .add_filter("SketchUp model", &["skp"])
            .pick_file()
            .await
        {
            if file.inner().size() > crate::interchange::MAX_SKP_BYTES as f64 {
                Err("SketchUp import is limited to 64 MiB per file".into())
            } else {
                match wasm_bindgen_futures::JsFuture::from(file.inner().array_buffer()).await {
                    Ok(buffer) => {
                        let name = file.file_name();
                        let name = std::path::Path::new(&name)
                            .file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or("SketchUp import");
                        crate::interchange::import_skp_async(
                            &js_sys::Uint8Array::new(&buffer).to_vec(),
                            name,
                        )
                        .await
                        .and_then(|imported| prepare_import(model, &imported))
                    }
                    Err(_) => Err("The browser could not read this SketchUp file".into()),
                }
            }
        } else {
            Ok(Outcome::Cancelled)
        };
        let _ = sender.send(result);
        ctx.request_repaint();
    });
    receiver
}

#[cfg(not(target_arch = "wasm32"))]
pub fn export(ctx: &eframe::egui::Context, name: String, contents: String) -> Pending {
    run(ctx, move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export Wavefront OBJ")
            .add_filter("Wavefront OBJ", &["obj"])
            .set_file_name(exchange_name(&name, "obj"))
            .save_file()
        else {
            return Ok(Outcome::Cancelled);
        };
        write(&extension_path(path, "obj")?, contents.as_bytes())?;
        Ok(Outcome::Exported)
    })
}

#[cfg(target_arch = "wasm32")]
pub fn export(ctx: &eframe::egui::Context, name: String, contents: String) -> Pending {
    let (sender, receiver) = mpsc::channel();
    let result = download(&exchange_name(&name, "obj"), &contents)
        .map(|_| Outcome::Exported)
        .map_err(|e| {
            e.as_string()
                .unwrap_or_else(|| "The browser could not download this OBJ file".into())
        });
    let _ = sender.send(result);
    ctx.request_repaint();
    receiver
}

/// Prepare the complete next document on the file-operation thread on desktop;
/// committing its result on the UI thread only swaps owned models.
pub fn prepare_import(
    mut model: ketchup_core::Model,
    imported: &ketchup_core::Model,
) -> Result<Outcome, String> {
    imported.validate().map_err(str::to_owned)?;
    let selected = model.paste_entities(imported, ketchup_core::DVec3::ZERO);
    if selected.is_empty() {
        return Err("The imported geometry cannot be inserted in this editing context".into());
    }
    model.validate().map_err(str::to_owned)?;
    Ok(Outcome::Imported(Box::new(model), selected))
}
