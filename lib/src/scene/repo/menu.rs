use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use cgmath::{Deg, Quaternion, Rotation3, Vector3};

use crate::asset::AssetManagerRc;
use crate::audio::{AudioEngineRc, AudioFileFactory, AudioFileHandle, AudioFileTimestamp};
use crate::model::*;
use crate::scene::{GameParam, Scene, SceneFactory, SceneInput, SceneManager, create_floor, create_saber, create_stats_window};
use crate::songinfo::{SongInfo, ColorScheme};
use crate::ui::{AboutWindow, PoweredByWindow, SearchWindow, UILoop};
use crate::util::StatsRc;

const POINTER_COLOR: Color = Color([0.4, 0.4, 0.4]);

pub struct MenuParam;

impl MenuParam {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
        }
    }
}

impl SceneFactory for MenuParam {
    type Scene = Menu;

    fn load(self, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, stats: StatsRc, audio_engine: AudioEngineRc, ui_loop: &UILoop) -> Self::Scene {
        Menu::new(self, asset_mgr, model_reg, stats, audio_engine, ui_loop)
    }
}

pub struct Menu {
    asset_mgr: AssetManagerRc,
    audio_engine: AudioEngineRc,
    about_window: Rc<Window>,
    search_window_rx: Receiver<bool>,
    search_window: Rc<Window>,
    poweredby_window: Rc<Window>,
    saber_l: Rc<Saber>,
    saber_r: Rc<Saber>,
    pointer: Rc<Pointer>,
    inner: RefCell<Inner>,
}

struct Inner {
    audio_file_opt: Option<AudioFileHandle>,
}

impl Menu {
    fn new(_param: MenuParam, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, stats: StatsRc, audio_engine: AudioEngineRc, ui_loop: &UILoop) -> Self {
        // Setup about by window.

        let window_param = WindowParam::new(500, 500, || {
            AboutWindow::new().unwrap()
        });

        let about_window = model_reg.create(window_param);
        about_window.set_visible(true);
        about_window.set_scale(2.0, 2.0);
        about_window.set_pos(&Vector3::new(-5.0, 3.5, 2.0));
        about_window.set_rot(&Quaternion::from_angle_z(Deg(45.0)));

        // Setup search window.

        let (search_window_tx, search_window_rx) = mpsc::channel();

        let window_param = WindowParam::new(1000, 500, move || {
            let window = SearchWindow::new().unwrap();
            window.set_test_visible(cfg!(feature = "test"));

            window.on_start({
                let search_window_tx = search_window_tx.clone();
                move || search_window_tx.send(false).unwrap()
            });

            // TODO: Add support for https://github.com/BeatLeader/BS-Open-Replay ?
            #[cfg(feature = "test")]
            window.on_test({
                move || search_window_tx.send(true).unwrap()
            });

            window
        });

        let search_window = model_reg.create(window_param);
        search_window.set_visible(true);
        search_window.set_scale(4.0, 2.0);
        search_window.set_pos(&Vector3::new(0.0, 4.0, 2.0));

        // Setup powered by window.

        let window_param = WindowParam::new(500, 500, || {
            PoweredByWindow::new().unwrap()
        });

        let poweredby_window = model_reg.create(window_param);
        poweredby_window.set_visible(true);
        poweredby_window.set_scale(2.0, 2.0);
        poweredby_window.set_pos(&Vector3::new(5.0, 3.5, 2.0));
        poweredby_window.set_rot(&Quaternion::from_angle_z(Deg(-45.0)));

        // Setup floor.

        create_floor(model_reg);
        create_stats_window(model_reg, stats, ui_loop);

        // Setup sabers.

        let color_scheme = ColorScheme::default();
        let color_l = color_scheme.get_color_l();
        let color_r = color_scheme.get_color_r();

        let (saber_l, saber_r) = create_saber(model_reg, color_l, color_r);

        // Setup pointer.

        let pointer_param = PointerParam::new(&POINTER_COLOR);
        let pointer = model_reg.create(pointer_param);
        
        let inner = Inner {
            audio_file_opt: None,
        };

        Self {
            asset_mgr,
            audio_engine,
            about_window,
            search_window_rx,
            search_window,
            poweredby_window,
            saber_l,
            saber_r,
            pointer,
            inner: RefCell::new(inner),
        }
    }
}

impl Scene for Menu {
    fn update(&self, scene_mgr: &SceneManager, scene_input: &SceneInput) {
        // Start audio on first update, and when ended.
        // TODO: implement lifecycle methods?

        let mut inner = self.inner.borrow_mut();
        let mut restart = false;

        if let Some(audio_file) = &inner.audio_file_opt {
            if matches!(audio_file.get_timestamp(), AudioFileTimestamp::Done) {
                restart = true;
            }
        } else {
            restart = true;
        }

        if restart {
            let audio_file_factory = AudioFileFactory::new(Arc::clone(&self.asset_mgr), "audio/menu.mp3");
            let audio_file = self.audio_engine.add(audio_file_factory);
            audio_file.play();
            inner.audio_file_opt = Some(audio_file);
        }

        // Update UI.

        scene_mgr.get_ui_subr().update(&self.saber_l, &self.saber_r, &self.pointer, [&self.about_window, &self.search_window, &self.poweredby_window].into_iter(), scene_input);

        // Poll receiver.

        match self.search_window_rx.try_recv() {
            Ok(test) => {
                let (song_info, beatmap_info_index) = if !test {
                    (SongInfo::load(Arc::clone(&self.asset_mgr), "demo").unwrap(), 1)
                } else {
                    #[allow(unused_assignments)]
                    #[allow(unused_mut)]
                    let mut song_info_opt = None;
                    #[cfg(feature = "test")]
                    {
                        song_info_opt = Some((SongInfo::test(Arc::clone(&self.asset_mgr)), 0));
                    }
                    song_info_opt.unwrap()
                };

                scene_mgr.load(GameParam::new(song_info, beatmap_info_index, #[cfg(feature = "test")] test));
            },
            Err(e) => {
                assert!(matches!(e, TryRecvError::Empty));
            },
        };
    }
}
