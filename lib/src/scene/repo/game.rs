use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use cgmath::{Angle, Deg, Quaternion, Rotation3, Vector3};

use crate::asset::AssetManagerRc;
use crate::audio::{AudioEngineRc, AudioFileFactory, AudioFileHandle, AudioFileTimestamp};
use crate::model::*;
use crate::scene::{MenuParam, Scene, SceneFactory, SceneInput, SceneManager, ScenePose};
use crate::songinfo::{BPMInfo, NoteCutDir, NoteType, SongInfo};

const CUBE_SIZE: f32 = 0.5; // [m]
const CUBE_SPACING: f32 = 0.10; // [m]
const CUBE_FLOOR: f32 = 0.6; // [m]

const ZONE_IN1_DIST: f32 = 100.0; // [m]
const ZONE_IN1_V: f32 = 200.0; // [m/s]
const ZONE_IN2_DIST: f32 = 10.0; // [m]
const ZONE_IN2_V: f32 = 15.0; // [m/s]
const ZONE_IN3_DIST: f32 = 10.0; // [m]
const ZONE_IN3_V: f32 = 15.0; // [m/s]
const ZONE_OUT_DIST: f32 = 15.0; // [m]
const ZONE_OUT_V: f32 = 15.0; // [m/s]

pub struct GameParam {
    song_info: SongInfo,
    beatmap_info_index: usize, // TODO: usize or smaller?
}

impl GameParam {
    pub fn new(song_info: SongInfo, beatmap_info_index: usize) -> Self {
        Self {
            song_info,
            beatmap_info_index,
        }
    }
}

impl SceneFactory for GameParam {
    type Scene = Game;

    fn load(self, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, audio_engine: AudioEngineRc) -> Self::Scene {
        Game::new(self, asset_mgr, model_reg, audio_engine)
    }
}

pub struct Game {
    cube_infos: Box<[CubeInfo]>,
    saber_l: Rc<Saber>,
    saber_r: Rc<Saber>,
    audio_file: AudioFileHandle,
    inner: RefCell<Inner>,
}

struct CubeInfo {
    ts: f32,
    x: f32,
    z: f32,
    angle: f32,
    cube: Rc<Cube>,
}

struct Inner {
    start: bool,
    cube_range: Range<usize>,
    prev_click: bool,
}

impl Game {
    fn new(param: GameParam, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, audio_engine: AudioEngineRc) -> Self {
        let song_info = param.song_info;

        // Determine color scheme.

        let beatmap_info = &song_info.get_beatmap_infos()[param.beatmap_info_index];

        let color_scheme = if let Some(color_scheme_index) = beatmap_info.get_color_scheme_index_opt() && let Some(color_scheme) = song_info.get_color_scheme(color_scheme_index) {
            color_scheme
        } else {
            beatmap_info.get_def_color_scheme()
        };

        let color_l = *color_scheme.get_color_l();
        let color_r = *color_scheme.get_color_r();

        // Setup cubes.

        let bpm_info = song_info.get_bpm_info().expect("Unable to load bpm info");

        let body_phong_param = PhongParam::new(0.1, 0.3, 0.6, 16.0);
        let symbol_phong_param = PhongParam::new(0.5, 0.3, 0.6, 16.0);

        let beatmap = beatmap_info.load().expect("Unable to load beatmap");
        let cube_infos = Box::from_iter(beatmap.get_notes().iter().filter_map(|note| {
            let bpm_pos = note.get_bpm_pos();

            let ts_opt = match &bpm_info {
                BPMInfo::Fixed(bpm) => {
                    Some(60.0 / bpm * bpm_pos)
                },
                BPMInfo::Mapped(bpm_map) => {
                    bpm_map.get_ts(bpm_pos)
                },
            };

            if let Some(ts) = ts_opt {
                let mut symbol = CubeSymbol::Arrow;

                let angle = match note.get_cut_dir() {
                    NoteCutDir::Up => 180.0,
                    NoteCutDir::Down => 0.0,
                    NoteCutDir::Left => 90.0,
                    NoteCutDir::Right => -90.0,
                    NoteCutDir::UpLeft => 135.0,
                    NoteCutDir::UpRight => -135.0,
                    NoteCutDir::DownLeft => 45.0,
                    NoteCutDir::DownRight => -45.0,
                    NoteCutDir::Any => {
                        symbol = CubeSymbol::Dot;
                        0.0
                    }
                };

                let color = match note.get_note_type() {
                    NoteType::Left => &color_l,
                    NoteType::Right => &color_r,
                };

                let cube_param = CubeParam::new(symbol, color, &body_phong_param, &COLOR_WHITE, &symbol_phong_param);
                let cube = model_reg.create(cube_param);
                cube.set_scale(CUBE_SIZE);

                // Notes regarding the cube:
                // - Its bounding box is unit (1m) sized and the object center is at the origin.
                // - It is scaled to CUBE_SIZE.

                let x_val = note.get_x() as f32;
                let (x_index, right) = if x_val >= 2.0 { (x_val - 2.0, 1.0) } else { (1.0 - x_val, -1.0) };
                let x = right * (CUBE_SPACING / 2.0 + x_index * (CUBE_SIZE + CUBE_SPACING) + CUBE_SIZE / 2.0);

                let y_val = note.get_y() as f32;
                let z = y_val * (CUBE_SIZE + CUBE_SPACING) + CUBE_SIZE / 2.0;

                let cube_info = CubeInfo {
                    ts,
                    x,
                    z,
                    angle,
                    cube,
                };

                Some(cube_info)
            } else {
                None
            }
        }));

        // Setup floor.

        let floor_param = FloorParam::new(&COLOR_WHITE);
        let floor = model_reg.create(floor_param);
        floor.set_visible(true);
        floor.set_pos(&Vector3::new(0.0, 0.0, 0.0));

        // Setup sabers.

        let saber_param = SaberParam::new(&color_l, &SABER_HANDLE_PHONG_PARAM, &color_l, &SABER_RAY_PHONG_PARAM);
        let saber_l = model_reg.create(saber_param);

        let saber_param = SaberParam::new(&color_r, &SABER_HANDLE_PHONG_PARAM, &color_r, &SABER_RAY_PHONG_PARAM);
        let saber_r = model_reg.create(saber_param);

        // Setup audio.

        let audio_file_factory = AudioFileFactory::new(asset_mgr, song_info.get_song_filename());
        let audio_file = audio_engine.add(audio_file_factory);

        let inner = Inner {
            start: true,
            cube_range: Range {
                start: 0,
                end: 0,
            },
            prev_click: true,
        };
        
        Self {
            cube_infos,
            saber_l,
            saber_r,
            audio_file,
            inner: RefCell::new(inner),
        }
    }

