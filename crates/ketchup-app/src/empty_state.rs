//! Shared empty states fill allocated panes and wrap text without widening them.
use crate::theme::Palette;
use eframe::egui::{self, Align, FontId, Vec2};

pub(crate) struct EmptyState<'a> {
    title: &'a str,
    description: &'a str,
}
impl<'a> EmptyState<'a> {
    pub fn new(title: &'a str, description: &'a str) -> Self {
        Self { title, description }
    }
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let colors = Palette::for_ui(ui);
        let available = ui.available_size();
        let width = if available.x.is_finite() {
            available.x.max(0.)
        } else {
            ui.clip_rect().width().max(0.)
        };
        let padding = 20_f32.min(width * 0.1);
        let text_width = (width - padding * 2.).max(1.);
        let layout = |text: &str, size, color| {
            let mut job = egui::text::LayoutJob::simple(
                text.into(),
                FontId::proportional(size),
                color,
                text_width,
            );
            job.halign = Align::Center;
            ui.painter().layout_job(job)
        };
        let title = layout(self.title, 14., colors.ink);
        let description = layout(self.description, 12., colors.muted);
        let gap = if self.title.is_empty() || self.description.is_empty() {
            0.
        } else {
            8.
        };
        let content_height = title.size().y + gap + description.size().y;
        // Stacked content-sized sections have no assigned height. Give those
        // a compact natural size; docked/floating panes use their full height.
        let height = if available.y.is_finite() {
            available.y.max(0.)
        } else {
            120.
        }
        .max(content_height + 32.);
        ui.allocate_ui_with_layout(
            Vec2::new(width, height),
            egui::Layout::top_down(Align::Center),
            |ui| {
                ui.set_min_size(Vec2::new(width, height));
                ui.spacing_mut().item_spacing.y = 0.;
                ui.add_space((height - content_height) * 0.5);
                ui.add(egui::Label::new(title));
                ui.add_space(gap);
                ui.add(egui::Label::new(description));
            },
        )
        .response
    }
}
