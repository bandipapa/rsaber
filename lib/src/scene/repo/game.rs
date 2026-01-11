use std::cell::RefCell;
use std::rc::Rc;

#[cfg(feature = "test")]
use std::time::Instant;

use cgmath::{Angle, Deg, InnerSpace, Matrix4, Quaternion, Rotation3, Vector3};

use crate::asset::AssetManagerRc;
use crate::audio::{AudioEngineRc, AudioFileFactory, AudioFileHandle, AudioFileTimestamp};
use crate::model::*;
use crate::scene::{MenuParam, Scene, SceneFactory, SceneInput, SceneManager, ScenePose, create_floor, create_saber, create_stats_window};
use crate::songinfo::{BPMInfo, NoteCutDir, NoteType, SongInfo};
use crate::ui::{GameStatsWindow, UILoop, UIWindowWeak};
use crate::util::StatsRc;

const CUBE_SIZE: f32 = 0.5; // [m]
const CUBE_SPACING: f32 = 0.10; // [m]
const CUBE_FLOOR: f32 = 0.6; // [m]

const OFFSET_Y: f32 = CUBE_SIZE / 2.0 + 1.0; // When ts == cube_info.ts, then distance between the player and center of the cube [m]

// TODO: These are depending on songinfo + environmental geometry:
const ZONE_IN1_DIST: f32 = 100.0; // [m]
const ZONE_IN1_V: f32 = 200.0; // [m/s]
const ZONE_IN2_DIST: f32 = 10.0; // [m]
const ZONE_IN3_DIST: f32 = 10.0; // [m]
const ZONE_OUT_DIST: f32 = 15.0; // [m]

const G: f32 = 9.8; // [m/s2]

pub struct GameParam {
    song_info: SongInfo,
    beatmap_info_index: usize, // TODO: usize or smaller?
    #[cfg(feature = "test")]
    test: bool,
}

impl GameParam {
    pub fn new(song_info: SongInfo, beatmap_info_index: usize, #[cfg(feature = "test")] test: bool) -> Self {
        Self {
            song_info,
            beatmap_info_index,
            #[cfg(feature = "test")]
            test,
        }
    }
}

impl SceneFactory for GameParam {
    type Scene = Game;

    fn load(self, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, stats: StatsRc, audio_engine: AudioEngineRc, ui_loop: &UILoop) -> Self::Scene {
        Game::new(self, asset_mgr, model_reg, stats, audio_engine, ui_loop)
    }
}

pub struct Game {
    ui_loop: UILoop,
    zone_info: Rc<ZoneInfo>,
    cube_infos: Box<[Rc<CubeInfo>]>,
    game_stats_window_weak: UIWindowWeak<GameStatsWindow>,
    saber_l: Rc<Saber>,
    saber_r: Rc<Saber>,
    audio_file_opt: Option<AudioFileHandle>,
    inner: RefCell<Inner>,
}

struct ZoneInfo {
    in1_dist: f32,
    in2_dist: f32,
    in3_dist: f32,
    in3_v: f32,
    in3_t: f32,
    in23_t: f32,
    in123_t: f32,
    out_v: f32,
    out_t: f32,
}

struct CubeInfo {
    ts: f32,
    x: f32,
    z: f32,
    note_type: NoteType,
    angle: f32,
    any: bool,
    cube: Rc<Cube>,
}

struct Inner {
    start: bool,
    #[cfg(feature = "test")]
    start_time: Instant,
    alive_objs: AliveObjs,
    cube_range_end: usize,
    prev_audio_ts: f32,
    prev_click: bool,
    game_stats: GameStats,
}

type AliveObjs = Vec<Box<dyn Obj>>;

// Implementors of the Obj trait are providing the actual object behaviour.
trait Obj {
    fn update(&mut self, audio_ts: f32, ts_diff: f32, scene_input: &SceneInput, game_stats: &mut GameStats) -> UpdateResult;
}

enum UpdateResult {
    Keep,
    Remove,
    Replace(AliveObjs),
}

