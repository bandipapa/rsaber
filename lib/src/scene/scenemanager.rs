use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cgmath::{Quaternion, Vector3};
use tokio::runtime::Runtime;
use wgpu::CommandEncoder;

use crate::asset::AssetManagerRc;
use crate::audio::AudioEngineRc;
use crate::net::NetManager;
use crate::output::{Frame, OutputDeviceRc};
use crate::render::RenderGraph;
use crate::ui::{UIManager, UIManagerRc, UISubr};
use crate::util::StatsRc;

pub trait SceneFactory {
    type Scene: Scene + 'static;
    type Error;

    fn load(self, asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, audio_engine: AudioEngineRc, ui_manager: UIManagerRc, net_manager: &NetManager) -> Result<Self::Scene, Self::Error>; // TODO: Put all these parameters into a struct?
}

pub trait Scene { // TODO: add lifecycle methods?
    fn update(&self, scene_mgr: &SceneManager, scene_input: &SceneInput);
    fn get_rg(&self) -> &RenderGraph;
}

pub struct SceneInput<'a> {
    // As we are using Scene as a trait object, we can't have type parameters
    // for SceneInput. Therefore, use trait object for ScenePose.
    
    pub pose_l_opt: Option<&'a dyn ScenePose>,
    pub pose_r_opt: Option<&'a dyn ScenePose>,
}

pub trait ScenePose {
    fn get_pos(&self) -> &Vector3<f32>;
    fn get_rot(&self) -> &Quaternion<f32>;
    fn get_click(&self) -> bool;
    fn get_scroll(&self) -> ScenePoseScroll;
    fn get_render(&self) -> bool;
    fn apply_haptic(&self);
}

pub type ScenePoseScroll = (f32, f32);

type SceneBox = Box<dyn Scene>;

pub struct SceneManager {
    asset_mgr: AssetManagerRc,
    output_device: OutputDeviceRc,
    stats: StatsRc,
    audio_engine: AudioEngineRc,
    ui_manager: UIManagerRc,
    ui_subr: UISubr,
    net_manager: NetManager,
    next_scene_opt: RefCell<Option<SceneBox>>,
    in_render: Cell<bool>,
    inner: RefCell<Inner>,
}

struct Inner {
    scene_opt: Option<SceneBox>,
    size_opt: Option<(u32, u32)>,
}

impl SceneManager {
    pub fn new(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, audio_engine: AudioEngineRc) -> Self {
        // Init UI subsystem.

        let ui_manager = Rc::new(UIManager::new(Rc::clone(&output_device)));
        let ui_subr = UISubr::new();

        // Init async runtime: at the moment it is created outside of NetManager,
        // since in the future we can have additional subsystems which are using the
        // async runtime.

        let async_runtime = Arc::new(Runtime::new().expect("Failed to initialize async runtime")); // TODO: Specify config parameters?
        let net_manager = NetManager::new(async_runtime);

        let inner = Inner {
            scene_opt: None,
            size_opt: None,
        };

        Self {
            asset_mgr,
            output_device,
            stats,
            audio_engine,
            ui_manager,
            ui_subr,
            net_manager,
            next_scene_opt: RefCell::new(None),
            in_render: Cell::new(false),
            inner: RefCell::new(inner),
        }
    }

    pub fn load<F: SceneFactory>(&self, factory: F) -> Result<(), F::Error> {
        {
            let mut next_scene_opt = self.next_scene_opt.borrow_mut();
            assert!(next_scene_opt.is_none());

            let next_scene = factory.load(Arc::clone(&self.asset_mgr), Rc::clone(&self.output_device), Arc::clone(&self.stats), Rc::clone(&self.audio_engine), Rc::clone(&self.ui_manager), &self.net_manager);

            // Drop cache after scene load, so the next load will be
            // started with empty cache.

            self.output_device.clear_cache();

            let next_scene = Box::new(next_scene?); // TODO: Load next scene: this is going to block the renderloop. Do it on different thread?
            *next_scene_opt = Some(next_scene);
        }

        if !self.in_render.get() {
            self.change_scene();
        }

        Ok(())
    }

    pub fn configure(&self, width: u32, height: u32) {
        let mut inner = self.inner.borrow_mut();
        inner.size_opt = Some((width, height));

        let scene = inner.scene_opt.as_ref().unwrap();
        let rg = scene.get_rg();
        rg.configure(width, height);
    }

    pub fn render(&self, scene_input: &SceneInput, encoder: &mut CommandEncoder, frame: &dyn Frame) {
        // Frame is received as a trait object, so we don't have to
        // utilize type parameters.

        {
            let inner = self.inner.borrow();
            assert!(inner.size_opt.is_some());
            let scene = inner.scene_opt.as_ref().unwrap();
            
            self.in_render.set(true); // Prevent immediate scene change, see load().
            scene.update(self, scene_input);
            self.in_render.set(false);
        }

        // If we have loaded a next scene:
        // - Don't do rendering of the current scene.
        // - The next invocation of render() will render the next scene to have scene.update() called.

        if !self.change_scene() {
            let inner = self.inner.borrow();
            let scene = inner.scene_opt.as_ref().unwrap();
            let rg = scene.get_rg();
            rg.render(encoder, frame);
        }
    }

    pub fn get_ui_subr(&self) -> &UISubr {
        &self.ui_subr
    }

    fn change_scene(&self) -> bool {
        let mut next_scene_opt = self.next_scene_opt.borrow_mut();
        if let Some(next_scene) = next_scene_opt.take() {
            // Replace current scene with next scene.

            let mut inner = self.inner.borrow_mut();

            if let Some((width, height)) = inner.size_opt {
                let rg = next_scene.get_rg();
                rg.configure(width, height);
            }

            inner.scene_opt = Some(next_scene); // TODO: Drop current scene: this is going to block the renderloop. Do it on different thread?

            self.ui_subr.reset();

            true
        } else {
            false
        }
    }
}
