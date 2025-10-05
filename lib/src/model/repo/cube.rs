// TODO: rename to Note, to be consistent with songinfo?
use std::cell::RefCell;

use cgmath::{Matrix4, Quaternion, Vector3};
use wgpu::Device;

use crate::asset::AssetManagerRc;
use crate::model::{Color, InstPhongColorBuf, InstShaderImplType, InstShaderType, Mesh, Model, ModelFactory, ModelHandle, Obj, PhongParam};
use crate::ui::UIManagerRc;

pub enum CubeSymbol {
    Arrow,
    Dot,
}

pub struct CubeParam {
    symbol: CubeSymbol,
    body_color: Color,
    body_phong_param: PhongParam,
    symbol_color: Color,
    symbol_phong_param: PhongParam,
}

impl CubeParam {
    pub fn new(symbol: CubeSymbol, body_color: &Color, body_phong_param: &PhongParam, symbol_color: &Color, symbol_phong_param: &PhongParam) -> Self {
        Self {
            symbol,
            body_color: *body_color,
            body_phong_param: *body_phong_param,
            symbol_color: *symbol_color,
            symbol_phong_param: *symbol_phong_param,
        }
    }
}

impl ModelFactory for CubeParam {
    type Model = Cube;

    fn get_name() -> &'static str {
        "cube"
    }

    fn get_mesh(asset_mgr: AssetManagerRc, device: &Device) -> Mesh {
        Obj::open(asset_mgr, device, "cube", &[
            ("body", &InstShaderType::PhongColor), // 0
            ("arrow", &InstShaderType::PhongColor), // 1
            ("dot", &InstShaderType::PhongColor), // 2
        ])
    }

    fn create(self, handle: ModelHandle, _device: &Device, _inst_sh_impls: &mut [InstShaderImplType], _ui_manager: UIManagerRc) -> Self::Model {
        Cube::new(self, handle)
    }
}

pub struct Cube {
    param: CubeParam,
    handle: ModelHandle,
    inner: RefCell<Inner>,
}

struct Inner {
    scale: f32,
    pos: Vector3<f32>,
    rot: Quaternion<f32>,
}

impl Cube {
    fn new(param: CubeParam, handle: ModelHandle) -> Self {
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
        self.handle.set_visible(1, visible && matches!(self.param.symbol, CubeSymbol::Arrow));
        self.handle.set_visible(2, visible && matches!(self.param.symbol, CubeSymbol::Dot));
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

impl Model for Cube {
    fn fill_phong_color(&self, inst_index: u32, inst_sh_buf: &mut InstPhongColorBuf) {
        let (color, phong_param) = match inst_index {
            0 => (&self.param.body_color, &self.param.body_phong_param),
            1 | 2 => (&self.param.symbol_color, &self.param.symbol_phong_param),
            _ => panic!("Unknown inst_index"),
        };

        let inner = self.inner.borrow();
        let model_m = Matrix4::from_translation(inner.pos) * Matrix4::from(inner.rot) * Matrix4::from_scale(inner.scale);
        inst_sh_buf.fill(color, phong_param, &model_m);
    }
}