impl Game {
    fn new(param: GameParam, asset_mgr: AssetManagerRc, model_reg: &mut ModelRegistry, stats: StatsRc, audio_engine: AudioEngineRc, ui_loop: &UILoop) -> Self {
        let song_info = param.song_info;

        // Determine color scheme.

        let beatmap_info = &song_info.get_beatmap_infos()[param.beatmap_info_index];

        let color_scheme = if let Some(color_scheme_index) = beatmap_info.get_color_scheme_index_opt() && let Some(color_scheme) = song_info.get_color_scheme(color_scheme_index) {
            color_scheme
        } else {
            beatmap_info.get_def_color_scheme()
        };

        let color_l = color_scheme.get_color_l();
        let color_r = color_scheme.get_color_r();

        // Calculate zone info.
        
        let notejump_speed = beatmap_info.get_notejump_speed();

        let in1_dist = ZONE_IN1_DIST;
        let in1_v = ZONE_IN1_V;
        let in1_t = in1_dist / in1_v;

        let in2_dist = ZONE_IN2_DIST;
        let in2_v = notejump_speed;
        let in2_t = in2_dist / in2_v;

        let in3_dist = ZONE_IN3_DIST;
        let in3_v = notejump_speed;
        let in3_t = in3_dist / in3_v;

        let in23_t = in2_t + in3_t;
        let in123_t = in1_t + in2_t + in3_t;

        let out_dist = ZONE_OUT_DIST;
        let out_v = notejump_speed;
        let out_t = out_dist / out_v;

        let zone_info = Rc::new(ZoneInfo {
            in1_dist,
            in2_dist,
            in3_dist,
            in3_v,
            in3_t,
            in23_t,
            in123_t,
            out_v,
            out_t,
        });

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
                let note_type = note.get_note_type();
                let mut any = false;
                let mut symbol = CubeSymbol::Arrow;

                let angle = match note.get_cut_dir() {
                    NoteCutDir::Up => match note_type {
                        NoteType::Left => -180.0,
                        NoteType::Right => 180.0,
                    },
                    NoteCutDir::Down => 0.0,
                    NoteCutDir::Left => 90.0,
                    NoteCutDir::Right => -90.0,
                    NoteCutDir::UpLeft => 135.0,
                    NoteCutDir::UpRight => -135.0,
                    NoteCutDir::DownLeft => 45.0,
                    NoteCutDir::DownRight => -45.0,
                    NoteCutDir::Any => {
                        any = true;
                        symbol = CubeSymbol::Dot;
                        0.0
                    }
                };

                let color = match note_type {
                    NoteType::Left => color_l,
                    NoteType::Right => color_r,
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

                let cube_info = Rc::new(CubeInfo {
                    ts,
                    x,
                    z,
                    note_type,
                    angle,
                    any,
                    cube,
                });

                Some(cube_info)
            } else {
                None
            }
        }));

        // Setup stat window.

        let window_param = WindowParam::new(500, 250, || {
            GameStatsWindow::new().unwrap()
        });

        let game_stats_window = model_reg.create(window_param);
        game_stats_window.set_visible(true);
        game_stats_window.set_scale(2.0, 1.0);
        game_stats_window.set_pos(&Vector3::new(-3.0, 6.0, 3.0));
        game_stats_window.set_rot(&Quaternion::from_angle_z(Deg(20.0)));

        let game_stats_window_weak = game_stats_window.as_weak();

        // Setup floor.

        create_floor(model_reg);
        create_stats_window(model_reg, stats, ui_loop);

        // Setup sabers.

        let (saber_l, saber_r) = create_saber(model_reg, color_l, color_r);

        // Setup audio.

        #[allow(unused_assignments)]
        #[allow(unused_mut)]
        let mut test = false;
        #[cfg(feature = "test")]
        {
            test = param.test;
        }

        let audio_file_opt = if !test {
            let audio_file_factory = AudioFileFactory::new(asset_mgr, song_info.get_song_filename());
            Some(audio_engine.add(audio_file_factory))
        } else {
            None
        };

        let inner = Inner {
            start: true,
            #[cfg(feature = "test")]
            start_time: Instant::now(),
            alive_objs: Vec::new(),
            cube_range_end: 0,
            prev_audio_ts: 0.0, // TODO: is this correct to default it to 0?
            prev_click: true,
            game_stats: GameStats::new(cube_infos.len().try_into().unwrap()),
        };
        
        Self {
            ui_loop: ui_loop.clone(),
            zone_info,
            cube_infos,
            game_stats_window_weak,
            saber_l,
            saber_r,
            audio_file_opt,
            inner: RefCell::new(inner),
        }
    }

    fn update_objs(&self, inner: &mut Inner, audio_ts: f32, scene_input: &SceneInput) {
        let alive_objs = &mut inner.alive_objs;

        // Show incoming cubes.

        let zone_info = &self.zone_info;
        let cube_infos = &self.cube_infos;
        let cube_range_end = &mut inner.cube_range_end;

        if self.audio_file_opt.is_some() {
            let ts_in = audio_ts + zone_info.in123_t;

            for i in *cube_range_end..cube_infos.len() {
                let cube_info = &cube_infos[i];

                if cube_info.ts <= ts_in {
                    let obj = CubeObj::new(Rc::clone(zone_info), Rc::clone(cube_info), #[cfg(feature = "test")] false);
                    alive_objs.push(Box::new(obj));

                    *cube_range_end = i + 1;
                } else {
                    break;
                }
            }
        } else {
            #[cfg(feature = "test")]
            {
                if alive_objs.is_empty() {
                    let cube_info = &cube_infos[*cube_range_end];

                    let obj = CubeObj::new(Rc::clone(zone_info), Rc::clone(cube_info), true);
                    alive_objs.push(Box::new(obj));

                    *cube_range_end += 1;
                }
            }
        }

        // Update objects.

        let prev_audio_ts = &mut inner.prev_audio_ts;
        let game_stats = &mut inner.game_stats;

        let ts_diff = audio_ts - *prev_audio_ts;
        let mut i = 0;

        while i < alive_objs.len() {
            let obj = &mut alive_objs[i];

            match obj.update(audio_ts, ts_diff, scene_input, game_stats) {
                UpdateResult::Keep => {
                    i += 1;
                },
                UpdateResult::Remove => {
                    alive_objs.swap_remove(i);
                },
                UpdateResult::Replace(mut new_alive_objs) => {
                    alive_objs.swap_remove(i);
                    alive_objs.append(&mut new_alive_objs);
                },
            }
        }

        // Display game stats, if changed.

        if game_stats.is_changed() {
            self.ui_loop.add_callback({
                let stats_inner = game_stats.get_inner();
                let window_weak = self.game_stats_window_weak.cloned();

                move || { // TODO: use slint struct?
                    let window = window_weak.upgrade().unwrap();

                    window.set_count(stats_inner.count.try_into().unwrap());
                    window.set_total(stats_inner.total.try_into().unwrap());
                }
            });
        }

        *prev_audio_ts = audio_ts;
    }

    fn update_saber(saber: &Saber, pose_opt: &Option<&dyn ScenePose>) {
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
        let inner = &mut *self.inner.borrow_mut();
        let mut done = false;

        if let Some(audio_file) = &self.audio_file_opt {
            // Start audio on first update.
            // TODO: implement lifecycle methods?

            if inner.start {
                audio_file.play();
                inner.start = false;
            }

            // Update cubes.

            match audio_file.get_timestamp() {
                AudioFileTimestamp::Inactive => unreachable!(),
                AudioFileTimestamp::Unavail => (),
                AudioFileTimestamp::Playing(ts) => self.update_objs(inner, ts as f32, scene_input), // TODO: or use 64 bit ts?
                AudioFileTimestamp::Done => done = true,
            };
        } else {
            #[cfg(feature = "test")]
            {
                if inner.start {
                    inner.start_time = Instant::now();
                    inner.start = false;
                }

                let curr_time = Instant::now();
                let ts = curr_time.duration_since(inner.start_time).as_secs_f32();

                if !(inner.alive_objs.is_empty() && inner.cube_range_end >= self.cube_infos.len()) {
                    self.update_objs(inner, ts, scene_input);
                } else {
                    done = true;
                }
            }
        }

        // Update sabers.

        Self::update_saber(&self.saber_l, &scene_input.pose_l_opt);
        Self::update_saber(&self.saber_r, &scene_input.pose_r_opt);

        // TODO: Implement pause menu.

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

        // If we are finished, then go to menu.

        if done {
            scene_mgr.load(MenuParam::new());
        }
    }
}

