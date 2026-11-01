use crate::{icons, theme::Palette};
use eframe::egui::{self, Color32, Margin, Stroke};

const RESIZE_OUTSET: f32 = 6.;

/// The shared shell for app dialogs. Slots are rendered sequentially so their
/// closures can edit the same state without holding overlapping borrows.
pub(crate) struct Modal {
    id: egui::Id,
    width: f32,
    key: &'static str,
    resizable: bool,
    dismissible: bool,
}

impl Modal {
    pub(crate) fn new(id: &'static str, width: f32) -> Self {
        Self {
            id: egui::Id::unique(id),
            width,
            key: id,
            resizable: false,
            dismissible: true,
        }
    }

    /// Opt in to resizing; ordinary dialogs retain their content-sized layout.
    pub(crate) fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
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
        let position = crate::app::modal_position(ctx, self.key);
        let area = if let Some(position) = position {
            // Construct an unanchored area: default_area has a center anchor.
            egui::Area::new(self.id)
                .kind(egui::UiKind::Modal)
                .sense(egui::Sense::hover())
                .order(egui::Order::Foreground)
                .interactable(true)
                .fixed_pos(position + egui::vec2(shake_offset, 0.))
        } else {
            egui::Modal::default_area(self.id)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(shake_offset, 0.))
        };
        let response = egui::Modal::new(self.id)
            .area(area)
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
                ui.spacing_mut().item_spacing.y = 0.;
                if self.resizable {
                    let max_size = (ctx.content_rect().size() - egui::vec2(32., 32.))
                        .max(egui::vec2(80., 80.));
                    let min_size = egui::vec2(400., 280.).min(max_size);
                    let size = crate::app::modal_size(ctx, self.key)
                        .unwrap_or(egui::vec2(self.width, 790.))
                        .max(min_size)
                        .min(max_size);
                    ui.allocate_ui_with_layout(
                        size,
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_size(size);
                            let inner = content(&mut ModalSlots {
                                ui,
                                id: self.id,
                                key: self.key,
                                dismissible: self.dismissible,
                                resizable: true,
                            });
                            if !ui.is_sizing_pass() {
                                crate::app::remember_modal_size(ctx, self.key, size);
                                self.resize_borders(ui, size, min_size, max_size);
                            }
                            inner
                        },
                    )
                    .inner
                } else {
                    ui.set_width(self.width.min((ctx.content_rect().width() - 32.).max(80.)));
                    content(&mut ModalSlots {
                        ui,
                        id: self.id,
                        key: self.key,
                        dismissible: self.dismissible,
                        resizable: false,
                    })
                }
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

    fn resize_borders(
        &self,
        ui: &mut egui::Ui,
        size: egui::Vec2,
        min: egui::Vec2,
        max: egui::Vec2,
    ) {
        // The shell's one-point stroke surrounds the content rectangle.
        let frame = ui.min_rect().expand(1.);
        // Only resize interactions extend beyond the shell; keep content clipped.
        let original_clip = ui.clip_rect();
        ui.set_clip_rect(
            frame
                .expand(RESIZE_OUTSET)
                .intersect(ui.ctx().content_rect()),
        );
        for border in ResizeBorder::ALL {
            let id = self.id.with(("resize_border", border as u8));
            let hitbox = border.hitbox(frame);
            let response = ui
                .interact(hitbox, id, egui::Sense::drag())
                .on_hover_cursor(border.cursor());
            let origin_id = id.with("origin");
            if response.dragged() {
                let origin = ui.ctx().data_mut(|data| {
                    *data.get_temp_mut_or(
                        origin_id,
                        egui::Rect::from_min_size(frame.min, size + egui::vec2(2., 2.)),
                    )
                });
                let rect = border.resize(
                    origin,
                    response.total_drag_delta().unwrap_or_default(),
                    min + egui::vec2(2., 2.),
                    max + egui::vec2(2., 2.),
                );
                crate::app::remember_modal_position(ui.ctx(), self.key, rect.min);
                crate::app::remember_modal_size(
                    ui.ctx(),
                    self.key,
                    rect.size() - egui::vec2(2., 2.),
                );
                ui.ctx().set_cursor_icon(border.cursor());
                ui.ctx().request_repaint();
            } else {
                ui.ctx()
                    .data_mut(|data| data.remove::<egui::Rect>(origin_id));
            }
        }
        ui.set_clip_rect(original_clip);
    }
}

