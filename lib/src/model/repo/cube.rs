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
        // TODO: Make it possible to use the same submesh, e.g. body_l and body_r are the same, but mirrored.
        Obj::open(asset_mgr, device, "cube", &[
            ("body_l", &InstShaderType::PhongColor), // 0
            ("body_r", &InstShaderType::PhongColor), // 1
            ("arrow", &InstShaderType::PhongColor), // 2
            ("dot", &InstShaderType::PhongColor), // 3
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
    mode: CubeMode,
    scale: f32,
    pos: Vector3<f32>,
    pos_l: Vector3<f32>,
    pos_r: Vector3<f32>,
    rot: Quaternion<f32>,
    rot_l: Quaternion<f32>,
    rot_r: Quaternion<f32>,
}

// CubeMode variants don't encapsulate any mode specific data, since
// it is easier to implement logic this way.
// TODO: maybe create an additional model for sliced (half)cubes and simplify this code?
enum CubeMode {
    Single,
    Sliced,
}

impl Cube {
    fn new(param: CubeParam, handle: ModelHandle) -> Self {
        Self {
            param,
            handle,
            inner: RefCell::new(Inner {
                mode: CubeMode::Single,
                scale: 1.0,
                pos: Vector3::new(0.0, 0.0, 0.0),
                pos_l: Vector3::new(0.0, 0.0, 0.0),
                pos_r: Vector3::new(0.0, 0.0, 0.0),
                rot: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                rot_l: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                rot_r: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            }),
        }
    }

    pub fn sliced(&self) {
        let mut inner = self.inner.borrow_mut();
        assert!(matches!(inner.mode, CubeMode::Single));

        inner.mode = CubeMode::Sliced;

        // Symbols are not visible in sliced mode.

        self.handle.set_visible(2, false);
        self.handle.set_visible(3, false);
    }

    pub fn set_visible(&self, visible: bool) {
        let inner = self.inner.borrow();
        assert!(matches!(inner.mode, CubeMode::Single));
        
        self.handle.set_visible(0, visible);
        self.handle.set_visible(1, visible);
        self.handle.set_visible(2, visible && matches!(self.param.symbol, CubeSymbol::Arrow));
        self.handle.set_visible(3, visible && matches!(self.param.symbol, CubeSymbol::Dot));
    }

    pub fn set_visible_l(&self, visible: bool) {
        let inner = self.inner.borrow();
        assert!(matches!(inner.mode, CubeMode::Sliced));
        
        self.handle.set_visible(0, visible);
    }

    pub fn set_visible_r(&self, visible: bool) {
        let inner = self.inner.borrow();
        assert!(matches!(inner.mode, CubeMode::Sliced));
        
        self.handle.set_visible(1, visible);
    }

    pub fn set_scale(&self, scale: f32) {
        self.inner.borrow_mut().scale = scale;
    }

    pub fn set_pos(&self, pos: &Vector3<f32>) {
        let mut inner = self.inner.borrow_mut();
        assert!(matches!(inner.mode, CubeMode::Single));

        inner.pos = *pos;
    }

    pub fn set_pos_l(&self, pos: &Vector3<f32>) {
        let mut inner = self.inner.borrow_mut();
        assert!(matches!(inner.mode, CubeMode::Sliced));

        inner.pos_l = *pos;
    }

    pub fn set_pos_r(&self, pos: &Vector3<f32>) {
        let mut inner = self.inner.borrow_mut();
        assert!(matches!(inner.mode, CubeMode::Sliced));

        inner.pos_r = *pos;
    }

    pub fn set_rot(&self, rot: &Quaternion<f32>) {
        let mut inner = self.inner.borrow_mut();
        assert!(matches!(inner.mode, CubeMode::Single));

        inner.rot = *rot;
    }

    pub fn set_rot_l(&self, rot: &Quaternion<f32>) {
        let mut inner = self.inner.borrow_mut();
        assert!(matches!(inner.mode, CubeMode::Sliced));

        inner.rot_l = *rot;
    }

    pub fn set_rot_r(&self, rot: &Quaternion<f32>) {
        let mut inner = self.inner.borrow_mut();
        assert!(matches!(inner.mode, CubeMode::Sliced));

        inner.rot_r = *rot;
    }
}

impl Model for Cube {
    fn fill_phong_color(&self, inst_index: u32, inst_sh_buf: &mut InstPhongColorBuf) {
        let (color, phong_param) = match inst_index {
            0 | 1 => (&self.param.body_color, &self.param.body_phong_param),
            2 | 3 => (&self.param.symbol_color, &self.param.symbol_phong_param),
            _ => panic!("Unknown inst_index"),
        };

        let inner = self.inner.borrow();

        let (pos, rot) = match inner.mode {
            CubeMode::Single => (inner.pos, inner.rot),
            CubeMode::Sliced => match inst_index {
                0 => (inner.pos_l, inner.rot_l),
                1 => (inner.pos_r, inner.rot_r),
                _ => panic!("Mode is Sliced"),
            }
        };

        let model_m = Matrix4::from_translation(pos) * Matrix4::from(rot) * Matrix4::from_scale(inner.scale);
        inst_sh_buf.fill(color, phong_param, &model_m);
    }
}