struct CubeObj {
    zone_info: Rc<ZoneInfo>,
    cube_info: Rc<CubeInfo>,
    sliced_status: SlicedStatus,
    #[cfg(feature = "test")]
    test: bool,
}

#[derive(Clone, Copy)]
enum SlicedStatus {
    WaitForAbove,
    WaitForBelow(f32, f32),
    AtBelow,
}

impl CubeObj {
    fn new(zone_info: Rc<ZoneInfo>, cube_info: Rc<CubeInfo>, #[cfg(feature = "test")] test: bool) -> Self {
        cube_info.cube.set_visible(true);

        Self {
            zone_info,
            cube_info,
            sliced_status: SlicedStatus::WaitForAbove,
            #[cfg(feature = "test")]
            test,
        }
    }

    fn test_touch(&self, cube_pos: &Vector3<f32>, cube_rot: &Quaternion<f32>, pose: &dyn ScenePose) -> Option<f32> {
        // Short circuit calculation, if the cube and the saber are too far from each other.

        let saber_len = SABER_DIR.magnitude();

        let d = cube_pos - pose.get_pos();
        if d.magnitude() > saber_len + 3.0_f32.sqrt() * (CUBE_SIZE / 2.0) { // TODO: precalculate sqrt(3)?
            return None;
        }

        // Define hitbox. If changed, then short circuit (see above) needs to be adjusted as well.

        let x_range = -(CUBE_SIZE / 2.0)..=(CUBE_SIZE / 2.0);
        let y_range = -(CUBE_SIZE / 2.0)..=(CUBE_SIZE / 2.0);
        let z_range = -(CUBE_SIZE / 2.0)..=(CUBE_SIZE / 2.0);

        // Calculate the shortest length of saber which just intersects the cube.
        // TODO: faster implementation?

        let center_m = self.calc_center_m(cube_pos, cube_rot);
        let saber_pos = center_m * pose.get_pos().extend(1.0);
        let saber_dir = center_m * Matrix4::from(*pose.get_rot()) * SABER_DIR.normalize().extend(0.0);

        // p = saber_pos + saber_dir * len

        let calc_len = |p, pos, dir, compare_x: bool, compare_y: bool, compare_z: bool| {
            if dir != 0.0 {
                let len = (p - pos) / dir;
                if (0.0..=saber_len).contains(&len) && (
                    (!compare_x || x_range.contains(&(saber_pos.x + saber_dir.x * len))) &&
                    (!compare_y || y_range.contains(&(saber_pos.y + saber_dir.y * len))) &&
                    (!compare_z || z_range.contains(&(saber_pos.z + saber_dir.z * len)))
                ) {
                    Some(len)
                } else {
                    None
                }
            } else {
                None // TODO: Handle edge case: saber is on the plane defined by p.
            }
        };

        [
            calc_len(x_range.start(), saber_pos.x, saber_dir.x, false, true, true),
            calc_len(x_range.end(), saber_pos.x, saber_dir.x, false, true, true),
            calc_len(y_range.start(), saber_pos.y, saber_dir.y, true, false, true),
            calc_len(y_range.end(), saber_pos.y, saber_dir.y, true, false, true),
            calc_len(z_range.start(), saber_pos.z, saber_dir.z, true, true, false),
            calc_len(z_range.end(), saber_pos.z, saber_dir.z, true, true, false)
        ].into_iter().flatten().reduce(f32::min)
    }

