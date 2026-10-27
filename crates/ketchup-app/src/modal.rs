use crate::{icons, theme::Palette};
use eframe::egui::{self, Color32, Margin, Stroke};

/// The shared shell for app dialogs. Slots are rendered sequentially so their
/// closures can edit the same state without holding overlapping borrows.
pub(crate) struct Modal {
    id: egui::Id,
    width: f32,
    dismissible: bool,
}

impl Modal {
    pub(crate) fn new(id: &'static str, width: f32) -> Self {
        Self {
            id: egui::Id::unique(id),
            width,
            dismissible: true,
        }
    }

    pub(crate) fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    pub(crate) fn show<R>(
        self,
        ctx: &egui::Context,
        content: impl FnOnce(&mut ModalSlots<'_>) -> R,
    ) -> ModalResponse<R> {
        let colors = Palette::for_painter(&ctx.debug_painter());
        let shake_id = self.id.with("backdrop_shake");
        let now = ctx.input(|input| input.time);
        let shake_started = ctx.data_mut(|data| data.get_temp::<f64>(shake_id));
        let mut shake_offset = 0.;
        if let Some(started) = shake_started {
            let progress = ((now - started) / 0.24) as f32;
            if (0.0..1.0).contains(&progress) {
                // One short, decaying burst, then return exactly to center.
                shake_offset = (progress * std::f32::consts::TAU * 3.).sin() * 6. * (1. - progress);
                ctx.request_repaint();
            } else {
                ctx.data_mut(|data| data.remove::<f64>(shake_id));
            }
        }
        let response = egui::Modal::new(self.id)
            .area(
                egui::Modal::default_area(self.id)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(shake_offset, 0.)),
            )
            .backdrop_color(Color32::from_black_alpha(80))
            .frame(
                egui::Frame::popup(&ctx.global_style())
                    .fill(colors.paper)
                    .stroke(Stroke::new(1., colors.border))
                    .inner_margin(0)
                    .corner_radius(16)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 32,
                        spread: 2,
                        color: Color32::from_black_alpha(48),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_width(self.width.min((ctx.content_rect().width() - 32.).max(80.)));
                ui.spacing_mut().item_spacing.y = 0.;
                content(&mut ModalSlots {
                    ui,
                    id: self.id,
                    dismissible: self.dismissible,
                })
            });
        if response.is_top_modal && !response.any_popup_open && response.backdrop_response.clicked()
        {
            ctx.data_mut(|data| data.insert_temp(shake_id, now));
            ctx.request_repaint();
        }
        // The egui modal helper also closes on backdrop clicks. Only explicit
        // close actions and Escape should dismiss our dialogs.
        let close_requested = self.dismissible
            && (response.response.should_close()
                || (response.is_top_modal
                    && !response.any_popup_open
                    && ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    })));
        if close_requested {
            ctx.data_mut(|data| data.remove::<f64>(shake_id));
        }
        ModalResponse {
            inner: response.inner,
            close_requested,
        }
    }
}

pub(crate) struct ModalResponse<R> {
    pub(crate) inner: R,
    close_requested: bool,
}

impl<R> ModalResponse<R> {
    pub(crate) fn should_close(&self) -> bool {
        self.close_requested
    }
}

pub(crate) struct ModalSlots<'a> {
    ui: &'a mut egui::Ui,
    id: egui::Id,
    dismissible: bool,
}

impl ModalSlots<'_> {
    pub(crate) fn header(&mut self, content: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new().inner_margin(20).show(self.ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(8., 6.);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if self.dismissible {
                    let colors = Palette::for_ui(ui);
                    let response = ui.add(
                        egui::Button::new("")
                            .frame(false)
                            .min_size(egui::vec2(28., 28.)),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, "Close dialog")
                    });
                    if response.hovered() {
                        ui.painter().rect_stroke(
                            response.rect,
                            7,
                            ui.visuals().widgets.hovered.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                    }
                    icons::line_icon(ui.painter(), response.rect.center(), "close", colors.muted);
                    if response.on_hover_text("Close (Esc)").clicked() {
                        ui.close();
                    }
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    content,
                );
            });
        });
        self.divider();
    }

    pub(crate) fn body<R>(&mut self, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
        self.body_with_scroll(
            egui::ScrollArea::vertical().id_salt(self.id.with("body")),
            content,
        )
    }

    /// A body whose contents supply their own padding and scrolling, such as
    /// a settings column beside a preview that meets the modal's edges.
    pub(crate) fn body_unpadded<R>(
        &mut self,
        height: f32,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let max_height =
            (self.ui.ctx().content_rect().height() - self.ui.min_rect().height() - 136.).max(24.);
        self.ui
            .allocate_ui_with_layout(
                egui::vec2(self.ui.available_width(), height.min(max_height)),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    content(ui)
                },
            )
            .inner
    }

    /// Supply scroll state when a picker or search needs to reset the body.
    pub(crate) fn body_with_scroll<R>(
        &mut self,
        scroll: egui::ScrollArea,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        // Reserve room for body padding, wrapped footer actions and screen edges.
        let max_height =
            (self.ui.ctx().content_rect().height() - self.ui.min_rect().height() - 176.).max(24.);
        egui::Frame::new()
            .inner_margin(20)
            .show(self.ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(8., 8.);
                scroll
                    .min_scrolled_height(24.)
                    .max_height(max_height)
                    .show(ui, content)
                    .inner
            })
            .inner
    }

    /// Render the least impactful action first (usually Cancel or Close), then
    /// actions of increasing impact. Actions flow from right to left, keeping the
    /// least impactful action at the right edge, and wrap on small screens.
    /// The footer never scrolls.
    pub(crate) fn footer<R>(&mut self, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
        self.divider();
        let colors = Palette::for_ui(self.ui);
        egui::Frame::new()
            .fill(colors.canvas)
            .corner_radius(egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 16,
                se: 16,
            })
            .inner_margin(Margin::symmetric(20, 14))
            .show(self.ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(8., 8.);
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true),
                    content,
                )
                .inner
            })
            .inner
    }

    fn divider(&mut self) {
        let colors = Palette::for_ui(self.ui);
        let (rect, _) = self.ui.allocate_exact_size(
            egui::vec2(self.ui.available_width(), 1.),
            egui::Sense::hover(),
        );
        self.ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1., colors.border),
        );
    }
}

pub(crate) fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let colors = Palette::for_ui(ui);
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(label).color(colors.paper).strong())
            .fill(colors.ink)
            .stroke(Stroke::NONE)
            .corner_radius(7),
    )
}
