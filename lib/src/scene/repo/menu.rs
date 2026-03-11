use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use cgmath::{Deg, Quaternion, Rotation3, Vector3};
use url::Url;

use crate::APP_VERSION;
use crate::asset::{AssetFileBox, AssetManagerRc};
use crate::audio::{AudioEngineRc, AudioFader, AudioFaderHandle, AudioFile, AudioFileHandle};
use crate::mailbox::{self, Receiver, TryRecvError};
use crate::model::*;
use crate::net::{AssetFileRequest, BeatSaverSearchRequest, ImageRequest, NetManager, SongZipRequest};
use crate::scene::{GameParam, Scene, SceneFactory, SceneInput, SceneManager, create_floor, create_saber, create_stats_window};
use crate::songdef::SongDifficulty;
use crate::songinfo::{SongInfo, ColorScheme};
use crate::ui::{AboutWindow, PoweredByWindow, SearchWindow, SearchWindowItem, SearchWindowMode, UILoop, VirtualKeyboardWindow};
use crate::ui::slintimpl::{self, ComponentHandle as slintimpl_ComponentHandle, Model as slintimpl_Model, WindowUtil as slintimpl_WindowUtil};
use crate::util::StatsRc;

const POINTER_COLOR: Color = Color([0.4, 0.4, 0.4]);
const FADE_RATE: u8 = 80; // [dB/s]
const STANDARD: &str = "Standard";

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
    type Error = ();

    fn load(self, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, stats: StatsRc, audio_engine: AudioEngineRc, ui_loop: &UILoop, net_manager: &NetManager) -> Result<Self::Scene, Self::Error> {
        Menu::new(self, asset_mgr, model_reg, stats, audio_engine, ui_loop, net_manager)
    }
}

pub struct Menu {
    asset_mgr: AssetManagerRc,
    audio_engine: AudioEngineRc,
    ui_loop: UILoop,
    vkbd_window: Rc<Window>,
    vkbd_window_rx: Receiver<VirtualKeyboardMessage>,
    about_window: Rc<Window>,
    search_window_rx: Receiver<SearchMessage>,
    search_window_state_mutex: Arc<Mutex<SearchState>>,
    search_window: Rc<Window>,
    poweredby_window: Rc<Window>,
    saber_l: Rc<Saber>,
    saber_r: Rc<Saber>,
    pointer: Rc<Pointer>,
    inner: RefCell<Inner>,
}

struct Inner {
    audio_info_opt: Option<AudioInfo>,
    preview_info_opt: Option<PreviewInfo>,
}

enum SearchMessage {
    PreviewStart(AssetFileBox, usize),
    PreviewStop,
    GameStart(AssetManagerRc, SongInfo, usize),
    #[cfg(feature = "test")]
    TestStart,
}

struct SearchState {
    active_info_opt: Option<ActiveInfo>,
    preview_serial: usize,
}

struct ActiveInfo {
    item_index: usize,
    preview_active: bool,
}

struct AudioInfo {
    file_handle: AudioFileHandle,
    fader_handle: AudioFaderHandle,
}

struct PreviewInfo {
    file_handle: AudioFileHandle,
    serial: usize,
}

enum VirtualKeyboardMessage {
    Open,
    Close,
}

type VirtualKeyboardFuncMutex = Arc<Mutex<Option<Arc<dyn Fn(String) + Send + Sync>>>>;

enum UpdateItemOp {
    Clear,
    Active(usize),
    PreviewStop,
}

