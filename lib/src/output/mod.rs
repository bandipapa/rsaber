use std::rc::Rc;

use cfg_if::cfg_if;
use cgmath::Vector3;
use wgpu::{Device, Extent3d, Features, Limits, Queue, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView};

cfg_if! {
    if #[cfg(feature = "window")] {
        mod window;
        pub use window::{WindowBegin, WindowOutput};
    }
}

cfg_if! {
    if #[cfg(feature = "xr")] {
        mod xr;
        pub use xr::XROutput;
    }
}

const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
const NEAR_Z: f32 = 0.1;
const FAR_Z: f32 = 100.0;

pub type ViewMat = [[f32; 4]; 4];

pub type OutputInfoRc = Rc<OutputInfo>;

pub struct OutputInfo {
    device: Device,
    queue: Queue,
    color_format: TextureFormat,
    depth_format: TextureFormat,
    sample_count: u32,
    view_len: u32,
    view_index_def: String,
    view_index_val: String,
}

impl OutputInfo {
    #[allow(clippy::too_many_arguments)]
    fn new<S: AsRef<str>>(device: &Device, queue: &Queue, color_format: TextureFormat, depth_format: TextureFormat, sample_count: u32, view_len: u32, view_index_def: S, view_index_val: S) -> Self {
        assert!(sample_count > 0);
        assert!(view_len > 0);

        Self {
            device: device.clone(),
            queue: queue.clone(),
            color_format,
            depth_format,
            sample_count,
            view_len,
            view_index_def: view_index_def.as_ref().to_string(),
            view_index_val: view_index_val.as_ref().to_string(),
        }
    }

    pub fn get_device(&self) -> &Device {
        &self.device
    }

    pub fn get_queue(&self) -> &Queue {
        &self.queue
    }

    pub fn get_color_format(&self) -> TextureFormat {
        self.color_format
    }

    pub fn get_depth_format(&self) -> TextureFormat {
        self.depth_format
    }

    pub fn get_sample_count(&self) -> u32 {
        self.sample_count
    }

    pub fn get_view_len(&self) -> u32 {
        self.view_len
    }

    pub fn get_view_index_def(&self) -> &str {
        &self.view_index_def
    }

    pub fn get_view_index_val(&self) -> &str {
        &self.view_index_val
    }
}

pub trait Frame {
    fn get_color_view(&self) -> &TextureView;
    fn get_multisample_view(&self) -> Option<&TextureView>;
    fn get_depth_view(&self) -> &TextureView;
    fn get_cam_pos(&self) -> Vector3<f32>; // TODO: For stereo rendering, is a single cam_pos (used for lighting calcs) sufficient? // TODO: return with &?
    fn set_view_m(&self, buf: &mut [u8]);
    fn end(self);
}

fn get_default_features() -> Features {
    Features::default() | Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING | Features::TEXTURE_BINDING_ARRAY | Features::TIMESTAMP_QUERY
}

fn get_default_limits() -> Limits {
    // These are arbitrary limits, change them if needed.
    
    Limits {
        max_binding_array_elements_per_shader_stage: 8,
        max_binding_array_sampler_elements_per_shader_stage: 8,
        ..Default::default()
    }
}

fn create_texture(device: &Device, width: u32, height: u32, layers: u32, sample_count: u32, format: TextureFormat) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
