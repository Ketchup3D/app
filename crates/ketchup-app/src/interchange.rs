//! Geometry interchange with a native SKP reader on desktop and a browser worker.
use ketchup_core::Model;

// Published JavaScript artifacts are vendored to avoid an SDK, subprocess,
// network request, or JavaScript build step in either application.
#[cfg(target_arch = "wasm32")]
const SKP_READER: &str = concat!(
    "var ketchupModules = {};\n",
    "ketchupModules.fflate = (function() { var exports = {}, module = {exports};\n",
    include_str!("../vendor/skp/fflate.cjs"),
    "\nreturn module.exports; })();\n",
    "ketchupModules.earcut = (function() { var exports = {}, module = {exports};\n",
    include_str!("../vendor/skp/earcut.cjs"),
    "\nreturn module.exports; })();\n",
    "ketchupModules.flatbuffers = (function() {\n",
    include_str!("../vendor/skp/flatbuffers.js"),
    "\nreturn flatbuffers; })();\n",
    "ketchupModules.openskp = (function() { var exports = {}, module = {exports}; var require = name => ketchupModules[name];\n",
    include_str!("../vendor/skp/openskp.cjs"),
    "\nreturn module.exports; })();\n",
    include_str!("skp_adapter.js"),
);

pub const MAX_SKP_BYTES: usize = 64 * 1024 * 1024;

#[cfg(not(target_arch = "wasm32"))]
pub fn import_skp(bytes: &[u8], name: &str) -> Result<Model, String> {
    if bytes.len() > MAX_SKP_BYTES {
        return Err("SketchUp import is limited to 64 MiB per file".into());
    }
    let json = parse_skp(bytes).map_err(|e| format!("Could not read SketchUp file: {e}"))?;
    decode_model(&json, name)
}

fn decode_model(json: &str, name: &str) -> Result<Model, String> {
    let mut model: Model = serde_json::from_str(json).map_err(|e| e.to_string())?;
    model.validate().map_err(str::to_owned)?;
    // The adapter always emits the outer import group last.
    if let Some((_, group)) = model.objects.last_key_value() {
        let id = group.id;
        model.objects.get_mut(&id).unwrap().name = name.to_owned();
    }
    Ok(model)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_skp(bytes: &[u8]) -> Result<String, String> {
    use std::ffi::{CStr, c_char};
    unsafe extern "C" {
        fn ketchup_skp_parse(bytes: *const u8, len: usize, success: *mut i32) -> *mut c_char;
        fn ketchup_skp_free(value: *mut c_char);
    }
    let mut success = 0;
    // The adapter catches C++ exceptions and returns one owned, NUL-terminated
    // UTF-8 buffer. It borrows the input only for the duration of this call.
    let value = unsafe { ketchup_skp_parse(bytes.as_ptr(), bytes.len(), &mut success) };
    if value.is_null() {
        return Err("Not enough memory to read this SketchUp file".into());
    }
    let text = unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        ketchup_skp_free(value);
    }
    if success == 1 { Ok(text) } else { Err(text) }
}

#[cfg(target_arch = "wasm32")]
pub async fn import_skp_async(bytes: &[u8], name: &str) -> Result<Model, String> {
    use eframe::wasm_bindgen::JsCast;
    if bytes.len() > MAX_SKP_BYTES {
        return Err("SketchUp import is limited to 64 MiB per file".into());
    }
    // Parsing runs in a dedicated worker so the picker, spinner, and application
    // keep responding while the reader expands the component tree.
    let launch = js_sys::Function::new_with_args("source, bytes", include_str!("skp_worker.js"));
    let promise = launch
        .call2(
            &eframe::wasm_bindgen::JsValue::NULL,
            &SKP_READER.into(),
            &js_sys::Uint8Array::from(bytes),
        )
        .map_err(js_error)?
        .dyn_into::<js_sys::Promise>()
        .map_err(js_error)?;
    let json = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(js_error)?
        .as_string()
        .ok_or("SketchUp reader returned no model")?;
    decode_model(&json, name)
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: eframe::wasm_bindgen::JsValue) -> String {
    js_sys::Reflect::get(&error, &"message".into())
        .ok()
        .and_then(|message| message.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "SketchUp reader failed".into())
}

/// OBJ uses metre coordinates with KetchUp's Z-up convention. Triangulating
/// faces keeps holes intact even in readers without polygon-hole support.
pub fn export_obj(model: &Model) -> Result<String, String> {
    use std::{collections::BTreeMap, fmt::Write};
    let mut out = String::from("# KetchUp OBJ export\n# Units: metres; Z up\n");
    let visible_vertices: std::collections::BTreeSet<_> = model
        .edges
        .values()
        .filter(|e| !model.is_hidden(e.id))
        .flat_map(|e| [e.a, e.b])
        .chain(
            model
                .faces
                .values()
                .filter(|f| !model.is_hidden(f.id))
                .flat_map(|f| f.vertices.iter().chain(f.holes.iter().flatten()).copied()),
        )
        .collect();
    let mut indices = BTreeMap::new();
    let mut positions = BTreeMap::new();
    for vertex in model
        .vertices
        .values()
        .filter(|v| visible_vertices.contains(&v.id))
    {
        let p = vertex.position;
        let _ = writeln!(out, "v {} {} {}", p.x, p.y, p.z);
        indices.insert(vertex.id, indices.len() + 1);
        positions.insert(p.to_array().map(f64::to_bits), indices[&vertex.id]);
    }
    for face in model.faces.values().filter(|f| !model.is_hidden(f.id)) {
        for triangle in model.face_triangles(face) {
            let [a, b, c] = triangle.map(|p| positions[&p.to_array().map(f64::to_bits)]);
            let _ = writeln!(out, "f {a} {b} {c}");
        }
    }
    for edge in model.edges.values().filter(|e| !model.is_hidden(e.id)) {
        let _ = writeln!(out, "l {} {}", indices[&edge.a], indices[&edge.b]);
    }
    if !out
        .lines()
        .any(|line| line.starts_with("f ") || line.starts_with("l "))
    {
        return Err("There is no visible geometry to export".into());
    }
    Ok(out)
}

#[cfg(test)]
#[path = "interchange_tests.rs"]
mod tests;
