mod app;
mod brand_mark;
mod cutting_layout;
mod cutting_plan;
mod environment;
mod estimate_pdf;
mod icons;
mod inspector_table;
mod interchange;
mod mesh_exchange;
mod modal;
mod natural_text;
mod picking;
mod project_files;
mod release_notes;
mod renderer;
mod screenshots;
mod shadows;
mod storage;
mod theme;
#[cfg(not(target_arch = "wasm32"))]
mod updates;

#[cfg(not(target_arch = "wasm32"))]
pub use updates::initialize as initialize_updater;

pub use app::KetchupApp;

#[cfg(target_arch = "wasm32")]
mod tool_cursor;

mod frame_rate;

mod system_info;

#[cfg(test)]
mod test_ui;
