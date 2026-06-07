use std::cell::RefCell;

use cgmath::{One, Quaternion, Vector3, Zero};
use wgpu::BufferUsages;
use wgpu::util::BufferInitDescriptor;

use crate::asset::AssetManagerRc;
use crate::output::OutputDeviceRc;
use crate::render::model::{Color, InstOutlineBoxBuf, InstShaderImplType, InstShaderType, Mesh, Model, ModelFactory, ModelHandle, Submesh, VertexPosNormal, VertexShaderType, get_default_primitive_state};
use crate::ui::UIManagerRc;

const POS: f32 = 0.5;
const OUTLINE: f32 = 1.0;

pub struct OutlineBoxParam {
    color: Color,
    outline_width: f32,
}

impl OutlineBoxParam {
    pub fn new(color: &Color, outline_width: f32) -> Self {
        Self {
            color: *color,
            outline_width,
        }
    }    
}

impl ModelFactory for OutlineBoxParam {
    type Model = OutlineBox;

    fn get_mesh(_asset_mgr: AssetManagerRc, output_device: OutputDeviceRc) -> Mesh {
        // Implementation notes:
        // - We don't have .obj file for box, calculate mesh.
        // - The box is freely scalable, however its outline width
        //   should remain constant.
        // - Each side of the box is composed of four rectangles.
        // - These rectangles (after transformation) are copied to all 6 sides
        //   to construct the final box.
        // - The normal component of VertexPosNormal is used to store
        //   the outline parameters to avoid defining new vertex type.
        //
        // +---+------------+---+
        // |   |            |   |
        // +---+------------+---+
        // |   |      ^     |   |
        // |   |      |+y   |   |
        // |   |      o-+x->|   |
        // |   |            |   |
        // |   |            |   |
        // +---+------------+---+
        // |   |            |   |
        // +---+------------+---+

        let side_datas = [ // pos_x, pos_y, outline_x, outline_y
            (-POS, POS, 0.0, 0.0),
            (-POS, -POS, 0.0, 0.0),
            (-POS, -POS, OUTLINE, 0.0),
            (-POS, POS, OUTLINE, 0.0),

            (-POS, -POS, 0.0, OUTLINE),
            (-POS, -POS, 0.0, 0.0),
            (POS, -POS, 0.0, 0.0),
            (POS, -POS, 0.0, OUTLINE),

            (POS, POS, -OUTLINE, 0.0),
            (POS, -POS, -OUTLINE, 0.0),
            (POS, -POS, 0.0, 0.0),
            (POS, POS, 0.0, 0.0),

            (-POS, POS, 0.0, 0.0),
            (-POS, POS, 0.0, -OUTLINE),
            (POS, POS, 0.0, -OUTLINE),
            (POS, POS, 0.0, 0.0),
        ];

        let tr_x_neg = |(pos_x, pos_y, outline_x, outline_y): (f32, f32, f32, f32)| VertexPosNormal { pos: [-POS, -pos_x, pos_y], normal: [0.0, -outline_x, outline_y] };
        let tr_x_pos = |(pos_x, pos_y, outline_x, outline_y): (f32, f32, f32, f32)| VertexPosNormal { pos: [POS, pos_x, pos_y], normal: [0.0, outline_x, outline_y] };
        let tr_y_neg = |(pos_x, pos_y, outline_x, outline_y): (f32, f32, f32, f32)| VertexPosNormal { pos: [pos_x, -POS, pos_y], normal: [outline_x, 0.0, outline_y] };
        let tr_y_pos = |(pos_x, pos_y, outline_x, outline_y): (f32, f32, f32, f32)| VertexPosNormal { pos: [-pos_x, POS, pos_y], normal: [-outline_x, 0.0, outline_y] };
        let tr_z_neg = |(pos_x, pos_y, outline_x, outline_y): (f32, f32, f32, f32)| VertexPosNormal { pos: [pos_x, -pos_y, -POS], normal: [outline_x, -outline_y, 0.0] };
        let tr_z_pos = |(pos_x, pos_y, outline_x, outline_y): (f32, f32, f32, f32)| VertexPosNormal { pos: [pos_x, pos_y, POS], normal: [outline_x, outline_y, 0.0] };

        let mut vertexes = Vec::new();

        for tr in [tr_x_neg, tr_x_pos, tr_y_neg, tr_y_pos, tr_z_neg, tr_z_pos] {
            for side_data in side_datas {
                let vertex = tr(side_data);
                vertexes.push(vertex);
            }
        }

        let mut indexes: Vec<u16> = Vec::new();
        let mut vertex_index = 0;

        for _i in 0..6 {
            for _j in 0..4 {
                indexes.push(vertex_index);
                indexes.push(vertex_index + 1);
                indexes.push(vertex_index + 2);
                indexes.push(vertex_index);
                indexes.push(vertex_index + 2);
                indexes.push(vertex_index + 3);

                vertex_index += 4;
            }
        }

        let mut primitive_state = get_default_primitive_state();
        primitive_state.cull_mode = None;

        let submesh = Submesh::new(0, indexes.len() as u32, 0, primitive_state, InstShaderType::OutlineBox); // 0

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

        Mesh::new(vertex_buf, index_buf, VertexShaderType::PosNormal, submeshes)
    }

    fn create(self, handle: ModelHandle, _output_device: OutputDeviceRc, _inst_sh_impls: &mut [InstShaderImplType], _ui_manager: UIManagerRc) -> Self::Model {
        OutlineBox::new(self, handle)
    }
}

pub struct OutlineBox {
    param: OutlineBoxParam,
    handle: ModelHandle,
    inner: RefCell<Inner>,
}

struct Inner {
    scale: (f32, f32, f32),
    pos: Vector3<f32>,
}

impl OutlineBox {
    fn new(param: OutlineBoxParam, handle: ModelHandle) -> Self {
        Self {
            param,
            handle,
            inner: RefCell::new(Inner {
                scale: (1.0, 1.0, 1.0),
                pos: Vector3::zero(),
            }),
        }
    }

    pub fn set_visible(&self, visible: bool) {
        self.handle.set_visible(0, visible);
    }

    pub fn set_scale(&self, scale_x: f32, scale_y: f32, scale_z: f32) {
        self.inner.borrow_mut().scale = (scale_x, scale_y, scale_z);
    }

    pub fn set_pos(&self, pos: &Vector3<f32>) {
        self.inner.borrow_mut().pos = *pos;
    }
}

impl Model for OutlineBox {
    fn fill_outlinebox(&self, inst_index: u32) -> InstOutlineBoxBuf {
        assert!(inst_index == 0);

        let inner = self.inner.borrow();
        InstOutlineBoxBuf::fill(&self.param.color, self.param.outline_width, &Vector3::new(inner.scale.0, inner.scale.1, inner.scale.2), &Quaternion::one(), &inner.pos)
    }
}