    fn test_slice(&self, cube_pos: &Vector3<f32>, cube_rot: &Quaternion<f32>, pose: &dyn ScenePose, len: f32, sliced_status: SlicedStatus) -> SlicedStatus {
        // Check whether the [saber handle..len] passes through the center plane.

        let calc_z = |len| {
            let center_m = self.calc_center_m(cube_pos, cube_rot);
            let saber_pos = pose.get_pos() + pose.get_rot() * SABER_DIR.normalize() * len;
            let pos = center_m * saber_pos.extend(1.0);
            pos.z
        };

        let mut new_sliced_status = sliced_status;

        match new_sliced_status {
            SlicedStatus::WaitForAbove => {
                let z = calc_z(len);
                if z > 0.0 {
                    // Remember the shortest length of saber which intersects the cube. This point on
                    // the saber has to move into the direction of the cube center.

                    new_sliced_status = SlicedStatus::WaitForBelow(len, z); 
                }
            },
            SlicedStatus::WaitForBelow(len, z) => {
                if z - calc_z(len) >= CUBE_SIZE / 4.0 {
                    new_sliced_status = SlicedStatus::AtBelow;
                }
            },
            _ => panic!("Invalid status"),
        };

        new_sliced_status
    }

    fn calc_center_m(&self, cube_pos: &Vector3<f32>, cube_rot: &Quaternion<f32>) -> Matrix4<f32> {
        // The matrix (see below) is used to transform cube center to
        // the XY plane, depending on the angle.

        Matrix4::from(cube_rot.conjugate()) * Matrix4::from_translation(-*cube_pos)
    }
}

