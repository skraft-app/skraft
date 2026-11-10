//! SolidSketch's viewport renderer (docs/ARCHITECTURE.md §7 bis).
//!
//! The renderer knows meshes by key, never the document: the caller (the display scene in
//! `tools`) uploads one mesh per definition — the concatenation of its direct solids, each a
//! [`Part`] with its own index range — and draws each definition once, instanced over all its
//! placements. Highlights redraw one part of one placement over it.
//!
//! It draws into its own off-screen target (MSAA colour + depth, resolved into a sampleable
//! texture), so it is independent of the UI and testable without a window.
//!
//! Precision (ARCHITECTURE.md §7 bis): the GPU only sees `f32`, so everything is drawn
//! **relative to the camera**. Placements, the view and lines are `f64`; each placement's
//! `view × world × recentre` is computed on the CPU in `f64` and only then made `f32`, and each
//! definition's mesh is stored around its own centre. Depth is **reversed** (1 near, 0 far) in
//! a `Depth32Float` buffer: [`Frame::proj`] must map near to 1, e.g.
//! `Mat4::perspective_infinite_reverse_rh`.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::{DMat3, DMat4, DVec3, Mat4, Vec3};
use solidsketch_kernel::Mesh;
use wgpu::util::DeviceExt;

/// The format of the resolved target: what a UI samples.
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

/// One solid of a definition's mesh.
pub struct Part<'a> {
    /// The caller's id for it (the solid's id).
    pub id: u64,
    pub mesh: &'a Mesh,
}

/// A tint mixed into what is drawn: `rgb` (linear) by `a` (0 leaves it as is).
pub type Tint = [f32; 4];

pub const NO_TINT: Tint = [0.0; 4];

/// All placements of one definition's mesh.
pub struct Batch {
    pub key: u64,
    pub placements: Vec<Placement>,
}

#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// From the definition's coordinates to the world's.
    pub transform: DMat4,
    /// Faces and edges tint (e.g. faded outside the context being edited).
    pub tint: Tint,
}

/// One part of one placement drawn again over the scene, tinted (selection, hover).
#[derive(Clone, Copy, Debug)]
pub struct Highlight {
    pub key: u64,
    pub part: u64,
    pub transform: DMat4,
    pub face_tint: Tint,
    pub edge_tint: Tint,
}

/// A world-space segment, coloured (`rgb` linear, `a` mixed over the background).
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub a: DVec3,
    pub b: DVec3,
    pub color: [f32; 4],
}

/// A square grid on the XY plane centred on the origin.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub spacing: f64,
    /// Lines on each side of the axes.
    pub count: u32,
}

/// Everything one frame draws.
pub struct Frame<'a> {
    /// World to camera (`f64`: the camera may be far from the origin).
    pub view: DMat4,
    /// Camera to clip, **reversed depth** (near → 1, far → 0).
    pub proj: Mat4,
    /// Linear RGB.
    pub background: [f32; 3],
    pub batches: &'a [Batch],
    pub highlights: &'a [Highlight],
    /// Depth-tested lines (inferences, guides).
    pub lines: &'a [Line],
    /// Lines drawn over everything (tool previews).
    pub overlay: &'a [Line],
    pub grid: Option<Grid>,
    pub axes: bool,
    /// Where the grid and the axes are: the frame of the context being edited, to world.
    pub axes_frame: DMat4,
    /// Pixels per point of the screen: line widths are in points, so they look the same on
    /// any display.
    pub pixels_per_point: f32,
}

/// Line widths, in points.
pub const EDGE_WIDTH: f32 = 1.5;
pub const LINE_WIDTH: f32 = 1.5;
pub const GRID_WIDTH: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
}

/// One corner of a segment drawn as a screen-space quad (6 per segment, two triangles): both
/// ends, which end this corner is at (`corner.x`: 0 = `a`, 1 = `b`) and on which side
/// (`corner.y`: ±1). The vertex shader widens it to the line width in pixels.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EdgeVertex {
    a: [f32; 3],
    b: [f32; 3],
    corner: [f32; 2],
}

