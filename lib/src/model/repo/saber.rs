use std::cell::RefCell;

use cgmath::{Matrix4, Quaternion, Vector3};
use wgpu::Device;

use crate::AssetManagerRc;
use crate::model::{Color, InstPhongColor, InstShaderType, Mesh, Model, ModelFactory, ModelHandle, Obj, PhongParam};

pub struct SaberParam {
    handle_color: Color,
    handle_phong_param: PhongParam,
    ray_color: Color,
    ray_phong_param: PhongParam,
}

impl SaberParam {
    pub fn new(handle_color: &Color, handle_phong_param: &PhongParam, ray_color: &Color, ray_phong_param: &PhongParam) -> Self {
        Self {
            handle_color: *handle_color,
            handle_phong_param: *handle_phong_param,
            ray_color: *ray_color,
            ray_phong_param: *ray_phong_param,
        }
    }
}

impl ModelFactory for SaberParam {
    type Model = Saber;

    fn get_name() -> &'static str {
        "saber"
    }

    fn get_mesh(asset_mgr: AssetManagerRc, device: &Device) -> Mesh {
        Obj::open(asset_mgr, device, "saber", &[
            ("handle", &InstShaderType::PhongColor), // 0
            ("ray", &InstShaderType::PhongColor), // 1
        ])
    }

    fn create(self, handle: ModelHandle) -> Self::Model {
        Saber::new(self, handle)
    }
}

pub struct Saber {
    param: SaberParam,
    handle: ModelHandle,
    inner: RefCell<Inner>,
}

struct Inner {
    pos: Vector3<f32>,
    rot: Quaternion<f32>,
}

impl Saber {
    fn new(param: SaberParam, handle: ModelHandle) -> Self {
        Self {
            param,
            handle,
            inner: RefCell::new(Inner {
                pos: Vector3::new(0.0, 0.0, 0.0),
                rot: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            }),
        }
    }

    pub fn set_visible(&self, visible: bool) {
        self.handle.set_visible(0, visible);
        self.handle.set_visible(1, visible);
    }

    pub fn set_pos(&self, pos: &Vector3<f32>) {
        self.inner.borrow_mut().pos = *pos;
    }

    pub fn set_rot(&self, rot: &Quaternion<f32>) {
        self.inner.borrow_mut().rot = *rot;
    }
}

impl Model for Saber {
    fn fill_phong_color(&self, inst_index: u32, inst_sh_buf: &mut InstPhongColor) {
        let (color, phong_param) = match inst_index {
            0 => (&self.param.handle_color, &self.param.handle_phong_param),
            1 => (&self.param.ray_color, &self.param.ray_phong_param),
            _ => panic!("Unknown inst_index"),
        };

        let inner = self.inner.borrow();
        let model_m = Matrix4::from_translation(inner.pos) * Matrix4::from(inner.rot);
        inst_sh_buf.fill(color, phong_param, &model_m);
    }
}
