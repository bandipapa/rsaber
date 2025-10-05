use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::iter;
use std::mem;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use bytemuck::{Pod, Zeroable};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, PlatformError};
use slint::platform::{self, Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType, TargetPixel};
use wgpu::{Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect};

pub type UIManagerRc = Rc<UIManager>;

pub struct UIManager {
    inner_mutex: InnerMutex,
    cond: CondRc,
    next_window_id: Cell<WindowId>,
}

struct Inner {
    window_ops_opt: Option<Vec<WindowOp>>, // TODO: use queue instead?
    current_event_opt: Option<CurrentEvent>,
}

impl Inner {
    fn window_ops(&mut self) -> &mut Vec<WindowOp> {
        self.window_ops_opt.get_or_insert_default()
    }
}

type InnerMutex = Arc<Mutex<Inner>>;
type CondRc = Arc<Condvar>;

enum WindowOp {
    Create(WindowOpCreate),
    Drop(WindowId),
}

struct WindowOpCreate {
    window_id: WindowId,
    width: u32,
    height: u32,
    func: Box<dyn FnOnce() -> Box<dyn SlintComponentHandle + 'static> + Send + 'static>,
    texture: Texture,
}

struct CurrentEvent {
    window_id: WindowId,
    event: UIEvent,
}

type WindowId = u32;

impl UIManager {
    pub fn new(queue: Queue) -> Self {
        let inner = Inner {
            window_ops_opt: None,
            current_event_opt: None,
        };
        let inner_mutex = Arc::new(Mutex::new(inner));

        let cond = Arc::new(Condvar::new());

        thread::spawn({
            let inner_mutex = Arc::clone(&inner_mutex);
            let cond = Arc::clone(&cond);

            move || {
                // Spawn thread to run slint event loop, so UI rendering is not going to block
                // the main render thread.
                
                let platform = UIPlatform::new(inner_mutex, cond, queue);
                platform::set_platform(Box::new(platform)).expect("Unable to set platform");
                slint::run_event_loop().expect("Unable to run event loop");
            }
        });

        Self {
            inner_mutex,
            cond,
            next_window_id: Cell::new(0),
        }
    }

    pub fn create_window<F: FnOnce() -> C + Send + 'static, C: SlintComponentHandle + 'static>(&self, width: u32, height: u32, func: F, texture: Texture) -> UIWindow {
        // Schedule func to run on the slint event loop.

        let window_id = self.next_window_id.get();
        self.next_window_id.set(window_id + 1);

        let window_op = WindowOp::Create(WindowOpCreate {
            window_id,
            width,
            height,
            func: Box::new(move || Box::new(func())),
            texture,
        });

        {
            let mut inner = self.inner_mutex.lock().unwrap();

            let window_ops = inner.window_ops();
            window_ops.push(window_op);

            self.cond.notify_all();
        }

        UIWindow::new(Arc::clone(&self.inner_mutex), Arc::clone(&self.cond), window_id)
    }
}

pub struct UIWindow {
    inner_mutex: InnerMutex,
    cond: CondRc,
    window_id: WindowId,
}

impl UIWindow {
    fn new(inner_mutex: InnerMutex, cond: CondRc, window_id: WindowId) -> Self {
        Self {
            inner_mutex,
            cond,
            window_id,
        }
    }

    pub fn handle_event(&self, event: UIEvent) {
        let mut inner = self.inner_mutex.lock().unwrap();

        // Overwrite the previous (unprocessed) event, since we are interested
        // only in the most recent one.

        inner.current_event_opt = Some(CurrentEvent {
            window_id: self.window_id,
            event,
        });

        self.cond.notify_all();
    }
}

impl Drop for UIWindow {
    fn drop(&mut self) {
        let window_op = WindowOp::Drop(self.window_id);

        let mut inner = self.inner_mutex.lock().unwrap();

        let window_ops = inner.window_ops();
        window_ops.push(window_op);

        self.cond.notify_all();
    }
}

#[allow(clippy::enum_variant_names)]
pub enum UIEvent {
    PointerMove(f32, f32),
    PointerPress(f32, f32),
    PointerExit,
}

struct UIPlatform {
    inner_mutex: InnerMutex,
    cond: CondRc,
    queue: Queue,
    current_soft_window: RefCell<Option<Rc<MinimalSoftwareWindow>>>,
}

struct WindowInfo {
    _handle: Box<dyn SlintComponentHandle>, // To keep window alive.
    soft_window: Rc<MinimalSoftwareWindow>,
    width: u32,
    height: u32,
    buf: Box<[Rgba]>,
    texture: Texture,
}

impl UIPlatform {
    fn new(inner_mutex: InnerMutex, cond: CondRc, queue: Queue) -> Self {
        Self {
            inner_mutex,
            cond,
            queue,
            current_soft_window: RefCell::new(None),
        }
    }
}

