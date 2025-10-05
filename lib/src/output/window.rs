use std::cell::RefCell;

use cgmath::{Angle, Deg, InnerSpace, Matrix, Matrix3, Matrix4, Point3, Rad, Vector3};
use wgpu::{CompositeAlphaMode, Device, DeviceDescriptor, Instance, PresentMode, Queue, RequestAdapterOptions, Surface, SurfaceConfiguration, SurfaceError, SurfaceTarget, SurfaceTexture, TextureFormat, TextureFormatFeatureFlags, TextureUsages, TextureView};

use crate::output::{DEPTH_FORMAT, NEAR_Z, FAR_Z, Frame, OutputInfo, ViewMat, create_texture, get_default_features, get_default_limits};

type OutputViewMat = ViewMat;

const FOVY: Deg<f32> = Deg(45.0);

pub struct WindowOutput {
    device: Device,
    queue: Queue,
    surface: Surface<'static>,
    color_format: TextureFormat,
    sample_count: u32,
    inner: RefCell<Inner>,
}

struct Inner {
    surface_config: SurfaceConfiguration,
    view_obj: Option<(Option<TextureView>, TextureView)>,
}

impl WindowOutput {
    pub async fn new(surface_target: SurfaceTarget<'static>) -> Self {
        let instance = Instance::new(&Default::default());
        let surface = instance.create_surface(surface_target).expect("Unable to create render surface");

        let adapter_opt = RequestAdapterOptions {
            power_preference: Default::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        };
        let adapter = instance.request_adapter(&adapter_opt).await.expect("Unable to request adapter");

        let features = get_default_features();
        let limits = get_default_limits();

        let device_desc = DeviceDescriptor {
            required_features: features,
            required_limits: limits,
            ..Default::default()
        };
        let (device, queue) = adapter.request_device(&device_desc).await.expect("Unable to request device");

        let surface_caps = surface.get_capabilities(&adapter);
        let color_format = *surface_caps.formats.iter().find(|format| format.is_srgb()).expect("Missing sRGB texture format");

        let mut sample_count = 1;

        let color_flags = adapter.get_texture_format_features(color_format).flags;
        for (flag, count) in [(TextureFormatFeatureFlags::MULTISAMPLE_X4, 4), (TextureFormatFeatureFlags::MULTISAMPLE_X2, 2)] { // TODO: enable higher msaa? needs TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            if color_flags.contains(flag) {
                sample_count = count;
                break;
            }
        }

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: color_format,
            width: 0,
            height: 0,
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: vec![],
        };

        Self {
            device,
            queue,
            surface,
            color_format,
            sample_count,
            inner: RefCell::new(Inner {
                surface_config,
                view_obj: None,
            })
        }
    }

    pub fn get_info(&self) -> OutputInfo { // TODO: prepare it it new and don't create new instance everytime?
        OutputInfo::new(&self.device, &self.queue, self.color_format, DEPTH_FORMAT, self.sample_count, 1, "", "0")
    }

    pub fn resize(&self, width: u32, height: u32) {
        if width == 0 || height == 0 { // TODO: how to handle zero size?
            return;
        }
        
        let mut inner = self.inner.borrow_mut();

        // Setup surface.

        inner.surface_config.width = width;
        inner.surface_config.height = height;
        self.surface.configure(&self.device, &inner.surface_config);

        // Setup multisample buffer.

        let multisample_view = if self.sample_count > 1 {
            let texture = create_texture(&self.device, width, height, 1, self.sample_count, self.color_format);
            Some(texture.create_view(&Default::default()))
        } else {
            None
        };

        // Setup depth buffer.

        let depth_texture = create_texture(&self.device, width, height, 1, self.sample_count, DEPTH_FORMAT);
        let depth_view = depth_texture.create_view(&Default::default());

        inner.view_obj = Some((multisample_view, depth_view));
    }

    pub fn begin(&self, cam_pos: &Vector3<f32>, cam_dir: &Vector3<f32>) -> WindowBegin {
        let inner = self.inner.borrow();

        let (multisample_view, depth_view) = match &inner.view_obj {
            Some(view_obj) => view_obj.clone(),
            None => return WindowBegin::NotInited,
        };
        
        let surface_texture = match self.surface.get_current_texture() {
            Ok(surface_texture) => surface_texture,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => return WindowBegin::ResizeNeeded,
            _ => panic!("Surface error"),
        };

        // Calculate view matrix.

        let surface_config = &inner.surface_config;
        let aspect = surface_config.width as f32 / surface_config.height as f32;
        
        let cam_m = Matrix4::look_to_rh(Point3::new(cam_pos.x, cam_pos.y, cam_pos.z), *cam_dir, Vector3::unit_z()); // my -> rh
        let proj_m = perspective(aspect, FOVY, NEAR_Z, FAR_Z); // rh -> lh
        let view_m = proj_m * cam_m;

        // Calculate inverse. I hope transpose is faster than cam_m.invert() :).

        let inv_cam_m = Matrix3::from_cols(cam_m.x.truncate(),cam_m.y.truncate(),cam_m.z.truncate()).transpose();
        
        let frame = WindowFrame::new(surface_texture, multisample_view, depth_view, view_m.into(), *cam_pos, inv_cam_m, aspect);
        WindowBegin::Frame(frame)
    }
}

