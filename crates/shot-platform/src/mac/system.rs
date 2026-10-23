//! Small things the app asks of the OS: folders, time, the theme.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL, NSUserDefaults, ns_string};
use shot_core::naming::LocalTime;

use super::Error;

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `~/Pictures`.
pub fn pictures_dir() -> Option<PathBuf> {
    Some(home()?.join("Pictures"))
}

/// `~/Library/Logs/Shot`: the log.
pub fn log_dir() -> Option<PathBuf> {
    Some(home()?.join("Library/Logs/Shot"))
}

/// `~/Library/Application Support/Shot`: the settings.
pub fn config_dir() -> Option<PathBuf> {
    Some(home()?.join("Library/Application Support/Shot"))
}

/// How far the mouse moves with the button down before it is a drag, in pixels: 3 points on
/// Retina, as AppKit's own drags.
pub fn drag_threshold() -> i32 {
    6
}

/// The time since the process started.
pub fn since_process_start() -> Duration {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as i32;
    let got = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    if got != size {
        return Duration::ZERO;
    }
    let start = UNIX_EPOCH
        + Duration::new(info.pbi_start_tvsec, 0)
        + Duration::from_micros(info.pbi_start_tvusec);
    SystemTime::now().duration_since(start).unwrap_or_default()
}

pub fn local_time() -> LocalTime {
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    unsafe { libc::localtime_r(&now, &mut t) };
    LocalTime {
        year: (t.tm_year + 1900) as u16,
        month: (t.tm_mon + 1) as u8,
        day: t.tm_mday as u8,
        hour: t.tm_hour as u8,
        minute: t.tm_min as u8,
        second: t.tm_sec as u8,
    }
}

/// Opens a folder in Finder, or a file in its program.
pub fn open_path(path: &Path) -> Result<(), Error> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(Error::new("Finder did not open it"))
    }
}

/// The system appearance is dark, whatever Shot asked of its own windows.
pub fn apps_use_dark_theme() -> bool {
    NSUserDefaults::standardUserDefaults()
        .stringForKey(ns_string!("AppleInterfaceStyle"))
        .is_some_and(|s| s.to_string() == "Dark")
}

/// Ends the event loop.
pub fn quit() {
    super::pump::quit();
}
