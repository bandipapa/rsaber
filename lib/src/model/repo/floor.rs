use std::cell::RefCell;

use cgmath::{Matrix4, Vector3};
use wgpu::{BufferUsages, Device};
use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::asset::AssetManagerRc;
use crate::model::{Color, InstGridBuf, InstShaderImplType, InstShaderType, Mesh, Model, ModelFactory, ModelHandle, PrimitiveStateType, Submesh, VertexPos, VertexShaderType};
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

    fn get_name() -> &'static str {
        "floor"
    }

    fn get_mesh(_asset_mgr: AssetManagerRc, device: &Device) -> Mesh {
        // We don't have .obj file for floor, calculate mesh.

        let mut vertexes = Vec::new();
        let mut indexes: Vec<u16> = Vec::new();

        // Create quad.

        vertexes.push(VertexPos { pos: [-RADIUS, -RADIUS, 0.0] });
        vertexes.push(VertexPos { pos: [RADIUS, -RADIUS, 0.0] });
        vertexes.push(VertexPos { pos: [-RADIUS, RADIUS, 0.0] });
        vertexes.push(VertexPos { pos: [RADIUS, RADIUS, 0.0] });

        indexes.push(0);
        indexes.push(1);
        indexes.push(2);
        indexes.push(1);
        indexes.push(3);
        indexes.push(2);

        let submesh = Submesh::new(0, indexes.len() as u32, 0, PrimitiveStateType::TriangleList, InstShaderType::Grid); // 0

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
                pos: Vector3::new(0.0, 0.0, 0.0),
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
    fn fill_grid(&self, inst_index: u32, inst_sh_buf: &mut InstGridBuf) {
        assert!(inst_index == 0);

        let inner = self.inner.borrow();
        let model_m = Matrix4::from_translation(inner.pos);
        inst_sh_buf.fill(&self.param.color, &model_m);
    }
}