#[allow(clippy::large_enum_variant)]
pub enum WindowBegin {
    NotInited,
    ResizeNeeded,
    Frame(WindowFrame),
}

pub struct WindowFrame {
    surface_texture: SurfaceTexture,
    color_view: TextureView,
    multisample_view: Option<TextureView>,
    depth_view: TextureView,
    view_m: OutputViewMat,
    cam_pos: Vector3<f32>,
    inv_cam_m: Matrix3<f32>,
    aspect: f32,
}

impl WindowFrame {
    fn new(surface_texture: SurfaceTexture, multisample_view: Option<TextureView>, depth_view: TextureView, view_m: OutputViewMat, cam_pos: Vector3<f32>, inv_cam_m: Matrix3<f32>, aspect: f32) -> Self {
        let color_view = surface_texture.texture.create_view(&Default::default());

        Self {
            surface_texture,
            color_view,
            multisample_view,
            depth_view,
            view_m,
            cam_pos,
            inv_cam_m,
            aspect,
        }
    }

    pub fn raycast(&self, x: f32, y: f32) -> Vector3<f32> {
        // For raycasting theory, see:
        // - https://antongerdelan.net/opengl/raycasting.html
        // - https://www.youtube.com/watch?v=lj5hx6pa_jE

        let tan_half_fovy = (FOVY / 2.0).tan();

        let dir = self.inv_cam_m * Vector3::new(x * self.aspect * tan_half_fovy, y * tan_half_fovy, -1.0);
        dir.normalize()
    }
}

impl Frame for WindowFrame {
    fn get_color_view(&self) -> &TextureView {
        &self.color_view
    }

    fn get_multisample_view(&self) -> Option<&TextureView> {
        self.multisample_view.as_ref()
    }

    fn get_depth_view(&self) -> &TextureView {
        &self.depth_view
    }

    fn get_cam_pos(&self) -> Vector3<f32> {
        self.cam_pos
    }

    fn set_view_m(&self, buf: &mut [u8]) {
        let buf_sl: &mut [OutputViewMat] = bytemuck::cast_slice_mut(buf);
        let view_m = &mut buf_sl[0];
        *view_m = self.view_m;
    }

    fn end(self) {
        self.surface_texture.present();
    }
}

fn perspective<A: Into<Rad<f32>>>(aspect: f32, fovy: A, near: f32, far: f32) -> Matrix4<f32> {
    // Calculate projection matrix suitable for wgpu NDC: (-1, -1, 0) ... (1, 1, 1).
    // Taken from nalgebra-glm->perspective_rh_zo.

    let tan_half_fovy = (fovy.into() / 2.0).tan();

    Matrix4::new(
        1.0 / (aspect * tan_half_fovy), 0.0, 0.0, 0.0,
        0.0, 1.0 / tan_half_fovy, 0.0, 0.0,
        0.0, 0.0, far / (near - far), -1.0,
        0.0, 0.0, -(far * near) / (far - near), 0.0
    )
}
