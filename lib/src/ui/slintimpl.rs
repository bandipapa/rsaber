use slint::{Global, SharedString};
use slint::platform::{Key, WindowEvent};

use crate::ui::WindowBaseConfig;

// Usage pattern:
// - Outside of crate::ui, only the slint builtins listed below are permitted.
// - Always use slintimpl namespace when referring these.
// - In the future, it can happen that we replace slint with an alternative
//   UI framework, and it is much easier to locate code which
//   needs a change.

pub use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel, Weak};

pub trait WindowUtil {
    // This trait contains useful functions to avoid bringing in too much slint
    // internals into consumers.

    fn set_input_enabled<'a>(&'a self, enabled: bool) where WindowBaseConfig<'a>: Global<'a, Self>, Self: Sized;
    fn handle_key<S: AsRef<str>>(&self, key: S);
    fn handle_key_end(&self);
}

impl<C: ComponentHandle> WindowUtil for C {
    fn set_input_enabled<'a>(&'a self, enabled: bool) where WindowBaseConfig<'a>: Global<'a, Self>, Self: Sized {
        let config = self.global::<WindowBaseConfig>();
        config.set_input_enabled(enabled);
    }

    fn handle_key<S: AsRef<str>>(&self, key: S) {
        handle_key_impl(self, key.as_ref().into());
    }

    fn handle_key_end(&self) {
        handle_key_impl(self, Key::End.into());
    }
}

fn handle_key_impl<C: ComponentHandle>(comp: &C, text: SharedString) {
    let window = comp.window();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
}
