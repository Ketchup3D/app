#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    // Installer hooks must run before opening windows or loading user data.
    ketchup_app::initialize_updater();

    let viewport = eframe::egui::ViewportBuilder::default()
        .with_decorations(false)
        .with_inner_size([1440., 900.])
        .with_min_inner_size([800., 540.])
        .with_icon(
            eframe::icon_data::from_png_bytes(include_bytes!(
                "../../../assets/branding/icon-256.png"
            ))
            .expect("The bundled KetchUp icon must be a valid PNG"),
        );
    // GNOME/Wayland resolves the icon through the matching .desktop file.
    #[cfg(target_os = "linux")]
    let viewport = viewport.with_app_id("io.github.Ketchup3D.Ketchup");

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "KetchUp — 3D workspace",
        options,
        Box::new(|cc| Ok(Box::new(ketchup_app::KetchupApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("A browser document is required");
        let canvas = document
            .get_element_by_id("ketchup-canvas")
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("Missing viewport canvas");
        let options = eframe::WebOptions {
            renderer: eframe::Renderer::Wgpu,
            depth_buffer: 32,
            ..Default::default()
        };
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| Ok(Box::new(ketchup_app::KetchupApp::new(cc)))),
            )
            .await;
        if let Some(loading) = document.get_element_by_id("loading") {
            match result {
                Ok(())=>loading.remove(),
                Err(_)=>loading.set_text_content(Some("KetchUp could not start its GPU renderer. Enable hardware acceleration or try a browser with WebGPU or WebGL2 support.")),
            }
        }
    });
}
