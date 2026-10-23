//! Shot entry point: the event loop that wires `shot-core` to `shot-platform`.
//!
//! The tray or menu bar, the hotkey, the overlay, the preview cards and the editor; captures go to
//! the clipboard and PNG files as `settings.toml` says, with the time of each step in the log.

// Release builds on Windows are GUI apps without a console window; debug builds keep the console
// for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(any(windows, target_os = "macos"))]
mod app;
#[cfg(any(windows, target_os = "macos"))]
mod editor;
#[cfg(any(windows, target_os = "macos"))]
mod folder_pick;
#[cfg(any(windows, target_os = "macos"))]
mod log;
#[cfg(target_os = "macos")]
mod mac_main;
#[cfg(any(windows, target_os = "macos"))]
mod preview;
#[cfg(any(windows, target_os = "macos"))]
mod settings_ui;
#[cfg(windows)]
mod win_main;

#[cfg(target_os = "macos")]
#[global_allocator]
static ALLOCATOR: shot_platform::mac::Allocator = shot_platform::mac::Allocator;

fn main() {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|a| a == folder_pick::PICK_FOLDER) {
            return folder_pick::pick_folder_child(&args[1..]);
        }
    }
    #[cfg(windows)]
    win_main::run();
    #[cfg(target_os = "macos")]
    mac_main::run();
}
