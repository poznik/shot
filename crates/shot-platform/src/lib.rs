//! Platform layer of Shot: the thin part that differs between Windows and macOS.
//!
//! It hands pixels and events to `shot-core` and shows the buffers `shot-core` draws: screen
//! capture, window list, monitors and DPI, global hotkey, tray / menu bar, clipboard, autostart
//! and the special windows (overlay, preview). SPEC §5.
//!
//! [`sys`] is the module of the OS being built for: both have the same names for the same things.

#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(windows)]
pub mod win;

#[cfg(target_os = "macos")]
pub use mac as sys;
#[cfg(windows)]
pub use win as sys;
