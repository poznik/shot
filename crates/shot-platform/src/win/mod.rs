//! Windows implementation. All coordinates are physical pixels: call [`enable_per_monitor_dpi`]
//! before anything else.

mod autostart;
mod capture;
mod clipboard;
mod dialog;
mod drag;
mod editor;
mod form;
mod hotkey;
mod monitors;
mod overlay;
mod preview;
mod registry;
mod system;
mod tray;
mod wait;
mod window_list;

pub use autostart::{autostart, set_autostart};
pub use capture::{Bitmap, capture_rects};
pub use clipboard::Clipboard;
pub use dialog::pick_folder;
pub use editor::{EditorHandler, EditorNotifier, EditorRef, EditorWindow, WindowEvent};
pub use form::{FormEvent, FormHandler, FormRef, FormWindow};
pub use hotkey::{key_of, register_hotkey, unregister_hotkey, vk_of};
pub use monitors::{Monitor, cursor_pos, monitors};
pub use overlay::{OverlayWindows, wait_for_compose};
pub use preview::{Notifier, PreviewEvent, PreviewWindows, Sprite};
pub use system::{
    apps_use_dark_theme, ask_screen_permission, config_dir, drag_threshold, enable_per_monitor_dpi,
    local_time, log_dir, open_path, pictures_dir, quit, screen_permission, since_process_start,
};

/// What the calls that can fail return on error.
pub type Error = windows::core::Error;
pub use tray::{InstanceLock, Tray, TrayEvent, claim_instance, set_menu_theme};
pub use wait::{FolderWatch, Wake, next_event};
pub use window_list::window_rects;
