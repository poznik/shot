//! Process-wide setup, time and known folders.

use std::path::{Path, PathBuf};
use std::time::Duration;

use shot_core::naming::LocalTime;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTimePreciseAsFileTime};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Pictures, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT,
    SHGetKnownFolderPath, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, PostQuitMessage, SM_CXDRAG, SW_SHOWNORMAL,
};
use windows::core::{GUID, HSTRING, w};

/// Makes every coordinate and size the process sees physical pixels, on every monitor.
/// Fails harmlessly if the awareness was already set (by a manifest or by winit).
pub fn enable_per_monitor_dpi() -> windows::core::Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
}

/// How far the mouse moves with the button down before a click becomes a drag, in pixels.
pub fn drag_threshold() -> i32 {
    unsafe { GetSystemMetrics(SM_CXDRAG) }.max(1)
}

/// Time since the process was created, loading included.
pub fn since_process_start() -> Duration {
    let ticks = |t: FILETIME| (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64;
    let mut created = FILETIME::default();
    let mut unused = [FILETIME::default(); 3];
    let [exited, kernel, user] = &mut unused;
    unsafe {
        if GetProcessTimes(GetCurrentProcess(), &mut created, exited, kernel, user).is_err() {
            return Duration::ZERO;
        }
        let now = GetSystemTimePreciseAsFileTime();
        Duration::from_nanos(ticks(now).saturating_sub(ticks(created)) * 100)
    }
}

pub fn local_time() -> LocalTime {
    let t = unsafe { GetLocalTime() };
    LocalTime {
        year: t.wYear,
        month: t.wMonth as u8,
        day: t.wDay as u8,
        hour: t.wHour as u8,
        minute: t.wMinute as u8,
        second: t.wSecond as u8,
    }
}

/// The user's Pictures folder, wherever it is redirected (OneDrive, another drive).
pub fn pictures_dir() -> Option<PathBuf> {
    known_folder(&FOLDERID_Pictures)
}

/// `%LOCALAPPDATA%\Shot`: the log.
pub fn log_dir() -> Option<PathBuf> {
    Some(known_folder(&FOLDERID_LocalAppData)?.join("Shot"))
}

/// `%APPDATA%\Shot`: the settings, which roam with the user's profile.
pub fn config_dir() -> Option<PathBuf> {
    Some(known_folder(&FOLDERID_RoamingAppData)?.join("Shot"))
}

/// Opens a folder in Explorer, or a file in its program; a file type with no program opens in
/// Notepad.
pub fn open_path(path: &Path) -> windows::core::Result<()> {
    let shell = |file: &HSTRING, params: Option<&HSTRING>| {
        let code = unsafe {
            match params {
                Some(p) => ShellExecuteW(None, w!("open"), file, p, None, SW_SHOWNORMAL),
                None => ShellExecuteW(None, w!("open"), file, None, None, SW_SHOWNORMAL),
            }
        };
        // Codes up to 32 are errors.
        code.0 as usize > 32
    };
    let file = HSTRING::from(path);
    if shell(&file, None) || (path.is_file() && shell(&HSTRING::from("notepad.exe"), Some(&file))) {
        Ok(())
    } else {
        Err(windows::core::Error::from_thread())
    }
}

/// Ends the message loop: [`super::next_event`] returns `Wake::Quit`.
/// Apps use the dark theme: Settings → Personalization → Colors → app mode.
pub fn apps_use_dark_theme() -> bool {
    super::registry::get_dword(
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        w!("AppsUseLightTheme"),
    ) == Some(0)
}

pub fn quit() {
    unsafe { PostQuitMessage(0) };
}

/// Windows asks no permission to capture the screen.
pub fn screen_permission() -> bool {
    true
}

/// Nothing to ask for on Windows.
pub fn ask_screen_permission() {}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let result = path.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(path.0 as *const _));
        result
    }
}
