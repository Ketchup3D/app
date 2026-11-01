//! Text keeps natural glyph spacing even inside full-width control layouts.
use eframe::egui::{self, Response, RichText, Ui, Widget, WidgetText};

pub(crate) struct Label(egui::Label);

impl Label {
    pub(crate) fn new(text: impl Into<WidgetText>) -> Self {
        Self(egui::Label::new(text))
    }

    pub(crate) fn wrap(self) -> Self {
        Self(self.0.wrap())
    }

    pub(crate) fn truncate(self) -> Self {
        Self(self.0.truncate())
    }
}

impl Widget for Label {
    fn ui(self, ui: &mut Ui) -> Response {
        let layout = *ui.layout();
        if !layout.horizontal_justify() {
            return self.0.ui(ui);
        }
        // Justification sizes controls, but egui also uses it to spread label
        // glyphs across wrapped lines. Disable it only for this label's scope.
        // Keep the scope full-width so surrounding controls and hit areas retain
        // their existing layout. egui still handles selection and elision.
        let width = ui.available_width();
        let layout = if layout.is_horizontal() {
            layout.with_main_justify(false)
        } else {
            layout.with_cross_justify(false)
        };
        let response = ui.scope_builder(egui::UiBuilder::new().layout(layout), |ui| {
            if width.is_finite() {
                ui.set_min_width(width);
            }
            self.0.ui(ui)
        });
        response.inner.union(response.response)
    }
}

pub(crate) trait TextUi {
    fn text_label(&mut self, text: impl Into<WidgetText>) -> Response;
    fn text_heading(&mut self, text: impl Into<RichText>) -> Response;
    fn text_small(&mut self, text: impl Into<RichText>) -> Response;
    fn text_strong(&mut self, text: impl Into<RichText>) -> Response;
    fn text_weak(&mut self, text: impl Into<RichText>) -> Response;
    fn text_monospace(&mut self, text: impl Into<RichText>) -> Response;
    fn text_colored_label(
        &mut self,
        color: impl Into<egui::Color32>,
        text: impl Into<RichText>,
    ) -> Response;
}

impl TextUi for Ui {
    fn text_label(&mut self, text: impl Into<WidgetText>) -> Response {
        self.add(Label::new(text))
    }
    fn text_heading(&mut self, text: impl Into<RichText>) -> Response {
        self.text_label(text.into().heading())
    }
    fn text_small(&mut self, text: impl Into<RichText>) -> Response {
        self.text_label(text.into().small())
    }
    fn text_strong(&mut self, text: impl Into<RichText>) -> Response {
        self.text_label(text.into().strong())
    }
    fn text_weak(&mut self, text: impl Into<RichText>) -> Response {
        self.text_label(text.into().weak())
    }
    fn text_monospace(&mut self, text: impl Into<RichText>) -> Response {
        self.text_label(text.into().monospace())
    }
    fn text_colored_label(
        &mut self,
        color: impl Into<egui::Color32>,
        text: impl Into<RichText>,
    ) -> Response {
        self.text_label(text.into().color(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn galleys(shape: &egui::Shape, result: &mut Vec<Arc<egui::Galley>>) {
        match shape {
            egui::Shape::Text(text) => result.push(text.galley.clone()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    galleys(shape, result);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn wrapped_words_keep_the_same_glyph_positions_in_full_width_rows() {
        for width in [70., 130., 240.] {
            let ctx = egui::Context::default();
            let mut rendered = Vec::new();
            for justified in [false, true] {
                let output =
                    ctx.run_ui(egui::RawInput::default(), |ui| {
                        ui.set_width(width);
                        ui.with_layout(
                            egui::Layout::top_down(egui::Align::Min).with_cross_justify(justified),
                            |ui| {
                                let label = ui.add(Label::new(
                                "Natural spacing for wrapped words and verylongunbrokenwords",
                            ).wrap());
                                if justified {
                                    assert!((label.rect.width() - width).abs() < 1.);
                                }
                                let input = ui.add(egui::TextEdit::singleline(&mut String::new()));
                                if justified {
                                    assert!((input.rect.width() - width).abs() < 1.);
                                }
                            },
                        );
                    });
                let mut text = Vec::new();
                for shape in output.shapes {
                    galleys(&shape.shape, &mut text);
                }
                rendered.push(text.remove(0));
            }
            assert!(rendered[0].rows.len() > 1);
            assert!(!rendered[1].job.justify);
            assert_eq!(rendered[0].rows.len(), rendered[1].rows.len());
            for (natural, full_width) in rendered[0].rows.iter().zip(&rendered[1].rows) {
                assert_eq!(natural.glyphs, full_width.glyphs);
            }
        }
    }

    #[test]
    fn rich_text_truncation_and_text_shortcuts_never_justify() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(110.);
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                ui.add(
                    Label::new(RichText::new("Long rich text that must be elided").strong())
                        .truncate(),
                );
                ui.text_label("Wrapped label with several words");
                ui.text_heading("Heading with several words");
                ui.text_small("Small text with several words");
                ui.text_strong("Strong text with several words");
                ui.text_weak("Weak text with several words");
                ui.text_monospace("Monospace with several words");
                ui.text_colored_label(egui::Color32::RED, "Colored text with several words");
            });
        });
        let mut text = Vec::new();
        for shape in output.shapes {
            galleys(&shape.shape, &mut text);
        }
        assert_eq!(text.len(), 8);
        assert!(text[0].elided);
        assert_eq!(text[0].rows.len(), 1);
        assert!(text.iter().all(|galley| !galley.job.justify));
    }
}
