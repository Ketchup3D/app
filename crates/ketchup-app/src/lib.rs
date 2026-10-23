mod app;
mod brand_mark;
mod environment;
mod icons;
mod interchange;
mod picking;
mod project_files;
mod renderer;
mod shadows;
mod storage;
mod theme;
#[cfg(not(target_arch = "wasm32"))]
mod updates;

#[cfg(not(target_arch = "wasm32"))]
pub use updates::initialize as initialize_updater;

pub use app::KetchupApp;