    fn update_cubes(&self, audio_ts: f32) {
        let mut inner = self.inner.borrow_mut();
        let cube_range = &mut inner.cube_range;

        let zone_in1_t = ZONE_IN1_DIST / ZONE_IN1_V;
        let zone_in2_t = ZONE_IN2_DIST / ZONE_IN2_V;
        let zone_in3_t = ZONE_IN3_DIST / ZONE_IN3_V;

        // Show incoming cubes.

        let ts_in = audio_ts + zone_in1_t + zone_in2_t + zone_in3_t;

        for i in cube_range.end..self.cube_infos.len() {
            let cube_info = &self.cube_infos[i];

            if cube_info.ts <= ts_in {
                cube_info.cube.set_visible(true);
                cube_range.end = i + 1;
            } else {
                break;
            }
        }

        // Hide outgoing cubes.

        let zone_out_t = ZONE_OUT_DIST / ZONE_OUT_V;
        let ts_out = audio_ts - zone_out_t;

        for i in cube_range.clone() {
            let cube_info = &self.cube_infos[i];

            if cube_info.ts < ts_out {
                cube_info.cube.set_visible(false);
                cube_range.start = i + 1;
            } else {
                break;
            }
        }

        // Update positions.
        // TODO: shorter rotation time? z_base is fine.
        // TODO: always display cubes at z=0 and then move them up?

        let zone_in23_t = zone_in2_t + zone_in3_t;
        let zone_in123_t = zone_in1_t + zone_in2_t + zone_in3_t;

        for i in cube_range.clone() {
            let cube_info = &self.cube_infos[i];
            let ts = cube_info.ts - audio_ts;

            let (y, z_base, angle) = if ts <= 0.0 {
                (ts * ZONE_OUT_V, CUBE_FLOOR, cube_info.angle)
            } else if ts <= zone_in3_t {
                (ts * ZONE_IN3_V, CUBE_FLOOR, cube_info.angle)
            } else if ts <= zone_in23_t {
                let factor = (zone_in23_t - ts) / (zone_in23_t - zone_in3_t);
                (ZONE_IN2_DIST * (1.0 - factor) + ZONE_IN3_DIST, CUBE_FLOOR * Deg(90.0 * factor).sin(), cube_info.angle * factor)
            } else {
                let factor = (zone_in123_t - ts) / (zone_in123_t - zone_in23_t);
                (ZONE_IN1_DIST * (1.0 - factor) + ZONE_IN2_DIST + ZONE_IN3_DIST, 0.0, 0.0)
            };

            cube_info.cube.set_pos(&Vector3::new(cube_info.x, y + CUBE_SIZE / 2.0 + 1.0, cube_info.z + z_base)); // TODO: ts_in/ts_out should be offseted because of CUBE_SIZE / 2.0 + 1.0.

            let rot = Quaternion::from_angle_y(Deg(angle));
            cube_info.cube.set_rot(&rot);
        }
    }

    fn update_saber(&self, saber: &Saber, pose_opt: &Option<ScenePose>) {
        if let Some(pose) = pose_opt && pose.get_render() {
            saber.set_visible(SaberVisibility::HandleRay);
            saber.set_pos(pose.get_pos());
            saber.set_rot(pose.get_rot());
        } else {
            saber.set_visible(SaberVisibility::Hidden);
        }
    }
}

impl Scene for Game {
    fn update(&self, scene_mgr: &SceneManager, scene_input: &SceneInput) {
        // Start audio on first update.
        // TODO: implement lifecycle methods?

        {
            let mut inner = self.inner.borrow_mut();

            if inner.start {
                self.audio_file.play();
                inner.start = false;
            }
        }

        // Update cubes.

        let mut done = false;

        match self.audio_file.get_timestamp() {
            AudioFileTimestamp::Inactive => unreachable!(),
            AudioFileTimestamp::Unavail => (),
            AudioFileTimestamp::Playing(ts) => self.update_cubes(ts as f32), // TODO: or use 64 bit ts?
            AudioFileTimestamp::Done => done = true,
        };

        // Update sabers.

        self.update_saber(&self.saber_l, &scene_input.pose_l_opt);
        self.update_saber(&self.saber_r, &scene_input.pose_r_opt);

        // TODO: Implement pause menu.

        {
            let mut inner = self.inner.borrow_mut();
            let mut click = false;

            if let Some(pose_l) = &scene_input.pose_l_opt {
                click |= pose_l.get_click();
            }

            if let Some(pose_r) = &scene_input.pose_r_opt {
                click |= pose_r.get_click();
            }

            if !inner.prev_click && click {
                done = true;
            }

            inner.prev_click = click;
        }

        // If we are finished, then go to menu.

        if done {
            scene_mgr.load(MenuParam::new());
        }
    }
}
