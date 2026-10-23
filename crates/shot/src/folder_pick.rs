//! The folder dialog of the settings window.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use shot_platform::sys::{self, FormRef, Notifier};

use crate::log::log;

const FOLDER_TITLE: &str = "Папка для снимков";
/// `shot --pick-folder <owner> <start>` shows the folder dialog and prints the folder chosen.
pub const PICK_FOLDER: &str = "--pick-folder";

/// The folder dialog, shown by a second `shot`: what it loads stays in the process that showed it
/// after it closes — ~6 MB of the shell on Windows (WIN-011), ~3 MB on macOS (MAC-006) — and
/// leaves with that one.
pub struct FolderPick {
    owner: FormRef,
    result: Receiver<Option<PathBuf>>,
}

impl FolderPick {
    /// Shows the dialog over `owner`, starting in `start`; `notifier` wakes the loop when it
    /// closes.
    pub fn start(owner: FormRef, start: &Path, notifier: Notifier) -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let child = Command::new(exe)
            .arg(PICK_FOLDER)
            .arg(owner.raw().to_string())
            .arg(start)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(e) => {
                log!("folder dialog: {e}");
                return None;
            }
        };
        let (tx, result) = mpsc::channel();
        std::thread::spawn(move || {
            let chosen = child
                .wait_with_output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|s| s.trim_end().to_owned())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let _ = tx.send(chosen);
            notifier.notify();
        });
        Some(FolderPick { owner, result })
    }

    /// The dialog closed: with the folder chosen, or `None`.
    pub fn done(&self) -> Option<Option<PathBuf>> {
        let chosen = match self.result.try_recv() {
            Ok(chosen) => chosen,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
        self.owner.enable();
        Some(chosen)
    }
}

/// `shot --pick-folder <owner> <start>`: the other end of [`FolderPick`].
pub fn pick_folder_child(args: &[OsString]) {
    #[cfg(windows)]
    let _ = sys::enable_per_monitor_dpi();
    let owner = args
        .first()
        .and_then(|a| a.to_str())
        .and_then(|a| a.parse().ok())
        .map(FormRef::from_raw);
    // A window of another process is open to this one on Windows only.
    #[cfg(windows)]
    let owner = owner.filter(FormRef::is_open);
    let start = args.get(1).map(OsStr::new).map(Path::new);
    if let Ok(Some(dir)) = sys::pick_folder(owner, FOLDER_TITLE, start) {
        let mut out = std::io::stdout();
        let _ = out.write_all(dir.to_string_lossy().as_bytes());
        let _ = out.flush();
    }
}
