use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cgmath::{Quaternion, Vector3};
use wgpu::{BindGroupLayout, RenderPass};

use crate::asset::AssetManagerRc;
use crate::audio::AudioEngineRc;
use crate::model::{ModelRegistry, ModelRenderer};
use crate::output::OutputInfoRc;
use crate::ui::{UILoop, UIManager, UIManagerRc, UISubr};
use crate::util::StatsRc;

pub trait SceneFactory {
    type Scene: Scene + 'static;

    fn load(self, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, stats: StatsRc, audio_engine: AudioEngineRc, ui_loop: &UILoop) -> Self::Scene; // TODO: Put all these parameters into a struct?
}

pub trait Scene { // TODO: add lifecycle methods?
    fn update(&self, scene_mgr: &SceneManager, scene_input: &SceneInput);
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
    fn get_render(&self) -> bool;
    fn apply_haptic(&self);
}

pub struct SceneManager {
    asset_mgr: AssetManagerRc,
    output_info: OutputInfoRc,
    stats: StatsRc,
    uni_bg_layout: BindGroupLayout,
    audio_engine: AudioEngineRc,
    ui_manager: UIManagerRc,
    ui_subr: UISubr,
    scene_info_opt: RefCell<Option<SceneInfo>>,
    next_scene_info_opt: RefCell<Option<SceneInfo>>,
    in_render: Cell<bool>,
}

impl SceneManager {
    pub fn new(asset_mgr: AssetManagerRc, output_info: OutputInfoRc, stats: StatsRc, uni_bg_layout: BindGroupLayout, audio_engine: AudioEngineRc) -> Self {
        let ui_manager = Rc::new(UIManager::new(output_info.get_queue().clone()));
        let ui_subr = UISubr::new();

        Self {
            asset_mgr,
            output_info,
            stats,
            uni_bg_layout,
            audio_engine,
            ui_manager,
            ui_subr,
            scene_info_opt: RefCell::new(None),
            next_scene_info_opt: RefCell::new(None),
            in_render: Cell::new(false),
        }
    }

    pub fn load<F: SceneFactory>(&self, factory: F) {
        {
            let mut next_scene_info_opt = self.next_scene_info_opt.borrow_mut();
            assert!(next_scene_info_opt.is_none());

            // TODO: Implement cache, since ModelRegistry/Obj is going to reload/compile assets on scene switch.
            let mut model_reg = ModelRegistry::new(Arc::clone(&self.asset_mgr), Rc::clone(&self.output_info), Rc::clone(&self.ui_manager));
            let scene = Box::new(factory.load(Arc::clone(&self.asset_mgr), &mut model_reg, Arc::clone(&self.stats), Rc::clone(&self.audio_engine), self.ui_manager.get_ui_loop())); // TODO: Load next scene: this is going to block the renderloop. Do it on different thread?
            let model_renderer = model_reg.build(Arc::clone(&self.stats), &self.uni_bg_layout);

            let next_scene_info = SceneInfo::new(scene, model_renderer);
            *next_scene_info_opt = Some(next_scene_info);
        }

        if !self.in_render.get() {
            self.change_scene();
        }
    }

    pub fn render(&self, scene_input: &SceneInput, render_pass: &mut RenderPass) {
        let have_scene = {
            let scene_info_opt = self.scene_info_opt.borrow();

            if let Some(scene_info) = &*scene_info_opt {
                self.in_render.set(true); // Prevent immediate scene change, see load().
                scene_info.scene.update(self, scene_input);
                self.in_render.set(false);

                true
            } else {
                false
            }
        };

        if have_scene {
            // If we have loaded a next scene:
            // - Don't do rendering of the current scene, which will result in a black screen.
            // - The next invocation of render() will render the next scene.

            if !self.change_scene() {
                let scene_info_opt = self.scene_info_opt.borrow();
                scene_info_opt.as_ref().unwrap().model_renderer.render(render_pass);
            }
        }
    }

    pub fn get_ui_subr(&self) -> &UISubr {
        &self.ui_subr
    }

    fn change_scene(&self) -> bool {
        let mut next_scene_info_opt = self.next_scene_info_opt.borrow_mut();
        if let Some(next_scene_info) = next_scene_info_opt.take() {
            // Replace current scene with next scene.

            let mut scene_info_opt = self.scene_info_opt.borrow_mut();
            *scene_info_opt = Some(next_scene_info); // TODO: Drop current scene: this is going to block the renderloop. Do it on different thread?

            self.ui_subr.reset();

            true
        } else {
            false
        }
    }
}

struct SceneInfo {
    scene: Box<dyn Scene>,
    model_renderer: ModelRenderer,
}

impl SceneInfo {
    fn new(scene: Box<dyn Scene>, model_renderer: ModelRenderer) -> Self {
        Self {
            scene,
            model_renderer,
        }
    }
}
