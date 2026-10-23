//! macOS implementation, on AppKit through `objc2` (MAC-002). Coordinates are physical pixels of
//! one virtual desktop, as on Windows: a monitor's origin is its point origin times the largest
//! backing scale, its size is its point size times its own scale (see [`monitors`]). Everything
//! runs on the main thread except what says otherwise.

mod alloc;
mod autostart;
mod capture;
mod clipboard;
mod dev;
mod drag;
mod editor;
mod folder;
mod form;
mod hotkey;
mod instance;
mod monitors;
mod overlay;
mod permission;
mod preview;
mod pump;
mod system;
mod titled;
mod tray;
mod watch;
mod window_list;

use std::fmt;

pub use alloc::Allocator;
pub use autostart::{autostart, set_autostart};
pub use capture::{Bitmap, capture_rects};
pub use clipboard::Clipboard;
pub use editor::{EditorHandler, EditorNotifier, EditorRef, EditorWindow, WindowEvent};
pub use folder::pick_folder;
pub use form::{FormEvent, FormHandler, FormRef, FormWindow};
pub use hotkey::{code_of, key_of, register_hotkey, unregister_hotkey};
pub use instance::{InstanceLock, claim_instance};
pub use monitors::{Monitor, cursor_pos, monitors};
pub use overlay::{OverlayWindows, wait_for_compose};
pub use permission::{ask_screen_permission, screen_permission};
pub use preview::{Notifier, PreviewEvent, PreviewWindows, Sprite};
pub use pump::{Event, init, next_event};
pub use system::{
    apps_use_dark_theme, config_dir, drag_threshold, local_time, log_dir, open_path, pictures_dir,
    quit, since_process_start,
};
pub use tray::{Tray, TrayEvent, set_menu_theme};
pub use watch::FolderWatch;
pub use window_list::window_rects;

/// What the calls that can fail return on error.
#[derive(Clone, Debug)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(what: impl Into<String>) -> Self {
        Error(what.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