/// A corner of a coloured line's quad; `corner.z` is its half width in pixels.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineVertex {
    a: [f32; 3],
    b: [f32; 3],
    corner: [f32; 3],
    color: [f32; 4],
}

/// The corners of a segment's quad, in drawing order.
const QUAD: [[f32; 2]; 6] = [[0.0, -1.0], [0.0, 1.0], [1.0, -1.0], [1.0, -1.0], [0.0, 1.0], [1.0, 1.0]];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 3],
    tint: [f32; 4],
}

impl InstanceRaw {
    /// `model_view`: from the mesh's (recentred) coordinates to the camera's, in `f64`.
    fn new(model_view: &DMat4, tint: Tint) -> Self {
        let linear = DMat3::from_mat4(*model_view);
        let n = linear.inverse().transpose();
        let c = |v: DVec3, w: f64| [v.x as f32, v.y as f32, v.z as f32, w as f32];
        // The winding's sign, for the shader to tell outside from inside under a mirror.
        let mirrored = if linear.determinant() < 0.0 { -1.0 } else { 1.0 };
        InstanceRaw {
            model: model_view.as_mat4().to_cols_array_2d(),
            normal: [c(n.x_axis, mirrored), c(n.y_axis, 0.0), c(n.z_axis, 0.0)],
            tint,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    proj: [[f32; 4]; 4],
    /// World +Z and the key light's direction, in camera coordinates.
    up: [f32; 4],
    key: [f32; 4],
    background: [f32; 4],
    /// Target size in pixels, then half the width of edges and of lines, in pixels.
    viewport: [f32; 4],
}

struct PartRange {
    id: u64,
    indices: Range<u32>,
    edges: Range<u32>,
}

struct GpuMesh {
    revision: u64,
    /// The definition coordinates the vertices are relative to.
    origin: DVec3,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    index_count: u32,
    edges: Option<wgpu::Buffer>,
    edge_vertex_count: u32,
    parts: Vec<PartRange>,
}

struct Target {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::Texture,
    resolved_view: wgpu::TextureView,
}

/// A growable vertex buffer rewritten each frame.
struct Dynamic {
    label: &'static str,
    buffer: Option<wgpu::Buffer>,
    capacity: u64,
}

impl Dynamic {
    fn new(label: &'static str) -> Self {
        Dynamic { label, buffer: None, capacity: 0 }
    }

    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let len = bytes.len() as u64;
        if self.buffer.is_none() || self.capacity < len {
            let capacity = len.next_power_of_two().max(4096);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = capacity;
        }
        if let Some(b) = &self.buffer {
            queue.write_buffer(b, 0, bytes);
        }
    }
}

/// The viewport renderer.
pub struct Renderer {
    faces: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    meshes: std::collections::HashMap<u64, GpuMesh>,
    instances: Dynamic,
    line_vertices: Dynamic,
    target: Option<Target>,
    target_generation: u64,
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("solidsketch shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("solidsketch"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let instance_attrs = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4
        ];
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<InstanceRaw>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attrs,
        };
        let face_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let face_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &face_attrs,
        };
        let edge_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let edge_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<EdgeVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &edge_attrs,
        };
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x4];
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };

        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        topology: wgpu::PrimitiveTopology,
                        depth: wgpu::DepthStencilState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(depth),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: TARGET_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let depth = |write: bool, compare: wgpu::CompareFunction, bias: wgpu::DepthBiasState| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: Default::default(),
            bias,
        };
        // Reversed depth: nearer is greater. Faces are pushed back (towards 0) a little so
        // their edges win the depth test.
        let face_bias = wgpu::DepthBiasState { constant: -2, slope_scale: -1.0, clamp: 0.0 };
        let nearer = wgpu::CompareFunction::GreaterEqual;
        let faces = pipeline(
            "faces",
            "vs_face",
            "fs_face",
            &[Some(face_layout), Some(instance_layout.clone())],
            wgpu::PrimitiveTopology::TriangleList,
            depth(true, nearer, face_bias),
        );
        let edges = pipeline(
            "edges",
            "vs_edge",
            "fs_edge",
            &[Some(edge_layout), Some(instance_layout)],
            wgpu::PrimitiveTopology::TriangleList,
            depth(true, nearer, Default::default()),
        );
        let lines = pipeline(
            "lines",
            "vs_line",
            "fs_line",
            &[Some(line_layout.clone())],
            wgpu::PrimitiveTopology::TriangleList,
            depth(true, nearer, Default::default()),
        );
        let overlay = pipeline(
            "overlay",
            "vs_line",
            "fs_line",
            &[Some(line_layout)],
            wgpu::PrimitiveTopology::TriangleList,
            depth(false, wgpu::CompareFunction::Always, Default::default()),
        );

        Renderer {
            faces,
            edges,
            lines,
            overlay,
            globals,
            bind_group,
            meshes: Default::default(),
            instances: Dynamic::new("instances"),
            line_vertices: Dynamic::new("lines"),
            target: None,
            target_generation: 0,
        }
    }

    /// Is the mesh `key` uploaded at `revision`?
    pub fn has_mesh(&self, key: u64, revision: u64) -> bool {
        self.meshes.get(&key).is_some_and(|m| m.revision == revision)
    }

    /// Uploads the mesh of `key` (a definition) as the concatenation of `parts`, unless it is
    /// already there at `revision`.
    pub fn set_mesh(&mut self, device: &wgpu::Device, key: u64, revision: u64, parts: &[Part<'_>]) {
        if self.has_mesh(key, revision) {
            return;
        }
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut edges: Vec<EdgeVertex> = Vec::new();
        let mut ranges = Vec::with_capacity(parts.len());
        // The centre of the parts' origins: vertices are stored relative to it.
        let origin = parts
            .iter()
            .map(|p| p.mesh.origin)
            .fold(None, |b: Option<(DVec3, DVec3)>, o| Some(b.map_or((o, o), |(lo, hi)| (lo.min(o), hi.max(o)))))
            .map_or(DVec3::ZERO, |(lo, hi)| (lo + hi) * 0.5);
        for p in parts {
            let shift = (p.mesh.origin - origin).as_vec3();
            let at = |v: &[f32; 3]| (Vec3::from_array(*v) + shift).to_array();
            let base = vertices.len() as u32;
            let (i0, e0) = (indices.len() as u32, edges.len() as u32);
            vertices.extend(p.mesh.positions.iter().zip(&p.mesh.normals).map(|(pos, n)| Vertex { pos: at(pos), normal: *n }));
            indices.extend(p.mesh.indices.iter().map(|i| i + base));
            for poly in &p.mesh.edges {
                for w in poly.windows(2) {
                    let (a, b) = (at(&w[0]), at(&w[1]));
                    edges.extend(QUAD.map(|corner| EdgeVertex { a, b, corner }));
                }
            }
            ranges.push(PartRange { id: p.id, indices: i0..indices.len() as u32, edges: e0..edges.len() as u32 });
        }
        let buffer = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
            (!bytes.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage })
            })
        };
        let mesh = GpuMesh {
            revision,
            origin,
            vertices: buffer("vertices", bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX),
            indices: buffer("indices", bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX),
            index_count: indices.len() as u32,
            edges: buffer("edges", bytemuck::cast_slice(&edges), wgpu::BufferUsages::VERTEX),
            edge_vertex_count: edges.len() as u32,
            parts: ranges,
        };
        self.meshes.insert(key, mesh);
    }

    /// Drops the meshes whose key `keep` refuses.
    pub fn retain_meshes(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.meshes.retain(|k, _| keep(*k));
    }

    /// The resolved target of the last frame, and a number that changes whenever it is
    /// recreated (a UI re-registers its texture then).
    pub fn target(&self) -> Option<(&wgpu::TextureView, u64)> {
        self.target.as_ref().map(|t| (&t.resolved_view, self.target_generation))
    }

    /// The resolved target texture (to copy it out, e.g. for a screenshot).
    pub fn target_texture(&self) -> Option<&wgpu::Texture> {
        self.target.as_ref().map(|t| &t.resolved)
    }

    /// Draws `frame` at `size` pixels into the off-screen target.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        frame: &Frame<'_>,
    ) {
        let size = [size[0].max(1), size[1].max(1)];
        self.ensure_target(device, size);

        let bg = frame.background;
        let dir = |v: DVec3| {
            let v = frame.view.transform_vector3(v).normalize_or_zero();
            [v.x as f32, v.y as f32, v.z as f32, 0.0]
        };
        queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals {
                proj: frame.proj.to_cols_array_2d(),
                up: dir(DVec3::Z),
                key: dir(DVec3::new(0.35, 0.25, 0.9)),
                background: [bg[0], bg[1], bg[2], 1.0],
                viewport: [size[0] as f32, size[1] as f32, EDGE_WIDTH * frame.pixels_per_point / 2.0, 0.0],
            }),
        );
        let origin = |key: u64| self.meshes.get(&key).map_or(DVec3::ZERO, |m| m.origin);
        let model_view = |t: &DMat4, key: u64| frame.view * *t * DMat4::from_translation(origin(key));

        // Instances: every batch's placements, then one per highlight.
        let mut raw = Vec::new();
        let mut batch_ranges = Vec::with_capacity(frame.batches.len());
        for b in frame.batches {
            let start = raw.len() as u32;
            raw.extend(b.placements.iter().map(|p| InstanceRaw::new(&model_view(&p.transform, b.key), p.tint)));
            batch_ranges.push(start..raw.len() as u32);
        }
        let highlight_start = raw.len() as u32;
        for h in frame.highlights {
            let mv = model_view(&h.transform, h.key);
            raw.push(InstanceRaw::new(&mv, h.face_tint));
            raw.push(InstanceRaw::new(&mv, h.edge_tint));
        }
        self.instances.write(device, queue, bytemuck::cast_slice(&raw));

        // Lines: grid and axes, depth-tested lines, then the overlay.
        let mut lv = Vec::new();
        let line_half = LINE_WIDTH * frame.pixels_per_point / 2.0;
        push_scene_lines(&mut lv, frame);
        let tested = lv.len() as u32;
        for l in frame.overlay {
            push_line(&mut lv, &frame.view, l, line_half);
        }
        self.line_vertices.write(device, queue, bytemuck::cast_slice(&lv));

        let Some(target) = &self.target else { return };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewport"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.msaa,
                depth_slice: None,
                resolve_target: Some(&target.resolved_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0 }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);

        if let Some(inst) = &self.instances.buffer {
            // Faces.
            pass.set_pipeline(&self.faces);
            pass.set_vertex_buffer(1, inst.slice(..));
            for (b, r) in frame.batches.iter().zip(&batch_ranges) {
                let Some(m) = self.meshes.get(&b.key) else { continue };
                if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.index_count, 0, r.clone());
                }
            }
            for (k, h) in frame.highlights.iter().enumerate() {
                let Some((m, part)) = self.part(h) else { continue };
                if let (Some(v), Some(i)) = (&m.vertices, &m.indices) {
                    let at = highlight_start + 2 * k as u32;
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(part.indices.clone(), 0, at..at + 1);
                }
            }
            // Edges.
            pass.set_pipeline(&self.edges);
            for (b, r) in frame.batches.iter().zip(&batch_ranges) {
                let Some(m) = self.meshes.get(&b.key) else { continue };
                if let Some(e) = &m.edges {
                    pass.set_vertex_buffer(0, e.slice(..));
                    pass.draw(0..m.edge_vertex_count, r.clone());
                }
            }
            for (k, h) in frame.highlights.iter().enumerate() {
                let Some((m, part)) = self.part(h) else { continue };
                if let Some(e) = &m.edges {
                    let at = highlight_start + 2 * k as u32 + 1;
                    pass.set_vertex_buffer(0, e.slice(..));
                    pass.draw(part.edges.clone(), at..at + 1);
                }
            }
        }
        if let Some(lb) = &self.line_vertices.buffer {
            let total = lv.len() as u32;
            pass.set_vertex_buffer(0, lb.slice(..));
            if tested > 0 {
                pass.set_pipeline(&self.lines);
                pass.draw(0..tested, 0..1);
            }
            if total > tested {
                pass.set_pipeline(&self.overlay);
                pass.draw(tested..total, 0..1);
            }
        }
    }

    fn part(&self, h: &Highlight) -> Option<(&GpuMesh, &PartRange)> {
        let m = self.meshes.get(&h.key)?;
        Some((m, m.parts.iter().find(|p| p.id == h.part)?))
    }

    fn ensure_target(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let texture = |label: &str, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = texture("viewport msaa", TARGET_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = texture("viewport depth", DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolved = texture(
            "viewport",
            TARGET_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        self.target = Some(Target {
            size,
            msaa: view(&msaa),
            depth: view(&depth),
            resolved_view: view(&resolved),
            resolved,
        });
        self.target_generation += 1;
    }
}

/// A world line, in camera coordinates (subtracted in `f64`), `half` pixels on each side.
fn push_line(out: &mut Vec<LineVertex>, view: &DMat4, l: &Line, half: f32) {
    let a = view.transform_point3(l.a).as_vec3().to_array();
    let b = view.transform_point3(l.b).as_vec3().to_array();
    out.extend(QUAD.map(|[end, side]| LineVertex { a, b, corner: [end, side, half], color: l.color }));
}

/// SketchUp's axis colours (linear).
pub const AXIS_X: [f32; 4] = [0.85, 0.05, 0.05, 1.0];
pub const AXIS_Y: [f32; 4] = [0.05, 0.6, 0.05, 1.0];
pub const AXIS_Z: [f32; 4] = [0.05, 0.1, 0.85, 1.0];

fn push_scene_lines(out: &mut Vec<LineVertex>, frame: &Frame<'_>) {
    // Grid and axes in the context's frame (composed in f64, so far frames stay exact).
    let v = &(frame.view * frame.axes_frame);
    let ppp = frame.pixels_per_point;
    let (grid_half, line_half) = (GRID_WIDTH * ppp / 2.0, LINE_WIDTH * ppp / 2.0);
    if let Some(g) = frame.grid {
        let n = g.count as i32;
        let ext = g.spacing * g.count as f64;
        for i in -n..=n {
            if i == 0 && frame.axes {
                continue;
            }
            let t = i as f64 * g.spacing;
            let alpha = if i % 10 == 0 { 0.30 } else { 0.12 };
            let c = [0.3, 0.3, 0.3, alpha];
            push_line(out, v, &Line { a: DVec3::new(-ext, t, 0.0), b: DVec3::new(ext, t, 0.0), color: c }, grid_half);
            push_line(out, v, &Line { a: DVec3::new(t, -ext, 0.0), b: DVec3::new(t, ext, 0.0), color: c }, grid_half);
        }
    }
    if frame.axes {
        let ext = frame.grid.map_or(1e4, |g| g.spacing * g.count as f64);
        for (dir, c) in [(DVec3::X, AXIS_X), (DVec3::Y, AXIS_Y), (DVec3::Z, AXIS_Z)] {
            push_line(out, v, &Line { a: DVec3::ZERO, b: dir * ext, color: c }, line_half);
            push_line(out, v, &Line { a: DVec3::ZERO, b: -dir * ext, color: [c[0], c[1], c[2], 0.35] }, line_half);
        }
    }
    for l in frame.lines {
        push_line(out, &frame.view, l, line_half);
    }
}

#[cfg(test)]
mod tests;
