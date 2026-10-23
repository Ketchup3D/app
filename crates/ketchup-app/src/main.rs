#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1440., 900.])
            .with_min_inner_size([800., 540.]),
        ..Default::default()
    };
    eframe::run_native(
        "Ketchup — 3D workspace",
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
                Err(_)=>loading.set_text_content(Some("Ketchup could not start its GPU renderer. Enable hardware acceleration or try a browser with WebGPU or WebGL2 support.")),
            }
        }
    });
}
