use std::cell::RefCell;

use cgmath::{One, Quaternion, Vector3, Zero};

use crate::asset::AssetManagerRc;
use crate::output::OutputDeviceRc;
use crate::render::model::{Color, InstPhongColorBuf, InstShaderImplType, InstShaderType, Mesh, Model, ModelFactory, ModelHandle, Obj, PhongParam};
use crate::ui::UIManagerRc;

pub struct BombParam {
    color: Color,
    phong_param: PhongParam,
}

impl BombParam {
    pub fn new(color: &Color, phong_param: &PhongParam) -> Self {
        Self {
            color: *color,
            phong_param: *phong_param,
        }
    }
}

impl ModelFactory for BombParam {
    type Model = Bomb;

    fn get_mesh(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc) -> Mesh {
        Obj::open(asset_mgr, output_device, "bomb", &[
            ("bomb", &InstShaderType::PhongColor), // 0
        ])
    }

    fn create(self, handle: ModelHandle, _output_device: OutputDeviceRc, _inst_sh_impls: &mut [InstShaderImplType], _ui_manager: UIManagerRc) -> Self::Model {
        Bomb::new(self, handle)
    }
}

pub struct Bomb {
    param: BombParam,
    handle: ModelHandle,
    inner: RefCell<Inner>,
}

struct Inner {
    scale: f32,
    pos: Vector3<f32>,
}

impl Bomb {
    fn new(param: BombParam, handle: ModelHandle) -> Self {
        Self {
            param,
            handle,
            inner: RefCell::new(Inner {
                scale: 1.0,
                pos: Vector3::zero(),
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
}

impl Model for Bomb {
    fn fill_phong_color(&self, inst_index: u32) -> InstPhongColorBuf {
        assert!(inst_index == 0);

        let inner = self.inner.borrow();
        InstPhongColorBuf::fill(&self.param.color, &self.param.phong_param, &Vector3::new(inner.scale, inner.scale, inner.scale), &Quaternion::one(), &inner.pos)
    }
}
