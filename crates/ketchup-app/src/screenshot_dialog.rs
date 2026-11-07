use super::*;
use crate::screenshots::{self, Format, Paper, Settings, VIEW_LABELS};
use eframe::{egui_wgpu, wgpu};
use image::RgbaImage;
use std::sync::mpsc;

#[derive(Default)]
pub(super) struct ScreenshotDialog {
    pub open: bool,
    pub settings: Settings,
    source: Option<Document>,
    preview_settings: Option<Settings>,
    previews: Vec<egui::TextureHandle>,
    preview_page: usize,
    capture: Option<Capture>,
    exporting: bool,
    error: Option<String>,
}

impl KetchupApp {
    pub(super) fn screenshot_dialog(&mut self, ctx: &egui::Context) {
        if !self.screenshot.open {
            return;
        }
        // Older preferences may select JPEG, which cannot retain transparency.
        if self.screenshot.settings.format == Format::Jpeg {
            self.screenshot.settings.format = Format::Png;
        }
        let mut dialog = std::mem::take(&mut self.screenshot);
        let source = dialog.source.get_or_insert_with(|| self.doc.clone());
        if let Some(capture) = &mut dialog.capture {
            match capture.poll(ctx) {
                Ok(Some(images)) => {
                    let pages = screenshots::compose(&capture.settings, &images, capture.page_size);
                    dialog.capture = None;
                    if dialog.exporting {
                        dialog.exporting = false;
                        self.project.operation = projects::Operation::Screenshot;
                        self.project.pending = Some(crate::project_files::export_screenshots(
                            ctx,
                            source.name.clone(),
                            dialog.settings.clone(),
                            pages,
                        ));
                        dialog.open = false;
                    } else {
                        dialog.previews = pages
                            .iter()
                            .enumerate()
                            .map(|(i, page)| {
                                ctx.load_texture(
                                    format!("screenshot-preview-{i}"),
                                    egui::ColorImage::from_rgba_unmultiplied(
                                        [page.width() as usize, page.height() as usize],
                                        page.as_raw(),
                                    ),
                                    egui::TextureOptions::LINEAR,
                                )
                            })
                            .collect();
                    }
                }
                Err(error) => {
                    dialog.error = Some(error);
                    dialog.capture = None;
                    dialog.exporting = false;
                }
                _ => {
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
            }
        }
        if !dialog.open {
            dialog.source = None;
            dialog.preview_settings = None;
            self.screenshot = dialog;
            return;
        }
        let mut export = false;
        let mut cancel = false;
        let response = Modal::new("generate_screenshot", 1100.).show(ctx, |modal| {
            modal.header(|ui| {
                ui.text_heading("Generate screenshot");
                ui.text_weak("Choose your views, page layout and export format.");
            });
            modal.body_unpadded(600., |ui| {
                let size = ui.available_size();
                let (body, _) = ui.allocate_exact_size(size, Sense::hover());
                let stacked = body.width() < 660.;
                let controls_width = if stacked { body.width() } else { 320. };
                let controls = Rect::from_min_size(
                    body.min,
                    egui::vec2(
                        controls_width,
                        if stacked {
                            body.height() * 0.55
                        } else {
                            body.height()
                        },
                    ),
                );
                let preview = if stacked {
                    Rect::from_min_max(egui::pos2(body.left(), controls.bottom()), body.max)
                } else {
                    Rect::from_min_max(egui::pos2(controls.right() + 1., body.top()), body.max)
                };
                ui.scope_builder(egui::UiBuilder::new().max_rect(controls), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(controls));
                    egui::Frame::new().inner_margin(20).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8., 8.);
                        egui::ScrollArea::vertical()
                            .id_salt("screenshot_controls")
                            .max_height((controls.height() - 40.).max(24.))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                screenshot_controls(
                                    ui,
                                    &mut dialog.settings,
                                    dialog.exporting,
                                    dialog.error.as_deref(),
                                )
                            });
                    });
                });
                ui.scope_builder(egui::UiBuilder::new().max_rect(preview), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(preview));
                    screenshot_preview(
                        ui,
                        &mut dialog.preview_page,
                        &dialog.settings,
                        &dialog.previews,
                        dialog.capture.is_some(),
                        dialog.exporting,
                        &source.name,
                    );
                });
                let colors = Palette::for_ui(ui);
                if stacked {
                    ui.painter().hline(
                        body.x_range(),
                        controls.bottom(),
                        Stroke::new(1., colors.border),
                    );
                } else {
                    ui.painter().vline(
                        controls.right(),
                        body.y_range(),
                        Stroke::new(1., colors.border),
                    );
                }
            });
            modal.footer(|ui| {
                cancel = ui.button("Cancel").clicked();
                export = primary_button(
                    ui,
                    "Generate",
                    !dialog.exporting
                        && dialog.settings.validate().is_ok()
                        && self.screenshot_gpu.is_some(),
                )
                .clicked();
            });
        });
        if cancel || response.should_close() {
            dialog.open = false;
            dialog.source = None;
            dialog.capture = None;
            dialog.preview_settings = None;
            dialog.exporting = false;
        } else if export
            || (!dialog.exporting && dialog.preview_settings.as_ref() != Some(&dialog.settings))
        {
            dialog.capture = None;
            dialog.error = None;
            dialog.previews.clear();
            dialog.preview_page = 0;
            dialog.preview_settings = Some(dialog.settings.clone());
            dialog.exporting = export;
            if !dialog.settings.selected().is_empty() {
                if let Some((device, queue)) = &self.screenshot_gpu {
                    match Capture::new(device, queue, ctx, source, &dialog.settings, export) {
                        Ok(capture) => {
                            dialog.capture = Some(capture);
                            ctx.request_repaint();
                        }
                        Err(error) => {
                            dialog.error = Some(error);
                            dialog.exporting = false;
                        }
                    }
                } else {
                    dialog.error =
                        Some("A compatible GPU renderer is required for screenshots.".into());
                }
            }
        }
        self.screenshot = dialog;
    }
}

