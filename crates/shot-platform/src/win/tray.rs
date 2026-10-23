//! The tray icon and its menu (SPEC §3.5, `docs/design/settings.md`, "Tray menu"), and the lock
//! that keeps Shot a single process.
//!
//! A hidden top-level window owns the icon: it gets the icon's clicks and the shell's broadcasts
//! (a message-only window would miss those), and a second launch finds the first by its class.
//! The window procedure only posts messages to itself; the app reads them with [`Tray::event`],
//! as it reads the overlay's.

use std::sync::atomic::{AtomicU32, Ordering};

use shot_core::geom::Point;
use shot_core::tray::{Command, Item};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, FindWindowW,
    GetSystemMetrics, GetWindowThreadProcessId, HICON, KillTimer, LR_DEFAULTCOLOR, MF_CHECKED,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, RegisterClassW, RegisterWindowMessageW,
    SM_MENUDROPALIGNMENT, SetForegroundWindow, SetMenuDefaultItem, SetTimer, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTALIGN, TPM_RIGHTBUTTON, TrackPopupMenuEx,
    WM_APP, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_NULL, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{HSTRING, PCSTR, PCWSTR, w};

const CLASS: PCWSTR = w!("ShotTray");
const ICON_ID: u32 = 1;
/// The shell's callback for the icon; it is sent, not posted, so the window procedure posts one
/// of the messages below instead.
const WM_TRAY_ICON: u32 = WM_APP + 0x20;
/// A left click on the icon, or `Enter` on it.
const WM_TRAY_SELECT: u32 = WM_APP + 0x21;
/// The menu was asked for; `wParam` holds the anchor point.
const WM_TRAY_MENU: u32 = WM_APP + 0x22;
/// The taskbar was recreated, its theme or the displays changed: add or redraw the icon.
const WM_TRAY_REFRESH: u32 = WM_APP + 0x23;
/// Posted by a second launch of Shot.
const WM_TRAY_WAKE: u32 = WM_APP + 0x24;
/// `NIN_SELECT | NINF_KEY`.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
/// Delays a capture chosen in the menu until the menu has faded out (`Tray::menu`).
const TIMER_CAPTURE: usize = 1;
const MENU_FADE_MS: u32 = 250;

/// `RegisterWindowMessageW("TaskbarCreated")`: Explorer restarted, icons must be added again.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// The tray PNGs of one look at each size the tray uses (`docs/design/icons.md`).
macro_rules! icon_set {
    ($dir:literal, $name:literal) => {
        [
            (
                16,
                include_bytes!(concat!(
                    "../../../../assets/tray/windows/",
                    $dir,
                    "/",
                    $name,
                    "-16.png"
                )) as &[u8],
            ),
            (
                20,
                include_bytes!(concat!(
                    "../../../../assets/tray/windows/",
                    $dir,
                    "/",
                    $name,
                    "-20.png"
                )) as &[u8],
            ),
            (
                24,
                include_bytes!(concat!(
                    "../../../../assets/tray/windows/",
                    $dir,
                    "/",
                    $name,
                    "-24.png"
                )) as &[u8],
            ),
            (
                32,
                include_bytes!(concat!(
                    "../../../../assets/tray/windows/",
                    $dir,
                    "/",
                    $name,
                    "-32.png"
                )) as &[u8],
            ),
            (
                40,
                include_bytes!(concat!(
                    "../../../../assets/tray/windows/",
                    $dir,
                    "/",
                    $name,
                    "-40.png"
                )) as &[u8],
            ),
            (
                48,
                include_bytes!(concat!(
                    "../../../../assets/tray/windows/",
                    $dir,
                    "/",
                    $name,
                    "-48.png"
                )) as &[u8],
            ),
        ]
    };
}

