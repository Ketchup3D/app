use eframe::{egui, egui_wgpu, wgpu};
use egui::{Color32, Rect};
use ketchup_core::{Camera, DVec2, DVec3};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
    lateral: f32,
}

pub struct Mesh {
    vertices: Vec<Vertex>,
    size: DVec2,
}
impl Mesh {
    pub fn new(size: DVec2) -> Self {
        Self {
            vertices: Vec::new(),
            size,
        }
    }
    pub fn line(&mut self, a: DVec2, b: DVec2, width: f64, color: Color32) {
        let delta = b - a;
        if delta.length_squared() < 0.01 {
            return;
        }
        let n = DVec2::new(-delta.y, delta.x).normalize() * (width + 1.0) * 0.5;
        let points = [a + n, a - n, b + n, b + n, a - n, b - n];
        for (p, lateral) in points.into_iter().zip([1., -1., 1., 1., -1., -1.]) {
            self.vertices.push(Vertex {
                position: [
                    (p.x / self.size.x * 2. - 1.) as f32,
                    (1. - p.y / self.size.y * 2.) as f32,
                ],
                color: color.to_normalized_gamma_f32(),
                lateral,
            });
        }
    }
    pub fn world_line(&mut self, camera: &Camera, a: DVec3, b: DVec3, width: f64, color: Color32) {
        if let Some((a, b)) = camera.segment(a, b, self.size) {
            self.line(a, b, width, color);
        }
    }
    pub fn paint(self, painter: &egui::Painter, rect: Rect) {
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            SceneCallback {
                vertices: self.vertices,
            },
        ));
    }
}

struct Resources {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    capacity: u64,
    srgb: bool,
}
pub fn install(cc: &eframe::CreationContext<'_>) -> bool {
    let Some(state) = cc.wgpu_render_state.as_ref() else {
        return false;
    };
    let device = &state.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Ketchup edges"),
        source: wgpu::ShaderSource::Wgsl(include_str!("edges.wgsl").into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Ketchup edge strips"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x4,2=>Float32],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: state.target_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let capacity = 4096;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Ketchup vertices"),
        size: capacity,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    state.renderer.write().callback_resources.insert(Resources {
        pipeline,
        buffer,
        capacity,
        srgb: state.target_format.is_srgb(),
    });
    true
}
struct SceneCallback {
    vertices: Vec<Vertex>,
}
impl egui_wgpu::CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(r) = resources.get_mut::<Resources>() else {
            return vec![];
        };
        let bytes = (self.vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        if bytes > r.capacity {
            r.capacity = bytes.next_power_of_two();
            r.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Ketchup vertices"),
                size: r.capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if bytes > 0 {
            if r.srgb {
                let mut vertices = self.vertices.clone();
                for v in &mut vertices {
                    for c in &mut v.color[..3] {
                        *c = if *c <= 0.04045 {
                            *c / 12.92
                        } else {
                            ((*c + 0.055) / 1.055).powf(2.4)
                        };
                    }
                }
                queue.write_buffer(&r.buffer, 0, bytemuck::cast_slice(&vertices));
            } else {
                queue.write_buffer(&r.buffer, 0, bytemuck::cast_slice(&self.vertices));
            }
        }
        vec![]
    }
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(r) = resources.get::<Resources>() else {
            return;
        };
        if self.vertices.is_empty() {
            return;
        }
        pass.set_pipeline(&r.pipeline);
        pass.set_vertex_buffer(0, r.buffer.slice(..));
        pass.draw(0..self.vertices.len() as u32, 0..1);
    }
}
