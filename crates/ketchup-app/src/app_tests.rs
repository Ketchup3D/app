use super::*;
use eframe::App;

struct Harness {
    ctx: egui::Context,
    app: KetchupApp,
    time: f64,
}
impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut harness = Self {
            ctx,
            app: KetchupApp::with_document(Document::default(), None, true),
            time: 0.,
        };
        harness.frame(vec![], egui::Modifiers::NONE);
        harness.frame(vec![], egui::Modifiers::NONE);
        harness
    }
    fn frame(
        &mut self,
        mut events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        self.time += 0.01;
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1440., 900.))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let app = &mut self.app;
        let mut output = self
            .ctx
            .run_ui(input, |ui| app.ui(ui, &mut eframe::Frame::_new_kittest()));
        output.textures_delta.clear();
        output
    }
    fn pointer(&mut self, pos: Pos2, button: egui::PointerButton, pressed: bool, shift: bool) {
        let modifiers = egui::Modifiers {
            shift,
            ..Default::default()
        };
        self.frame(
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                },
            ],
            modifiers,
        );
    }
    fn click(&mut self, pos: Pos2) {
        self.pointer(pos, egui::PointerButton::Primary, true, false);
        self.pointer(pos, egui::PointerButton::Primary, false, false);
    }
    fn at(&self, p: DVec3) -> Pos2 {
        self.app.viewport.min
            + pv(self
                .app
                .doc
                .camera
                .project(p, dv(self.app.viewport.size()))
                .unwrap())
    }
    fn key(&mut self, key: Key) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![event(true)], egui::Modifiers::NONE);
        self.frame(vec![event(false)], egui::Modifiers::NONE);
    }
}

#[test]
fn click_line_precision_and_axis_lock_through_real_ui() {
    let mut h = Harness::new();
    h.key(Key::L);
    let start = h.at(DVec3::ZERO);
    h.click(start);
    assert_eq!(h.app.line_start, Some(DVec3::ZERO));
    h.key(Key::ArrowUp);
    assert_eq!(h.app.lock, Some(Axis::Z));
    let end = h.at(DVec3::Z * 3.);
    h.frame(
        vec![
            egui::Event::PointerMoved(end),
            egui::Event::Text("2.5".into()),
        ],
        egui::Modifiers::NONE,
    );
    h.key(Key::Enter);
    assert_eq!(h.app.doc.model.edges.len(), 1);
    let e = h.app.doc.model.edges.values().next().unwrap();
    let (a, b) = h.app.doc.model.points(e);
    assert!(a.distance(DVec3::ZERO) < 1e-7);
    assert!(b.distance(DVec3::Z * 2.5) < 1e-5);
    h.key(Key::Escape);
    assert!(h.app.line_start.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 1);
}

