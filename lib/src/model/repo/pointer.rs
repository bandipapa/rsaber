use std::cell::RefCell;

use cgmath::{Matrix4, Quaternion, Vector3};
use wgpu::{BufferUsages, Device};
use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::asset::AssetManagerRc;
use crate::model::{Color, InstShaderImplType, InstShaderType, InstSimpleColorBuf, Mesh, Model, ModelFactory, ModelHandle, PrimitiveStateType, Submesh, VertexPos, VertexShaderType, SABER_DIR};
use crate::ui::UIManagerRc;

pub struct PointerParam {
    color: Color,
}

impl PointerParam {
    pub fn new(color: &Color) -> Self {
        Self {
            color: *color,
        }
    }    
}

impl ModelFactory for PointerParam {
    type Model = Pointer;

    fn get_name() -> &'static str {
        "pointer"
    }

    fn get_mesh(_asset_mgr: AssetManagerRc, device: &Device) -> Mesh {
        // We don't have .obj file for pointer, calculate mesh.

        let mut vertexes = Vec::new();
        let mut indexes: Vec<u16> = Vec::new();

        // Create line.

        vertexes.push(VertexPos { pos: [0.0, 0.0, 0.0] });
        vertexes.push(VertexPos { pos: [SABER_DIR.x, SABER_DIR.y, SABER_DIR.z] });

        indexes.push(0);
        indexes.push(1);

        let submesh = Submesh::new(0, indexes.len() as u32, 0, PrimitiveStateType::LineList, InstShaderType::SimpleColor); // 0

        // Create buffers.

        let vertex_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&vertexes),
            usage: BufferUsages::VERTEX,
        });

        let index_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&indexes),
            usage: BufferUsages::INDEX,
        });

        let submeshes = Box::from([submesh]);

        Mesh::new(vertex_buf, index_buf, VertexShaderType::Pos, submeshes)
    }

    fn create(self, handle: ModelHandle, _device: &Device, _inst_sh_impls: &mut [InstShaderImplType], _ui_manager: UIManagerRc) -> Self::Model {
        Pointer::new(self, handle)
    }
}

pub struct Pointer {
    param: PointerParam,
    handle: ModelHandle,
    inner: RefCell<Inner>,    
}

struct Inner {
    scale: f32,
    pos: Vector3<f32>,
    rot: Quaternion<f32>,
}

impl Pointer { // TODO: would be nice if we can integrate this one to saber model
    fn new(param: PointerParam, handle: ModelHandle) -> Self {
        Self {
            param,
            handle,
            inner: RefCell::new(Inner {
                scale: 1.0,
                pos: Vector3::new(0.0, 0.0, 0.0),
                rot: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            }),
        }
    }

    pub fn set_visible(&self, visible: bool) {
        self.handle.set_visible(0, visible);
    }

    pub fn set_scale(&self, scale: f32) {
        self.inner.borrow_mut().scale = scale;
    }

    pub fn set_pos(&self, pos: &Vector3<f32>) {
        self.inner.borrow_mut().pos = *pos;
    }

    pub fn set_rot(&self, rot: &Quaternion<f32>) {
        self.inner.borrow_mut().rot = *rot;
    }
}

impl Model for Pointer {
    fn fill_simple_color(&self, inst_index: u32, inst_sh_buf: &mut InstSimpleColorBuf) {
        assert!(inst_index == 0);

        let inner = self.inner.borrow();
        let model_m = Matrix4::from_translation(inner.pos) * Matrix4::from(inner.rot) * Matrix4::from_scale(inner.scale);
        inst_sh_buf.fill(&self.param.color, &model_m);
    }
}