#[derive(Clone, Copy)]
enum ResizeBorder {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Left,
    Right,
    Top,
    Bottom,
}

impl ResizeBorder {
    const ALL: [Self; 8] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::Left,
        Self::Right,
        Self::Top,
        Self::Bottom,
    ];

    fn inward(self) -> egui::Vec2 {
        match self {
            Self::TopLeft => egui::vec2(1., 1.),
            Self::TopRight => egui::vec2(-1., 1.),
            Self::BottomLeft => egui::vec2(1., -1.),
            Self::BottomRight => egui::vec2(-1., -1.),
            Self::Left => egui::vec2(1., 0.),
            Self::Right => egui::vec2(-1., 0.),
            Self::Top => egui::vec2(0., 1.),
            Self::Bottom => egui::vec2(0., -1.),
        }
    }

    fn point(self, rect: egui::Rect) -> egui::Pos2 {
        rect.center() - rect.size() * self.inward() * 0.5
    }

    fn hitbox(self, rect: egui::Rect) -> egui::Rect {
        // Edge strips stop at the corner zones, so diagonal resizing wins there.
        match self {
            Self::Left => egui::Rect::from_min_max(
                egui::pos2(rect.left() - RESIZE_OUTSET, rect.top() + 12.),
                egui::pos2(rect.left() + 6., rect.bottom() - 12.),
            ),
            Self::Right => egui::Rect::from_min_max(
                egui::pos2(rect.right() - 6., rect.top() + 12.),
                egui::pos2(rect.right() + RESIZE_OUTSET, rect.bottom() - 12.),
            ),
            Self::Top => egui::Rect::from_min_max(
                egui::pos2(rect.left() + 12., rect.top() - RESIZE_OUTSET),
                egui::pos2(rect.right() - 12., rect.top() + 6.),
            ),
            Self::Bottom => egui::Rect::from_min_max(
                egui::pos2(rect.left() + 12., rect.bottom() - 6.),
                egui::pos2(rect.right() - 12., rect.bottom() + RESIZE_OUTSET),
            ),
            _ => egui::Rect::from_center_size(
                self.point(rect) + self.inward() * ((12. - RESIZE_OUTSET) * 0.5),
                egui::Vec2::splat(12. + RESIZE_OUTSET),
            ),
        }
    }

    fn cursor(self) -> egui::CursorIcon {
        match self {
            Self::TopLeft | Self::BottomRight => egui::CursorIcon::ResizeNwSe,
            Self::TopRight | Self::BottomLeft => egui::CursorIcon::ResizeNeSw,
            Self::Left | Self::Right => egui::CursorIcon::ResizeHorizontal,
            Self::Top | Self::Bottom => egui::CursorIcon::ResizeVertical,
        }
    }

    fn resize(
        self,
        origin: egui::Rect,
        delta: egui::Vec2,
        min: egui::Vec2,
        max: egui::Vec2,
    ) -> egui::Rect {
        let inward = self.inward();
        let size = (origin.size() - delta * inward).max(min).min(max);
        let position = origin.min
            + egui::vec2(
                if inward.x > 0. {
                    origin.width() - size.x
                } else {
                    0.
                },
                if inward.y > 0. {
                    origin.height() - size.y
                } else {
                    0.
                },
            );
        egui::Rect::from_min_size(position, size)
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
    resizable: bool,
    ui: &'a mut egui::Ui,
    id: egui::Id,
    key: &'static str,
    dismissible: bool,
}

