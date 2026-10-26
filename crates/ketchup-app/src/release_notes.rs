use crate::theme::Palette;
use eframe::egui::{self, RichText};

#[derive(serde::Deserialize)]
struct Release {
    version: String,
    date: String,
    title: String,
    sections: Vec<Section>,
}

#[derive(serde::Deserialize)]
struct Section {
    title: String,
    changes: Vec<String>,
}

pub(crate) struct ReleaseNotes {
    pub(crate) open: bool,
    pub(crate) last_seen_version: Option<String>,
    selected: usize,
    reset_scroll: bool,
    releases: Vec<Release>,
}

impl Default for ReleaseNotes {
    fn default() -> Self {
        Self {
            open: false,
            last_seen_version: None,
            selected: 0,
            reset_scroll: true,
            // Also validated by the desktop release script before publishing.
            releases: serde_json::from_str(include_str!("../../../assets/release-notes.json"))
                .expect("bundled release notes must be valid"),
        }
    }
}

impl ReleaseNotes {
    pub(crate) fn latest_version(&self) -> &str {
        &self.releases[0].version
    }

    pub(crate) fn has_unread(&self) -> bool {
        self.last_seen_version.as_deref() != Some(self.latest_version())
    }

    // Called once at startup, after restoring the saved read version.
    pub(crate) fn open_if_unread(&mut self) {
        if self.has_unread() {
            self.open();
        }
    }

    pub(crate) fn open(&mut self) {
        self.open = true;
        self.selected = 0;
        self.reset_scroll = true;
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let response = egui::Modal::new(egui::Id::unique("release_notes"))
            .frame(
                egui::Frame::popup(&ctx.global_style())
                    .inner_margin(24)
                    .corner_radius(12),
            )
            .show(ctx, |ui| {
                let colors = Palette::for_ui(ui);
                ui.set_width((ctx.content_rect().width() - 96.).clamp(220., 560.));
                ui.heading("Release notes");
                ui.label(
                    RichText::new(format!(
                        "You're using KetchUp {}",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .color(colors.muted),
                );
                ui.add_space(12.);
                let previous = self.selected;
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(self.selected > 0, egui::Button::new("Newer"))
                        .clicked()
                    {
                        self.selected -= 1;
                    }
                    egui::ComboBox::from_id_salt("release_notes_version")
                        .selected_text(format!("v{}", self.releases[self.selected].version))
                        .show_ui(ui, |ui| {
                            for (index, release) in self.releases.iter().enumerate() {
                                let label = if index == 0 {
                                    format!("v{} · Latest", release.version)
                                } else {
                                    format!("v{}", release.version)
                                };
                                ui.selectable_value(&mut self.selected, index, label);
                            }
                        });
                    if ui
                        .add_enabled(
                            self.selected + 1 < self.releases.len(),
                            egui::Button::new("Older"),
                        )
                        .clicked()
                    {
                        self.selected += 1;
                    }
                });
                ui.add_space(12.);
                ui.separator();
                ui.add_space(8.);
                let release = &self.releases[self.selected];
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("release_notes_content")
                    .auto_shrink([false, false])
                    .max_height((ctx.content_rect().height() - 300.).clamp(80., 480.));
                if self.reset_scroll || previous != self.selected {
                    scroll = scroll.vertical_scroll_offset(0.);
                    self.reset_scroll = false;
                }
                scroll.show(ui, |ui| {
                    ui.heading(&release.title);
                    let latest = if self.selected == 0 { " · Latest" } else { "" };
                    ui.label(
                        RichText::new(format!("v{} · {}{latest}", release.version, release.date))
                            .color(colors.muted),
                    );
                    for section in &release.sections {
                        ui.add_space(14.);
                        ui.label(RichText::new(&section.title).strong());
                        for change in &section.changes {
                            ui.horizontal_top(|ui| {
                                ui.label("•");
                                ui.add(egui::Label::new(change).wrap());
                            });
                        }
                    }
                });
                if self.selected == 0 && self.has_unread() {
                    self.last_seen_version = Some(release.version.clone());
                    ctx.request_repaint();
                }
                ui.add_space(12.);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        ui.close();
                    }
                });
            });
        if response.should_close() {
            self.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_state_tracks_the_latest_displayed_version() {
        let ctx = egui::Context::default();
        let mut notes = ReleaseNotes::default();
        let show = |notes: &mut ReleaseNotes| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000., 700.),
                    )),
                    ..Default::default()
                },
                |_| notes.show(&ctx),
            );
        };
        assert!(notes.has_unread());
        notes.open();
        assert!(
            notes.has_unread(),
            "opening alone must not mark notes as displayed"
        );
        notes.selected = notes.releases.len() - 1;
        show(&mut notes);
        assert!(
            notes.has_unread(),
            "older notes do not mark the latest release as read"
        );
        notes.open();
        show(&mut notes);
        assert!(!notes.has_unread());
        notes.selected = notes.releases.len() - 1;
        show(&mut notes);
        assert!(!notes.has_unread());
        // Updating the catalog must bring back the notice for the next version.
        notes.releases[0].version = "999.0.0".into();
        assert!(notes.has_unread());
        notes.open = false;
        notes.open_if_unread();
        assert!(notes.open);
        show(&mut notes);
        assert!(!notes.has_unread());
        notes.open = false;
        notes.open_if_unread();
        assert!(
            !notes.open,
            "the same release must not open automatically twice"
        );
    }
}
