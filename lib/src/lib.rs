// TODO: run rustfmt
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

// Re-export crates, so targets don't need them specified in their Cargo.toml.

pub use cgmath;
pub use wgpu;

#[cfg(feature = "xr")]
pub use openxr;

pub mod asset;
use asset::AssetManagerTrait;

mod audio;
use audio::{AudioEngine, AudioEngineRc};

mod circbuf;

mod macros {
    pub use rsaber_macros::*;
}

mod mailbox;

mod net;

pub mod output;
use output::{Frame, OutputDeviceRc};

mod render;
use render::Render;

pub mod scene;
use scene::SceneInput;

mod simd;

mod songdef;

mod songinfo;

mod ui;

pub mod util;
use util::Stats;

#[cfg(test)]
mod tests;

pub const APP_NAME: &str = "rsaber";

const APP_VERSION_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");
const APP_VERSION_MINOR: &str = env!("CARGO_PKG_VERSION_MINOR");
const APP_VERSION_PATCH: &str = env!("CARGO_PKG_VERSION_PATCH");

static APP_VERSION: LazyLock<String> = LazyLock::new(|| format!("{}.{}.{}", APP_VERSION_MAJOR, APP_VERSION_MINOR, APP_VERSION_PATCH));

pub struct Main {
    audio_engine: AudioEngineRc,
    render: Render,
}

impl Main {
    pub fn new<A: AssetManagerTrait + Send + Sync + 'static>(asset_mgr: A, output_device: OutputDeviceRc, stats: Stats) -> Self {
        let audio_engine = Rc::new(AudioEngine::new());
        let render = Render::new(Arc::new(asset_mgr), output_device, Arc::new(stats), Rc::clone(&audio_engine));

        Self {
            audio_engine,
            render,
        }
    }

    pub fn get_audio_engine(&self) -> &AudioEngine {
        &self.audio_engine
    }

    pub fn configure(&self, width: u32, height: u32) {
        self.render.configure(width, height);
    }

    pub fn render<F: Frame>(&self, frame: F, scene_input: &SceneInput) {
        self.render.render(frame, scene_input);
    }
}