type IconSet = [(i32, &'static [u8]); 6];
const LIGHT: IconSet = icon_set!("light", "tray");
const LIGHT_ALERT: IconSet = icon_set!("light", "tray-alert");
const DARK: IconSet = icon_set!("dark", "tray");
const DARK_ALERT: IconSet = icon_set!("dark", "tray-alert");

pub enum TrayEvent {
    /// A left click on the icon or `Enter` on it, or `Снять` chosen in the menu a moment ago.
    Capture,
    /// A right click: show the menu with [`Tray::menu`] at this point.
    Menu(Point),
    /// Shot was launched again.
    Wake,
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    tooltip: String,
    alert: bool,
}

impl Tray {
    pub fn new(tooltip: &str, alert: bool) -> windows::core::Result<Self> {
        unsafe {
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(w!("TaskbarCreated")),
                Ordering::Relaxed,
            );
            let instance = GetModuleHandleW(None)?.into();
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CLASS,
                w!("Shot"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?;
            let mut tray = Tray {
                hwnd,
                icon: HICON::default(),
                tooltip: tooltip.to_string(),
                alert,
            };
            tray.add();
            Ok(tray)
        }
    }

    /// Updates the tooltip and the alert look, if they changed.
    pub fn set(&mut self, tooltip: &str, alert: bool) {
        if self.tooltip != tooltip || self.alert != alert {
            self.tooltip = tooltip.to_string();
            self.alert = alert;
            self.load_icon();
            self.notify(NIM_MODIFY);
        }
    }

    /// The tray input a message carries, if it is one of the tray window's.
    pub fn event(&mut self, msg: &MSG) -> Option<TrayEvent> {
        if msg.hwnd != self.hwnd {
            return None;
        }
        match msg.message {
            WM_TRAY_SELECT => Some(TrayEvent::Capture),
            WM_TRAY_MENU => {
                let (x, y) = (msg.wParam.0 as i16, (msg.wParam.0 >> 16) as i16);
                Some(TrayEvent::Menu(Point::new(x.into(), y.into())))
            }
            WM_TRAY_WAKE => Some(TrayEvent::Wake),
            WM_TRAY_REFRESH => {
                self.add();
                None
            }
            WM_TIMER if msg.wParam.0 == TIMER_CAPTURE => {
                let _ = unsafe { KillTimer(Some(self.hwnd), TIMER_CAPTURE) };
                Some(TrayEvent::Capture)
            }
            _ => None,
        }
    }

    /// Shows the menu at `at` and returns the chosen command. `Снять` comes back later as
    /// [`TrayEvent::Capture`] instead: the menu fades out for a moment after the click, and the
    /// capture must not catch it.
    pub fn menu(&self, items: &[Item], at: Point) -> Option<Command> {
        let mut commands = Vec::new();
        let chosen = unsafe {
            let menu = CreatePopupMenu().ok()?;
            for item in items {
                match item {
                    Item::Separator => {
                        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                    }
                    Item::Command {
                        command,
                        label,
                        shortcut,
                        checked,
                        enabled,
                        default,
                    } => {
                        commands.push(*command);
                        let id = commands.len();
                        let text = match shortcut {
                            Some(s) => format!("{label}\t{s}"),
                            None => label.to_string(),
                        };
                        let mut flags = MF_STRING;
                        if *checked {
                            flags |= MF_CHECKED;
                        }
                        if !*enabled {
                            flags |= MF_GRAYED;
                        }
                        let _ = AppendMenuW(menu, flags, id, &HSTRING::from(text));
                        if *default {
                            let _ = SetMenuDefaultItem(menu, id as u32, 0);
                        }
                    }
                }
            }
            // The menu closes on a click elsewhere only if its window is in the foreground.
            let _ = SetForegroundWindow(self.hwnd);
            let align = if GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0 {
                TPM_RIGHTALIGN
            } else {
                TPM_LEFTALIGN
            };
            let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | align;
            let id = TrackPopupMenuEx(menu, flags.0, at.x, at.y, self.hwnd, None);
            let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            commands.get((id.0 as usize).checked_sub(1)?).copied()
        };
        if chosen == Some(Command::Capture) {
            unsafe { SetTimer(Some(self.hwnd), TIMER_CAPTURE, MENU_FADE_MS, None) };
            return None;
        }
        chosen
    }

    /// Adds the icon to the notification area, again after Explorer restarted.
    fn add(&mut self) {
        self.load_icon();
        if !self.notify(NIM_MODIFY) && self.notify(NIM_ADD) {
            let mut data = self.data();
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) };
        }
    }

    /// Loads the icon for the taskbar's theme and scale.
    fn load_icon(&mut self) {
        let set = match (taskbar_is_light(), self.alert) {
            (true, false) => &LIGHT,
            (true, true) => &LIGHT_ALERT,
            (false, false) => &DARK,
            (false, true) => &DARK_ALERT,
        };
        let size = 16 * taskbar_dpi() as i32 / 96;
        let &(px, png) = set
            .iter()
            .find(|(px, _)| *px >= size)
            .unwrap_or(&set[set.len() - 1]);
        if let Ok(icon) =
            unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, px, px, LR_DEFAULTCOLOR) }
        {
            if !self.icon.is_invalid() {
                let _ = unsafe { DestroyIcon(self.icon) };
            }
            self.icon = icon;
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY_ICON,
            hIcon: self.icon,
            ..Default::default()
        };
        for (slot, c) in data
            .szTip
            .iter_mut()
            .zip(self.tooltip.encode_utf16().take(127))
        {
            *slot = c;
        }
        data
    }

    fn notify(&self, message: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE) -> bool {
        unsafe { Shell_NotifyIconW(message, &self.data()) }.as_bool()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.notify(NIM_DELETE);
        unsafe {
            let _ = DestroyIcon(self.icon);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Whether the taskbar is light: `SystemUsesLightTheme`, the taskbar's own setting.
fn taskbar_is_light() -> bool {
    super::registry::get_dword(
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("SystemUsesLightTheme"),
    ) == Some(1)
}

/// The DPI of the primary taskbar, which holds the notification area.
fn taskbar_dpi() -> u32 {
    unsafe {
        FindWindowW(w!("Shell_TrayWnd"), None)
            .map(|taskbar| GetDpiForWindow(taskbar))
            .ok()
            .filter(|&dpi| dpi > 0)
            .unwrap_or(96)
    }
}

/// Makes native menus follow the theme: `Some(true)` dark, `Some(false)` light, `None` the OS
/// apps' theme. Uses uxtheme's unnamed exports (Windows 10 1903 and later), the only way to get
/// dark Win32 menus; on older systems the menus stay light.
pub fn set_menu_theme(dark: Option<bool>) {
    unsafe {
        let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) else {
            return;
        };
        // 135: SetPreferredAppMode(Default, AllowDark, ForceDark, ForceLight); 136: FlushMenuThemes.
        let set_mode = GetProcAddress(uxtheme, PCSTR(135 as *const u8));
        let flush = GetProcAddress(uxtheme, PCSTR(136 as *const u8));
        if let (Some(set_mode), Some(flush)) = (set_mode, flush) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            set_mode(match dark {
                None => 1,
                Some(true) => 2,
                Some(false) => 3,
            });
            flush();
        }
    }
}

