use std::cmp::Reverse;
use std::num::NonZeroU32;

use cgmath::Vector3;
use wgpu::{Adapter, Features, Limits, TextureFormat, TextureView};

mod output_device;
pub use output_device::*;

cfg_select! {
    feature = "window" => {
        mod window;
        pub use window::{WindowBegin, WindowOutput};
    },
    _ => {},
}

cfg_select! {
    feature = "xr" => {
        mod xr;
        pub use xr::XROutput;
    },
    _ => {},
}

const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
const NEAR_Z: f32 = 0.1;
const FAR_Z: f32 = 100.0;
const MAX_SAMPLE_COUNT: u32 = 4;

pub type ViewMat = [[f32; 4]; 4];

pub struct OutputInfo {
    color_format: TextureFormat,
    depth_format: TextureFormat,
    sample_count: u32,
    view_len: u32,
    diags: Vec<String>,
}

impl OutputInfo {
    fn new(color_format: TextureFormat, depth_format: TextureFormat, sample_count: u32, view_len: u32, diags: Vec<String>) -> Self {
        assert!(sample_count > 0);
        assert!(view_len > 0);

        Self {
            color_format,
            depth_format,
            sample_count,
            view_len,
            diags,
        }
    }

    pub fn get_color_format(&self) -> TextureFormat {
        // Swapchain texture format.

        self.color_format
    }

    pub fn get_depth_format(&self) -> TextureFormat {
        // Depth-buffer format.

        self.depth_format
    }

    pub fn get_sample_count(&self) -> u32 {
        // Sample count for MSAA.

        self.sample_count
    }

    pub fn get_view_len(&self) -> u32 {
        // Number of views:
        // - For non-stereo rendering, it is 1.
        // - For stereo (multiview) rendering, it is 2.

        self.view_len
    }

    pub fn get_view_mask(&self) -> Option<NonZeroU32> {
        NonZeroU32::new(if self.view_len == 1 { 0 } else { (1 << self.view_len) - 1 })
    }

    pub fn get_diags(&self) -> &[String] {
        &self.diags
    }
}

pub trait Frame {
    // Get current swapchain texture.
    fn get_color_view(&self) -> &TextureView;

    // Get camera position.
    fn get_cam_pos(&self) -> &Vector3<f32>; // TODO: For stereo rendering, is a single cam_pos (used for lighting calcs) sufficient?

    // Get view matrix.
    fn get_view_m(&self) -> &[u8];

    // Frame rendering has been finished.
    fn end(self);
}

fn get_default_features() -> Features {
    Features::default() |
    Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING |
    Features::TEXTURE_BINDING_ARRAY |
    Features::TIMESTAMP_QUERY |
    Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
}

fn get_default_limits() -> Limits {
    // These are arbitrary limits, change them if needed.
    
    Limits {
        max_binding_array_elements_per_shader_stage: 16,
        max_binding_array_sampler_elements_per_shader_stage: 16,
        ..Default::default()
    }
}

fn get_sample_count(adapter: &Adapter, color_format: TextureFormat) -> u32 {
    let mut sample_counts = adapter.get_texture_format_features(color_format).flags.supported_sample_counts();
    sample_counts.sort_by_key(|count| Reverse(*count));

    sample_counts.into_iter().find(|count| *count <= MAX_SAMPLE_COUNT).unwrap_or(1)
}
