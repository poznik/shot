//! Watches a folder for changes to the files in it: a thread waits on a kqueue and wakes the event
//! loop with [`Event::Folder`]. An editor that saves in place writes the file; one that saves
//! atomically renames a new file over it: the folder and each file in it are watched, and the
//! files again after each change.

use std::collections::HashMap;
use std::fs::File;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::Error;
use super::pump::{Event, push, register};

/// Lives as long as the app; its thread ends with the process.
pub struct FolderWatch {
    _key: isize,
}

impl FolderWatch {
    pub fn new(dir: &Path) -> Result<Self, Error> {
        let dir = dir.to_path_buf();
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return Err(Error::new(std::io::Error::last_os_error().to_string()));
        }
        let key = register(Rc::new(|| push(Event::Folder)));
        std::thread::Builder::new()
            .name("folder watch".into())
            .spawn(move || watch(queue, &dir, key))
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(FolderWatch { _key: key })
    }
}

const FLAGS: u32 = libc::NOTE_WRITE
    | libc::NOTE_EXTEND
    | libc::NOTE_DELETE
    | libc::NOTE_RENAME
    | libc::NOTE_ATTRIB;

fn add(queue: RawFd, fd: RawFd) {
    let change = libc::kevent {
        ident: fd as usize,
        filter: libc::EVFILT_VNODE,
        flags: libc::EV_ADD | libc::EV_CLEAR,
        fflags: FLAGS,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
}

/// Opens every file in `dir` that is not watched yet; closing a file drops its kevent.
fn open_files(queue: RawFd, dir: &Path, files: &mut HashMap<PathBuf, File>) {
    files.retain(|path, _| path.exists());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if files.contains_key(&path) || !path.is_file() {
            continue;
        }
        if let Some(file) = open_events(&path) {
            add(queue, file.as_raw_fd());
            files.insert(path, file);
        }
    }
}

/// Opens `path` for its events only: watching does not keep the volume from unmounting.
fn open_events(path: &Path) -> Option<File> {
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_EVTONLY)
        .open(path)
        .ok()
}

fn watch(queue: RawFd, dir: &Path, key: isize) {
    let _ = std::fs::create_dir_all(dir);
    let Some(folder) = open_events(dir) else {
        return;
    };
    add(queue, folder.as_raw_fd());
    let mut files = HashMap::new();
    open_files(queue, dir, &mut files);
    let mut events: [libc::kevent; 8] = unsafe { std::mem::zeroed() };
    loop {
        let n = unsafe {
            libc::kevent(
                queue,
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as i32,
                std::ptr::null(),
            )
        };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        // Files come and go with renames and deletes: let go of the old ones, open the new ones.
        let gone = |f: &File| {
            events[..n as usize].iter().any(|e| {
                e.ident == f.as_raw_fd() as usize
                    && e.fflags & (libc::NOTE_DELETE | libc::NOTE_RENAME) != 0
            })
        };
        files.retain(|_, f| !gone(f));
        open_files(queue, dir, &mut files);
        super::pump::post(key);
    }
}
