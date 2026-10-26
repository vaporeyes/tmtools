// ABOUTME: The textured renderer: uploads map and vehicle parts with their materials and mipmapped textures, then draws
// ABOUTME: sky, opaque, blended, and additive passes with racing lines into an offscreen 4x MSAA target egui displays.

use crate::loader::LoadedScene;
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use glam::Mat4;
use std::collections::HashMap;
use std::sync::Arc;
use tmassets::material::MaterialSpec;
use tmassets::texture::Texture;
use tmscene::mood::{linear, Mood, MoodName};
use tmscene::scene::{DrawPart, Surface};
use wgpu::util::DeviceExt;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;
// A 32x32 map is 1024 units across; haze starts beyond it so an overview stays readable (as the web viewer)
const FOG_NEAR: f32 = 800.0;
const FOG_FAR: f32 = 4000.0;

const HAS_DIFFUSE: u32 = 1;
const WORLD_UV: u32 = 2;
const BLEND_BOTH: u32 = 4;
const HAS_BLEND3: u32 = 8;
const FLIP_Y: u32 = 16;
const UNLIT: u32 = 32;
const EMISSIVE: u32 = 64;
const FIXED_OPACITY: u32 = 128;
const PHONG: u32 = 256;
const HAS_SPECULAR: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky_ambient: [f32; 4],
    ground_ambient: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    sky: [[f32; 4]; 5],
}

