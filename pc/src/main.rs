use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use cgmath::{Deg, Quaternion, Rotation3, Vector3};
use pollster::FutureExt;
use wgpu::SurfaceTarget;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

mod asset;
use asset::AssetManager;

use rsaber_lib::{APP_NAME, Main};
use rsaber_lib::output::{WindowBegin, WindowOutput};
use rsaber_lib::scene::SceneInput;

const MIN_SIZE: PhysicalSize<u32> = PhysicalSize { width: 800, height: 600 };
const DEFAULT_POS: Vector3<f32> = Vector3::new(0.0, 0.0, 1.8); // TODO: configurable height
const ROT_SPEED: f32 = 50.0; // [deg/s]
const MOVE_SPEED: f32 = 5.0; // [m/s]

struct App {
    asset_mgr: Option<AssetManager>,
    data: Option<AppData>,
}

struct AppData {
    window: Arc<Window>,
    output: WindowOutput,
    main: Main,
    scene_input: SceneInput,
    pos: Vector3<f32>,
    pitch: f32,
    yaw: f32,
    keys: HashSet<KeyCode>,
    prev_ts_opt: Option<Instant>,
    active: bool,
}

impl App {
    fn new(asset_mgr: AssetManager) -> Self {
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
            let main = Main::new(self.asset_mgr.take().unwrap(), output.get_info());

            let audio_engine = main.get_audio_engine();
            audio_engine.start();

            self.data = Some(AppData {
                window,
                output,
                main,
                scene_input: SceneInput { // TODO: read from keyboard
                    pose_l_opt: None,
                    pose_r_opt: None
                },
                pos: DEFAULT_POS,
                pitch: 0.0,
                yaw: 0.0,
                keys: HashSet::new(),
                prev_ts_opt: None,
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
        let keys = &mut data.keys;
        let prev_ts_opt = &mut data.prev_ts_opt;
        let active = &mut data.active;

        match event {
            WindowEvent::Resized(_) => {
                let audio_engine = data.main.get_audio_engine();

                if size.width == 0 || size.height == 0 {
                    audio_engine.pause();
                    keys.clear();
                    *prev_ts_opt = None;
                    *active = false;
                } else {
                    data.output.resize(size.width, size.height);
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

                let rot = Quaternion::from_angle_z(Deg(*yaw)) * Quaternion::from_angle_x(Deg(*pitch)) * Vector3::unit_y();

                match data.output.begin(pos, &rot) {
                    WindowBegin::NotInited => (),
                    WindowBegin::ResizeNeeded => data.output.resize(size.width, size.height),
                    WindowBegin::Frame(frame) => data.main.render(frame, &data.scene_input),
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
            WindowEvent::CloseRequested => {
                event_loop.exit();
            },
            _ => (),
        }
    }
}

fn main() {
    let asset_mgr = AssetManager::new();
    let mut app = App::new(asset_mgr);

    let event_loop = EventLoop::new().expect("Unable to create event loop");
    event_loop.run_app(&mut app).expect("Unable to run event loop");
}
