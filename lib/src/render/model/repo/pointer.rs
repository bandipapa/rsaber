use std::cell::RefCell;

use cgmath::{InnerSpace, One, Quaternion, Vector3, Zero};
use wgpu::{BufferUsages, PrimitiveTopology};
use wgpu::util::BufferInitDescriptor;

use crate::asset::AssetManagerRc;
use crate::output::OutputDeviceRc;
use crate::render::model::{Color, InstShaderImplType, InstShaderType, InstSimpleColorBuf, Mesh, Model, ModelFactory, ModelHandle, Submesh, VertexPos, VertexShaderType, SABER_DIR, get_default_primitive_state};
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

    fn get_mesh(_asset_mgr: AssetManagerRc, output_device: OutputDeviceRc) -> Mesh {
        // We don't have .obj file for pointer, calculate mesh.

        let dir = SABER_DIR.normalize();

        let vertexes = [
            VertexPos { pos: [0.0, 0.0, 0.0] },
            VertexPos { pos: [dir.x, dir.y, dir.z] },
        ];

        let indexes: [u16; 2] = [
            0,
            1,
        ];

        let mut primitive_state = get_default_primitive_state();
        primitive_state.topology = PrimitiveTopology::LineList;

        let submesh = Submesh::new(0, indexes.len() as u32, 0, primitive_state, InstShaderType::SimpleColor); // 0

        // Create buffers.

        let vertex_buf = output_device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&vertexes),
            usage: BufferUsages::VERTEX,
        });

        let index_buf = output_device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&indexes),
            usage: BufferUsages::INDEX,
        });

        let submeshes = Box::from([submesh]);

        Mesh::new(vertex_buf, index_buf, VertexShaderType::Pos, submeshes)
    }

    fn create(self, handle: ModelHandle, _output_device: OutputDeviceRc, _inst_sh_impls: &mut [InstShaderImplType], _ui_manager: UIManagerRc) -> Self::Model {
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
                pos: Vector3::zero(),
                rot: Quaternion::one(),
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
    fn fill_simple_color(&self, inst_index: u32) -> InstSimpleColorBuf {
        assert!(inst_index == 0);

        let inner = self.inner.borrow();
        InstSimpleColorBuf::fill(&self.param.color, &Vector3::new(inner.scale, inner.scale, inner.scale), &inner.rot, &inner.pos)
    }
}
