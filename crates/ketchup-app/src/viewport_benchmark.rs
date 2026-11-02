//! Opt-in native viewport workload. Uses only the launcher's isolated project
//! copy and preferences; never enabled by a saved user setting.
use super::*;
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, time::Instant};

thread_local! {
    static TIMINGS: RefCell<Option<BTreeMap<&'static str, f64>>> = const { RefCell::new(None) };
}

pub(crate) struct Scope(Option<(&'static str, Instant)>);
pub(crate) fn scope(name: &'static str) -> Scope {
    Scope(TIMINGS.with(|timings| timings.borrow().as_ref().map(|_| (name, Instant::now()))))
}
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some((name, start)) = self.0 {
            TIMINGS.with(|timings| {
                if let Some(timings) = timings.borrow_mut().as_mut() {
                    *timings.entry(name).or_default() += start.elapsed().as_secs_f64() * 1000.;
                }
            });
        }
    }
}

pub(super) struct Benchmark {
    output: PathBuf,
    environment: serde_json::Value,
    camera: Camera,
    editing_connection: Option<u64>,
    edited_settings: bool,
    stage: usize,
    stage_start: Instant,
    last_frame: Instant,
    last_number: Option<u64>,
    seconds: f64,
    samples: Vec<f64>,
    cpu: Vec<f64>,
    scopes: BTreeMap<String, Vec<f64>>,
    reports: Vec<serde_json::Value>,
}

