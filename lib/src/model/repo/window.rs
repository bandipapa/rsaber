use std::cell::RefCell;

use cgmath::{InnerSpace, Matrix4, Quaternion, Vector3};
use slint::ComponentHandle;
use wgpu::{BufferUsages, Device, Extent3d, FilterMode, SamplerDescriptor, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};
use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::asset::AssetManagerRc;
use crate::model::{InstShaderImplType, InstShaderType, InstWindowBuf, Mesh, Model, ModelFactory, ModelHandle, PrimitiveStateType, SamplerId, Submesh, TextureId, VertexPos, VertexShaderType, SABER_DIR};
use crate::scene::ScenePose;
use crate::ui::{SlintComponentHandle, UIEvent, UIManagerRc, UIWindow, UIWindowWeak};

const SIZE: f32 = 0.5;

pub struct WindowParam<F> {
    width: u32,
    height: u32,
    func: F,
}

impl<F: FnOnce() -> C + Send + 'static, C: SlintComponentHandle + 'static> WindowParam<F> {
    pub fn new(width: u32, height: u32, func: F) -> Self {
        assert!(width > 0 && height > 0);

        Self {
            width,
            height,
            func,
        }
    }    
}

impl<F: FnOnce() -> C + Send + 'static, C: SlintComponentHandle + 'static> ModelFactory for WindowParam<F> {
    type Model = Window;

    fn get_name() -> &'static str {
        "window"
    }

    fn get_mesh(_asset_mgr: AssetManagerRc, device: &Device) -> Mesh {
        // We don't have .obj file for window, calculate mesh.

        let mut vertexes = Vec::new();
        let mut indexes: Vec<u16> = Vec::new();

        // Create quad.

        vertexes.push(VertexPos { pos: [-SIZE, 0.0, -SIZE] });
        vertexes.push(VertexPos { pos: [SIZE, 0.0, -SIZE] });
        vertexes.push(VertexPos { pos: [-SIZE, 0.0, SIZE] });
        vertexes.push(VertexPos { pos: [SIZE, 0.0, SIZE] });

        indexes.push(0);
        indexes.push(1);
        indexes.push(2);
        indexes.push(1);
        indexes.push(3);
        indexes.push(2);

        let submesh = Submesh::new(0, indexes.len() as u32, 0, PrimitiveStateType::TriangleList, InstShaderType::Window); // 0

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

    fn create(self, handle: ModelHandle, device: &Device, inst_sh_impls: &mut [InstShaderImplType], ui_manager: UIManagerRc) -> Self::Model {
        Window::new(self, handle, device, inst_sh_impls, ui_manager)
    }
}

pub struct Window {
    handle: ModelHandle,
    sampler_id: SamplerId,
    texture_id: TextureId,
    ui_window: UIWindow,
    inner: RefCell<Inner>,
}

struct Inner {
    scale: (f32, f32),
    pos: Vector3<f32>,
    rot: Quaternion<f32>,
}

impl Window {
    fn new<F: FnOnce() -> C + Send + 'static, C: SlintComponentHandle + 'static>(param: WindowParam<F>, handle: ModelHandle, device: &Device, inst_sh_impls: &mut [InstShaderImplType], ui_manager: UIManagerRc) -> Self {
        let inst_window = if let InstShaderImplType::Window(inst_window) = &mut inst_sh_impls[0] {
            inst_window
        } else {
            panic!("Shader mismatch");
        };

        let width = param.width;
        let height = param.height;

        // TODO: We can use a single sampler instance here, don't need to create one per window.
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });

        let sampler_id = inst_window.add_sampler(&sampler);

        let texture = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texture_id = inst_window.add_texture(&texture.create_view(&Default::default()));

        let ui_window = ui_manager.create_window(width, height, param.func, texture);

        Self {
            handle,
            sampler_id,
            texture_id,
            ui_window,
            inner: RefCell::new(Inner {
                scale: (1.0, 1.0),
                pos: Vector3::new(0.0, 0.0, 0.0),
                rot: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            }),
        }
    }

    pub fn set_visible(&self, visible: bool) {
        self.handle.set_visible(0, visible);
    }

    pub fn set_scale(&self, scale_x: f32, scale_z: f32) {
        self.inner.borrow_mut().scale = (scale_x, scale_z);
    }

    pub fn set_pos(&self, pos: &Vector3<f32>) {
        self.inner.borrow_mut().pos = *pos;
    }

    pub fn set_rot(&self, rot: &Quaternion<f32>) {
        self.inner.borrow_mut().rot = *rot;
    }

    pub fn as_weak<C: ComponentHandle + 'static>(&self) -> UIWindowWeak<C> {
        self.ui_window.as_weak()
    }

    pub fn intersect(&self, pose: &dyn ScenePose) -> Option<(f32, f32, f32)> { // TODO: introduce struct for return type?
        // TODO: Test for window visibility.
        // Calculate line-plane intersection, see https://en.wikipedia.org/wiki/Line%E2%80%93plane_intersection .

        let inner = self.inner.borrow();

        // 1) Check if the pose is pointing to the window facing toward us.

        let l = pose.get_rot() * SABER_DIR.normalize();
        let n = inner.rot * Vector3::new(0.0, -1.0, 0.0);

        let ln_dot = cgmath::dot(l, n);
        if ln_dot >= 0.0 {
            return None;
        }

        // 2) Calculate distance.

        let p0 = &inner.pos;
        let l0 = pose.get_pos();

        let d = cgmath::dot(p0 - l0, n) / ln_dot;
        if d <= 0.0 {
            return None;
        }

        // 3) Calculate intersection point on XZ plane.

        let (scale_x, scale_z) = inner.scale;

        let p = l0 + l * d;
        let p = inner.rot.conjugate() * (p - p0) + Vector3::new(scale_x / 2.0, 0.0, scale_z / 2.0); // p - p0: apply inverse transformation of p0 to p.
        let (p_x, p_z) = (p.x, p.z);

        // 4) Do AABB test.

        if (0.0..scale_x).contains(&p_x) && (0.0..scale_z).contains(&p_z) {
            Some((d, p_x / scale_x, 1.0 - p_z / scale_z))
        } else {
            None
        }
    }

    pub fn handle_event(&self, event: UIEvent) {
        self.ui_window.handle_event(event);
    }
}

impl Model for Window {
    fn fill_window(&self, inst_index: u32, inst_sh_buf: &mut InstWindowBuf) {
        assert!(inst_index == 0);

        let inner = self.inner.borrow();
        let model_m = Matrix4::from_translation(inner.pos) * Matrix4::from(inner.rot) * Matrix4::from_nonuniform_scale(inner.scale.0, 1.0, inner.scale.1);
        inst_sh_buf.fill(self.sampler_id, self.texture_id, &model_m);
    }
}