impl ModalSlots<'_> {
    pub(crate) fn header(&mut self, content: impl FnOnce(&mut egui::Ui)) {
        // A drag-sensing parent lets buttons in the header keep priority.
        let header =
            self.ui
                .scope_builder(egui::UiBuilder::new().sense(egui::Sense::drag()), |ui| {
                    egui::Frame::new().inner_margin(20).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.spacing_mut().item_spacing = egui::vec2(8., 6.);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if self.dismissible {
                                let colors = Palette::for_ui(ui);
                                let response = ui.scope(|ui| {
                                    // Match the Material header menu: a 32px target
                                    // with the painted button frame inset by 4px.
                                    let widgets = &mut ui.visuals_mut().widgets;
                                    widgets.inactive.expansion = -4.;
                                    widgets.hovered.expansion = -4.;
                                    widgets.active.expansion = -4.;
                                    widgets.open.expansion = -4.;
                                    ui.add(egui::Button::new("").min_size(egui::vec2(32., 32.)))
                                }).inner;
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::Role::Button,
                                        true,
                                        "Close dialog",
                                    )
                                });
                                icons::line_icon(
                                    ui.painter(),
                                    response.rect.center(),
                                    "close",
                                    colors.muted,
                                );
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
                });
        let response = header.response.on_hover_cursor(egui::CursorIcon::Grab);
        let origin_id = self.id.with("header_drag_origin");
        if response.dragged() {
            let ctx = self.ui.ctx();
            let current = egui::AreaState::load(ctx, self.id)
                .map(|state| state.left_top_pos())
                .unwrap_or(response.rect.min);
            let origin = ctx.data_mut(|data| *data.get_temp_mut_or(origin_id, current));
            crate::app::remember_modal_position(
                ctx,
                self.key,
                origin + response.total_drag_delta().unwrap_or_default(),
            );
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            ctx.request_repaint();
        } else {
            self.ui
                .ctx()
                .data_mut(|data| data.remove::<egui::Pos2>(origin_id));
        }
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
        let height = if self.resizable {
            (self.ui.available_height() - 64.).max(24.)
        } else {
            height.min(max_height)
        };
        self.ui
            .allocate_ui_with_layout(
                egui::vec2(self.ui.available_width(), height),
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
        let max_height = if self.resizable {
            (self.ui.available_height() - 104.).max(24.)
        } else {
            max_height
        };
        let scroll = if self.resizable {
            scroll.auto_shrink([false, false])
        } else {
            scroll
        };
        egui::Frame::new()
            .inner_margin(20)
            .show(self.ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(8., 8.);
                // Areas reuse their last size. Let content-sized dialogs grow
                // again after showing a shorter body, while still shrinking
                // naturally and keeping the footer within the screen.
                if !self.resizable {
                    ui.set_max_height(max_height);
                }
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
                // A centered horizontal layout otherwise consumes the area's
                // previous remaining height, preventing short bodies shrinking.
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::natural_text::TextUi as _;

    #[test]
    fn resizing_is_opt_in() {
        assert!(!Modal::new("ordinary_dialog", 400.).resizable);
        assert!(
            Modal::new("settings_dialog", 950.)
                .resizable(true)
                .resizable
        );
    }

    #[test]
    fn each_border_keeps_the_opposite_side_fixed_and_obeys_size_limits() {
        let origin = egui::Rect::from_min_size(egui::pos2(200., 150.), egui::vec2(600., 400.));
        let min = egui::vec2(400., 280.);
        let max = egui::vec2(1000., 800.);
        for border in ResizeBorder::ALL {
            for delta in [
                egui::vec2(80., 60.),
                egui::Vec2::splat(-2000.),
                egui::Vec2::splat(2000.),
            ] {
                let resized = border.resize(origin, delta, min, max);
                let opposite_before = border.point(origin) + origin.size() * border.inward();
                let opposite_after = border.point(resized) + resized.size() * border.inward();
                assert_eq!(opposite_before, opposite_after);
                if border.inward().x == 0. {
                    assert_eq!(resized.width(), origin.width());
                }
                if border.inward().y == 0. {
                    assert_eq!(resized.height(), origin.height());
                }
                assert!(resized.width() >= min.x && resized.width() <= max.x);
                assert!(resized.height() >= min.y && resized.height() <= max.y);
            }
        }
    }

    #[test]
    fn all_edges_and_corners_resize_the_modal() {
        for (border, offset) in ResizeBorder::ALL
            .into_iter()
            .flat_map(|border| [-4., 5.].map(|offset| (border, offset)))
        {
            let ctx = egui::Context::default();
            let key = "border_resize_test";
            let id = egui::Id::unique(key);
            crate::app::remember_modal_size(&ctx, key, egui::vec2(600., 400.));
            let frame = |events| {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1400., 1100.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |_| {
                        Modal::new(key, 600.).resizable(true).show(&ctx, |modal| {
                            modal.header(|ui| {
                                ui.text_heading("Resize");
                            });
                            modal.body_unpadded(300., |_| {});
                            modal.footer(|ui| {
                                let _ = ui.button("Close");
                            });
                        });
                    },
                );
                egui::AreaState::load(&ctx, id).unwrap().rect()
            };
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![]);
            frame(vec![]);
            let before = frame(vec![]);
            let start = border.point(before) + border.inward() * offset;
            frame(vec![egui::Event::PointerMoved(start)]);
            frame(vec![button(start, true)]);
            let end = start - border.inward() * egui::vec2(80., 60.);
            frame(vec![egui::Event::PointerMoved(end)]);
            frame(vec![button(end, false)]);
            let after = frame(vec![]);
            let fixed_before = border.point(before) + before.size() * border.inward();
            let fixed_after = border.point(after) + after.size() * border.inward();
            assert!((fixed_after - fixed_before).length() < 3.);
            let direction = border.inward();
            let expected_growth = egui::vec2(direction.x.abs() * 80., direction.y.abs() * 60.);
            assert!((after.size() - before.size() - expected_growth).length() < 3.);
        }
    }

    #[test]
    fn header_drag_moves_modal_and_resize_keeps_top_left() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("modal_geometry_test");
        crate::app::remember_modal_size(&ctx, "modal_geometry_test", egui::vec2(600., 400.));
        let frame = |events| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200., 1000.),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    Modal::new("modal_geometry_test", 600.)
                        .resizable(true)
                        .show(&ctx, |modal| {
                            modal.header(|ui| {
                                ui.text_heading("Drag me");
                            });
                            modal.body_unpadded(300., |_| {});
                            modal.footer(|ui| {
                                let _ = ui.button("Close");
                            });
                        });
                },
            );
            egui::AreaState::load(&ctx, id).unwrap().rect()
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![]);
        frame(vec![]);
        let centered = frame(vec![]);
        let corner = centered.max - egui::vec2(5., 5.);
        frame(vec![egui::Event::PointerMoved(corner)]);
        frame(vec![button(corner, true)]);
        let end = corner + egui::vec2(40., 30.);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![]);
        frame(vec![button(end, false)]);
        let initial = frame(vec![]);
        assert!((initial.min - centered.min).length() < 3.);
        assert!(initial.width() > centered.width() + 20.);
        let start = initial.min + egui::vec2(50., 15.);
        frame(vec![egui::Event::PointerMoved(start)]);
        frame(vec![button(start, true)]);
        let end = start + egui::vec2(100., 50.);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![button(end, false)]);
        let moved = frame(vec![]);
        assert!((moved.min - initial.min - egui::vec2(100., 50.)).length() < 3.);
        let corner = moved.max - egui::vec2(5., 5.);
        frame(vec![egui::Event::PointerMoved(corner)]);
        frame(vec![button(corner, true)]);
        let end = corner + egui::vec2(80., 60.);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![]);
        frame(vec![button(end, false)]);
        let resized = frame(vec![]);
        assert!((resized.min - moved.min).length() < 3.);
        assert!(resized.width() > moved.width() + 50.);
        assert!(resized.height() > moved.height() + 30.);
    }

    #[test]
    fn restored_size_controls_resizable_body_dimensions() {
        fn body_size(size: egui::Vec2) -> egui::Vec2 {
            let ctx = egui::Context::default();
            crate::app::remember_modal_size(&ctx, "resizable_test", size);
            let mut body_size = egui::Vec2::ZERO;
            for _ in 0..3 {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200., 1000.),
                        )),
                        ..Default::default()
                    },
                    |_| {
                        Modal::new("resizable_test", 950.)
                            .resizable(true)
                            .show(&ctx, |modal| {
                                modal.header(|ui| {
                                    ui.text_heading("Resize test");
                                });
                                modal.body_unpadded(650., |ui| {
                                    body_size = ui.available_size();
                                });
                                modal.footer(|ui| {
                                    let _ = ui.button("Close");
                                });
                            });
                    },
                );
            }
            body_size
        }
        let small = body_size(egui::vec2(600., 400.));
        let large = body_size(egui::vec2(900., 700.));
        assert!((large.x - small.x - 300.).abs() < 2.);
        assert!((large.y - small.y - 300.).abs() < 2.);
    }
}