impl Obj for CubeObj {
    fn update(&mut self, audio_ts: f32, _ts_diff: f32, scene_input: &SceneInput, game_stats: &mut GameStats) -> UpdateResult {
        #[allow(unused_assignments)]
        #[allow(unused_mut)]
        let mut test = false;
        #[cfg(feature = "test")]
        {
            test = self.test;
        }

        // Hide outgoing cube.

        let zone_info = &self.zone_info;
        let cube_info = &self.cube_info;

        if !test {
            let ts_out = audio_ts - zone_info.out_t;

            if cube_info.ts < ts_out {
                cube_info.cube.set_visible(false);
                return UpdateResult::Remove;
            }
        }

        // Update position.
        // TODO: shorter rotation time? z_base is fine.
        // TODO: always display cubes at z=0 and then move them up?

        let ts = if !test { cube_info.ts - audio_ts } else { 0.0 };

        let (y, z_base, angle) = if ts <= 0.0 {
            (ts * zone_info.out_v, CUBE_FLOOR, cube_info.angle)
        } else if ts <= zone_info.in3_t {
            (ts * zone_info.in3_v, CUBE_FLOOR, cube_info.angle)
        } else if ts <= zone_info.in23_t {
            let factor = (zone_info.in23_t - ts) / (zone_info.in23_t - zone_info.in3_t);
            (zone_info.in2_dist * (1.0 - factor) + zone_info.in3_dist, CUBE_FLOOR * Deg(90.0 * factor).sin(), cube_info.angle * factor)
        } else {
            let factor = (zone_info.in123_t - ts) / (zone_info.in123_t - zone_info.in23_t);
            (zone_info.in1_dist * (1.0 - factor) + zone_info.in2_dist + zone_info.in3_dist, 0.0, 0.0)
        };

        let pos = Vector3::new(cube_info.x, y + OFFSET_Y, cube_info.z + z_base); // TODO: ts_in/ts_out should be offseted because of OFFSET_Y.
        cube_info.cube.set_pos(&pos);

        let rot = Quaternion::from_angle_y(Deg(angle));
        cube_info.cube.set_rot(&rot);

        // Select matching saber.

        let pose_opt = match cube_info.note_type {
            NoteType::Left => scene_input.pose_l_opt,
            NoteType::Right => scene_input.pose_r_opt,
        };

        // Do hit detection.

        if let Some(pose) = pose_opt && let Some(len) = self.test_touch(&pos, &rot, pose) {
            let mut sliced = false;

            if cube_info.any {
                // Once the saber touches cube, the cube is becoming sliced.

                sliced = true;
            } else {
                // The saber should stay in contact with the cube (see test_touch above),
                // while the slicing test is still in progress.
                
                self.sliced_status = self.test_slice(&pos, &rot, pose, len, self.sliced_status);
                if matches!(self.sliced_status, SlicedStatus::AtBelow) {
                    sliced = true;
                }
            }

            if sliced {
                cube_info.cube.sliced();

                let new_alive_objs: AliveObjs = vec![
                    Box::new(SlicedObj::new(Rc::clone(cube_info), &pos, false)),
                    Box::new(SlicedObj::new(Rc::clone(cube_info), &pos, true)),
                ];

                game_stats.inc_count();
                pose.apply_haptic();

                return UpdateResult::Replace(new_alive_objs);
            }
        } else {
            // If the saber disappears or it is not touching cube, then restart detection.

            self.sliced_status = SlicedStatus::WaitForAbove;
        }

        UpdateResult::Keep
    }
}