impl FrameUniform {
    pub fn new(view_proj: Mat4, camera: glam::Vec3, mood: &Mood) -> Self {
        let rgb = |c: [f32; 3], k: f32| [c[0] * k, c[1] * k, c[2] * k, 1.0];
        let sun = mood.sun_direction();
        Self {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_pos: [camera.x, camera.y, camera.z, 1.0],
            sun_dir: [sun[0], sun[1], sun[2], 0.0],
            sun_color: rgb(linear(mood.sun), mood.sun_intensity),
            sky_ambient: rgb(linear(mood.ambient), mood.ambient_intensity),
            // The web viewer's hemisphere light uses a darker copy of the ambient for the ground
            ground_ambient: rgb(linear(mood.ambient), mood.ambient_intensity * 0.55),
            fog_color: rgb(linear(mood.sky[4]), 1.0),
            fog: [FOG_NEAR, FOG_FAR, 0.0, 0.0],
            sky: mood.sky.map(|c| rgb(linear(c), 1.0)),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    color: [f32; 4],
    flags: u32,
    opacity: f32,
    pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Pass {
    Opaque,
    Blend,
    Add,
}

struct GpuPart {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    instances: wgpu::Buffer,
    instance_count: u32,
    /// For vehicles: which of the vehicle's parts this is, to pick its matrix from each car's posed parts
    source: usize,
    material: Arc<wgpu::BindGroup>,
    pass: Pass,
    double_sided: bool,
}

/// One car to draw: its vehicle, and the world transform of each of the vehicle's parts (wheels posed)
pub struct CarDraw {
    pub vehicle: String,
    pub parts: Vec<Mat4>,
}

/// What the module wants drawn this frame
pub struct FrameRequest {
    pub uniform: FrameUniform,
    pub cars: Vec<CarDraw>,
    pub lines: Vec<LineVertex>,
}

struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    resolve: wgpu::TextureView,
    depth: wgpu::TextureView,
    egui_id: egui::TextureId,
}

pub struct Renderer {
    frame_buffer: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: HashMap<(Pass, bool), wgpu::RenderPipeline>,
    sky: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    white: wgpu::TextureView,
    textures: HashMap<(String, bool), Arc<wgpu::TextureView>>,
    materials: HashMap<String, Arc<wgpu::BindGroup>>,
    targets: Option<Targets>,
    /// The scene key the uploaded map belongs to
    pub scene_key: Option<String>,
    map: Vec<GpuPart>,
    vehicles: HashMap<String, Vec<GpuPart>>,
    request: Option<FrameRequest>,
    lines: Option<(wgpu::Buffer, u32)>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("replayview"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                // The vertex stage reads the flags too, for world-space terrain UVs
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                wgpu::BindGroupLayoutEntry { binding: 5, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                texture_entry(6),
            ],
        });
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame_buffer.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        let layout = |groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: groups, immediate_size: 0 })
        };
        let part_layout = layout(&[Some(&frame_layout), Some(&material_layout)]);
        let frame_only = layout(&[Some(&frame_layout)]);

        let mut pipelines = HashMap::new();
        for pass in [Pass::Opaque, Pass::Blend, Pass::Add] {
            for double_sided in [false, true] {
                pipelines.insert((pass, double_sided), part_pipeline(device, &shader, &part_layout, pass, double_sided));
            }
        }
        let sky = simple_pipeline(device, &shader, &frame_only, "vs_sky", "fs_sky", &[], wgpu::PrimitiveTopology::TriangleList, None, false);
        let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let line_buffer = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<LineVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &line_attributes };
        let line = simple_pipeline(device, &shader, &frame_only, "vs_line", "fs_line", &[Some(line_buffer)], wgpu::PrimitiveTopology::LineList, Some(wgpu::BlendState::ALPHA_BLENDING), true);

        let white = upload_texture(device, queue, &Texture { width: 1, height: 1, rgba: vec![255; 4] }, true);
        Self {
            frame_buffer,
            frame_group,
            material_layout,
            sampler,
            pipelines,
            sky,
            line,
            white,
            textures: HashMap::new(),
            materials: HashMap::new(),
            targets: None,
            scene_key: None,
            map: Vec::new(),
            vehicles: HashMap::new(),
            request: None,
            lines: None,
        }
    }

    /// Makes the offscreen target match size (in pixels) and returns the egui texture showing it
    pub fn ensure_targets(&mut self, device: &wgpu::Device, egui_renderer: &mut egui_wgpu::Renderer, size: [u32; 2]) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        if let Some(t) = &self.targets {
            if t.size == size {
                return t.egui_id;
            }
        }
        let make = |format, samples, usage, view_formats: &[wgpu::TextureFormat]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("replayview target"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats,
            })
        };
        let view = |texture: &wgpu::Texture, format| texture.create_view(&wgpu::TextureViewDescriptor { format, ..Default::default() });
        let msaa = view(&make(COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]), None);
        let depth = view(&make(DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]), None);
        // egui samples textures as gamma-encoded values, so it gets a plain view of the result; rendering and
        // resolving go through an sRGB view of the same texture, which encodes the linear colors on write
        let output = make(wgpu::TextureFormat::Rgba8Unorm, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, &[COLOR_FORMAT]);
        let resolve = view(&output, Some(COLOR_FORMAT));
        let shown = view(&output, None);
        let egui_id = match &self.targets {
            Some(old) => {
                egui_renderer.update_egui_texture_from_wgpu_texture(device, &shown, wgpu::FilterMode::Linear, old.egui_id);
                old.egui_id
            }
            None => egui_renderer.register_native_texture(device, &shown, wgpu::FilterMode::Linear),
        };
        self.targets = Some(Targets { size, msaa, resolve, depth, egui_id });
        egui_id
    }

    /// Replaces the map and adds any new vehicles from a finished load
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &LoadedScene) {
        let night = scene.map.as_ref().is_some_and(|m| m.mood.name == MoodName::Night);
        let textures = &scene.textures;
        self.map = scene.map.as_ref().map_or_else(Vec::new, |m| m.parts.iter().filter_map(|p| self.upload_part(device, queue, p, textures, night, None)).collect());
        for (name, model) in &scene.vehicles {
            if !self.vehicles.contains_key(name) {
                let gpu = model.parts.iter().enumerate().filter_map(|(i, p)| self.upload_part(device, queue, p, textures, night, Some(i))).collect();
                self.vehicles.insert(name.clone(), gpu);
            }
        }
        self.scene_key = Some(scene.key.clone());
    }

    pub fn set_request(&mut self, request: FrameRequest) {
        self.request = Some(request);
    }

    fn texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, textures: &HashMap<String, Arc<Texture>>, path: Option<&String>, srgb: bool) -> Option<Arc<wgpu::TextureView>> {
        let path = path?;
        if let Some(view) = self.textures.get(&(path.clone(), srgb)) {
            return Some(view.clone());
        }
        let view = Arc::new(upload_texture(device, queue, textures.get(path)?, srgb));
        self.textures.insert((path.clone(), srgb), view.clone());
        Some(view)
    }

    /// Uploads one part; `vehicle_part` is its index in a vehicle's parts, whose instances are rewritten every frame
    fn upload_part(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, part: &DrawPart, textures: &HashMap<String, Arc<Texture>>, night: bool, vehicle_part: Option<usize>) -> Option<GpuPart> {
        let (pass, double_sided, uniform, slots) = match &part.surface {
            Surface::Invisible => return None,
            Surface::Glass => (Pass::Blend, false, MaterialUniform { color: [1.0; 4], flags: FIXED_OPACITY, opacity: 0.35, pad: [0.0; 2] }, None),
            Surface::Color { rgb, opacity, emissive } => {
                let c = linear(*rgb);
                let flags = if *emissive { EMISSIVE } else { 0 } | if *opacity < 1.0 { FIXED_OPACITY } else { 0 };
                let pass = if *opacity < 1.0 { Pass::Blend } else { Pass::Opaque };
                (pass, true, MaterialUniform { color: [c[0], c[1], c[2], 1.0], flags, opacity: *opacity, pad: [0.0; 2] }, None)
            }
            Surface::Textured(MaterialSpec::Textured { textures: slots, properties: p, .. }) => {
                // Night-only glows show only in a Night mood
                if p.night_only && !night {
                    return None;
                }
                if p.water {
                    // A translucent stand-in, as the web viewer draws water
                    let c = linear([0x5f, 0x9f, 0xd0]);
                    (Pass::Blend, false, MaterialUniform { color: [c[0], c[1], c[2], 1.0], flags: FIXED_OPACITY, opacity: 0.75, pad: [0.0; 2] }, None)
                } else {
                    let pass = if p.add {
                        Pass::Add
                    } else if p.transparent || p.opacity().is_some() {
                        Pass::Blend
                    } else {
                        Pass::Opaque
                    };
                    let mut flags = 0;
                    if p.world_uv && slots.diffuse.is_some() {
                        flags |= WORLD_UV;
                    } else if p.dds_flip_y {
                        flags |= FLIP_Y;
                    }
                    if p.basic {
                        flags |= UNLIT;
                    }
                    if p.opacity().is_some() {
                        flags |= FIXED_OPACITY;
                    }
                    // The web viewer draws these with three.js's Phong material
                    if !p.basic && (p.specular_alpha || slots.specular.is_some()) {
                        flags |= PHONG;
                    }
                    let uniform = MaterialUniform { color: [1.0; 4], flags, opacity: p.opacity().unwrap_or(1.0), pad: [0.0; 2] };
                    (pass, p.double_sided, uniform, Some(slots.clone()))
                }
            }
            Surface::Textured(_) => return None,
        };

        let key = format!("{}|{:?}|{pass:?}", part.material_name, part.surface);
        let material = match self.materials.get(&key) {
            Some(m) => m.clone(),
            None => {
                let mut uniform = uniform;
                let mut view = |path: Option<&String>, srgb: bool| self.texture(device, queue, textures, path, srgb);
                let (diffuse, blend2, blend_i, blend3, specular) = match &slots {
                    Some(s) => (
                        view(s.diffuse.as_ref(), true),
                        view(s.blend2.as_ref(), true),
                        view(s.blend_intensity.as_ref(), false),
                        view(s.blend3.as_ref(), true),
                        // Specular maps hold strengths, not colors
                        view(s.specular.as_ref(), false),
                    ),
                    None => (None, None, None, None, None),
                };
                if specular.is_some() && uniform.flags & PHONG != 0 {
                    uniform.flags |= HAS_SPECULAR;
                }
                if diffuse.is_some() {
                    uniform.flags |= HAS_DIFFUSE;
                }
                if blend2.is_some() && blend_i.is_some() {
                    uniform.flags |= BLEND_BOTH;
                }
                if blend3.is_some() {
                    uniform.flags |= HAS_BLEND3;
                }
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("material"), contents: bytemuck::bytes_of(&uniform), usage: wgpu::BufferUsages::UNIFORM });
                let white = &self.white;
                let pick = |v: &Option<Arc<wgpu::TextureView>>| v.as_ref().map_or(white, |v| v.as_ref()).clone();
                let views = [pick(&diffuse), pick(&blend2), pick(&blend_i), pick(&blend3), pick(&specular)];
                let group = Arc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material"),
                    layout: &self.material_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[2]) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&views[3]) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&views[4]) },
                    ],
                }));
                self.materials.insert(key, group.clone());
                group
            }
        };

        let g = &part.geometry;
        if g.indices.is_empty() || part.instances.is_empty() {
            return None;
        }
        let vertices: Vec<Vertex> = (0..g.positions.len()).map(|i| Vertex { position: g.positions[i], normal: g.normals[i], uv: g.uvs[i] }).collect();
        let mut indices = g.indices.clone();
        if part.flip_winding {
            for tri in indices.chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
        }
        let init = |label, contents: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage });
        let (instances, instance_count, source) = if let Some(source) = vehicle_part {
            // Rewritten every frame with each car's posed part; sized for a few cars and grown on demand
            let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("car instances"), size: 64 * 8, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            (buffer, 0, source)
        } else {
            let data: Vec<[[f32; 4]; 4]> = part.instances.iter().map(|m| m.to_cols_array_2d()).collect();
            (init("instances", bytemuck::cast_slice(&data), wgpu::BufferUsages::VERTEX), part.instances.len() as u32, 0)
        };
        Some(GpuPart {
            vertices: init("vertices", bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX),
            indices: init("indices", bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX),
            index_count: g.indices.len() as u32,
            instances,
            instance_count,
            source,
            material,
            pass,
            double_sided,
        })
    }

    /// Draws the last request into the offscreen target; called from the paint callback's prepare
    fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder) {
        let (Some(request), Some(targets)) = (self.request.take(), &self.targets) else { return };
        queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&request.uniform));

        // Each vehicle's parts get one instance per car of that vehicle
        let mut by_vehicle: HashMap<&str, Vec<&CarDraw>> = HashMap::new();
        for car in &request.cars {
            by_vehicle.entry(car.vehicle.as_str()).or_default().push(car);
        }
        for (name, parts) in &mut self.vehicles {
            let cars = by_vehicle.get(name.as_str()).map_or(&[][..], Vec::as_slice);
            for part in parts.iter_mut() {
                let data: Vec<[[f32; 4]; 4]> = cars.iter().filter_map(|car| car.parts.get(part.source)).map(|m| m.to_cols_array_2d()).collect();
                let bytes: &[u8] = bytemuck::cast_slice(&data);
                if bytes.len() as u64 > part.instances.size() {
                    part.instances = device.create_buffer(&wgpu::BufferDescriptor { label: Some("car instances"), size: bytes.len() as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                }
                if !bytes.is_empty() {
                    queue.write_buffer(&part.instances, 0, bytes);
                }
                part.instance_count = data.len() as u32;
            }
        }

        self.lines = (!request.lines.is_empty()).then(|| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("lines"), contents: bytemuck::cast_slice(&request.lines), usage: wgpu::BufferUsages::VERTEX });
            (buffer, request.lines.len() as u32)
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("replayview"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.msaa,
                depth_slice: None,
                resolve_target: Some(&targets.resolve),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Discard },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.frame_group, &[]);
        pass.set_pipeline(&self.sky);
        pass.draw(0..3, 0..1);

        let parts: Vec<&GpuPart> = self.map.iter().chain(self.vehicles.values().flatten()).filter(|p| p.instance_count > 0).collect();
        for wanted in [Pass::Opaque, Pass::Blend, Pass::Add] {
            for part in parts.iter().filter(|p| p.pass == wanted) {
                pass.set_pipeline(&self.pipelines[&(part.pass, part.double_sided)]);
                pass.set_bind_group(1, part.material.as_ref(), &[]);
                pass.set_vertex_buffer(0, part.vertices.slice(..));
                pass.set_vertex_buffer(1, part.instances.slice(..));
                pass.set_index_buffer(part.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..part.index_count, 0, 0..part.instance_count);
            }
            if wanted == Pass::Opaque {
                if let Some((buffer, count)) = &self.lines {
                    pass.set_pipeline(&self.line);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
        }
    }
}