impl Menu {
    fn new(_param: MenuParam, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, stats: StatsRc, audio_engine: AudioEngineRc, ui_loop: &UILoop, net_manager: &NetManager) -> Result<Self, ()> {
        // Implementation notes:
        // - Weak window references in event handlers (on_*):
        //   - If a weak reference to its parent window is unwrapped (window_weak.unwrap()),
        //     then it is safe.
        //   - Otherwise, if it is a weak reference to a different window, then testing
        //     is needed (window_weak.upgrade()).
        // - For net_manager_exec.submit():
        //   - The done_func closure is executed on the UI thread, if the returned handle
        //     is still alive.
        //   - Don't store strong references (owned, Rc, Arc) of handles in the done_func
        //     closure, as this would make cancelling of requests impossible.
        // TODO: Rework: would be nice if we can replace/simplify Arc/Mutex with Rc/RefCell in the
        // window + net_manager logic.

        // Setup virtual keyboard window.
        // TODO: move into UISubr, since it is common?

        let (vkbd_window_tx, vkbd_window_rx) = mailbox::mailbox();
        let vkbd_window_func_opt_mutex: VirtualKeyboardFuncMutex = Arc::new(Mutex::new(None));

        let window_param = WindowParam::new(700, 350, {
            let vkbd_window_tx = vkbd_window_tx.clone();
            let vkbd_window_func_opt_mutex = Arc::clone(&vkbd_window_func_opt_mutex);
            
            || {
                let window = VirtualKeyboardWindow::new().unwrap();

                window.on_pressed({
                    let window_weak = window.as_weak();
                    
                    move |value| {
                        // Send key events.

                        let window = window_weak.unwrap();
                        window.handle_key(value);
                    }
                });

                window.on_edited({
                    let vkbd_window_func_opt_mutex = Arc::clone(&vkbd_window_func_opt_mutex);

                    move |value| {
                        // Value has been changed.

                        let func_opt = vkbd_window_func_opt_mutex.lock().unwrap().as_ref().map(Arc::clone);

                        // Invoke callback (the lock has been already released).

                        if let Some(func) = func_opt {
                            func(value.into());
                        }
                    }
                });

                window.on_enter(move || {
                    // Remove callback: the remaining keystrokes (if any) will be discarded.

                    let mut vkbd_window_func_opt = vkbd_window_func_opt_mutex.lock().unwrap();
                    *vkbd_window_func_opt = None;

                    // Close keyboard.

                    vkbd_window_tx.send(VirtualKeyboardMessage::Close).unwrap();
                });

                window
            }
        });

        let vkbd_window = model_reg.create(window_param);
        vkbd_window.set_scale(2.5, 0.8);
        vkbd_window.set_pos(&Vector3::new(0.0, 3.0, 0.75));
        vkbd_window.set_rot(&Quaternion::from_angle_x(Deg(-45.0 / 2.0)));

        // Setup about by window.

        let window_param = WindowParam::new(500, 500, || {
            let window = AboutWindow::new().unwrap();
            window.set_version(APP_VERSION.clone().into());

            window
        });

        let about_window = model_reg.create(window_param);
        about_window.set_visible(true);
        about_window.set_scale(2.0, 2.0);
        about_window.set_pos(&Vector3::new(-5.0, 3.5, 2.0));
        about_window.set_rot(&Quaternion::from_angle_z(Deg(45.0)));

        // Setup search window.

        let (search_window_tx, search_window_rx) = mailbox::mailbox();
        let search_window_state_mutex = Arc::new(Mutex::new(SearchState {
            active_info_opt: None,
            preview_serial: 0,
        }));

        let window_param = WindowParam::new(1200, 750, {
            let net_manager_exec = net_manager.create_executor(ui_loop.clone());
            let vkbd_window_weak = vkbd_window.as_weak::<VirtualKeyboardWindow>();
            let search_window_state_mutex = Arc::clone(&search_window_state_mutex);

            move || {
                let window = SearchWindow::new().unwrap();
                window.set_items(slintimpl::ModelRc::new(slintimpl::VecModel::default()));

                let handles_mutex = Arc::new(Mutex::new(Vec::new()));

                // Construct search method:
                // - Before calling search(), caller should ensure that the SearchWindow is still alive.
                // - The search method (owned by SearchWindow and VirtualKeyboardWindow (via vkbd_window_func_opt_mutex))
                //   has weak reference to handles_mutex. The handles_mutex is having the same lifecycle as the SearchWindow.
                //   If the SearchWindow is dropped, then all in-progress fetches will be terminated as well.

                let search = Arc::new({
                    let net_manager_exec = net_manager_exec.clone();
                    let search_window_state_mutex = Arc::clone(&search_window_state_mutex);
                    let window_weak = window.as_weak();
                    let handles_mutex_weak = Arc::downgrade(&handles_mutex);

                    move || {
                        let window = window_weak.unwrap();

                        let query = window.get_query();
                        let order = window.get_order();
                        let ascending = window.get_ascending();

                        window.set_mode(SearchWindowMode::Message);
                        window.set_show_detail(false);
                        window.set_message("Searching...".into());

                        // Clear active item. Since UI- and main-thread are running parallel, preview_serial is used
                        // to match the actual audio file.

                        {
                            let mut search_window_state = search_window_state_mutex.lock().unwrap();

                            search_window_state.preview_serial += 1;
                            Self::update_item(&window, &mut search_window_state, UpdateItemOp::Clear);
                        }

                        // Terminate in-progress fetches.

                        let handles_mutex = handles_mutex_weak.upgrade().unwrap();
                        let mut handles = handles_mutex.lock().unwrap();
                        handles.clear();

                        // Submit search.

                        let handle = net_manager_exec.submit(BeatSaverSearchRequest::new(query, order, ascending), {
                            let net_manager_exec = net_manager_exec.clone();
                            let window_weak = window_weak.clone();
                            let handles_mutex_weak = handles_mutex_weak.clone();

                            move |r| {
                                let window = window_weak.unwrap();
                                let mut items = Vec::new();

                                match r {
                                    Ok(r) => {
                                        let empty_img = slintimpl::Image::from_rgba8(slintimpl::SharedPixelBuffer::new(1, 1)); // slint does not allow (0, 0) for image dimension.
                                        let songs = r.get_songs();

                                        if !songs.is_empty() {
                                            window.set_mode(SearchWindowMode::Item);
                                            
                                            let handles_mutex = handles_mutex_weak.upgrade().unwrap();
                                            let mut handles = handles_mutex.lock().unwrap();

                                            for (item_index, (song, version)) in songs.iter().filter_map(|song| song.get_published_version().map(|version| (song, version))).enumerate() {
                                                // Map difficulties.

                                                let mut difficulties: Box<_> = version.get_variants().iter().filter_map(|variant| {
                                                    if variant.get_characteristic() == STANDARD {
                                                        Some(variant.get_difficulty())
                                                    } else {
                                                        None
                                                    }
                                                }).collect();
                                                difficulties.sort();

                                                let difficulty_ints: Vec<_> = difficulties.iter().map(|difficulty| (*difficulty).into()).collect();
                                                let difficulty_ints_model = slintimpl::VecModel::default();
                                                difficulty_ints_model.set_vec(difficulty_ints);

                                                let difficulty_strs: Vec<_> = difficulties.iter().map(|difficulty| {
                                                    match difficulty {
                                                        SongDifficulty::Easy => "Easy",
                                                        SongDifficulty::Normal => "Normal",
                                                        SongDifficulty::Hard => "Hard",
                                                        SongDifficulty::Expert => "Expert",
                                                        SongDifficulty::ExpertPlus => "Expert Plus",
                                                    }.into()
                                                }).collect();
                                                let difficulty_strs_model = slintimpl::VecModel::default();
                                                difficulty_strs_model.set_vec(difficulty_strs);

                                                // Create item.

                                                let metadata = song.get_metadata();
                                                let duration = metadata.get_duration();

                                                let item = SearchWindowItem {
                                                    name: song.get_name().into(),
                                                    uploader_name: song.get_uploader().get_name().into(),
                                                    cover_img: empty_img.clone(),
                                                    duration: format!("{}:{:02}", duration / 60, duration % 60).into(),
                                                    bpm: format!("{:.0}", metadata.get_bpm()).into(),
                                                    score: format!("{:.2}", song.get_stats().get_score() * 100.0).into(),
                                                    preview_url: version.get_preview_url().as_ref().into(),
                                                    download_url: version.get_download_url().as_ref().into(),
                                                    difficulty_ints: slintimpl::ModelRc::new(difficulty_ints_model),
                                                    difficulty_strs: slintimpl::ModelRc::new(difficulty_strs_model),
                                                    active: false,
                                                    preview_active: false,
                                                };

                                                items.push(item);

                                                // Submit cover image fetch.
                                                
                                                let handle = net_manager_exec.submit(ImageRequest::new(version.get_cover_url().clone()), { // TODO: cache?
                                                    let window_weak = window_weak.clone();

                                                    move |r| {
                                                        if let Ok(img_raw) = r {
                                                            let width = img_raw.get_width();
                                                            let height = img_raw.get_height();

                                                            if width > 0 && height > 0 {
                                                                let mut buf = slintimpl::SharedPixelBuffer::<slintimpl::Rgba8Pixel>::new(width, height);
                                                                let buf_raw = buf.make_mut_bytes();
                                                                buf_raw.copy_from_slice(img_raw.get_data());
                                                                let img = slintimpl::Image::from_rgba8(buf);

                                                                // Update model.

                                                                let window = window_weak.unwrap();
                                                                let model = window.get_items();

                                                                let mut item = model.row_data(item_index).expect("Item expected");
                                                                item.cover_img = img;
                                                                model.set_row_data(item_index, item);
                                                            }
                                                        }
                                                    }
                                                });

                                                handles.push(handle);
                                            }
                                        } else {
                                            window.set_mode(SearchWindowMode::Message);
                                            window.set_message("No results".into());
                                        }
                                    },
                                    Err(e) => {
                                        window.set_mode(SearchWindowMode::Message);
                                        window.set_message(format!("Network error: {}", e).into());
                                    },
                                }

                                // Update model.

                                let model = window.get_items();
                                let model = model.as_any().downcast_ref::<slintimpl::VecModel<SearchWindowItem>>().expect("Model expected");
                                model.set_vec(items);
                            }
                        });

                        handles.push(handle);
                    }
                });

                // TODO: move into UISubr, since it is common?
                let set_input_enabled = Arc::new({
                    // For events which result in scene switch:
                    // - Disable further input on the UI.
                    // - Prevent UI from sending any message to the main thread
                    //   (e.g. via mailbox), since the receiving side (current scene) is
                    //   going to be dropped.

                    let vkbd_window_weak = vkbd_window_weak.clone();
                    let window_weak = window.as_weak();

                    move |enabled| {
                        let vkbd_window_opt = vkbd_window_weak.upgrade();
                        if vkbd_window_opt.is_none() {
                            return;
                        }
                        let vkbd_window = vkbd_window_opt.unwrap();

                        vkbd_window.set_input_enabled(enabled);

                        let window = window_weak.unwrap();
                        window.set_input_enabled(enabled);
                    }
                });

                window.on_change_query({
                    let window_weak = window.as_weak();
                    let search = Arc::clone(&search);
                    
                    move || {
                        let _ = Arc::strong_count(&handles_mutex); // As long as the SearchWindow is alive, handles_mutex is alive as well.

                        // Open keyboard.

                        let vkbd_window_opt = vkbd_window_weak.upgrade();
                        if vkbd_window_opt.is_none() {
                            return;
                        }
                        let vkbd_window = vkbd_window_opt.unwrap();

                        let window = window_weak.unwrap();
                        let query = window.get_query();

                        vkbd_window.set_shift(false);
                        vkbd_window.set_value(query);
                        vkbd_window.handle_key_end(); // Move cursor to the end of the string.

                        let mut vkbd_window_func_opt = vkbd_window_func_opt_mutex.lock().unwrap();
                        *vkbd_window_func_opt = Some(Arc::new({
                            let window_weak = window_weak.clone();
                            let search = Arc::clone(&search);

                            move |query| {
                                let window_opt = window_weak.upgrade();
                                if window_opt.is_none() {
                                    return;
                                }
                                let window = window_opt.unwrap();

                                window.set_query(query.into());
                                search();
                            }
                        }));

                        vkbd_window_tx.send(VirtualKeyboardMessage::Open).unwrap();
                    }
                });

                window.on_change_other({
                    let search = Arc::clone(&search);

                    move || {
                        search();
                    }
                });

                window.on_refresh({
                    let search = Arc::clone(&search);
                    
                    move || {
                        search();
                    }
                });

                window.on_select({
                    let search_window_tx = search_window_tx.clone();
                    let net_manager_exec = net_manager_exec.clone();
                    let search_window_state_mutex = Arc::clone(&search_window_state_mutex);
                    let window_weak = window.as_weak();
                    let mut handle_opt = None;

                    move |item_index_selected| {
                        let window = window_weak.unwrap();
                        let item_index_selected: usize = item_index_selected.try_into().unwrap();
                        let mut search_window_state = search_window_state_mutex.lock().unwrap();
                        let handle_opt_ref = &mut handle_opt; // Suppress "value captured by ... is never read" warnings.

                        // Stop audio.

                        search_window_state.preview_serial += 1;
                        let preview_serial = search_window_state.preview_serial;

                        search_window_tx.send(SearchMessage::PreviewStop).unwrap();

                        // Terminate fetch.

                        *handle_opt_ref = None;

                        // Set active item.
                        // TODO: use window->detail_item instead of active_info.item_index?

                        let model = window.get_items();
                        let mut difficulty_int_active_opt = None;

                        if let Some(active_info) = &search_window_state.active_info_opt {
                            let difficulty_index = window.get_difficulty_index();
                            let item = model.row_data(active_info.item_index).expect("Item expected");
                            let difficulty_ints: Box<_> = item.difficulty_ints.iter().collect();

                            // If the active item doesn't have any difficulty (difficulty_ints), then
                            // difficulty_index is still set. Check if we have at least one difficulty.

                            if !difficulty_ints.is_empty() {
                                difficulty_int_active_opt = Some(difficulty_ints[difficulty_index as usize]);
                            }

                            if item_index_selected == active_info.item_index {
                                if active_info.preview_active {
                                    Self::update_item(&window, &mut search_window_state, UpdateItemOp::PreviewStop);
                                    return;
                                }
                            } else {
                                Self::update_item(&window, &mut search_window_state, UpdateItemOp::Clear);
                            }
                        }

                        Self::update_item(&window, &mut search_window_state, UpdateItemOp::Active(item_index_selected));

                        // Set detail.

                        let item = model.row_data(item_index_selected).expect("Item expected");
                        let preview_url: String = item.preview_url.clone().into();
                        let difficulty_ints: Box<_> = item.difficulty_ints.iter().collect();

                        window.set_show_detail(true);
                        window.set_detail_item(item);
                        window.set_detail_message("".into());

                        let difficulty_index = difficulty_int_active_opt.map_or(0, |difficulty_int_active| {
                            difficulty_ints.iter().position(|difficulty_int| *difficulty_int == difficulty_int_active).unwrap_or(0)
                        });

                        window.set_difficulty_index(difficulty_index.try_into().unwrap());

                        // Submit audio preview fetch.

                        let url = Url::parse(&preview_url).expect("Invalid url");
                        
                        let handle = net_manager_exec.submit(AssetFileRequest::new(url), { // TODO: cache?
                            let search_window_tx = search_window_tx.clone();
                            let search_window_state_mutex = Arc::clone(&search_window_state_mutex);
                            let window_weak = window_weak.clone();

                            move |r| {
                                if let Ok(asset_file) = r {
                                    search_window_tx.send(SearchMessage::PreviewStart(asset_file, preview_serial)).unwrap();
                                } else {
                                    let mut search_window_state = search_window_state_mutex.lock().unwrap();

                                    // Remove play icon.

                                    let window = window_weak.unwrap();
                                    Self::update_item(&window, &mut search_window_state, UpdateItemOp::PreviewStop);
                                }
                            }
                        });
                        
                        *handle_opt_ref = Some(handle);
                    }
                });

                window.on_play({
                    let search_window_tx = search_window_tx.clone();
                    let net_manager_exec = net_manager_exec.clone();
                    let search_window_state_mutex = Arc::clone(&search_window_state_mutex);
                    let window_weak = window.as_weak();
                    let set_input_enabled = Arc::clone(&set_input_enabled);
                    let mut handle_opt = None;

                    move || { // TODO: use window->detail_item instead of active_info.item_index?
                        let window = window_weak.unwrap();
                        let item_index = {
                            let search_window_state = search_window_state_mutex.lock().unwrap();
                            let active_info = search_window_state.active_info_opt.as_ref().expect("Active expected");
                            active_info.item_index
                        };

                        let model = window.get_items();
                        let item = model.row_data(item_index).expect("Item expected");

                        let difficulty_index = window.get_difficulty_index();
                        let difficulty_ints: Box<_> = item.difficulty_ints.iter().collect();
                        let difficulty_int = difficulty_ints[difficulty_index as usize];
                        let difficulty: SongDifficulty = difficulty_int.try_into().unwrap();

                        window.set_mode(SearchWindowMode::Message);
                        window.set_message("Downloading...".into());

                        set_input_enabled(false);

                        // Submit song zip fetch.

                        let download_url: String = item.download_url.clone().into();
                        let url = Url::parse(&download_url).expect("Invalid url");

                        let handle = net_manager_exec.submit(SongZipRequest::new(url), { // TODO: cache?
                            let search_window_tx = search_window_tx.clone();
                            let window_weak = window_weak.clone();
                            let set_input_enabled = Arc::clone(&set_input_enabled);

                            move |r| {
                                let mut e_opt = None;

                                match r {
                                    Ok(asset_mgr) => {
                                        // TODO: On which thread should we do the processing of the song data?
                                        match SongInfo::load(Arc::clone(&asset_mgr)) {
                                            Ok(song_info) => {
                                                let beatmap_infos = song_info.get_beatmap_infos();
                                                
                                                if let Some(beatmap_info_index) = beatmap_infos.iter().position(|beatmap_info| beatmap_info.get_characteristic() == STANDARD && beatmap_info.get_difficulty() == difficulty) {
                                                    search_window_tx.send(SearchMessage::GameStart(asset_mgr, song_info, beatmap_info_index)).unwrap();
                                                } else {
                                                    e_opt = Some("No such characteristic/difficulty".to_string());
                                                }
                                            },
                                            Err(e) => {
                                                e_opt = Some(format!("Unable to load song: {:?}", e)); // TODO: instead of debug, use display trait for formatting error msg?
                                            },
                                        }
                                    },
                                    Err(e) => {
                                        e_opt = Some(format!("Network error: {:?}", e)); // TODO: instead of debug, use display trait for formatting error msg?
                                    },
                                }

                                if let Some(e) = e_opt {
                                    let window = window_weak.unwrap();
                                    window.set_mode(SearchWindowMode::Item);
                                    window.set_detail_message(e.into());

                                    set_input_enabled(true);
                                }
                            }
                        });

                        let handle_opt_ref = &mut handle_opt; // Suppress "value captured by ... is never read" warnings.
                        *handle_opt_ref = Some(handle);
                    }
                });

                // Setup test, if configured.
                // TODO: Add support for https://github.com/BeatLeader/BS-Open-Replay ?

                #[cfg(feature = "test")]
                {
                    window.set_test_visible(true);

                    window.on_test(move || {
                        set_input_enabled(false);

                        search_window_tx.send(SearchMessage::TestStart).unwrap();
                    });
                }

                // Execute initial query.

                search();

                window
            }
        });

        let search_window = model_reg.create(window_param);
        search_window.set_visible(true);
        search_window.set_scale(4.8, 3.0);
        search_window.set_pos(&Vector3::new(0.0, 5.0, 2.0));

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
            audio_info_opt: None,
            preview_info_opt: None,
        };