impl Benchmark {
    pub(super) fn from_environment(app: &mut KetchupApp) -> Option<Self> {
        let output = std::env::var_os("KETCHUP_VIEWPORT_BENCHMARK")?;
        let seconds = std::env::var("KETCHUP_BENCH_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(12.)
            .clamp(4., 120.);
        let editing_connection =
            std::env::var("KETCHUP_BENCH_EDIT_JOINERY")
                .ok()
                .map(|requested| {
                    app.doc
                        .model
                        .connections
                        .iter()
                        .find(|(id, connection)| {
                            matches!(
                                connection.operation,
                                ketchup_core::connections::ConnectionOperation::Joinery(_)
                            ) && (requested == "auto"
                                || requested.parse::<u64>().ok() == Some(**id))
                        })
                        .map(|(&id, _)| id)
                        .expect("The benchmark requires an existing wood joint")
                });
        app.general_settings.show_fps = true;
        app.release_notes.open = false;
        app.set_tool(Tool::Orbit);
        let profiling = std::env::var("KETCHUP_BENCH_PROFILE").as_deref() == Ok("1");
        TIMINGS.with(|timings| *timings.borrow_mut() = profiling.then(BTreeMap::new));
        let now = Instant::now();
        eprintln!(
            "KETCHUP_BENCH starting: {} ({} faces, {} connections), {}s per stage",
            app.doc.name,
            app.doc.model.faces.len(),
            app.doc.model.connections.len(),
            seconds
        );
        Some(Self {
            output: output.into(),
            environment: serde_json::Value::Null,
            camera: app.doc.camera.clone(),
            editing_connection,
            edited_settings: false,
            stage: 0,
            stage_start: now,
            last_frame: now,
            last_number: None,
            seconds,
            samples: Vec::new(),
            cpu: Vec::new(),
            scopes: BTreeMap::new(),
            reports: Vec::new(),
        })
    }

    pub(super) fn tick(
        &mut self,
        app: &mut KetchupApp,
        ctx: &egui::Context,
        frame: &eframe::Frame,
    ) {
        let number = ctx.cumulative_frame_nr();
        if self.last_number == Some(number) {
            return;
        }
        if self.last_number.is_none() {
            if let Some(id) = self.editing_connection {
                let members = app.doc.model.connections[&id].members;
                app.selected = members.into_iter().collect();
                app.open_joinery(ctx);
                assert!(app.joinery.open, "Could not open the Wood Joinery editor");
                app.set_tool(Tool::Orbit);
                sidebar::set_section_open(ctx, "connections/wood_joinery", true);
            }
            let adapter = frame.wgpu_render_state().map(|render| {
                let info = render.adapter.get_info();
                serde_json::json!({"gpu": info.name, "backend": format!("{:?}", info.backend),
                    "driver": info.driver, "driver_info": info.driver_info})
            });
            self.environment = serde_json::json!({"adapter": adapter,
                "window_points": [ctx.viewport_rect().width(), ctx.viewport_rect().height()],
                "pixels_per_point": ctx.pixels_per_point(), "debug_assertions": cfg!(debug_assertions),
                "surface_config": format!("{:?}", frame.wgpu_surface_config()),
                "editing_connection": self.editing_connection});
        }
        self.last_number = Some(number);
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64() * 1000.;
        self.last_frame = now;
        let elapsed = now.duration_since(self.stage_start).as_secs_f64();
        let previous = TIMINGS.with(|timings| {
            timings
                .borrow_mut()
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default()
        });
        if elapsed >= 2. {
            self.samples.push(dt);
            if let Some(cpu) = frame.info().cpu_usage {
                self.cpu.push(f64::from(cpu) * 1000.);
            }
            for (name, ms) in previous {
                self.scopes.entry(name.into()).or_default().push(ms);
            }
        }
        if elapsed >= self.seconds + 2. {
            let output = self.output.with_file_name(format!(
                "{}-{}.png",
                self.output.file_stem().unwrap().to_string_lossy(),
                self.stage
            ));
            ctx.request_screenshot(move |image| {
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Err(error) = image::save_buffer(
                    &output,
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("KETCHUP_BENCH screenshot failed: {error}");
                }
            });
            if self.editing_connection.is_some() {
                assert!(app.joinery.open, "The editor closed during its benchmark");
                assert!(
                    app.joinery_geometry
                        .result
                        .as_ref()
                        .is_some_and(|evaluation| evaluation.errors.is_empty()),
                    "The edited joint preview must remain valid"
                );
            }
            self.report();
            self.stage += 1;
            if self.stage == 3 {
                if let Some(id) = self.editing_connection {
                    let before = app.doc.model.clone();
                    let original = app.doc.model.connections[&id].clone();
                    let connection = app.doc.model.connections.get_mut(&id).unwrap();
                    let ketchup_core::connections::ConnectionOperation::Joinery(settings) =
                        &mut connection.operation
                    else {
                        unreachable!()
                    };
                    settings.clearance += 0.00001;
                    app.finish_edit("Edit dynamic wood joint", before, ctx);
                    assert_ne!(
                        app.doc.model.connections[&id], original,
                        "The benchmark edit was rejected"
                    );
                    self.edited_settings = true;
                }
            }
            self.stage_start = now;
            self.samples.clear();
            self.cpu.clear();
            self.scopes.clear();
            if self.stage == 4 {
                app.doc.camera = self.camera.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        app.release_notes.open = false;
        app.doc.display.wood_joinery = self.stage != 1;
        app.doc.camera = self.camera.clone();
        if self.stage == 1 || self.stage == 2 {
            let t = now.duration_since(self.stage_start).as_secs_f64();
            app.doc.camera.yaw += 0.35 * (t * 0.8).sin();
            app.doc.camera.pitch += 0.1 * (t * 0.65).sin();
        }
        ctx.request_repaint();
    }

    fn report(&mut self) {
        let stats = |samples: &[f64]| {
            let mut values = samples.to_vec();
            values.sort_by(f64::total_cmp);
            let percentile = |p: f64| {
                values
                    .get(((values.len().saturating_sub(1)) as f64 * p).round() as usize)
                    .copied()
                    .unwrap_or(0.)
            };
            let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
            serde_json::json!({"frames": values.len(), "mean_ms": mean, "p50_ms": percentile(0.5),
                "p95_ms": percentile(0.95), "p99_ms": percentile(0.99), "max_ms": percentile(1.),
                "fps": if mean > 0. { 1000. / mean } else { 0. }})
        };
        let name = if self.editing_connection.is_some() {
            [
                "edit_static_on",
                "edit_orbit_off",
                "edit_orbit_on",
                "edit_after_change_on",
            ][self.stage]
        } else {
            ["static_on", "orbit_off", "orbit_on", "static_on_final"][self.stage]
        };
        let scopes: BTreeMap<_, _> = self
            .scopes
            .iter()
            .map(|(name, values)| (name, stats(values)))
            .collect();
        let report = serde_json::json!({"stage": name, "environment": self.environment, "frame": stats(&self.samples),
            "eframe_cpu": stats(&self.cpu), "scopes": scopes, "edited_settings": self.edited_settings});
        eprintln!("KETCHUP_BENCH {}", report);
        self.reports.push(report);
        if let Some(parent) = self.output.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(
            &self.output,
            serde_json::to_vec_pretty(&self.reports).unwrap(),
        ) {
            eprintln!("KETCHUP_BENCH report failed: {error}");
        }
    }
}
