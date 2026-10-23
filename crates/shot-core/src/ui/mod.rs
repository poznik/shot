//! Shot's small windows: the settings, the first-run welcome and, on macOS, the Screen Recording
//! window (`docs/design/settings.md`). As with the editor, each is a model here — it takes
//! [`Input`], draws itself and says what the app has to do — shown in a plain window on each OS.

mod form;
pub mod permission;
pub mod settings;
pub mod welcome;
mod widgets;

use crate::hotkey;

pub use permission::Permission;
pub use settings::SettingsWindow;
pub use welcome::Welcome;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Move {
        x: f32,
        y: f32,
    },
    Down {
        x: f32,
        y: f32,
    },
    Up {
        x: f32,
        y: f32,
    },
    /// The cursor left the window.
    Leave,
    /// A key went down with `mods` held.
    Key {
        key: Key,
        mods: Mods,
    },
    /// A key went up; `mods` are the ones still held.
    KeyUp {
        mods: Mods,
    },
    /// The window lost the keyboard.
    Blur,
    /// The close button or `Alt+F4`.
    Close,
    Resize {
        width: u32,
        height: u32,
        scale: f32,
    },
    Theme {
        dark: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A key a hotkey can use; `Tab`, `Space`, `Enter` and the arrows also work the form.
    Hot(hotkey::Key),
    Escape,
    /// A modifier went down: see `mods`.
    Modifier,
}

/// The modifiers held; `meta` is `Win` on Windows and `⌘` on macOS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Mods {
    /// Only `Shift`, or nothing: the keys that move through a form.
    fn plain(self) -> bool {
        !self.ctrl && !self.alt && !self.meta
    }

    fn array(self) -> [bool; 4] {
        [self.ctrl, self.alt, self.shift, self.meta]
    }
}
