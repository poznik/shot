//! The Screen Recording permission: ScreenCaptureKit captures nothing without it. The user gives
//! it in the system settings; macOS may then ask to restart Shot for it to take effect.
//!
//! With `SHOT_DEV` set the permission is make-believe, never asked of the system:
//! `SHOT_DEV_NO_PERMISSION` starts Shot without it, and the commands `permission on` and
//! `permission off` change it as the system settings would.

use std::sync::atomic::{AtomicBool, Ordering};

use objc2_app_kit::NSWorkspace;
use objc2_core_graphics::CGRequestScreenCaptureAccess;
use objc2_foundation::{NSString, NSURL};

use super::capture;

/// The section of the system settings with the permission, on macOS 13 and later.
const SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

/// The system was asked once: Shot is in its list since.
static ASKED: AtomicBool = AtomicBool::new(false);
/// What the permission is with `SHOT_DEV`.
static DEV_GRANTED: AtomicBool = AtomicBool::new(true);

/// Whether Shot may capture the screen now; checking asks the user nothing.
pub fn screen_permission() -> bool {
    if capture::dev() {
        return DEV_GRANTED.load(Ordering::Relaxed);
    }
    capture::may_capture()
}

/// Opens the system settings where the permission is given. The first time it also asks the
/// system for it, so that Shot is in the list there.
pub fn ask_screen_permission() {
    if capture::dev() {
        eprintln!("dev: the system settings would open");
        return;
    }
    if !ASKED.swap(true, Ordering::Relaxed) {
        CGRequestScreenCaptureAccess();
    }
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(SETTINGS_URL)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// With `SHOT_DEV`: Shot has the permission or not, as from now.
pub(crate) fn dev_grant(granted: bool) {
    DEV_GRANTED.store(granted, Ordering::Relaxed);
}