        Ok(Self {
            asset_mgr,
            audio_engine,
            ui_loop: ui_loop.clone(),
            vkbd_window,
            vkbd_window_rx,
            about_window,
            search_window_rx,
            search_window_state_mutex,
            search_window,
            poweredby_window,
            saber_l,
            saber_r,
            pointer,
            inner: RefCell::new(inner),
        })
    }

    fn update_item(window: &SearchWindow, state: &mut SearchState, op: UpdateItemOp) {
        let model = window.get_items();

        match op {
            UpdateItemOp::Clear => {
                if let Some(active_info) = &state.active_info_opt {
                    let item_index = active_info.item_index;
                    let mut item = model.row_data(item_index).expect("Item expected");

                    item.active = false;
                    item.preview_active = false;

                    model.set_row_data(item_index, item);

                    state.active_info_opt = None;
                }
            },
            UpdateItemOp::Active(item_index) => {
                let mut item = model.row_data(item_index).expect("Item expected");

                item.active = true;
                item.preview_active = true;

                model.set_row_data(item_index, item);

                state.active_info_opt = Some(ActiveInfo {
                    item_index,
                    preview_active: true,
                });
            },
            UpdateItemOp::PreviewStop => {
                let active_info = state.active_info_opt.as_mut().expect("Active expected");
                let item_index = active_info.item_index;
                let mut item = model.row_data(item_index).expect("Item expected");

                item.preview_active = false;

                model.set_row_data(item_index, item);

                active_info.preview_active = false;
            },
        }
    }
}

