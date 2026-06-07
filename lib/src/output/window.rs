use std::cell::RefCell;
use std::rc::Rc;

use cgmath::{Angle, Deg, InnerSpace, Matrix, Matrix3, Matrix4, Point3, Rad, Vector3};
use wgpu::{CompositeAlphaMode, CurrentSurfaceTexture, Device, DeviceDescriptor, Instance, InstanceDescriptor, PresentMode, Queue, RequestAdapterOptions, Surface, SurfaceColorSpace, SurfaceConfiguration, SurfaceTarget, SurfaceTexture, TextureUsages, TextureView};

use crate::output::{DEPTH_FORMAT, FAR_Z, Frame, NEAR_Z, OutputDevice, OutputDeviceRc, OutputInfo, ViewMat, get_default_features, get_default_limits, get_sample_count};

type OutputViewMat = ViewMat;

const FOVY: Deg<f32> = Deg(45.0);

pub struct WindowOutput {
    device: Device,
    queue: Queue,
    surface: Surface<'static>,
    output_device: OutputDeviceRc,
    inner: RefCell<Inner>,
}

struct Inner { // TODO: We have only a single field, keep struct?
    surface_config: SurfaceConfiguration,
}

impl WindowOutput {
    // Do not include any winit dependency into WindowOutput.

    pub async fn new(instance_desc: InstanceDescriptor, surface_target: SurfaceTarget<'static>) -> Self {
        let instance = Instance::new(instance_desc);
        let surface = instance.create_surface(surface_target).expect("Unable to create render surface");

        let adapter_opt = RequestAdapterOptions {
            power_preference: Default::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
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

        let adapter_info = device.adapter_info();
        let diags = vec![
            format!("Adapter: {}", adapter_info.name),
            format!("Driver: {}/{}", adapter_info.driver, adapter_info.driver_info),
        ];

        let surface_caps = surface.get_capabilities(&adapter);
        let color_format = *surface_caps.formats.iter().find(|format| format.is_srgb()).expect("Missing sRGB texture format");

        let sample_count = get_sample_count(&adapter, color_format);

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: color_format,
            color_space: SurfaceColorSpace::Auto,
            width: 0,
            height: 0,
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: vec![],
        };

        let output_info = OutputInfo::new(color_format, DEPTH_FORMAT, sample_count, 1, diags);
        let output_device = Rc::new(OutputDevice::new(&device, &queue, output_info));

        Self {
            device,
            queue,
            surface,
            output_device,
            inner: RefCell::new(Inner {
                surface_config,
            })
        }
    }

    pub fn get_output_device(&self) -> OutputDeviceRc {
        Rc::clone(&self.output_device)
    }

    pub fn resize(&self, width: u32, height: u32) {
        assert!(width > 0 && height > 0);

        // Setup surface.

        let mut inner = self.inner.borrow_mut();

        inner.surface_config.width = width;
        inner.surface_config.height = height;

        self.surface.configure(&self.device, &inner.surface_config);
    }

    pub fn begin(&self, cam_pos: &Vector3<f32>, cam_dir: &Vector3<f32>) -> WindowBegin {
        let inner = self.inner.borrow();
        
        let surface_texture = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(surface_texture) => surface_texture,
            CurrentSurfaceTexture::Occluded | CurrentSurfaceTexture::Timeout => return WindowBegin::Skip,
            CurrentSurfaceTexture::Suboptimal(_) | CurrentSurfaceTexture::Lost | CurrentSurfaceTexture::Outdated => return WindowBegin::ResizeNeeded,
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
        
        let frame = WindowFrame::new(&self.queue, surface_texture, view_m.into(), *cam_pos, inv_cam_m, aspect);
        WindowBegin::Frame(frame)
    }
}

#[expect(clippy::large_enum_variant)]
pub enum WindowBegin {
    Skip,
    ResizeNeeded,
    Frame(WindowFrame),
}

pub struct WindowFrame {
    queue: Queue,
    surface_texture: SurfaceTexture,
    color_view: TextureView,
    view_m: OutputViewMat,
    cam_pos: Vector3<f32>,
    inv_cam_m: Matrix3<f32>,
    aspect: f32,
}

impl WindowFrame {
    fn new(queue: &Queue, surface_texture: SurfaceTexture, view_m: OutputViewMat, cam_pos: Vector3<f32>, inv_cam_m: Matrix3<f32>, aspect: f32) -> Self {
        let color_view = surface_texture.texture.create_view(&Default::default());

        Self {
            queue: queue.clone(),
            surface_texture,
            color_view,
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

    fn get_cam_pos(&self) -> &Vector3<f32> {
        &self.cam_pos
    }

    fn get_view_m(&self) -> &[u8] {
        bytemuck::cast_slice(&self.view_m)
    }

    fn end(self) {
        self.queue.present(self.surface_texture);
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