struct SlicedObj {
    cube_info: Rc<CubeInfo>,
    pos: Vector3<f32>,
    right: bool,
    v: Vector3<f32>, // [m/s]
    rot_axis: Vector3<f32>,
    rot_angle: f32, // [deg/s]
    ts_diff_acc: f32,
}

impl SlicedObj {
    fn new(cube_info: Rc<CubeInfo>, pos: &Vector3<f32>, right: bool) -> Self {
        let factor = if !right {
            -1.0
        } else {
            1.0
        };

        let angle = cube_info.angle;

        Self {
            cube_info,
            pos: *pos,
            right,
            v: Quaternion::from_angle_y(Deg(angle)) * Vector3::new(factor * 3.0 + rand::random_range(-1.0..1.0), rand::random_range(-1.0..0.0), rand::random_range(-1.0..1.0)),
            rot_axis: Vector3::new(rand::random_range(-1.0..1.0), rand::random_range(-1.0..1.0), rand::random_range(-1.0..1.0)).normalize(),
            rot_angle: 4.0 * rand::random_range(30.0..100.0),
            ts_diff_acc: 0.0,
        }
    }
}

impl Obj for SlicedObj {
    fn update(&mut self, _audio_ts: f32, ts_diff: f32, _scene_input: &SceneInput, _game_stats: &mut GameStats) -> UpdateResult {
        let cube_info = &self.cube_info;

        self.ts_diff_acc += ts_diff;

        // Handle gravity and position.

        self.v.z -= G * ts_diff;
        self.pos += self.v * ts_diff;

        let visible = self.pos.z > -CUBE_SIZE; // Should be enough.
        let rot = Quaternion::from_angle_y(Deg(cube_info.angle)) * Quaternion::from_axis_angle(self.rot_axis, Deg(self.rot_angle) * self.ts_diff_acc); // TODO: Calculate rot from previous rot + delta (like self.pos)?

        if !self.right {
            if visible {
                cube_info.cube.set_pos_l(&self.pos);
                cube_info.cube.set_rot_l(&rot);
            } else {
                cube_info.cube.set_visible_l(false);
            }
        } else {
            #[allow(clippy::collapsible_else_if)]
            if visible {
                cube_info.cube.set_pos_r(&self.pos);
                cube_info.cube.set_rot_r(&rot);
            } else {
                cube_info.cube.set_visible_r(false);
            }
        }

        if visible {
            UpdateResult::Keep
        } else {
            UpdateResult::Remove
        }
    }
}

struct GameStats {
    changed: bool,
    inner: GameStatsInner,
}

#[derive(Copy, Clone)]
struct GameStatsInner {
    count: u32,
    total: u32,
}

impl GameStats {
    fn new(total: u32) -> Self {
        let inner = GameStatsInner {
            count: 0,
            total,
        };

        Self {
            changed: true, // Force change on first update.
            inner,
        }
    }

    fn get_inner(&self) -> GameStatsInner {
        self.inner
    }

    fn inc_count(&mut self) {
        self.inner.count += 1;
        self.changed()
    }

    fn changed(&mut self) {
        self.changed = true;
    }

    fn is_changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }
}