fn part_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, pass: Pass, double_sided: bool) -> wgpu::RenderPipeline {
    let vertex_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    let instance_attributes = wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4];
    let buffers = [
        Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &vertex_attributes }),
        Some(wgpu::VertexBufferLayout { array_stride: 64, step_mode: wgpu::VertexStepMode::Instance, attributes: &instance_attributes }),
    ];
    let blend = match pass {
        Pass::Opaque => None,
        Pass::Blend => Some(wgpu::BlendState::ALPHA_BLENDING),
        Pass::Add => Some(wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::OVER,
        }),
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("part"),
        layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &buffers },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: if double_sided { None } else { Some(wgpu::Face::Back) },
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // Additive glows don't hide what's behind them
            depth_write_enabled: Some(pass != Pass::Add),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        // Blended materials also use alpha to coverage, as three.js does for the web viewer's transparent materials
        multisample: wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: pass == Pass::Blend },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            // Opaque parts keep the cleared alpha of 1: texture alpha often holds masks, and egui would blend by it
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend,
                write_mask: if pass == Pass::Opaque { wgpu::ColorWrites::COLOR } else { wgpu::ColorWrites::ALL },
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn simple_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    vs: &str,
    fs: &str,
    buffers: &[Option<wgpu::VertexBufferLayout>],
    topology: wgpu::PrimitiveTopology,
    blend: Option<wgpu::BlendState>,
    depth_test: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vs),
        layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some(vs), compilation_options: Default::default(), buffers },
        primitive: wgpu::PrimitiveState { topology, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(if depth_test { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Always }),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The image upside down: rows in reverse order
fn flip_rows(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row = (width * 4) as usize;
    (0..height as usize).rev().flat_map(|y| rgba[y * row..(y + 1) * row].iter().copied()).collect()
}

/// Halves an RGBA image with a box filter, for the next mip level
fn downsample(rgba: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let mut sum = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    sum += rgba[((sy * w + sx) * 4 + c) as usize] as u32;
                }
                out[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
            }
        }
    }
    (out, nw, nh)
}

fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, image: &Texture, srgb: bool) -> wgpu::TextureView {
    let levels = 32 - image.width.max(image.height).max(1).leading_zeros();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("asset texture"),
        size: wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // three.js flips images vertically on upload (its default flipY), and the export's UVs expect that
    let (mut data, mut w, mut h) = (flip_rows(&image.rgba, image.width, image.height), image.width, image.height);
    for level in 0..levels {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        if level + 1 < levels {
            (data, w, h) = downsample(&data, w, h);
        }
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// The paint callback: renders the frame offscreen before egui draws the image that shows it
pub struct DrawCallback;

impl egui_wgpu::CallbackTrait for DrawCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<Renderer>() {
            renderer.render(device, queue, encoder);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, _pass: &mut wgpu::RenderPass<'static>, _resources: &egui_wgpu::CallbackResources) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mips_average_each_2x2_block() {
        let rgba = [0, 0, 0, 255, 100, 100, 100, 255, 200, 200, 200, 255, 100, 100, 100, 255];
        let (out, w, h) = downsample(&rgba, 2, 2);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, [100, 100, 100, 255]);
        // Odd edges reuse the last row and column
        assert_eq!(downsample(&[10, 20, 30, 40], 1, 1).0, [10, 20, 30, 40]);
    }

    #[test]
    fn flips_images_upside_down() {
        let rgba = [1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4];
        assert_eq!(flip_rows(&rgba, 2, 2), [3, 3, 3, 3, 4, 4, 4, 4, 1, 1, 1, 1, 2, 2, 2, 2]);
    }

    #[test]
    fn uniforms_match_the_shader_layout() {
        assert_eq!(std::mem::size_of::<FrameUniform>(), 320);
        assert_eq!(std::mem::size_of::<MaterialUniform>(), 32);
        assert_eq!(std::mem::size_of::<Vertex>(), 32);
    }
}
