//! Minimal log: one line per event, to a file next to the settings and to stderr in debug builds.
//! Release builds have no console, so the file is where timings are read from.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

static FILE: Mutex<Option<File>> = Mutex::new(None);

/// Opens (appends to) the log file. Without it, lines still go to stderr.
pub fn init(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = file;
}

pub fn line(text: &str) {
    if cfg!(debug_assertions) {
        eprintln!("{text}");
    }
    if let Some(file) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = writeln!(file, "{text}");
    }
}

macro_rules! log {
    ($($arg:tt)*) => { $crate::log::line(&format!($($arg)*)) };
}
pub(crate) use log;
