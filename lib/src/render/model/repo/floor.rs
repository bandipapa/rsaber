use std::cell::RefCell;

use cgmath::{One, Quaternion, Vector3, Zero};
use wgpu::BufferUsages;
use wgpu::util::BufferInitDescriptor;

use crate::asset::AssetManagerRc;
use crate::output::OutputDeviceRc;
use crate::render::model::{Color, InstGridBuf, InstShaderImplType, InstShaderType, Mesh, Model, ModelFactory, ModelHandle, Submesh, VertexPos, VertexShaderType, get_default_primitive_state};
use crate::ui::UIManagerRc;

const RADIUS: f32 = 15.0; // TODO: make it adjustable via FloorParam?

pub struct FloorParam {
    color: Color,
}

impl FloorParam {
    pub fn new(color: &Color) -> Self {
        Self {
            color: *color,
        }
    }    
}

impl ModelFactory for FloorParam {
    type Model = Floor;

    fn get_mesh(_asset_mgr: AssetManagerRc, output_device: OutputDeviceRc) -> Mesh {
        // We don't have .obj file for floor, calculate mesh.

        let vertexes = [
            VertexPos { pos: [-RADIUS, -RADIUS, 0.0] },
            VertexPos { pos: [RADIUS, -RADIUS, 0.0] },
            VertexPos { pos: [-RADIUS, RADIUS, 0.0] },
            VertexPos { pos: [RADIUS, RADIUS, 0.0] },
        ];

        let indexes: [u16; 6] = [
            0,
            1,
            2,
            1,
            3,
            2,
        ];

        let submesh = Submesh::new(0, indexes.len() as u32, 0, get_default_primitive_state(), InstShaderType::Grid); // 0

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
        Floor::new(self, handle)
    }
}

pub struct Floor {
    param: FloorParam,
    handle: ModelHandle,
    inner: RefCell<Inner>,
}

struct Inner {
    pos: Vector3<f32>,
}

impl Floor {
    fn new(param: FloorParam, handle: ModelHandle) -> Self {
        Self {
            param,
            handle,
            inner: RefCell::new(Inner {
                pos: Vector3::zero(),
            }),
        }
    }

    pub fn set_visible(&self, visible: bool) {
        self.handle.set_visible(0, visible);
    }

    pub fn set_pos(&self, pos: &Vector3<f32>) {
        self.inner.borrow_mut().pos = *pos;
    }
}

impl Model for Floor {
    fn fill_grid(&self, inst_index: u32) -> InstGridBuf {
        assert!(inst_index == 0);

        let inner = self.inner.borrow();
        InstGridBuf::fill(&self.param.color, &Vector3::new(1.0, 1.0, 1.0), &Quaternion::one(), &inner.pos)
    }
}
