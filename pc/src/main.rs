use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use cgmath::{Deg, InnerSpace, Matrix3, Quaternion, Rotation3, Vector3};
use pollster::FutureExt;
use wgpu::SurfaceTarget;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use rsaber_lib::{APP_NAME, Main};
use rsaber_lib::asset::EmbedAssetManager;
use rsaber_lib::output::{WindowBegin, WindowOutput};
use rsaber_lib::scene::{SceneInput, ScenePose};
use rsaber_lib::util::Stats;

const COMMENT: &str = "You can use keys w-a-s-d to move, z-x to change elevation, r to reset view and arrow keys to rotate camera. Interaction with UI controls can be done with mouse.";

const MIN_SIZE: PhysicalSize<u32> = PhysicalSize { width: 800, height: 600 };
const DEFAULT_POS: Vector3<f32> = Vector3::new(0.0, -2.5, 1.8); // TODO: configurable height
const ROT_SPEED: f32 = 50.0; // [deg/s]
const MOVE_SPEED: f32 = 5.0; // [m/s]

struct App {
    asset_mgr: Option<EmbedAssetManager>,
    data: Option<AppData>,
}

struct AppData {
    window: Arc<Window>,
    output: WindowOutput,
    main: Main,
    pos: Vector3<f32>,
    pitch: f32,
    yaw: f32,
    keys: HashSet<KeyCode>,
    prev_ts_opt: Option<Instant>,
    cursor_pos: Option<(u32, u32)>,
    cursor_click: bool,
    active: bool,
}

