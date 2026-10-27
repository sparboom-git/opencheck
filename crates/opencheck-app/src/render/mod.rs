//! GPU side of the 3D view.
//!
//! The scene is drawn inside egui's render pass through a paint callback.
//! [`SceneGpu`] lives in egui-wgpu's callback resources; the app uploads
//! geometry and element display modes to it, and [`ViewportCallback`]
//! carries the camera for one frame.

use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::{self, CallbackTrait, RenderState, wgpu};
use opencheck_core::ModelId;
use wgpu::util::DeviceExt;

/// Depth buffer format; matches `NativeOptions::depth_buffer = 32`.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Matches `NativeOptions::multisampling`.
pub const MSAA_SAMPLES: u32 = if cfg!(target_arch = "wasm32") { 1 } else { 4 };

/// How an element is drawn. Must match the constants in `scene.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DisplayMode {
    Hidden = 0,
    Normal = 1,
    Ghost = 2,
    Selected = 3,
    Hover = 4,
    /// First element of a finding under review.
    FocusA = 5,
    /// Second element of a finding under review.
    FocusB = 6,
}

/// Flag in an element's mode word: drawn in the sRGB colour in its low 24
/// bits (red in bits 16-23). Must match `MODE_PAINTED` in `scene.wgsl`.
pub const PAINTED: u32 = 0x8000_0000;

/// Vertices per GPU buffer. At 20 bytes per vertex this keeps every buffer
/// far below wgpu's default 256 MB limit, also for the index buffer.
pub const MAX_CHUNK_VERTICES: usize = 4_000_000;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    /// Unit normal as `Snorm8x4` (w unused).
    pub normal: [i8; 4],
    /// sRGB colour with straight alpha.
    pub color: [u8; 4],
    /// Index into the element mode buffer.
    pub slot: u32,
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Snorm8x4, 2 => Unorm8x4, 3 => Uint32];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    /// A point on the section plane and its normal (towards the side cut
    /// away), when `params.x` switches the section on.
    pub section_point: [f32; 4],
    pub section_normal: [f32; 4],
    /// x: section box enabled, y: ghost alpha.
    pub params: [f32; 4],
}

/// Part of a model's geometry that goes into one vertex and one index buffer.
#[derive(Default)]
pub struct GeometryChunk {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// One uploaded chunk. A large model owns several.
struct GpuModel {
    id: ModelId,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    visible: bool,
}

pub struct SceneGpu {
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    camera: wgpu::Buffer,
    modes: wgpu::Buffer,
    models: Vec<GpuModel>,
}

impl SceneGpu {
    /// Creates the pipelines and registers the scene in egui's callback resources.
    pub fn install(render_state: &RenderState) {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("opencheck_scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("opencheck_scene_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("opencheck_scene_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = |label: &str, vs: &str, transparent: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers: &[Some(Vertex::layout())],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(!transparent),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: MSAA_SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: render_state.target_format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = pipeline("opencheck_opaque", "vs_opaque", false);
        let transparent = pipeline("opencheck_transparent", "vs_transparent", true);

        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("opencheck_camera"),
            size: size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let modes = Self::create_modes_buffer(device, &[DisplayMode::Normal as u32; 1]);
        let bind_group = Self::create_bind_group(device, &bind_group_layout, &camera, &modes);

        render_state
            .renderer
            .write()
            .callback_resources
            .insert(SceneGpu {
                opaque,
                transparent,
                bind_group_layout,
                bind_group,
                camera,
                modes,
                models: Vec::new(),
            });
    }

    fn create_modes_buffer(device: &wgpu::Device, modes: &[u32]) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("opencheck_element_modes"),
            contents: bytemuck::cast_slice(modes),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })
    }

    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        camera: &wgpu::Buffer,
        modes: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("opencheck_scene_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: modes.as_entire_binding(),
                },
            ],
        })
    }

    fn with<R>(render_state: &RenderState, f: impl FnOnce(&mut SceneGpu) -> R) -> R {
        let mut renderer = render_state.renderer.write();
        let scene = renderer
            .callback_resources
            .get_mut::<SceneGpu>()
            .expect("SceneGpu::install runs at startup");
        f(scene)
    }

    pub fn upload_model(render_state: &RenderState, id: ModelId, chunks: &[GeometryChunk]) {
        let device = &render_state.device;
        let uploaded: Vec<GpuModel> = chunks
            .iter()
            .map(|chunk| GpuModel {
                id,
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("opencheck_model_vertices"),
                    contents: bytemuck::cast_slice(&chunk.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("opencheck_model_indices"),
                    contents: bytemuck::cast_slice(&chunk.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: chunk.indices.len() as u32,
                visible: true,
            })
            .collect();
        Self::with(render_state, |scene| {
            scene.models.retain(|m| m.id != id);
            scene.models.extend(uploaded);
        });
    }

    pub fn remove_model(render_state: &RenderState, id: ModelId) {
        Self::with(render_state, |scene| scene.models.retain(|m| m.id != id));
    }

    pub fn set_model_visible(render_state: &RenderState, id: ModelId, visible: bool) {
        Self::with(render_state, |scene| {
            for m in scene.models.iter_mut().filter(|m| m.id == id) {
                m.visible = visible;
            }
        });
    }

    /// Replaces the display mode of every element slot.
    pub fn write_modes(render_state: &RenderState, modes: &[u32]) {
        if modes.is_empty() {
            return;
        }
        let device = &render_state.device;
        Self::with(render_state, |scene| {
            let needed = std::mem::size_of_val(modes) as u64;
            if scene.modes.size() < needed {
                scene.modes = Self::create_modes_buffer(device, modes);
                scene.bind_group = Self::create_bind_group(
                    device,
                    &scene.bind_group_layout,
                    &scene.camera,
                    &scene.modes,
                );
            } else {
                render_state
                    .queue
                    .write_buffer(&scene.modes, 0, bytemuck::cast_slice(modes));
            }
        });
    }
}

/// Draws the scene into the 3D view rectangle for one frame.
pub struct ViewportCallback {
    pub camera: CameraUniform,
}

impl CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(scene) = resources.get::<SceneGpu>() {
            queue.write_buffer(&scene.camera, 0, bytemuck::bytes_of(&self.camera));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(scene) = resources.get::<SceneGpu>() else {
            return;
        };
        render_pass.set_bind_group(0, &scene.bind_group, &[]);
        for pipeline in [&scene.opaque, &scene.transparent] {
            render_pass.set_pipeline(pipeline);
            for model in scene.models.iter().filter(|m| m.visible) {
                render_pass.set_vertex_buffer(0, model.vertices.slice(..));
                render_pass.set_index_buffer(model.indices.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..model.index_count, 0, 0..1);
            }
        }
    }
}
