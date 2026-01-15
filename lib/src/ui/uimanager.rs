use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::iter;
use std::mem;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;

use bytemuck::{Pod, Zeroable};
use oneshot::Sender;
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, PlatformError, Weak};
use slint::platform::{self, Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType, TargetPixel};
use wgpu::{Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect};

use crate::net::NetManagerRunner;
use crate::util::MuCo;

pub type UIManagerRc = Rc<UIManager>;

pub struct UIManager {
    inner_muco: InnerMuCo,
    next_window_id: Cell<WindowId>,
    ui_loop: UILoop,
}

type InnerMuCo = Arc<MuCo<Inner>>;

struct Inner {
    window_ops_opt: Option<VecDeque<WindowOp>>,
    event_infos_opt: Option<VecDeque<EventInfo>>,
    callbacks_opt: Option<VecDeque<Box<dyn FnOnce() + Send + 'static>>>,
}

impl Inner {
    fn window_ops(&mut self) -> &mut VecDeque<WindowOp> {
        self.window_ops_opt.get_or_insert_default()
    }
}

enum WindowOp {
    Create(WindowOpCreate),
    Drop(WindowId),
}

struct WindowOpCreate {
    window_id: WindowId,
    width: u32,
    height: u32,
    func: Box<dyn FnOnce() -> Box<dyn IntComponentHandle + 'static> + Send + 'static>,
    texture: Texture,
    weak_tx: Sender<Box<dyn Any + Send>>,
}

struct EventInfo {
    window_id: WindowId,
    event: UIEvent,
}

type WindowId = usize;

impl UIManager {
    pub fn new(queue: Queue) -> Self {
        let inner = Inner {
            window_ops_opt: None,
            event_infos_opt: None,
            callbacks_opt: None,
        };
        let inner_muco = Arc::new(MuCo::new(inner));

        let ui_loop = UILoop::new(Arc::clone(&inner_muco));

        thread::spawn({
            let inner_muco = Arc::clone(&inner_muco);

            move || {
                // Spawn thread to run slint event loop, so UI rendering is not going to block
                // the main render thread.
                
                let platform = UIPlatform::new(inner_muco, queue);
                platform::set_platform(Box::new(platform)).expect("Unable to set platform");
                slint::run_event_loop().expect("Unable to run event loop");
            }
        });

        Self {
            inner_muco,
            next_window_id: Cell::new(0),
            ui_loop
        }
    }

    pub fn create_window<F: FnOnce() -> C + Send + 'static, C: ComponentHandle + 'static>(&self, width: u32, height: u32, func: F, texture: Texture) -> UIWindow {
        // Schedule func to run on the slint event loop.

        let window_id = self.next_window_id.get();
        self.next_window_id.set(window_id + 1);

        let (weak_tx, weak_rx) = oneshot::channel();

        let window_op = WindowOp::Create(WindowOpCreate {
            window_id,
            width,
            height,
            func: Box::new(move || Box::new(func())),
            texture,
            weak_tx,
        });

        {
            let mut inner = self.inner_muco.mutex.lock().unwrap();

            let window_ops = inner.window_ops();
            window_ops.push_back(window_op);

            self.inner_muco.cond.notify_all();
        }

        let weak = weak_rx.recv().expect("Unable to receive");

        UIWindow::new(Arc::clone(&self.inner_muco), window_id, weak)
    }

    pub fn get_ui_loop(&self) -> &UILoop {
        &self.ui_loop
    }
}

pub struct UIWindow {
    inner_muco: InnerMuCo,
    window_id: WindowId,
    weak: Box<dyn Any>,
}

impl UIWindow {
    fn new(inner_muco: InnerMuCo, window_id: WindowId, weak: Box<dyn Any>) -> Self {
        Self {
            inner_muco,
            window_id,
            weak,
        }
    }

    pub fn as_weak<C: ComponentHandle + 'static>(&self) -> Weak<C> {
        // At the moment, UIWindow is not aware of the proper slint window type,
        // so we are determining type based on return value.
        // TODO: improve this?

        self.weak.downcast_ref::<Weak<C>>().expect("Invalid type").clone()
    }

    pub fn handle_event(&self, event: UIEvent) {
        let mut inner = self.inner_muco.mutex.lock().unwrap();

        let event_infos = inner.event_infos_opt.get_or_insert_default();

        // Coalesce events (reduce number of events).

        let last_event_info_opt = event_infos.back_mut();
        let mut coalesce = false;
        
        if let Some(last_event_info) = &last_event_info_opt && last_event_info.window_id == self.window_id {
            let last_event = &last_event_info.event;
            coalesce = matches!(last_event, UIEvent::PointerExit);

            if !coalesce {
                coalesce = match event {
                    UIEvent::PointerMove(_, _) => matches!(last_event, UIEvent::PointerMove(_, _)),
                    UIEvent::PointerPress(_, _) => matches!(last_event, UIEvent::PointerPress(_, _)),
                    UIEvent::PointerScroll(_, _, _, _) => matches!(last_event, UIEvent::PointerScroll(_, _, _, _)),
                    UIEvent::PointerExit => false,
                };
            }
        }

        let event_info = EventInfo {
            window_id: self.window_id,
            event,
        };

        if coalesce {
            let last_event_info = last_event_info_opt.unwrap();
            *last_event_info = event_info;
        } else {
            event_infos.push_back(event_info);
        }

        self.inner_muco.cond.notify_all();
    }
}