fn screenshot_controls(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    exporting: bool,
    error: Option<&str>,
) {
    ui.add_enabled_ui(!exporting, |ui| {
        ui.horizontal(|ui| {
            ui.text_strong("Views");
            ui.text_weak(format!("{} selected", settings.selected().len()));
        });
        ui.text_small("Choose one or more views.");
        egui::Grid::new("screenshot_views")
            .num_columns(2)
            .spacing(egui::vec2(16., 6.))
            .show(ui, |ui| {
                let width = (ui.available_width() - 16.) / 2.;
                for (i, label) in VIEW_LABELS.iter().enumerate() {
                    ui.add_sized(
                        egui::vec2(width, 28.),
                        egui::Checkbox::new(&mut settings.views[i], *label),
                    )
                    .on_hover_text(if i == 0 {
                        "Fit visible content using the current viewing angle and projection."
                    } else {
                        "Fit the visible model in parallel projection."
                    });
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        screenshot_group_divider(ui);
        ui.text_strong("Page");
        ui.radio_value(&mut settings.combined, false, "One image per view");
        ui.radio_value(&mut settings.combined, true, "All views in one image");
        ui.add_space(6.);
        ui.text_label("Paper size");
        egui::ComboBox::from_id_salt("screenshot_paper")
            .width(ui.available_width())
            .selected_text(settings.paper.label())
            .show_ui(ui, |ui| {
                for paper in Paper::ALL {
                    ui.selectable_value(&mut settings.paper, paper, paper.label());
                }
            });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut settings.landscape, false, "Portrait");
            ui.selectable_value(&mut settings.landscape, true, "Landscape");
        });
        screenshot_group_divider(ui);
        ui.text_strong("Export");
        ui.columns(2, |columns| {
            columns[0].text_label("Format");
            egui::ComboBox::from_id_salt("screenshot_format")
                .width(columns[0].available_width())
                .selected_text(settings.format.label())
                .show_ui(&mut columns[0], |ui| {
                    for format in Format::ALL {
                        ui.selectable_value(&mut settings.format, format, format.label());
                    }
                });
            columns[1].text_label("Resolution");
            egui::ComboBox::from_id_salt("screenshot_dpi")
                .width(columns[1].available_width())
                .selected_text(format!("{} DPI", settings.dpi))
                .show_ui(&mut columns[1], |ui| {
                    for dpi in [96, 150, 300] {
                        ui.selectable_value(&mut settings.dpi, dpi, format!("{dpi} DPI"));
                    }
                });
        });
        let [w, h] = settings.pixels();
        ui.text_small(format!("{w} × {h} pixels per sheet"));
        ui.text_small("Light mode · Transparent background");
        if !settings.combined && settings.selected().len() > 1 {
            ui.text_small(if settings.format == Format::Pdf {
                "One PDF page per view."
            } else {
                "Separate images packaged in a ZIP file."
            });
        }
    });
    if let Err(error) = settings.validate() {
        ui.text_colored_label(ACCENT, error);
    }
    if let Some(error) = error {
        ui.text_colored_label(ACCENT, error);
    }
}

