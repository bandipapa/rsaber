use std::ops::Range;

use wgpu::{Buffer, PrimitiveState};

use crate::render::model::{InstShaderType, VertexShaderType};

pub struct Mesh {
    vertex_buf: Buffer,
    index_buf: Buffer,
    vertex_sh_type: VertexShaderType,
    submeshes: Box<[Submesh]>,
}

impl Mesh {
    // TODO: Add support for bufferless (to embed vertex coords into shader, e.g. for pointer).
    pub fn new(vertex_buf: Buffer, index_buf: Buffer, vertex_sh_type: VertexShaderType, submeshes: Box<[Submesh]>) -> Self {
        assert!(!submeshes.is_empty());

        Self {
            vertex_buf,
            index_buf,
            vertex_sh_type,
            submeshes,
        }
    }

    pub fn get_vertex_buf(&self) -> &Buffer {
        &self.vertex_buf
    }

    pub fn get_index_buf(&self) -> &Buffer {
        &self.index_buf
    }

    pub fn get_vertex_sh_type(&self) -> &VertexShaderType {
        &self.vertex_sh_type
    }

    pub fn get_submeshes(&self) -> &[Submesh] {
        &self.submeshes
    }
}

pub struct Submesh {
    index_start: u32,
    index_end: u32,
    base_vertex: i32, // It is i32, see ModelRenderer->render->draw_indexed().
    primitive_state: PrimitiveState,
    inst_sh_type: InstShaderType,
}

impl Submesh {
    pub fn new(index_start: u32, index_end: u32, base_vertex: i32, primitive_state: PrimitiveState, inst_sh_type: InstShaderType) -> Self {
        assert!(index_start < index_end);

        Self {
            index_start,
            index_end,
            base_vertex,
            primitive_state,
            inst_sh_type,
        }
    }

    pub fn get_indices(&self) -> Range<u32> {
        self.index_start..self.index_end
    }

    pub fn get_base_vertex(&self) -> i32 {
        self.base_vertex
    }

    pub fn get_primitive_state(&self) -> &PrimitiveState {
        &self.primitive_state
    }

    pub fn get_inst_sh_type(&self) -> &InstShaderType {
        &self.inst_sh_type
    }
}
