use super::*;
use crate::test_ui::ContextExt as _;

struct UiHarness {
    app: KetchupApp,
    ctx: egui::Context,
    time: f64,
}
impl UiHarness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut harness = Self {
            app: KetchupApp::with_document(Document::default(), None, true),
            ctx,
            time: 0.,
        };
        harness.frame(vec![]);
        harness.frame(vec![]);
        harness
    }
    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 0.01;
        self.ctx.run_test_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 700.))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                self.app.viewport_panel(ui);
                self.app.rename_viewport_window(&self.ctx);
            },
        )
    }
}

fn touch(id: u64, phase: egui::TouchPhase, pos: Pos2) -> egui::Event {
    egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id),
        phase,
        pos,
        force: None,
    }
}

#[test]
fn pinch_zooms_and_parallel_two_finger_motion_matches_shift_orbit_pan() {
    use egui::TouchPhase::*;
    let mut h = UiHarness::new();
    let center = h.app.viewport.center();
    let a = center - egui::vec2(50., 0.);
    let b = center + egui::vec2(50., 0.);
    let before = h.app.doc.camera.clone();
    h.frame(vec![touch(1, Start, a), touch(2, Start, b)]);
    h.frame(vec![
        touch(1, Move, a - egui::vec2(50., 0.)),
        touch(2, Move, b + egui::vec2(50., 0.)),
    ]);
    let zoomed = h.app.doc.camera.clone();
    assert!((zoomed.distance - before.distance / 2.).abs() < 1e-5);
    let delta = egui::vec2(20., 15.);
    h.frame(vec![
        touch(1, Move, a - egui::vec2(50., 0.) + delta),
        touch(2, Move, b + egui::vec2(50., 0.) + delta),
    ]);
    let mut expected = zoomed;
    expected.pan(
        dv(delta) * h.app.general_settings.pan_speed,
        h.app.viewport.height() as f64,
    );
    assert_eq!(h.app.doc.camera, expected);
}

#[test]
fn three_fingers_do_nothing_and_count_changes_rebase_without_tool_actions() {
    use egui::TouchPhase::*;
    let mut h = UiHarness::new();
    h.app.set_tool(Tool::Line);
    let center = h.app.viewport.center();
    let points = [
        center - egui::vec2(50., 0.),
        center + egui::vec2(50., 0.),
        center + egui::vec2(0., 50.),
    ];
    h.frame(
        points
            .iter()
            .enumerate()
            .map(|(i, p)| touch(i as u64, Start, *p))
            .collect(),
    );
    let expected = h.app.doc.camera.clone();
    let delta = egui::vec2(12., 8.);
    h.frame(
        points
            .iter()
            .enumerate()
            .map(|(i, p)| touch(i as u64, Move, *p + delta))
            .collect(),
    );
    assert_eq!(h.app.doc.camera, expected);
    h.frame(vec![touch(2, End, points[2] + delta)]);
    assert_eq!(h.app.doc.camera, expected);
    // Returning to two fingers uses their current positions as the baseline.
    let pan_delta = egui::vec2(9., 6.);
    h.frame(vec![
        touch(0, Move, points[0] + delta + pan_delta),
        touch(1, Move, points[1] + delta + pan_delta),
    ]);
    let mut panned = expected.clone();
    panned.pan(dv(pan_delta), h.app.viewport.height() as f64);
    assert_eq!(h.app.doc.camera, panned);
    h.frame(vec![
        touch(1, Cancel, points[1] + delta),
        egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    assert!(h.app.line_start.is_none());
    h.frame(vec![touch(0, End, points[0] + delta)]);
    assert!(!h.app.viewports.touch_zoom.captured);
    assert_eq!(h.app.tool, Tool::Line);
}

#[test]
fn contacts_in_different_panes_do_not_navigate() {
    use egui::TouchPhase::*;
    let mut h = UiHarness::new();
    let tab = h.app.viewports.add(0, &h.app.doc.camera);
    h.app.viewports.dock(tab, 0, DropSide::Right);
    h.frame(vec![]);
    let before = h.app.doc.camera.clone();
    let a = egui::pos2(250., h.app.viewport.center().y);
    let b = egui::pos2(650., a.y);
    h.frame(vec![touch(0, Start, a), touch(1, Start, b)]);
    h.frame(vec![
        touch(0, Move, a - egui::vec2(20., 0.)),
        touch(1, Move, b + egui::vec2(20., 0.)),
    ]);
    assert_eq!(h.app.doc.camera, before);
}

fn pair_state() -> TouchZoom {
    TouchZoom {
        contacts: [
            (
                (egui::TouchDeviceId(0), egui::TouchId(0)),
                (egui::pos2(0., 0.), Some(0)),
            ),
            (
                (egui::TouchDeviceId(0), egui::TouchId(1)),
                (egui::pos2(100., 0.), Some(0)),
            ),
        ]
        .into_iter()
        .collect(),
        owner: Some(0),
        captured: true,
        baseline: Some((100., egui::pos2(50., 0.))),
    }
}

#[test]
fn drag_filters_separation_noise_and_pinch_filters_centroid_noise() {
    let mut state = pair_state();
    for (index, (_, contact)) in state.contacts.iter_mut().enumerate() {
        contact.0 += egui::vec2(20. + index as f32, 10.);
    }
    let (pan, zoom, _) = state.motion();
    assert_eq!(pan, egui::vec2(20.5, 10.));
    assert_eq!(zoom, 0.);
    for (index, (_, contact)) in state.contacts.iter_mut().enumerate() {
        contact.0 += egui::vec2(if index == 0 { -10. } else { 11. }, 0.);
    }
    let (pan, zoom, _) = state.motion();
    assert_eq!(pan, Vec2::ZERO);
    assert!(zoom > 0.);
}

#[test]
fn slow_pan_accumulates_until_it_passes_the_dead_zone() {
    let mut state = pair_state();
    for step in 1..=3 {
        for contact in state.contacts.values_mut() {
            contact.0.x += 1.;
        }
        let (pan, zoom, _) = state.motion();
        assert_eq!(zoom, 0.);
        assert_eq!(
            pan,
            if step == 3 {
                egui::vec2(3., 0.)
            } else {
                Vec2::ZERO
            }
        );
    }
}