#[test]
fn shift_orbit_pans_and_middle_navigation_preserves_pending_line() {
    let mut h = Harness::new();
    h.key(Key::L);
    let origin = h.at(DVec3::ZERO);
    h.click(origin);
    let before = h.app.doc.camera.clone();
    let target = h.app.line_start;
    let pos = h.app.viewport.center();
    h.pointer(pos, egui::PointerButton::Middle, true, true);
    let next = pos + egui::vec2(60., 30.);
    h.frame(
        vec![egui::Event::PointerMoved(next)],
        egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(h.app.doc.camera.yaw, before.yaw);
    assert_eq!(h.app.doc.camera.pitch, before.pitch);
    assert!(h.app.doc.camera.target.distance(before.target) > 0.1);
    let pan_target = h.app.doc.camera.target;
    h.frame(
        vec![egui::Event::PointerMoved(next + egui::vec2(20., 0.))],
        egui::Modifiers::NONE,
    );
    assert_ne!(h.app.doc.camera.yaw, before.yaw);
    assert_eq!(h.app.doc.camera.target, pan_target);
    h.pointer(
        next + egui::vec2(20., 0.),
        egui::PointerButton::Middle,
        false,
        false,
    );
    assert_eq!(h.app.tool, Tool::Line);
    assert_eq!(h.app.line_start, target);
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn orbit_tool_shift_drag_only_translates_camera() {
    let mut h = Harness::new();
    h.key(Key::O);
    let before = h.app.doc.camera.clone();
    let p = h.app.viewport.center();
    h.pointer(p, egui::PointerButton::Primary, true, true);
    h.frame(
        vec![egui::Event::PointerMoved(p + egui::vec2(-80., 40.))],
        egui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    h.pointer(
        p + egui::vec2(-80., 40.),
        egui::PointerButton::Primary,
        false,
        true,
    );
    assert_eq!(h.app.doc.camera.yaw, before.yaw);
    assert_eq!(h.app.doc.camera.pitch, before.pitch);
    assert_ne!(h.app.doc.camera.target, before.target);
    assert!(h.app.doc.model.edges.is_empty());
}

#[test]
fn eraser_sweep_is_one_undo_step_and_ui_panels_never_draw() {
    let mut h = Harness::new();
    h.app
        .doc
        .model
        .add_edge(-DVec3::X * 2., DVec3::X * 2.)
        .unwrap();
    h.app
        .doc
        .model
        .add_edge(DVec3::Y - DVec3::X * 2., DVec3::Y + DVec3::X * 2.)
        .unwrap();
    h.key(Key::E);
    let a = h.at(-DVec3::Y);
    let b = h.at(DVec3::Y * 2.);
    h.pointer(a, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerMoved(b)], egui::Modifiers::NONE);
    h.pointer(b, egui::PointerButton::Primary, false, false);
    assert!(h.app.doc.model.edges.is_empty());
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edges.len(), 2);
    h.app.undo(&h.ctx, false);
    assert_eq!(h.app.doc.model.edges.len(), 2);
    h.app.set_tool(Tool::Line);
    h.click(egui::pos2(1300., 500.));
    assert!(h.app.line_start.is_none());
}

#[test]
fn naming_field_keeps_tool_shortcuts_from_firing() {
    let mut h = Harness::new();
    h.click(egui::pos2(490., 32.));
    assert!(h.ctx.egui_wants_keyboard_input());
    h.key(Key::L);
    assert_eq!(h.app.tool, Tool::Select);
    h.frame(
        vec![egui::Event::Text("Sketch".into())],
        egui::Modifiers::NONE,
    );
    assert!(h.app.doc.name.contains("Sketch"));
    assert_eq!(h.app.tool, Tool::Select);
}

#[test]
fn window_focus_loss_cancels_gesture_without_erasing_geometry() {
    let mut h = Harness::new();
    h.app.doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
    h.key(Key::E);
    let p = h.at(DVec3::ZERO);
    h.pointer(p, egui::PointerButton::Primary, true, false);
    h.frame(vec![egui::Event::PointerGone], egui::Modifiers::NONE);
    assert!(h.app.gesture.is_none());
    assert_eq!(h.app.doc.model.edges.len(), 1);
}

#[test]
#[ignore = "Requires a GPU adapter; writes target/qa/*.png for visual inspection"]
fn gpu_renders_empty_and_modeled_workspace() {
    use eframe::{egui_wgpu, wgpu};
    use std::sync::Arc;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU adapter");
    eprintln!("GPU test adapter: {:?}", adapter.get_info());
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = egui_wgpu::Renderer::new(&device, format, Default::default());
    let state = egui_wgpu::RenderState {
        adapter,
        available_adapters: vec![],
        instance,
        device,
        queue,
        target_format: format,
        renderer: Arc::new(egui::epaint::mutex::RwLock::new(renderer)),
    };
    render_snapshots(state);
}

fn render_snapshots(state: eframe::egui_wgpu::RenderState) {
    use eframe::wgpu;
    let ctx = egui::Context::default();
    apply(&ctx);
    let mut cc = eframe::CreationContext::_new_kittest(ctx.clone());
    cc.wgpu_render_state = Some(state.clone());
    assert!(renderer::install(&cc));
    let mut app = KetchupApp::with_document(Document::default(), None, true);
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/qa");
    std::fs::create_dir_all(&out_dir).unwrap();
    for snapshot in ["empty", "modeled", "compact"] {
        if snapshot == "modeled" {
            app.doc.name = "Courtyard study".into();
            let corners = [
                DVec3::new(-3., -2., 0.),
                DVec3::new(3., -2., 0.),
                DVec3::new(3., 2., 0.),
                DVec3::new(-3., 2., 0.),
            ];
            for i in 0..4 {
                let a = corners[i];
                let b = corners[(i + 1) % 4];
                app.doc.model.add_edge(a, b).unwrap();
                app.doc
                    .model
                    .add_edge(a + DVec3::Z * 3., b + DVec3::Z * 3.)
                    .unwrap();
                app.doc.model.add_edge(a, a + DVec3::Z * 3.).unwrap();
            }
            let ridge_a = DVec3::new(-3., 0., 5.);
            let ridge_b = DVec3::new(3., 0., 5.);
            app.doc.model.add_edge(ridge_a, ridge_b).unwrap();
            for (i, ridge) in [(0, ridge_a), (3, ridge_a), (1, ridge_b), (2, ridge_b)] {
                app.doc
                    .model
                    .add_edge(corners[i] + DVec3::Z * 3., ridge)
                    .unwrap();
            }
            app.selected
                .insert(*app.doc.model.edges.keys().next().unwrap());
            app.doc.camera.target = DVec3::new(0., 0., 2.);
            app.doc.camera.distance = 19.;
        }
        if snapshot == "compact" {
            app.inspector = false;
        }
        let (width, height) = if snapshot == "compact" {
            (900_u32, 600_u32)
        } else {
            (1440_u32, 900_u32)
        };
        let device = &state.device;
        let queue = &state.queue;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Ketchup QA screenshot"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        for frame in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(width as f32, height as f32),
                    )),
                    time: Some(frame as f64 * 0.01),
                    ..Default::default()
                },
                |ui| app.ui(ui, &mut eframe::Frame::_new_kittest()),
            );
            let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
            let screen = eframe::egui_wgpu::ScreenDescriptor {
                size_in_pixels: [width, height],
                pixels_per_point: output.pixels_per_point,
            };
            let mut renderer = state.renderer.write();
            for (id, deltas) in &output.textures_delta.set {
                for delta in deltas {
                    renderer.update_texture(device, queue, *id, delta);
                }
            }
            let mut encoder = device.create_command_encoder(&Default::default());
            let buffers = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
            {
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("QA scene"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
            }
            queue.submit(buffers.into_iter().chain([encoder.finish()]));
            for id in &output.textures_delta.free {
                renderer.free_texture(id);
            }
            output.textures_delta.clear();
        }
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("QA readback"),
            size: stride as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap()
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })
            .unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        let rgba: Vec<u8> = mapped
            .chunks(stride as usize)
            .flat_map(|row| row[..width as usize * 4].iter().copied())
            .collect();
        assert!(
            rgba.chunks(4)
                .any(|pixel| pixel[0] < 100 && pixel[1] < 100 && pixel[2] < 100),
            "UI should contain visible dark content"
        );
        let file = std::fs::File::create(out_dir.join(format!("{snapshot}.png"))).unwrap();
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
        drop(mapped);
        buffer.unmap();
    }
}
