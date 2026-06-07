use std::mem;
use std::num::NonZeroU32;

use bytemuck::{Pod, Zeroable};
use cgmath::{Quaternion, Vector3};
use wgpu::{vertex_attr_array, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutEntry, BindingResource, BindingType, Face, FrontFace, PolygonMode, PrimitiveState, PrimitiveTopology, Sampler, SamplerBindingType, ShaderStages, TextureView, TextureSampleType, TextureViewDimension, VertexAttribute, VertexStepMode};

use crate::output::{OutputBindGroupLayoutDesc, OutputDeviceRc, OutputVertexBufferLayout};
use crate::util::IndexMap;

// Vertex

const VERTEX_POS_ATTRS: [VertexAttribute; 1] = vertex_attr_array![ // See vertex shader->@location().
    0 => Float32x3, // pos
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct VertexPos {
    pub pos: [f32; 3],
}

const VERTEX_POSNORMAL_ATTRS: [VertexAttribute; 2] = vertex_attr_array![ // See vertex shader->@location().
    0 => Float32x3, // pos
    1 => Float32x3, // normal
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct VertexPosNormal {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

pub enum VertexShaderType {
    Pos,
    PosNormal,
}

impl VertexShaderType {
    pub fn get_name(&self) -> &str {
        match self {
            VertexShaderType::Pos => "p",
            VertexShaderType::PosNormal => "pn",
        }
    }

    pub fn get_layout(&self) -> OutputVertexBufferLayout<'_> {
        let (array_stride, attributes) = match self {
            VertexShaderType::Pos => (mem::size_of::<VertexPos>(), VERTEX_POS_ATTRS.as_slice()),
            VertexShaderType::PosNormal => (mem::size_of::<VertexPosNormal>(), VERTEX_POSNORMAL_ATTRS.as_slice()),
        };

        OutputVertexBufferLayout {
            array_stride: array_stride.try_into().unwrap(),
            step_mode: VertexStepMode::Vertex,
            attributes,
        }
    }
}

pub fn get_default_primitive_state() -> PrimitiveState {
    PrimitiveState {
        topology: PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: FrontFace::Ccw,
        cull_mode: Some(Face::Back),
        unclipped_depth: false,
        polygon_mode: PolygonMode::Fill,
        conservative: false,
    }
}

// Instance

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Color(pub [f32; 3]);

pub const COLOR_WHITE: Color = Color([1.0, 1.0, 1.0]);

impl Color {
    pub fn from_srgb_byte(r: u8, g: u8, b: u8) -> Self {
        // Convert sRGB to linear, see https://physicallybased.info/tools/ .

        let convert = |srgb: u8| {
            let srgb: f32 = srgb as f32 / u8::MAX as f32;

            #[expect(clippy::excessive_precision)]
            if srgb < 0.04045 {
                srgb * 0.0773993808
            } else {
                (srgb * 0.9478672986 + 0.0521327014).powf(2.4)
            }
        };

        Self([convert(r), convert(g), convert(b)])
    }

    pub fn from_srgb_float(r: f32, g: f32, b: f32) -> Self {
        Self::from_srgb_byte((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8) // TODO: clamping?
    }
}

#[derive(Clone, Copy)]
pub struct SamplerId(u32);

#[derive(Clone, Copy)]
pub struct TextureId(u32);

pub enum BindLayoutType {
    Sampler,
    Texture,
}

type BindLayout = (BindLayoutType, u32);

type BindLayouts = Box<[BindLayout]>;

fn empty_bind_layouts() -> BindLayouts {
    Box::from([])
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ModelBuf {
    scale: [f32; 3],
    rot: [f32; 4],
    pos: [f32; 3],
}

impl ModelBuf {
    fn fill(scale: &Vector3<f32>, rot: &Quaternion<f32>, pos: &Vector3<f32>) -> Self {
        Self {
            scale: (*scale).into(),
            rot: (*rot).into(),
            pos: (*pos).into(),
        }
    }
}

const INST_SIMPLECOLOR_ATTRS: [VertexAttribute; 4] = vertex_attr_array![ // See vertex shader->@location().
    12 => Float32x3, // color
    13 => Float32x3, // model_scale
    14 => Float32x4, // model_rot
    15 => Float32x3, // model_pos
];

pub struct InstSimpleColor;

impl InstSimpleColor {
    fn new() -> Self {
        Self {
        }
    }

    fn get_bind_layouts(&self) -> BindLayouts {
        empty_bind_layouts()
    }

    fn create_bind_group(&self, _output_device: OutputDeviceRc, _bg_layout: &BindGroupLayout) -> BindGroup {
        panic!("No bind entries");
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstSimpleColorBuf {
    color: Color,
    model_buf: ModelBuf,
}

impl InstSimpleColorBuf {
    pub fn fill(color: &Color, model_scale: &Vector3<f32>, model_rot: &Quaternion<f32>, model_pos: &Vector3<f32>) -> Self {
        Self {
            color: *color,
            model_buf: ModelBuf::fill(model_scale, model_rot, model_pos),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct PhongParam {
    ambient: f32,
    diffuse: f32,
    specular: f32,
    shininess: f32,
}

impl PhongParam {
    pub const fn new(ambient: f32, diffuse: f32, specular: f32, shininess: f32) -> Self {
        Self {
            ambient,
            diffuse,
            specular,
            shininess,
        }
    }
}

const INST_PHONGCOLOR_ATTRS: [VertexAttribute; 5] = vertex_attr_array![ // See vertex shader->@location().
    11 => Float32x3, // color
    12 => Float32x4, // phong_param
    13 => Float32x3, // model_scale
    14 => Float32x4, // model_rot
    15 => Float32x3, // model_pos
];

pub struct InstPhongColor;

impl InstPhongColor {
    fn new() -> Self {
        Self {
        }
    }

    fn get_bind_layouts(&self) -> BindLayouts {
        empty_bind_layouts()
    }

    fn create_bind_group(&self, _output_device: OutputDeviceRc, _bg_layout: &BindGroupLayout) -> BindGroup {
        panic!("No bind entries");
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstPhongColorBuf {
    color: Color,
    phong_param: PhongParam,
    model_buf: ModelBuf,
}

impl InstPhongColorBuf {
    pub fn fill(color: &Color, phong_param: &PhongParam, model_scale: &Vector3<f32>, model_rot: &Quaternion<f32>, model_pos: &Vector3<f32>) -> Self {
        Self {
            color: *color,
            phong_param: *phong_param,
            model_buf: ModelBuf::fill(model_scale, model_rot, model_pos),
        }
    }
}

const INST_GRID_ATTRS: [VertexAttribute; 4] = vertex_attr_array![ // See vertex shader->@location().
    12 => Float32x3, // color
    13 => Float32x3, // model_scale
    14 => Float32x4, // model_rot
    15 => Float32x3, // model_pos
];

pub struct InstGrid;

impl InstGrid {
    fn new() -> Self {
        Self {
        }
    }

    fn get_bind_layouts(&self) -> BindLayouts {
        empty_bind_layouts()
    }

    fn create_bind_group(&self, _output_device: OutputDeviceRc, _bg_layout: &BindGroupLayout) -> BindGroup {
        panic!("No bind entries");
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstGridBuf {
    color: Color,
    model_buf: ModelBuf,
}

impl InstGridBuf {
    pub fn fill(color: &Color, model_scale: &Vector3<f32>, model_rot: &Quaternion<f32>, model_pos: &Vector3<f32>) -> Self {
        Self {
            color: *color,
            model_buf: ModelBuf::fill(model_scale, model_rot, model_pos),
        }
    }
}

const INST_WINDOW_ATTRS: [VertexAttribute; 4] = vertex_attr_array![ // See vertex shader->@location().
    12 => Uint32x2, // bind_id
    13 => Float32x3, // model_scale
    14 => Float32x4, // model_rot
    15 => Float32x3, // model_pos
];

pub struct InstWindow {
    samplers: IndexMap<Sampler>,
    textures: IndexMap<TextureView>, // TODO: What to store? Texture or TextureView?
}

impl InstWindow {
    fn new() -> Self {
        Self {
            samplers: IndexMap::new(),
            textures: IndexMap::new(),
        }
    }

    pub fn add_sampler(&mut self, sampler: &Sampler) -> SamplerId {
        // TODO: Sampler should be global for all instances, don't need binding_array<sampler>.
        SamplerId(self.samplers.add(sampler.clone()).try_into().unwrap())
    }

    pub fn add_texture(&mut self, texture: &TextureView) -> TextureId {
        TextureId(self.textures.add(texture.clone()).try_into().unwrap())
    }

    fn get_bind_layouts(&self) -> BindLayouts {
        Box::from([
            (BindLayoutType::Sampler, self.samplers.len().try_into().unwrap()), // 0
            (BindLayoutType::Texture, self.textures.len().try_into().unwrap()), // 1
        ])
    }

    fn create_bind_group(&self, output_device: OutputDeviceRc, bg_layout: &BindGroupLayout) -> BindGroup {
        let sampler_refs = Box::from_iter(self.samplers.iter());
        let texture_refs = Box::from_iter(self.textures.iter());

        output_device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: bg_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0, // See vertex/fragment shader->@binding().
                    resource: BindingResource::SamplerArray(&sampler_refs),
                },
                BindGroupEntry {
                    binding: 1, // See vertex/fragment shader->@binding().
                    resource: BindingResource::TextureViewArray(&texture_refs),
                },
            ]
        })
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstWindowBuf {
    bind_id: [u32; 2],
    model_buf: ModelBuf,
}

impl InstWindowBuf {
    pub fn fill(sampler_id: SamplerId, texture_id: TextureId, model_scale: &Vector3<f32>, model_rot: &Quaternion<f32>, model_pos: &Vector3<f32>) -> Self {
        Self {
            bind_id: [sampler_id.0, texture_id.0],
            model_buf: ModelBuf::fill(model_scale, model_rot, model_pos),
        }
    }
}

const INST_OUTLINEBOX_ATTRS: [VertexAttribute; 5] = vertex_attr_array![ // See vertex shader->@location().
    11 => Float32x3, // color
    12 => Float32,   // outline_width
    13 => Float32x3, // model_scale
    14 => Float32x4, // model_rot
    15 => Float32x3, // model_pos
];

pub struct InstOutlineBox;

impl InstOutlineBox {
    fn new() -> Self {
        Self {
        }
    }

    fn get_bind_layouts(&self) -> BindLayouts {
        empty_bind_layouts()
    }

    fn create_bind_group(&self, _output_device: OutputDeviceRc, _bg_layout: &BindGroupLayout) -> BindGroup {
        panic!("No bind entries");
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstOutlineBoxBuf {
    color: Color,
    outline_width: f32,
    model_buf: ModelBuf,
}

impl InstOutlineBoxBuf {
    pub fn fill(color: &Color, outline_width: f32, model_scale: &Vector3<f32>, model_rot: &Quaternion<f32>, model_pos: &Vector3<f32>) -> Self {
        Self {
            color: *color,
            outline_width,
            model_buf: ModelBuf::fill(model_scale, model_rot, model_pos),
        }
    }
}

#[expect(non_snake_case)]
#[expect(non_upper_case_globals)]
pub mod InstShaderSize {
    use std::mem;

    pub static SimpleColor: usize = mem::size_of::<super::InstSimpleColorBuf>();
    pub static PhongColor: usize = mem::size_of::<super::InstPhongColorBuf>();
    pub static Grid: usize = mem::size_of::<super::InstGridBuf>();
    pub static Window: usize = mem::size_of::<super::InstWindowBuf>();
    pub static OutlineBox: usize = mem::size_of::<super::InstOutlineBoxBuf>();
}

#[derive(Clone)]
pub enum InstShaderType {
    SimpleColor,
    PhongColor,
    Grid,
    Window,
    OutlineBox,
}

impl InstShaderType {
    pub fn get_name(&self) -> &str {
        match self {
            InstShaderType::SimpleColor => "simplec",
            InstShaderType::PhongColor => "phongc",
            InstShaderType::Grid => "grid",
            InstShaderType::Window => "window",
            InstShaderType::OutlineBox => "outlinebox",
        }
    }

    pub fn get_layout(&self) -> OutputVertexBufferLayout<'_> {
        let (array_stride, attributes) = match self {
            InstShaderType::SimpleColor => (InstShaderSize::SimpleColor, INST_SIMPLECOLOR_ATTRS.as_slice()),
            InstShaderType::PhongColor => (InstShaderSize::PhongColor, INST_PHONGCOLOR_ATTRS.as_slice()),
            InstShaderType::Grid => (InstShaderSize::Grid, INST_GRID_ATTRS.as_slice()),
            InstShaderType::Window => (InstShaderSize::Window, INST_WINDOW_ATTRS.as_slice()),
            InstShaderType::OutlineBox => (InstShaderSize::OutlineBox, INST_OUTLINEBOX_ATTRS.as_slice()),
        };

        OutputVertexBufferLayout {
            array_stride: array_stride.try_into().unwrap(),
            step_mode: VertexStepMode::Instance,
            attributes,
        }
    }

    pub fn create_impl(&self) -> InstShaderImplType {
        match self { // TODO: Refactor match.
            InstShaderType::SimpleColor => InstShaderImplType::SimpleColor(InstSimpleColor::new()),
            InstShaderType::PhongColor => InstShaderImplType::PhongColor(InstPhongColor::new()),
            InstShaderType::Grid => InstShaderImplType::Grid(InstGrid::new()),
            InstShaderType::Window => InstShaderImplType::Window(InstWindow::new()),
            InstShaderType::OutlineBox => InstShaderImplType::OutlineBox(InstOutlineBox::new()),
        }
    }
}

pub enum InstShaderImplType {
    SimpleColor(InstSimpleColor),
    PhongColor(InstPhongColor),
    Grid(InstGrid),
    Window(InstWindow),
    OutlineBox(InstOutlineBox),
}

impl InstShaderImplType {
    pub fn create_bind_group_layout(&self, output_device: OutputDeviceRc) -> Option<BindGroupLayout> {
        let layouts = self.get_bind_layouts();

        if !layouts.is_empty() {
            let entries = Box::from_iter(layouts.iter().enumerate().map(|(i, (t, count))| {
                assert!(*count > 0);

                BindGroupLayoutEntry {
                    binding: i.try_into().unwrap(), // See vertex/fragment shader->@binding().
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: match t {
                        BindLayoutType::Sampler => BindingType::Sampler(SamplerBindingType::Filtering),
                        BindLayoutType::Texture => BindingType::Texture {
                            sample_type: TextureSampleType::Float {
                                filterable: true,
                            },
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        }
                    },  
                    count: NonZeroU32::new(*count),
                }
            }));

            Some(output_device.create_bind_group_layout(&OutputBindGroupLayoutDesc {
                entries: &entries
            }))
        } else {
            None
        }
    }

    pub fn create_bind_group(&self, output_device: OutputDeviceRc, bg_layout: &BindGroupLayout) -> BindGroup {
        // Since BindGroupEntry->BindingResource contains references, we need to create
        // bind group here.

        match self { // TODO: Refactor match.
            InstShaderImplType::SimpleColor(inst_sh_impl) => inst_sh_impl.create_bind_group(output_device, bg_layout),
            InstShaderImplType::PhongColor(inst_sh_impl) => inst_sh_impl.create_bind_group(output_device, bg_layout),
            InstShaderImplType::Grid(inst_sh_impl) => inst_sh_impl.create_bind_group(output_device, bg_layout),
            InstShaderImplType::Window(inst_sh_impl) => inst_sh_impl.create_bind_group(output_device, bg_layout),
            InstShaderImplType::OutlineBox(inst_sh_impl) => inst_sh_impl.create_bind_group(output_device, bg_layout),
        }
    }

    fn get_bind_layouts(&self) -> BindLayouts {
        match self { // TODO: Refactor match.
            InstShaderImplType::SimpleColor(inst_sh_impl) => inst_sh_impl.get_bind_layouts(),
            InstShaderImplType::PhongColor(inst_sh_impl) => inst_sh_impl.get_bind_layouts(),
            InstShaderImplType::Grid(inst_sh_impl) => inst_sh_impl.get_bind_layouts(),
            InstShaderImplType::Window(inst_sh_impl) => inst_sh_impl.get_bind_layouts(),
            InstShaderImplType::OutlineBox(inst_sh_impl) => inst_sh_impl.get_bind_layouts(),
        }
    }
}
