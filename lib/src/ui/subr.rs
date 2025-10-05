use std::cell::RefCell;
use std::rc::Rc;

use crate::model::{Pointer, Saber, SaberVisibility, Window};
use crate::scene::{SceneInput, ScenePose};
use crate::ui::UIEvent;

const ACTIVE: f32 = 0.15; // [m]

pub struct UISubr {
    inner: RefCell<Inner>,
}

struct Inner {
    prev_click_l: Option<bool>,
    prev_click_r: Option<bool>,
    active_saber: ActiveSaber,
    active_info_opt: Option<ActiveInfo>,
}

enum ActiveSaber {
    Left,
    Right,
}

struct ActiveInfo {
    window: Rc<Window>,
    last_move: Option<(f32, f32)>,
}

impl UISubr {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(Inner {
                prev_click_l: None,
                prev_click_r: None,
                active_saber: ActiveSaber::Right,
                active_info_opt: None,
            })
        }
    }

    pub fn reset(&self) {
        let mut inner = self.inner.borrow_mut();

        // Keep active saber during reset.

        inner.prev_click_l = None;
        inner.prev_click_r = None;
        inner.active_info_opt = None;
    }

    pub fn update<'a, T: Iterator<Item = &'a Rc<Window>>>(&self, saber_l: &Saber, saber_r: &Saber, pointer: &Pointer, windows: T, scene_input: &SceneInput) {
        let mut inner = self.inner.borrow_mut();

        let pose_l_opt = &scene_input.pose_l_opt;
        let pose_r_opt = &scene_input.pose_r_opt;

        // Update sabers.

        Self::update_saber(saber_l, pose_l_opt);
        Self::update_saber(saber_r, pose_r_opt);

        // Detect trigger (rising edge) click.

        let click_l = Self::update_click(&mut inner.prev_click_l, pose_l_opt);
        let click_r = Self::update_click(&mut inner.prev_click_r, pose_r_opt);

        // Determine active saber.

        let active_saber = &mut inner.active_saber;
        let mut click = false;

        if click_l && click_r {
            // Click both sabers at the same time: no action.
        } else if click_l {
            *active_saber = ActiveSaber::Left;
            click = true;
        } else if click_r {
            *active_saber = ActiveSaber::Right;
            click = true;
        } else {
            // No click: keep active saber.
        }

        // Query pose.

        let pose_opt = match active_saber {
            ActiveSaber::Left => pose_l_opt,
            ActiveSaber::Right => pose_r_opt,
        };

        // Check window<->saber intersection.

        let mut pointer_visible = false;

        if let Some(pose) = pose_opt {
            pointer_visible = true;
            let mut pointer_scale = ACTIVE;
            let mut have_window = false;

            for window in windows {
                if let Some((d, x, y)) = window.intersect(pose) {
                    // If active_info.window != window, don't send UIEvent::PointerExit, since UIWindow is
                    // already handling this case.

                    let mut active_info = ActiveInfo {
                        window: Rc::clone(window),
                        last_move: None,
                    };

                    let event_opt = if !click {
                        active_info.last_move = Some((x, y));

                        // Send UIEvent::PointerMove only if the position has been changed. This is
                        // to reduce the number of events send to the UI thread.

                        if let Some(active_info) = &inner.active_info_opt && Rc::ptr_eq(&active_info.window, window) && let Some(last_move) = &active_info.last_move && *last_move == (x, y) {
                            None
                        } else {
                            Some(UIEvent::PointerMove(x, y))
                        }
                    } else {
                        Some(UIEvent::PointerPress(x, y))
                    };

                    inner.active_info_opt = Some(active_info);

                    if let Some(event) = event_opt {
                        window.handle_event(event);
                    }
                    
                    pointer_scale = d;
                    have_window = true;
                    break;
                }
            }

            if !have_window && let Some(active_info) = inner.active_info_opt.take() {
                active_info.window.handle_event(UIEvent::PointerExit);
            }

            pointer.set_scale(pointer_scale);
            pointer.set_pos(pose.get_pos());
            pointer.set_rot(pose.get_rot());
        } else if let Some(active_info) = inner.active_info_opt.take() {
            active_info.window.handle_event(UIEvent::PointerExit);
        }

        pointer.set_visible(pointer_visible);
    }

    fn update_saber(saber: &Saber, pose_opt: &Option<ScenePose>) {
        if let Some(pose) = pose_opt && pose.get_render() {
            saber.set_visible(SaberVisibility::Handle);
            saber.set_pos(pose.get_pos());
            saber.set_rot(pose.get_rot());
        } else {
            saber.set_visible(SaberVisibility::Hidden);
        }
    }

    fn update_click(prev_click: &mut Option<bool>, pose_opt: &Option<ScenePose>) -> bool {
        let mut click_triggered = false;

        *prev_click = if let Some(pose) = pose_opt {
            let click = pose.get_click();

            if prev_click.is_some() && !prev_click.unwrap() && click {
                click_triggered = true;
            }

            Some(click)
        } else {
            None
        };

        click_triggered
    }
}