fn screenshot_group_divider(ui: &mut egui::Ui) {
    ui.add_space(10.);
    ui.separator();
    ui.add_space(10.);
}

fn preview_paper_rect(viewport: Rect, settings: &Settings) -> Rect {
    let [width, height] = settings.paper_mm().map(|v| v as f32);
    let available = (viewport.size() - Vec2::splat(32.)).max(Vec2::ZERO);
    let scale = (available.x / width).min(available.y / height);
    Rect::from_center_size(viewport.center(), egui::vec2(width, height) * scale)
}

fn screenshot_preview(
    ui: &mut egui::Ui,
    page: &mut usize,
    settings: &Settings,
    previews: &[egui::TextureHandle],
    rendering: bool,
    exporting: bool,
    project_name: &str,
) {
    let colors = Palette::for_ui(ui);
    let bounds = ui.max_rect();
    ui.painter().rect_filled(bounds, 0., colors.canvas);
    egui::Frame::new()
        .fill(colors.paper)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(6., 6.);
            ui.horizontal(|ui| {
                ui.text_strong("Preview viewport");
                ui.text_weak(if settings.combined {
                    "Combined sheet"
                } else {
                    "Individual views"
                });
            });
            let [width, height] = settings.paper_mm();
            ui.text_weak(format!("{width:.0} × {height:.0} mm · {}",
                if settings.landscape { "Landscape" } else { "Portrait" }));
            if !settings.combined {
                ui.horizontal_wrapped(|ui| {
                    for (index, &view) in settings.selected().iter().enumerate() {
                        ui.selectable_value(page, index, VIEW_LABELS[view]);
                    }
                });
            }
        });
    let viewport = Rect::from_min_max(egui::pos2(bounds.left(), ui.cursor().top()), bounds.max);
    let painter = ui.painter().with_clip_rect(viewport);
    let paper = preview_paper_rect(viewport, settings);
    painter.rect_filled(paper.translate(egui::vec2(0., 3.)), 0., Color32::from_black_alpha(24));
    painter.rect_filled(paper, 0., Color32::WHITE);
    let pdf_header = (settings.format == Format::Pdf).then(|| {
        let points = settings.paper_mm().map(|v| v * 72. / 25.4);
        let date = crate::estimate_pdf::generation_date().unwrap_or_default();
        let header = crate::pdf_header::layout(project_name,"Model Views",&date,points,crate::pdf_header::base_width);
        let scale = paper.width() / points[0] as f32;
        crate::brand_mark::paint(&painter,Rect::from_min_size(paper.min+Vec2::splat(42.*scale),Vec2::splat(32.*scale)));
        for text in &header.text {
            painter.text(egui::pos2(paper.left()+text.x as f32*scale,paper.bottom()-text.y as f32*scale),
                Align2::LEFT_BOTTOM,&text.value,FontId::proportional(text.size as f32*scale),Color32::from_rgb(38,51,43));
        }
        let y=paper.bottom()-(header.body_top+31.)as f32*scale;
        painter.hline(paper.left()+42.*scale..=paper.right()-42.*scale,y,Stroke::new(0.5*scale,Color32::from_gray(210)));
        (header,points,scale)
    });
    if let Some(texture) = previews.get((*page).min(previews.len().saturating_sub(1))) {
        let image = if let Some((header,[w,_],scale))=&pdf_header {
            let aspect=f64::from(texture.size_vec2().x/texture.size_vec2().y);
            let width=(w-84.).min((header.body_top-42.).max(1.)*aspect);
            let height=width/aspect;
            let x=(w-width)*0.5;
            let y=42.+(header.body_top-42.-height).max(0.)*0.5;
            Rect::from_min_size(egui::pos2(paper.left()+x as f32*scale,paper.bottom()-(y+height)as f32*scale),egui::vec2(width as f32,height as f32)* *scale)
        } else { paper };
        painter.image(
            texture.id(),
            image,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
    } else {
        painter.text(
            viewport.center(),
            Align2::CENTER_CENTER,
            if settings.selected().is_empty() {
                "Select a view to preview"
            } else if rendering && exporting {
                "Rendering export…"
            } else if rendering {
                "Preparing preview…"
            } else {
                "Preview unavailable"
            },
            FontId::proportional(14.),
            Color32::from_gray(100),
        );
    }
    painter.rect_stroke(paper, 0., Stroke::new(1., Color32::from_gray(180)), egui::StrokeKind::Inside);
    if rendering {
        egui::Spinner::new().color(Color32::from_gray(100)).paint_at(
            ui,
            Rect::from_center_size(viewport.center() - egui::vec2(0., 30.), Vec2::splat(22.)),
        );
    }
}

