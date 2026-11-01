//! UI-only tests inspect shapes and interactions without uploading GPU textures.
use eframe::egui;

pub(crate) trait ContextExt {
    fn run_test_ui(
        &self,
        input: egui::RawInput,
        ui: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput;
}

impl ContextExt for egui::Context {
    fn run_test_ui(
        &self,
        input: egui::RawInput,
        ui: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let mut output = self.run_ui(input, ui);
        // GPU integration tests use run_ui directly and apply their own deltas.
        output.textures_delta.clear();
        output
    }
}
