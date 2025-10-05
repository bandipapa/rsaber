use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use cgmath::{Deg, Quaternion, Rotation3, Vector3};

use crate::asset::AssetManagerRc;
use crate::audio::{AudioEngineRc, AudioFileFactory, AudioFileHandle, AudioFileTimestamp};
use crate::model::*;
use crate::scene::{GameParam, Scene, SceneFactory, SceneInput, SceneManager};
use crate::songinfo::{SongInfo, ColorScheme};
use crate::ui::{AboutWindow, PoweredByWindow, SearchWindow};

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

    fn load(self, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, audio_engine: AudioEngineRc) -> Self::Scene {
        Menu::new(self, asset_mgr, model_reg, audio_engine)
    }
}

pub struct Menu {
    asset_mgr: AssetManagerRc,
    audio_engine: AudioEngineRc,
    about_window: Rc<Window>,
    search_window_rx: Receiver<()>,
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
    fn new(_param: MenuParam, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, audio_engine: AudioEngineRc) -> Self {
        // Setup about by window.

        let window_param = WindowParam::new(500, 500, move || {
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

            window.on_start({
                let search_window_tx = search_window_tx.clone();
                move || search_window_tx.send(()).unwrap()
            });

            window
        });

        let search_window = model_reg.create(window_param);
        search_window.set_visible(true);
        search_window.set_scale(4.0, 2.0);
        search_window.set_pos(&Vector3::new(0.0, 4.0, 2.0));

        // Setup powered by window.

        let window_param = WindowParam::new(500, 500, move || {
            PoweredByWindow::new().unwrap()
        });

        let poweredby_window = model_reg.create(window_param);
        poweredby_window.set_visible(true);
        poweredby_window.set_scale(2.0, 2.0);
        poweredby_window.set_pos(&Vector3::new(5.0, 3.5, 2.0));
        poweredby_window.set_rot(&Quaternion::from_angle_z(Deg(-45.0)));

        // Setup floor.

        let floor_param = FloorParam::new(&COLOR_WHITE);
        let floor = model_reg.create(floor_param);
        floor.set_visible(true);
        floor.set_pos(&Vector3::new(0.0, 0.0, 0.0));

        // Setup sabers.

        let color_scheme = ColorScheme::default();
        let color_l = color_scheme.get_color_l();
        let color_r = color_scheme.get_color_r();

        let saber_param = SaberParam::new(color_l, &SABER_HANDLE_PHONG_PARAM, color_l, &SABER_RAY_PHONG_PARAM);
        let saber_l = model_reg.create(saber_param);

        let saber_param = SaberParam::new(color_r, &SABER_HANDLE_PHONG_PARAM, color_r, &SABER_RAY_PHONG_PARAM);
        let saber_r = model_reg.create(saber_param);

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
            Ok(_) => {
                let song_info = SongInfo::load(Arc::clone(&self.asset_mgr), "demo").unwrap();
                scene_mgr.load(GameParam::new(song_info, 1));
            },
            Err(e) => {
                assert!(matches!(e, TryRecvError::Empty));
            },
        };
    }
}
