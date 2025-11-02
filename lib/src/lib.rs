// TODO: run rustfmt
// TODO: re-export common libs, e.g. openxr, wgpu? -> targets don't need them as dependency in Cargo.toml
use std::rc::Rc;
use std::sync::Arc;

pub mod asset;
use asset::AssetManagerTrait;

mod audio;
use audio::{AudioEngine, AudioEngineRc};

pub mod circbuf;

mod model;

pub mod output;
use output::{Frame, OutputInfo};

mod render;
use render::Render;

pub mod scene;
use scene::SceneInput;

mod songinfo;

mod ui;

pub mod util;
use util::Stats;

#[cfg(test)]
mod tests;

pub const APP_NAME: &str = env!("CARGO_PKG_DESCRIPTION");
pub const APP_VERSION_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");
pub const APP_VERSION_MINOR: &str = env!("CARGO_PKG_VERSION_MINOR");
pub const APP_VERSION_PATCH: &str = env!("CARGO_PKG_VERSION_PATCH");

pub struct Main {
    audio_engine: AudioEngineRc,
    render: Render,
}

impl Main {
    pub fn new<A: AssetManagerTrait + Send + Sync + 'static>(asset_mgr: A, output_info: OutputInfo, stats: Stats) -> Self {
        let audio_engine = Rc::new(AudioEngine::new());
        let render = Render::new(Arc::new(asset_mgr), Rc::new(output_info), Arc::new(stats), Rc::clone(&audio_engine));

        Self {
            audio_engine,
            render,
        }
    }

    pub fn get_audio_engine(&self) -> &AudioEngine {
        &self.audio_engine
    }

    pub fn render<F: Frame>(&self, frame: F, scene_input: &SceneInput) {
        self.render.render(frame, scene_input);
    }
}