/// The per-session lock that keeps Shot a single process; released when dropped.
pub struct InstanceLock(HANDLE);

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Takes the lock, or, when another Shot holds it, asks that one to open its settings (SPEC §3.5)
/// and returns `None`.
pub fn claim_instance() -> Option<InstanceLock> {
    let lock = unsafe { CreateMutexW(None, false, w!("Local\\Shot.Instance")) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    match lock {
        Ok(handle) if !already => Some(InstanceLock(handle)),
        Ok(handle) => {
            unsafe {
                let _ = CloseHandle(handle);
                if let Ok(first) = FindWindowW(CLASS, None) {
                    // Let the first Shot bring its settings window to the front.
                    let mut pid = 0;
                    GetWindowThreadProcessId(first, Some(&mut pid));
                    let _ = AllowSetForegroundWindow(pid);
                    let _ = PostMessageW(Some(first), WM_TRAY_WAKE, WPARAM(0), LPARAM(0));
                }
            }
            None
        }
        // No lock at all: better two copies than none.
        Err(_) => Some(InstanceLock(HANDLE::default())),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let post = |m: u32, wp: WPARAM| {
        let _ = unsafe { PostMessageW(Some(hwnd), m, wp, LPARAM(0)) };
        LRESULT(0)
    };
    match msg {
        WM_TRAY_ICON => match lp.0 as u32 & 0xFFFF {
            NIN_SELECT | NIN_KEYSELECT => post(WM_TRAY_SELECT, WPARAM(0)),
            // Version 4 puts the anchor point in `wParam`.
            WM_CONTEXTMENU => post(WM_TRAY_MENU, wp),
            _ => LRESULT(0),
        },
        WM_SETTINGCHANGE | WM_DISPLAYCHANGE => {
            post(WM_TRAY_REFRESH, WPARAM(0));
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            post(WM_TRAY_REFRESH, WPARAM(0))
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
