use std::mem;

use bytemuck::{Pod, Zeroable};
use cgmath::Matrix4;
use wgpu::{vertex_attr_array, Face, FrontFace, PolygonMode, PrimitiveState, PrimitiveTopology, VertexAttribute, VertexBufferLayout, VertexStepMode};

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

#[derive(Clone, Eq, Hash, PartialEq)]
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

    pub fn get_layout(&self) -> VertexBufferLayout<'_> {
        let (array_stride, attributes) = match self {
            VertexShaderType::Pos => (mem::size_of::<VertexPos>(), VERTEX_POS_ATTRS.as_slice()),
            VertexShaderType::PosNormal => (mem::size_of::<VertexPosNormal>(), VERTEX_POSNORMAL_ATTRS.as_slice()),
        };

        VertexBufferLayout {
            array_stride: array_stride.try_into().unwrap(),
            step_mode: VertexStepMode::Vertex,
            attributes,
        }
    }
}

// PrimitiveState

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum PrimitiveStateType {
    LineList,
    TriangleList,
}

impl PrimitiveStateType {
    pub fn get_primitive(&self) -> PrimitiveState {
        let topology = match self {
            PrimitiveStateType::LineList => PrimitiveTopology::LineList,
            PrimitiveStateType::TriangleList => PrimitiveTopology::TriangleList,
        };

        PrimitiveState {
            topology,
            strip_index_format: None,
            front_face: FrontFace::Ccw,
            cull_mode: Some(Face::Back),
            unclipped_depth: false,
            polygon_mode: PolygonMode::Fill,
            conservative: false,
        }
    }
}

// Instance

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct Color(pub [f32; 3]);

impl Color {
    pub fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        // Convert sRGB to linear, see https://physicallybased.info/tools/ .

        let convert = |srgb: u8| {
            let srgb: f32 = srgb as f32 / u8::MAX as f32;

            #[allow(clippy::excessive_precision)]
            if srgb < 0.04045 {
                srgb * 0.0773993808
            } else {
                (srgb * 0.9478672986 + 0.0521327014).powf(2.4)
            }
        };

        Self([convert(r), convert(g), convert(b)])
    }
}

const INST_SIMPLECOLOR_ATTRS: [VertexAttribute; 5] = vertex_attr_array![ // See vertex shader->@location().
    11 => Float32x3, // color
    12 => Float32x4, // model_m
    13 => Float32x4,
    14 => Float32x4,
    15 => Float32x4,
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstSimpleColor {
    color: Color,
    model_m: [[f32; 4]; 4],
}

impl InstSimpleColor {
    #[allow(dead_code)]
    pub fn fill(&mut self, color: &Color, model_m: &Matrix4<f32>) {
        self.color = *color;
        self.model_m = (*model_m).into();
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
    pub fn new(ambient: f32, diffuse: f32, specular: f32, shininess: f32) -> Self {
        Self {
            ambient,
            diffuse,
            specular,
            shininess,
        }
    }
}

const INST_PHONGCOLOR_ATTRS: [VertexAttribute; 6] = vertex_attr_array![ // See vertex shader->@location().
    10 => Float32x3, // color
    11 => Float32x4, // phong_param
    12 => Float32x4, // model_m
    13 => Float32x4,
    14 => Float32x4,
    15 => Float32x4,
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstPhongColor {
    color: Color,
    phong_param: PhongParam,
    model_m: [[f32; 4]; 4],
}

impl InstPhongColor {
    pub fn fill(&mut self, color: &Color, phong_param: &PhongParam, model_m: &Matrix4<f32>) {
        self.color = *color;
        self.phong_param = *phong_param;
        self.model_m = (*model_m).into();
    }
}

const INST_GRID_ATTRS: [VertexAttribute; 5] = vertex_attr_array![ // See vertex shader->@location().
    11 => Float32x3, // color
    12 => Float32x4, // model_m
    13 => Float32x4,
    14 => Float32x4,
    15 => Float32x4,
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct InstGrid {
    color: Color,
    model_m: [[f32; 4]; 4],
}

impl InstGrid {
    pub fn fill(&mut self, color: &Color, model_m: &Matrix4<f32>) {
        self.color = *color;
        self.model_m = (*model_m).into();
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum InstShaderType {
    SimpleColor,
    PhongColor,
    Grid,
}

impl InstShaderType {
    pub fn get_name(&self) -> &str {
        match self {
            InstShaderType::SimpleColor => "simplec",
            InstShaderType::PhongColor => "phongc",
            InstShaderType::Grid => "grid",
        }
    }

    pub fn get_layout(&self) -> VertexBufferLayout<'_> {
        let (array_stride, attributes) = match self {
            InstShaderType::SimpleColor => (mem::size_of::<InstSimpleColor>(), INST_SIMPLECOLOR_ATTRS.as_slice()),
            InstShaderType::PhongColor => (mem::size_of::<InstPhongColor>(), INST_PHONGCOLOR_ATTRS.as_slice()),
            InstShaderType::Grid => (mem::size_of::<InstGrid>(), INST_GRID_ATTRS.as_slice()),
        };

        VertexBufferLayout {
            array_stride: array_stride.try_into().unwrap(),
            step_mode: VertexStepMode::Instance,
            attributes,
        }
    }
}
