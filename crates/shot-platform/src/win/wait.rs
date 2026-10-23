//! The main loop's wait: messages or a change in a watched folder, whichever comes first. Nothing
//! runs while neither comes (SPEC §5.4, item 7).

use std::path::Path;

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FindCloseChangeNotification,
    FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
};
use windows::core::HSTRING;

/// Signals when a file in a folder is written, created, renamed or deleted.
pub struct FolderWatch(HANDLE);

impl FolderWatch {
    pub fn new(dir: &Path) -> windows::core::Result<Self> {
        let handle = unsafe {
            FindFirstChangeNotificationW(
                &HSTRING::from(dir),
                false,
                FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME,
            )
        }?;
        Ok(FolderWatch(handle))
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        let _ = unsafe { FindCloseChangeNotification(self.0) };
    }
}

pub enum Wake {
    /// A message is in `msg`; dispatch it and call again.
    Message,
    /// The folder changed; the watch is armed again.
    Folder,
    /// `WM_QUIT`.
    Quit,
}

/// Takes the next queued message, or waits for one or for the folder to change.
pub fn next_event(msg: &mut MSG, folder: Option<&FolderWatch>) -> Wake {
    loop {
        if unsafe { PeekMessageW(msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            return if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                Wake::Quit
            } else {
                Wake::Message
            };
        }
        let handles: Vec<HANDLE> = folder.iter().map(|f| f.0).collect();
        let woke = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&handles), u32::MAX, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if let Some(f) = folder
            && woke == WAIT_OBJECT_0
        {
            let _ = unsafe { FindNextChangeNotification(f.0) };
            return Wake::Folder;
        }
    }
}