// Preserve the viewport's effective shadows, materials, theme and fog colors.
// Export hides editing aids without silently choosing a different render style.
fn capture_document(doc: &Document, dark: bool) -> Document {
    let mut source = doc.clone();
    source.display = display_scene::effective_display(&doc.display, &doc.model, dark);
    source.model.edit_context = None;
    source.selection.clear();
    source.selection_context = None;
    source.display.axes = false;
    source.display.grid = false;
    source.display.guides = false;
    source.display.group_emphasis = false;
    source
}

struct Readback {
    buffer: wgpu::Buffer,
    ready: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    size: [u32; 2],
    stride: u32,
    image: Option<RgbaImage>,
}
struct Capture {
    #[cfg(not(target_arch = "wasm32"))]
    device: wgpu::Device,
    settings: Settings,
    page_size: [u32; 2],
    views: Vec<Readback>,
    started: f64,
}
impl Capture {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        parent: &egui::Context,
        doc: &Document,
        settings: &Settings,
        export: bool,
    ) -> Result<Self, String> {
        if export {
            settings.validate()?;
        }
        let mut page_size = settings.pixels();
        if !export {
            let scale = 900. / page_size[0].max(page_size[1]) as f64;
            page_size = page_size.map(|v| (v as f64 * scale).round().max(1.) as u32);
        }
        let [cols, rows] = settings.grid();
        let size = [page_size[0] / cols, page_size[1] / rows];
        if size
            .iter()
            .any(|&v| v == 0 || v > device.limits().max_texture_dimension_2d)
        {
            return Err("This image exceeds the GPU texture size. Choose smaller paper or a lower resolution.".into());
        }
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut renderer = egui_wgpu::Renderer::new(
            device,
            format,
            egui_wgpu::RendererOptions {
                depth_stencil_format: Some(wgpu::TextureFormat::Depth32Float),
                dithering: false,
                ..Default::default()
            },
        );
        renderer::install_resources(device, format, &mut renderer);
        let ctx = egui::Context::default();
        apply(&ctx);
        ctx.set_visuals(parent.global_style().visuals.clone());
        let scale = (page_size[0] as f32 / settings.pixels()[0] as f32
            * settings.dpi.clamp(72, 300) as f32
            / 96.)
            .max(0.1);
        ctx.set_pixels_per_point(scale);
        let source = capture_document(doc, parent.global_style().visuals.dark_mode);
        let current = source.camera.clone();
        let mut app = KetchupApp::with_document(source, None, true);
        app.doc.display.group_emphasis = false;
        let mut views = Vec::new();
        for view in settings.selected() {
            let logical = egui::vec2(size[0] as f32, size[1] as f32) / scale;
            let rect = Rect::from_min_size(Pos2::ZERO, logical);
            // Leave a title and a margin around every view, including sheets.
            let scene =
                Rect::from_min_max(rect.min + egui::vec2(16., 36.), rect.max - Vec2::splat(16.));
            app.doc.camera = screenshots::view_camera(
                &app.doc.model,
                &current,
                view,
                [scene.width().max(1.) as u32, scene.height().max(1.) as u32],
            );
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    app.paint_scene_with_background(ui, scene, false);
                    ui.painter().text(
                        egui::pos2(16., 16.),
                        Align2::LEFT_CENTER,
                        VIEW_LABELS[view],
                        FontId::proportional(14.),
                        INK,
                    );
                },
            );
            let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
            let screen = egui_wgpu::ScreenDescriptor {
                size_in_pixels: size,
                pixels_per_point: output.pixels_per_point,
            };
            // Consuming the deltas acknowledges them to egui. Dropping an
            // applied but non-empty list panics and halts the web app.
            for (id, deltas) in output.textures_delta.set.drain() {
                for delta in deltas {
                    renderer.update_texture(device, queue, id, &delta);
                }
            }
            let extent = wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Screenshot color"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let depth = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Screenshot depth"),
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                ..texture_descriptor(extent)
            });
            let target = texture.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            let commands = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
            {
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Screenshot viewport"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
            }
            let stride = (size[0] * 4).div_ceil(256) * 256;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Screenshot readback"),
                size: stride as u64 * size[1] as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(size[1]),
                    },
                },
                extent,
            );
            queue.submit(commands.into_iter().chain([encoder.finish()]));
            let (send, ready) = mpsc::channel();
            let parent = parent.clone();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = send.send(result);
                    parent.request_repaint();
                });
            for id in output.textures_delta.free.drain() {
                renderer.free_texture(&id);
            }
            views.push(Readback {
                buffer,
                ready,
                size,
                stride,
                image: None,
            });
        }
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            device: device.clone(),
            settings: settings.clone(),
            page_size,
            views,
            started: parent.input(|i| i.time),
        })
    }
    fn poll(&mut self, ctx: &egui::Context) -> Result<Option<Vec<RgbaImage>>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        self.device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| e.to_string())?;
        if ctx.input(|i| i.time) - self.started > 60. {
            return Err(
                "Screenshot rendering timed out. Try fewer views or a lower resolution.".into(),
            );
        }
        for view in &mut self.views {
            if view.image.is_some() {
                continue;
            }
            match view.ready.try_recv() {
                Ok(result) => {
                    result.map_err(|e| format!("Could not read screenshot: {e}"))?;
                    let mapped = view
                        .buffer
                        .slice(..)
                        .get_mapped_range()
                        .map_err(|e| e.to_string())?;
                    let mut rgba: Vec<u8> = mapped
                        .chunks(view.stride as usize)
                        .flat_map(|row| row[..view.size[0] as usize * 4].iter().copied())
                        .collect();
                    // The renderer blends premultiplied colors. Image files
                    // and ColorImage::from_rgba_unmultiplied need straight alpha.
                    screenshots::unpremultiply(&mut rgba);
                    view.image = RgbaImage::from_raw(view.size[0], view.size[1], rgba);
                    drop(mapped);
                    view.buffer.unmap();
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(_) => return Err("Screenshot readback did not complete.".into()),
            }
        }
        Ok(Some(
            self.views
                .iter_mut()
                .filter_map(|view| view.image.take())
                .collect(),
        ))
    }
}
fn texture_descriptor(size: wgpu::Extent3d) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn preview_paper_matches_orientation_and_keeps_a_visible_margin() {
        let viewport=Rect::from_min_size(Pos2::ZERO,egui::vec2(640.,480.));
        for landscape in [false,true] {
            let settings=Settings{landscape,..Default::default()};
            let paper=preview_paper_rect(viewport,&settings);
            let [w,h]=settings.paper_mm();
            assert!((f64::from(paper.width()/paper.height())-w/h).abs()<1e-5);
            assert!(paper.center().distance(viewport.center())<1e-4);
            assert!(viewport.shrink(15.).contains_rect(paper));
        }
    }
    #[test]
    fn capture_preserves_viewport_rendering_settings_without_exporting_selection() {
        let mut doc=Document::default();
        doc.display.shadows=true;doc.display.shadow_antialiasing=true;
        doc.display.fog=true;doc.display.background=[36,42,48];
        doc.selection.insert(123);
        let captured=capture_document(&doc,true);
        let expected=display_scene::effective_display(&doc.display,&doc.model,true);
        assert_eq!(captured.display.shadows,expected.shadows);
        assert_eq!(captured.display.shadow_antialiasing,expected.shadow_antialiasing);
        assert_eq!(captured.display.background,expected.background);
        assert_eq!(captured.display.sun_angle,expected.sun_angle);
        assert!(captured.selection.is_empty());
        assert!(!captured.display.axes && !captured.display.grid);
        assert!(doc.selection.contains(&123));
    }


    #[test]
    #[ignore = "Requires GPU; verifies an isometric shadow interior does not stripe"]
    fn antialiased_isometric_shadow_has_uniform_interior() {
        let mut tested=0;
        for backend in [wgpu::Backends::PRIMARY,wgpu::Backends::GL] {
        let mut descriptor=wgpu::InstanceDescriptor::new_without_display_handle();descriptor.backends=backend;
        let instance=wgpu::Instance::new(descriptor);
        let Ok(adapter)=pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {eprintln!("Backend {backend:?} unavailable");continue;};
        eprintln!("Shadow capture backend: {:?}",adapter.get_info().backend);tested+=1;
        let (device,queue)=pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let ctx=egui::Context::default();apply(&ctx);ctx.set_visuals(egui::Visuals::light());
        let mut doc=Document::default();
        for (min,max) in [(DVec3::new(-3.,-3.,-0.2),DVec3::new(3.,3.,0.)),(DVec3::ZERO,DVec3::new(1.,1.,1.))] {
            let mut body=Model::default();
            let p=[min,DVec3::new(max.x,min.y,min.z),DVec3::new(max.x,max.y,min.z),DVec3::new(min.x,max.y,min.z)];
            for i in 0..4 {body.add_edge(p[i],p[(i+1)%4]).unwrap();}
            let face=*body.faces.keys().next().unwrap();
            let normal=ketchup_core::faces::plane(&body,&body.faces[&face].vertices).unwrap().normal;
            body.push_pull(face,(max.z-min.z)/normal.z).unwrap();
            let ids=doc.model.paste_entities(&body,DVec3::ZERO);
            doc.model.make_object(&ids,false,"Shadow fixture").unwrap();
        }
        doc.display.shadows=true;doc.display.shadow_antialiasing=true;
        doc.display.shadow_darkness=0.7;doc.display.sun_angle=135.;doc.display.sun_elevation=45.;
        doc.display.edges=false;doc.display.profiles=false;doc.display.fog=false;
        let mut settings=Settings{views:[false;8],paper:Paper::A5,dpi:96,..Default::default()};settings.views[7]=true;
        for export in [false,true] {
            let mut capture=Capture::new(&device,&queue,&ctx,&doc,&settings,export).unwrap();
            device.poll(wgpu::PollType::Wait{submission_index:None,timeout:Some(Duration::from_secs(30))}).unwrap();
            let pages=capture.poll(&ctx).unwrap().unwrap();let page=&pages[0];
            let scale=(capture.page_size[0] as f32/settings.pixels()[0] as f32*settings.dpi as f32/96.).max(0.1);
            let logical=egui::vec2(page.width() as f32,page.height() as f32)/scale;
            let scene=Rect::from_min_max(egui::pos2(16.,36.),logical.to_pos2()-Vec2::splat(16.));
            let camera=screenshots::view_camera(&doc.model,&doc.camera,7,[scene.width() as u32,scene.height() as u32]);
            let mut samples=Vec::new();
            for i in 0..16 {for j in 0..16 {
                let point=DVec3::new(1.2+i as f64*0.02,-0.5+j as f64*0.02,0.);
                let pixel=(scene.min+pv(camera.project(point,dv(scene.size())).unwrap())).to_vec2()*scale;
                let rgba=page.get_pixel(pixel.x.round()as u32,pixel.y.round()as u32);
                assert_eq!(rgba[3],255);samples.push(rgba[0]);
            }}
            std::fs::create_dir_all("/tmp/ketchup-shadow-qa").unwrap();
            page.save(format!("/tmp/ketchup-shadow-qa/isometric-{}.png",if export{"export"}else{"preview"})).unwrap();
            let lo=*samples.iter().min().unwrap();let hi=*samples.iter().max().unwrap();
            assert!(hi-lo<=5,"shadow stripes in export={export}: {lo}..{hi}");
        }
        }
        assert!(tested>0);
    }

    #[test]
    #[ignore = "Requires a GPU adapter; verifies every visible isometric face in preview and export"]
    fn isometric_capture_keeps_top_front_and_right_faces_opaque() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut doc = Document::default();
        let corners = [
            DVec3::new(-1., -1., -1.),
            DVec3::new(1., -1., -1.),
            DVec3::new(1., 1., -1.),
            DVec3::new(-1., 1., -1.),
        ];
        for i in 0..4 {
            doc.model
                .add_edge(corners[i], corners[(i + 1) % 4])
                .unwrap();
        }
        let face = *doc.model.faces.keys().next().unwrap();
        let normal = ketchup_core::faces::plane(&doc.model, &doc.model.faces[&face].vertices)
            .unwrap()
            .normal;
        doc.model.push_pull(face, 2. / normal.z).unwrap();
        assert_eq!(doc.model.faces.len(), 6);
        let colors: Vec<_> = doc
            .model
            .faces
            .values()
            .map(|face| {
                let center = face
                    .vertices
                    .iter()
                    .map(|id| doc.model.vertices[id].position)
                    .sum::<DVec3>()
                    / face.vertices.len() as f64;
                let rgb = if center.x > 0.99 {
                    [200, 50, 50]
                } else if center.y < -0.99 {
                    [50, 180, 50]
                } else if center.z > 0.99 {
                    [50, 70, 200]
                } else {
                    [190, 180, 40]
                };
                (face.id, rgb)
            })
            .collect();
        for (id, rgb) in colors {
            doc.model.paint_face(id, rgb).unwrap();
        }
        let mut settings = Settings {
            views: [false; 8],
            paper: Paper::A5,
            dpi: 96,
            ..Default::default()
        };
        settings.views[7] = true;
        for export in [false, true] {
            let mut capture = Capture::new(&device, &queue, &ctx, &doc, &settings, export).unwrap();
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(30)),
                })
                .unwrap();
            let pages = capture.poll(&ctx).unwrap().unwrap();
            let page = &pages[0];
            let scale = (capture.page_size[0] as f32 / settings.pixels()[0] as f32
                * settings.dpi as f32
                / 96.)
                .max(0.1);
            let logical = egui::vec2(page.width() as f32, page.height() as f32) / scale;
            let scene =
                Rect::from_min_max(egui::pos2(16., 36.), logical.to_pos2() - Vec2::splat(16.));
            let camera = screenshots::view_camera(
                &doc.model,
                &doc.camera,
                7,
                [scene.width() as u32, scene.height() as u32],
            );
            for (point, channel) in [(DVec3::X, 0), (-DVec3::Y, 1), (DVec3::Z, 2)] {
                let pixel = (scene.min + pv(camera.project(point, dv(scene.size())).unwrap()))
                    .to_vec2()
                    * scale;
                let rgba = page.get_pixel(pixel.x.round() as u32, pixel.y.round() as u32);
                assert_eq!(rgba[3], 255, "isometric face at {point:?} disappeared");
                for other in 0..3 {
                    if other != channel {
                        assert!(
                            rgba[channel] > rgba[other] + 30,
                            "wrong surface at {point:?}: {rgba:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "Requires a GPU adapter; exercises preview and export texture lifecycles"]
    fn screenshot_capture_consumes_font_updates_and_returns_each_view() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut doc = Document::default();
        doc.model.add_edge(-DVec3::X, DVec3::X).unwrap();
        let before = doc.clone();
        let settings = Settings {
            combined: false,
            paper: Paper::A5,
            dpi: 96,
            ..Default::default()
        };
        // The first view produces font texture deltas. Before the fix this
        // panicked when that view's output was dropped, before readback.
        for export in [false, true] {
            let mut baseline = None;
            for theme in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
                ctx.set_theme(theme);
                let mut capture =
                    Capture::new(&device, &queue, &ctx, &doc, &settings, export).unwrap();
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(Duration::from_secs(30)),
                    })
                    .unwrap();
                let pages = capture
                    .poll(&ctx)
                    .unwrap()
                    .expect("all readbacks completed");
                assert_eq!(pages.len(), settings.selected().len());
                for page in &pages {
                    assert_eq!([page.width(), page.height()], capture.page_size);
                    assert_eq!(page.get_pixel(0, 0).0, [0; 4]);
                    assert!(page.pixels().any(|pixel| pixel[3] > 0));
                }
                if let Some(baseline) = &baseline {
                    assert_eq!(
                        &pages, baseline,
                        "screenshot output must ignore the app theme"
                    );
                } else {
                    baseline = Some(pages);
                }
            }
        }
        assert_eq!(doc, before);
    }
}