impl Platform for UIPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let mut current_soft_window = self.current_soft_window.borrow_mut();
        assert!(current_soft_window.is_none()); // We can create a single window per WindowOpCreate->func.

        // Instantiate software renderer.
        // TODO: In the future, replace it with GPU renderer: https://github.com/slint-ui/slint/issues/6158

        let soft_window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        *current_soft_window = Some(Rc::clone(&soft_window));

        Ok(soft_window)
    }

    fn run_event_loop(&self) -> Result<(), PlatformError> {
        let mut window_infos = HashMap::new();
        let mut active_window_id_opt = None;

        // TODO: Pause loop when the app is not visible?
        loop {
            let dur_opt = platform::duration_until_next_timer_update();

            let (window_ops_opt, current_event_opt) = {
                // window_ops_opt is an Option to pass the inner Vec quickly (without copying Vec elements).

                let inner = self.inner_mutex.lock().unwrap();
                let check = |inner: &mut Inner| inner.window_ops_opt.is_none() && inner.current_event_opt.is_none();

                let mut inner = if let Some(dur) = dur_opt {
                    self.cond.wait_timeout_while(inner, dur, check).unwrap().0
                } else {
                    self.cond.wait_while(inner, check).unwrap()
                };

                (inner.window_ops_opt.take(), inner.current_event_opt.take())
            };

            platform::update_timers_and_animations();

            // Create/drop windows.

            if let Some(window_ops) = window_ops_opt {
                for window_op in window_ops {
                    match window_op {
                        WindowOp::Create(window_op_create) => {
                            let handle = (window_op_create.func)(); // This will call create_window_adapter().
                            let soft_window = self.current_soft_window.borrow_mut().take().expect("Missing window");

                            let width = window_op_create.width;
                            let height = window_op_create.height;

                            soft_window.window().set_size(PhysicalSize {
                                width,
                                height,
                            });

                            let window_info = WindowInfo {
                                _handle: handle,
                                soft_window,
                                width,
                                height,
                                buf: Box::from_iter(iter::repeat_n(Rgba::new(0, 0,0), (width * height).try_into().unwrap())),
                                texture: window_op_create.texture,
                            };

                            window_infos.insert(window_op_create.window_id, window_info);
                        },
                        WindowOp::Drop(window_id) => {
                            window_infos.remove(&window_id);
                        },
                    };
                }
            }

            // If the active window is dropped, then we don't send WindowEvent::PointerExited,
            // since there is no window to send to.

            if let Some(active_window_id) = &active_window_id_opt && !window_infos.contains_key(active_window_id) {
                active_window_id_opt = None;
            }

            // Handle event.

            if let Some(current_event) = current_event_opt {
                let window_id = current_event.window_id;
                let event = current_event.event;

                // If we receive an event for a window which is different from the active,
                // and the window has been dropped (see above), then we still send
                // WindowEvent::PointerExited for the active window.

                if let Some(active_window_id) = &active_window_id_opt && (*active_window_id != window_id || matches!(event, UIEvent::PointerExit)) {
                    let window_info = window_infos.get(active_window_id).unwrap();
                    let soft_window = &window_info.soft_window;

                    soft_window.dispatch_event(WindowEvent::PointerExited);

                    active_window_id_opt = None;
                }

                if let Some(window_info) = window_infos.get(&window_id) {
                    let soft_window = &window_info.soft_window;

                    let calc_pos = |x, y| LogicalPosition {
                        x: x * window_info.width as f32,
                        y: y * window_info.height as f32,
                    };

                    match event {
                        UIEvent::PointerMove(x, y) => {
                            let pos = calc_pos(x, y);
                            
                            soft_window.dispatch_event(WindowEvent::PointerMoved {
                                position: pos,
                            });

                            active_window_id_opt = Some(window_id);
                        },
                        UIEvent::PointerPress(x, y) => {
                            let pos = calc_pos(x, y);

                            soft_window.dispatch_event(WindowEvent::PointerPressed {
                                position: pos,
                                button: PointerEventButton::Left,
                            });

                            soft_window.dispatch_event(WindowEvent::PointerReleased {
                                position: pos,
                                button: PointerEventButton::Left,
                            });

                            active_window_id_opt = Some(window_id);
                        },
                        UIEvent::PointerExit => (),
                    };
                }
            }

            // Redraw windows.

            for window_info in window_infos.values_mut() {
                window_info.soft_window.draw_if_needed(|renderer| {
                    let width = window_info.width;
                    let buf = &mut window_info.buf;

                    let region = renderer.render(buf, width.try_into().unwrap());
                    let region_origin = region.bounding_box_origin();
                    let region_size = region.bounding_box_size();

                    let pixel_size = mem::size_of::<Rgba>();

                    self.queue.write_texture( // TODO: Improve write_texture performance, implement buffering scenario?
                        TexelCopyTextureInfo {
                            texture: &window_info.texture,
                            mip_level: 0,
                            origin: Origin3d {
                                x: region_origin.x.try_into().unwrap(),
                                y: region_origin.y.try_into().unwrap(),
                                z: 0,
                            },
                            aspect: TextureAspect::All,
                        },
                        bytemuck::cast_slice(buf),
                        TexelCopyBufferLayout {
                            offset: (region_origin.y as u64 * width as u64 + region_origin.x as u64) * pixel_size as u64,
                            bytes_per_row: Some(width * pixel_size as u32),
                            rows_per_image: None,
                        },
                        Extent3d {
                            width: region_size.width,
                            height: region_size.height,
                            depth_or_array_layers: 1,
                        }
                    );
                });
            }
        }
    }
}

pub trait SlintComponentHandle {
}

impl<T: ComponentHandle> SlintComponentHandle for T {
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Rgba {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Rgba {
    fn new(r: u8, g: u8, b: u8) -> Self {
        Self {
            r,
            g,
            b,
            a: 0xff,
        }
    }
}

impl TargetPixel for Rgba { // Taken from slint->internal/core/software_renderer/draw_functions.rs.
    fn blend(&mut self, color: PremultipliedRgbaColor) {
        let a = (u8::MAX - color.alpha) as u16;
        self.r = (self.r as u16 * a / 255) as u8 + color.red;
        self.g = (self.g as u16 * a / 255) as u8 + color.green;
        self.b = (self.b as u16 * a / 255) as u8 + color.blue;
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b)
    }
}