impl App {
    fn new(asset_mgr: EmbedAssetManager) -> Self {
        Self {
            asset_mgr: Some(asset_mgr),
            data: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.data.is_none() {
            // From https://docs.rs/winit/latest/winit/application/trait.ApplicationHandler.html#tymethod.resumed :
            // "It's recommended that applications should only initialize their graphics context and create a window after they have received their first Resumed event."

            let window_attrs = Window::default_attributes()
                .with_title(APP_NAME)
                .with_min_inner_size(MIN_SIZE);

            let window = Arc::new(event_loop.create_window(window_attrs).expect("Unable to create window"));
            let output = WindowOutput::new(SurfaceTarget::from(Arc::clone(&window))).block_on();
            let stats = Stats::new(COMMENT);
            let main = Main::new(self.asset_mgr.take().unwrap(), output.get_info(), stats);

            let audio_engine = main.get_audio_engine();
            audio_engine.start();

            self.data = Some(AppData {
                window,
                output,
                main,
                pos: DEFAULT_POS,
                pitch: 0.0,
                yaw: 0.0,
                keys: HashSet::new(),
                prev_ts_opt: None,
                cursor_pos: None,
                cursor_click: false,
                active: true,
            });
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        let data = match &mut self.data {
            Some(data) => data,
            None => return,
        };

        let size = data.window.inner_size();
        let output = &data.output;
        let keys = &mut data.keys;
        let prev_ts_opt = &mut data.prev_ts_opt;
        let cursor_pos = &mut data.cursor_pos;
        let cursor_click = &mut data.cursor_click;
        let active = &mut data.active;

        match event {
            WindowEvent::Resized(_) => {
                let audio_engine = data.main.get_audio_engine();

                if size.width == 0 || size.height == 0 {
                    audio_engine.pause();
                    keys.clear();
                    *prev_ts_opt = None;
                    *cursor_pos = None;
                    *cursor_click = false;
                    *active = false;
                } else {
                    output.resize(size.width, size.height);
                    audio_engine.start();
                    *active = true;
                }
            },
            WindowEvent::RedrawRequested => {
                // Handle input.

                let ts: Instant = Instant::now();

                let pos = &mut data.pos;
                let pitch = &mut data.pitch;
                let yaw = &mut data.yaw;

                if keys.contains(&KeyCode::KeyR) { // Reset
                    *pos = DEFAULT_POS;
                    *pitch = 0.0;
                    *yaw = 0.0;
                } else if let Some(prev_ts) = prev_ts_opt {
                    let ts_diff: f32 = ts.duration_since(*prev_ts).as_secs_f32();
                    let value = ROT_SPEED * ts_diff;

                    // Handle pitch.

                    if keys.contains(&KeyCode::ArrowUp) {
                        *pitch += value;
                    }

                    if keys.contains(&KeyCode::ArrowDown) {
                        *pitch -= value;
                    }

                    // Handle yaw.

                    if keys.contains(&KeyCode::ArrowLeft) {
                        *yaw += value;
                    }

                    if keys.contains(&KeyCode::ArrowRight) {
                        *yaw -= value;
                    }

                    // Handle forward/backward.

                    let value = MOVE_SPEED * ts_diff * (Quaternion::from_angle_z(Deg(*yaw)) * Vector3::unit_y());

                    if keys.contains(&KeyCode::KeyW) {
                        *pos += value;
                    }

                    if keys.contains(&KeyCode::KeyS) {
                        *pos -= value;
                    }

                    // Handle left/right.

                    let value = Vector3::unit_z().cross(value);

                    if keys.contains(&KeyCode::KeyA) {
                        *pos += value;
                    }

                    if keys.contains(&KeyCode::KeyD) {
                        *pos -= value;
                    }

                    // Handle up/down.

                    let value = MOVE_SPEED * ts_diff * Vector3::unit_z();

                    if keys.contains(&KeyCode::KeyX) {
                        *pos += value;
                    }

                    if keys.contains(&KeyCode::KeyZ) {
                        *pos -= value;
                    }
                }

                *prev_ts_opt = Some(ts);

                // Render frame.

                if *active {
                    data.window.request_redraw(); // TODO: do we need this refresh thingy? or we can run render in busy loop?
                }

                let dir = Quaternion::from_angle_z(Deg(*yaw)) * Quaternion::from_angle_x(Deg(*pitch)) * Vector3::unit_y();

                match output.begin(pos, &dir) {
                    WindowBegin::NotInited => (),
                    WindowBegin::ResizeNeeded => output.resize(size.width, size.height),
                    WindowBegin::Frame(frame) => {
                        let mut scene_input = SceneInput {
                            pose_l_opt: None,
                            pose_r_opt: None,
                        };

                        let pose;

                        if let Some((x, y)) = *cursor_pos {
                            let width = size.width as f32;
                            let height = size.height as f32;

                            let ndc_x = 2.0 * x as f32 / width - 1.0;
                            let ndc_y = -(2.0 * y as f32 / height - 1.0);

                            let unit_y = frame.raycast(ndc_x, ndc_y);
                            let unit_x = unit_y.cross(Vector3::unit_z()).normalize();
                            let unit_z = unit_x.cross(unit_y);

                            // Regarding from_angle_x(-90):
                            // - Lets assume that we are at the origin, and looking into the direction of +y.
                            // - If the mouse is at the center of the screen, then the rotation described
                            //   by unit_* vectors is an identity rotation. The mouse is pointing to +y.
                            // - The neutral/identity direction of the saber is +z (see SABER_DIR).
                            // - Therefore, we need to apply a rotation to the mouse direction to
                            //   simulate saber direction.

                            let rot_m = Matrix3::from_cols(unit_x, unit_y, unit_z) * Matrix3::from_angle_x(Deg(-90.0));
                            let rot = Quaternion::from(rot_m);

                            pose = Pose::new(pos, &rot, *cursor_click);
                            scene_input.pose_l_opt = Some(&pose);
                        }

                        data.main.render(frame, &scene_input);
                    }
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.repeat {
                    let pressed = match event.state {
                        ElementState::Pressed => true,
                        ElementState::Released => false,
                    };

                    if let PhysicalKey::Code(key) = event.physical_key {
                        if pressed {
                            keys.insert(key);
                        } else {
                            keys.remove(&key);
                        }
                    }
                }
            },
            WindowEvent::CursorMoved { position, .. } => {
                *cursor_pos = Some((position.x as u32, position.y as u32));
            },
            WindowEvent::CursorLeft { .. } => {
                *cursor_pos = None;
                *cursor_click = false;
            },
            WindowEvent::MouseInput { state, button, .. } => {
                if matches!(button, MouseButton::Left) {
                    *cursor_click = match state {
                        ElementState::Pressed => true,
                        ElementState::Released => false,
                    };
                }
            },
            WindowEvent::CloseRequested => {
                event_loop.exit();
            },
            _ => (),
        }
    }
}

struct Pose {
    pos: Vector3<f32>,
    rot: Quaternion<f32>,
    click: bool,
}

impl Pose {
    fn new(pos: &Vector3<f32>, rot: &Quaternion<f32>, click: bool) -> Self {
        Self {
            pos: *pos,
            rot: *rot,
            click,
        }
    }
}

impl ScenePose for Pose {
    fn get_pos(&self) -> &Vector3<f32> {
        &self.pos
    }

    fn get_rot(&self) -> &Quaternion<f32> {
        &self.rot
    }

    fn get_click(&self) -> bool {
        self.click
    }

    fn get_render(&self) -> bool {
        false
    }

    fn apply_haptic(&self) {
        // No haptic support in windowed mode.
    }
}

fn main() {
    let asset_mgr = EmbedAssetManager::new();
    let mut app = App::new(asset_mgr);

    let event_loop = EventLoop::new().expect("Unable to create event loop");
    event_loop.run_app(&mut app).expect("Unable to run event loop");
}