impl Scene for Menu {
    fn update(&self, scene_mgr: &SceneManager, scene_input: &SceneInput) {
        let inner = &mut *self.inner.borrow_mut();

        // Start audio on first update, and when ended.
        // TODO: implement lifecycle methods?

        let mut restart = false;

        if let Some(audio_info) = &inner.audio_info_opt {
            if audio_info.file_handle.at_eof() {
                restart = true;
            }
        } else {
            restart = true;
        }

        if restart {
            let asset_file = self.asset_mgr.open_or_err("/audio/menu.mp3");

            let (file_input, file_handle) = AudioFile::new(asset_file);
            let (fader_input, fader_handle) = AudioFader::new(file_input);
            self.audio_engine.add(fader_input);

            // If preview is active, then silence menu song.

            if inner.preview_info_opt.is_some() {
                fader_handle.silence();
            }

            file_handle.play();

            let audio_info = AudioInfo {
                file_handle,
                fader_handle,
            };

            inner.audio_info_opt = Some(audio_info);
        }

        let audio_info = inner.audio_info_opt.as_ref().unwrap();
        let fader_handle = &audio_info.fader_handle;

        // Handle UI events.

        let windows = &[&self.vkbd_window, &self.about_window, &self.search_window, &self.poweredby_window];
        scene_mgr.get_ui_subr().update(&self.saber_l, &self.saber_r, &self.pointer, windows, scene_input);

        // Handle virtual keyboard.

        match self.vkbd_window_rx.try_recv() {
            Ok(msg) => {
                match msg {
                    VirtualKeyboardMessage::Open => {
                        // Show keyboard.

                        self.vkbd_window.set_visible(true);
                    },
                    VirtualKeyboardMessage::Close => {
                        // Hide keyboard.

                        self.vkbd_window.set_visible(false);
                    },
                }
            },
            Err(e) => {
                assert!(matches!(e, TryRecvError::Empty));
            },
        }

        // Poll for messages from search window.

        match self.search_window_rx.try_recv() {
            Ok(msg) => {
                match msg {
                    SearchMessage::PreviewStart(asset_file, serial) => {
                        fader_handle.fade_out(FADE_RATE);

                        // TODO: At the moment we can't start preview directly on the UI thread,
                        // as the AudioEngineRc is Rc and not Arc. 
                        // TODO: Use Content-Type from response to avoid format guess?

                        let (input, file_handle) = AudioFile::new(asset_file);
                        self.audio_engine.add(input);

                        file_handle.play();

                        let preview_info = PreviewInfo {
                            file_handle,
                            serial,
                        };

                        inner.preview_info_opt = Some(preview_info);
                    },
                    SearchMessage::PreviewStop => {
                        fader_handle.fade_in(FADE_RATE);

                        inner.preview_info_opt = None;
                    },
                    SearchMessage::GameStart(asset_mgr, song_info, beatmap_info_index) => {
                        if let Err(e) = scene_mgr.load(GameParam::new(asset_mgr, song_info, beatmap_info_index, #[cfg(feature = "test")] false)) {
                            self.ui_loop.add_callback({
                                let vkbd_window_weak = self.vkbd_window.as_weak::<VirtualKeyboardWindow>();
                                let search_window_weak = self.search_window.as_weak::<SearchWindow>();

                                move || {
                                    let vkbd_window_opt = vkbd_window_weak.upgrade();
                                    let search_window_opt = search_window_weak.upgrade();
                                    if vkbd_window_opt.is_none() || search_window_opt.is_none() {
                                        return;
                                    }
                                    let vkbd_window = vkbd_window_opt.unwrap();
                                    let search_window = search_window_opt.unwrap();

                                    search_window.set_mode(SearchWindowMode::Item);
                                    search_window.set_detail_message(e.into());

                                    // TODO: Refactor to use a single set_input_enabled implementation.
                                    vkbd_window.set_input_enabled(true);
                                    search_window.set_input_enabled(true);
                                }
                            });
                        }
                    },
                    #[cfg(feature = "test")]
                    SearchMessage::TestStart => {
                        let song_info = SongInfo::test(Arc::clone(&self.asset_mgr));
                        scene_mgr.load(GameParam::new(Arc::clone(&self.asset_mgr), song_info, 0, true)).expect("Unable to load scene");
                    },
                }
            },
            Err(e) => {
                assert!(matches!(e, TryRecvError::Empty));
            },
        }

        // Handle the end of audio preview.

        if let Some(preview_info) = &inner.preview_info_opt && preview_info.file_handle.at_eof() {
            fader_handle.fade_in(FADE_RATE);

            self.ui_loop.add_callback({
                let search_window_state_mutex = Arc::clone(&self.search_window_state_mutex);            
                let window_weak = self.search_window.as_weak::<SearchWindow>();
                let preview_serial = preview_info.serial;

                move || {
                    let window_opt = window_weak.upgrade();
                    if window_opt.is_none() {
                        return;
                    }
                    let window = window_opt.unwrap();

                    let mut search_window_state = search_window_state_mutex.lock().unwrap();
                    if search_window_state.preview_serial == preview_serial {
                        Self::update_item(&window, &mut search_window_state, UpdateItemOp::PreviewStop);
                    }
                }
            });

            inner.preview_info_opt = None;
        }
    }
}
