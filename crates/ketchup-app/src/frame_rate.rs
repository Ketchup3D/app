//! Optional frame timing for performance diagnostics on desktop and web.
use eframe::egui::{self, FontId, Rect};

#[derive(Clone, Default)]
struct FrameRate {
    last_frame: Option<u64>,
    last_time: Option<f64>,
    elapsed: f64,
    frames: u32,
    fps: Option<f64>,
}

impl FrameRate {
    fn sample(&mut self, frame: u64, time: f64) -> Option<f64> {
        // Layout passes and multiple panes still represent just one rendered frame.
        if self.last_frame == Some(frame) || !time.is_finite() {
            return self.fps;
        }
        self.last_frame = Some(frame);
        if let Some(previous) = self.last_time.replace(time) {
            let dt = time - previous;
            if dt <= 0. || dt > 1. {
                self.elapsed = 0.;
                self.frames = 0;
                self.fps = None;
            } else {
                self.elapsed += dt;
                self.frames += 1;
                if self.elapsed >= 0.5 {
                    self.fps = Some(self.frames as f64 / self.elapsed);
                    self.elapsed = 0.;
                    self.frames = 0;
                }
            }
        }
        self.fps
    }
}

pub(crate) fn paint(ui: &egui::Ui, viewport: Rect) {
    let ctx = ui.ctx();
    let frame = ctx.cumulative_frame_nr();
    let time = ctx.input(|input| input.time);
    let fps = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<FrameRate>(egui::Id::unique("diagnostics_frame_rate"))
            .sample(frame, time)
    });
    // Measure continuous rendering while the indicator is enabled.
    ctx.request_repaint();
    let text = fps.map_or_else(|| "— FPS".into(), |fps| format!("{fps:.0} FPS"));
    let colors = crate::theme::Palette::for_ui(ui);
    let painter = ui
        .painter()
        .with_clip_rect(viewport.intersect(ui.clip_rect()));
    let galley = painter.layout_no_wrap(text, FontId::monospace(11.), colors.ink);
    let size = galley.size() + egui::vec2(12., 8.);
    let rect = Rect::from_min_size(viewport.left_bottom() + egui::vec2(8., -8. - size.y), size);
    painter.rect_filled(rect, 4, colors.paper.gamma_multiply(0.94));
    painter.galley(rect.min + egui::vec2(6., 4.), galley, colors.ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_rendered_frames_once_and_resets_after_suspension() {
        let mut rate = FrameRate::default();
        for frame in 0..=60 {
            rate.sample(frame, frame as f64 / 60.);
            rate.sample(frame, frame as f64 / 60.);
        }
        assert!((rate.fps.unwrap() - 60.).abs() < 0.01);
        assert_eq!(rate.sample(61, 10.), None);
        for frame in 62..=80 {
            rate.sample(frame, 10. + (frame - 61) as f64 / 30.);
        }
        assert!((rate.fps.unwrap() - 30.).abs() < 0.01);
    }
}

pub(crate) fn reset(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<FrameRate>(egui::Id::unique("diagnostics_frame_rate")));
}
