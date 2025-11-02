use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use cgmath::{Deg, Quaternion, Rotation3, Vector3};

use crate::model::*;
use crate::ui::{StatsWindow, UILoop};
use crate::util::StatsRc;

mod game;
pub use game::*;

mod menu;
pub use menu::*;

const STATS_BORDER: f32 = 0.01;
const STATS_REFRESH: f32 = 1.0; // [s]

const SABER_HANDLE_PHONG_PARAM: PhongParam = PhongParam::new(0.1, 0.2, 0.3, 64.0);
const SABER_RAY_PHONG_PARAM: PhongParam = PhongParam::new(1.0, 0.0, 0.0, 0.0);

// Convenience methods used by scenes.

pub fn create_floor(model_reg: &mut ModelRegistry) {
    let floor_param = FloorParam::new(&COLOR_WHITE);
    let floor = model_reg.create(floor_param);
    floor.set_visible(true);
    floor.set_pos(&Vector3::new(0.0, 0.0, 0.0));
}

pub fn create_stats_window(model_reg: &mut ModelRegistry, stats: StatsRc, ui_loop: &UILoop) {
    let window_param = WindowParam::new(500, 250, {
        let stats_inner = stats.get_inner();

        move || {
            let window = StatsWindow::new().unwrap();
            window.set_comment(stats_inner.comment.into());

            window
        }
    });

    let window = model_reg.create(window_param);
    window.set_visible(true);
    window.set_scale(2.0 - 2.0 * STATS_BORDER, 1.0 - 2.0 * STATS_BORDER);
    window.set_pos(&Vector3::new(0.0, 2.5, 0.001));
    window.set_rot(&Quaternion::from_angle_x(Deg(-90.0)));

    thread::spawn({
        let ui_loop = ui_loop.clone();
        let window_weak = window.as_weak::<StatsWindow>();

        move || {
            // If there are no strong reference to window (e.g. scene has been ended),
            // then signal thread to terminate.
            
            let alive = Arc::new(AtomicBool::new(true));

            while alive.load(Ordering::Relaxed) {
                ui_loop.add_callback({
                    let window_weak = window_weak.cloned();
                    let stats_inner = stats.get_inner();
                    let alive = Arc::clone(&alive);

                    move || {
                        match window_weak.upgrade() {
                            Some(window) => { // TODO: use slint struct?
                                window.set_fps(stats_inner.fps.try_into().unwrap());
                                window.set_frame_time(stats_inner.frame_time.try_into().unwrap());
                                window.set_draw_calls(stats_inner.draw_calls.try_into().unwrap());
                                window.set_inst_num(stats_inner.inst_num.try_into().unwrap());
                                window.set_inst_buf(stats_inner.inst_buf.try_into().unwrap());
                            },
                            None => alive.store(false, Ordering::Relaxed),
                        }
                    }
                });

                thread::sleep(Duration::from_secs_f32(STATS_REFRESH));
            }
        }
    });
}

pub fn create_saber(model_reg: &mut ModelRegistry, color_l: &Color, color_r: &Color) -> (Rc<Saber>, Rc<Saber>) {
    let saber_param = SaberParam::new(color_l, &SABER_HANDLE_PHONG_PARAM, color_l, &SABER_RAY_PHONG_PARAM);
    let saber_l = model_reg.create(saber_param);

    let saber_param = SaberParam::new(color_r, &SABER_HANDLE_PHONG_PARAM, color_r, &SABER_RAY_PHONG_PARAM);
    let saber_r = model_reg.create(saber_param);

    (saber_l, saber_r)
}