impl Drop for UIWindow {
    fn drop(&mut self) {
        let window_op = WindowOp::Drop(self.window_id);

        let mut inner = self.inner_muco.mutex.lock().unwrap();

        let window_ops = inner.window_ops();
        window_ops.push_back(window_op);

        self.inner_muco.cond.notify_all();
    }
}

#[allow(clippy::enum_variant_names)]
pub enum UIEvent { // TODO: Instead of (f32...) we can use structs.
    PointerMove(f32, f32),
    PointerPress(f32, f32),
    PointerScroll(f32, f32, f32, f32),
    PointerExit,
}

struct UIPlatform {
    inner_muco: InnerMuCo,
    queue: Queue,
    current_soft_window: RefCell<Option<Rc<MinimalSoftwareWindow>>>,
}

struct WindowInfo {
    _handle: Box<dyn IntComponentHandle>, // To keep window alive.
    soft_window: Rc<MinimalSoftwareWindow>,
    width: u32,
    height: u32,
    buf: Box<[Rgba]>,
    texture: Texture,
}

impl UIPlatform {
    fn new(inner_muco: InnerMuCo, queue: Queue) -> Self {
        Self {
            inner_muco,
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

        // TODO: Pause loop when the app is not visible?
        // TODO: Virtual keyboard is still rendered even if it is not visible (blinking cursor)
        loop {
            let dur_opt = platform::duration_until_next_timer_update();

            let (window_ops_opt, event_infos_opt, callbacks_opt) = {
                // Option is utilized to pass inner data quickly (without too much copying).

                let inner = self.inner_muco.mutex.lock().unwrap();
                let check = |inner: &mut Inner| inner.window_ops_opt.is_none() && inner.event_infos_opt.is_none() && inner.callbacks_opt.is_none();

                let mut inner = if let Some(dur) = dur_opt {
                    self.inner_muco.cond.wait_timeout_while(inner, dur, check).unwrap().0
                } else {
                    self.inner_muco.cond.wait_while(inner, check).unwrap()
                };

                (inner.window_ops_opt.take(), inner.event_infos_opt.take(), inner.callbacks_opt.take())
            };

            // Run callbacks.

            if let Some(callbacks) = callbacks_opt {
                for callback in callbacks {
                    callback();
                }
            }

            platform::update_timers_and_animations();

            // Create/drop windows.

            if let Some(window_ops) = window_ops_opt {
                for window_op in window_ops {
                    match window_op {
                        WindowOp::Create(window_op_create) => {
                            let handle = (window_op_create.func)(); // This will call create_window_adapter().
                            let weak = handle.as_weak();

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

                            window_op_create.weak_tx.send(weak).expect("Unable to send");
                        },
                        WindowOp::Drop(window_id) => {
                            window_infos.remove(&window_id);
                        },
                    }
                }
            }

            // Handle events.

            if let Some(event_infos) = event_infos_opt {
                for event_info in event_infos {
                    if let Some(window_info) = window_infos.get(&event_info.window_id) { // Test if the window is still exist.
                        let soft_window = &window_info.soft_window;

                        let calc_pos = |x, y| LogicalPosition {
                            x: x * window_info.width as f32,
                            y: y * window_info.height as f32,
                        };

                        match event_info.event {
                            UIEvent::PointerMove(x, y) => {
                                let pos = calc_pos(x, y);

                                soft_window.dispatch_event(WindowEvent::PointerMoved {
                                    position: pos,
                                });
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
                            },
                            UIEvent::PointerScroll(x, y, scroll_x, scroll_y) => {
                                let pos = calc_pos(x, y);

                                soft_window.dispatch_event(WindowEvent::PointerScrolled {
                                    position: pos,
                                    delta_x: scroll_x,
                                    delta_y: scroll_y,
                                });
                            },
                            UIEvent::PointerExit => {
                                soft_window.dispatch_event(WindowEvent::PointerExited);
                            },
                        }
                    }
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

#[derive(Clone)]
pub struct UILoop {
    inner_muco: InnerMuCo,
}

impl UILoop {
    fn new(inner_muco: InnerMuCo) -> Self {
        Self {
            inner_muco,
        }
    }

    pub fn add_callback<T: FnOnce() + Send + 'static>(&self, func: T) {
        // Prefer add_callback() to slint::invoke_from_event_loop(), to hide slint
        // implementation details.

        let mut inner = self.inner_muco.mutex.lock().unwrap();

        let callbacks = inner.callbacks_opt.get_or_insert_default();
        callbacks.push_back(Box::new(func));

        self.inner_muco.cond.notify_all();
    }
}

impl NetManagerRunner for UILoop {
    fn exec_done<T: FnOnce() + Send + 'static>(&self, func: T) {
        self.add_callback(func);
    }
}

trait IntComponentHandle {
    fn as_weak(&self) -> Box<dyn Any + Send>;
}

impl<C: ComponentHandle + 'static> IntComponentHandle for C {
    fn as_weak(&self) -> Box<dyn Any + Send> {
        Box::new(self.as_weak())
    }
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
