//! The editor window (SPEC §3.3, `docs/design/editor.md`, "Window"): an ordinary top-level window
//! showing what `shot-core` draws. It keeps a back buffer the size of its client area: the
//! handler draws the changed parts into it and puts them on screen, and `WM_PAINT` copies from it,
//! also while the window is being moved or sized.
//!
//! The window procedure turns messages into [`WindowEvent`]s for the [`EditorHandler`] that owns
//! the editor. An event that arrives while the handler runs — a message sent by a call it made —
//! waits until it returns. The handler is dropped with the window.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::{addr_of, addr_of_mut};
use std::rc::Rc;
use std::sync::Once;
use std::time::Duration;

use shot_core::editor::objects::Axis;
use shot_core::editor::{Cursor, Input, Key};
use shot_core::geom::Rect;
use shot_core::image::BgrxMut;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, EndPaint, GetDC, InvalidateRect, PAINTSTRUCT, ReleaseDC, SRCCOPY,
    ScreenToClient,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_NUMPAD0,
    VK_NUMPAD9, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow,
    GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, HCURSOR, HICON, HTCLIENT, ICON_BIG,
    ICON_SMALL, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS,
    IDC_SIZENWSE, IDC_SIZEWE, IsIconic, IsWindow, KillTimer, LR_DEFAULTCOLOR, LoadCursorW,
    MINMAXINFO, PostMessageW, RegisterClassW, SIZE_MINIMIZED, SW_RESTORE, SW_SHOWNORMAL,
    SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetCursor, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_APP,
    WM_CAPTURECHANGED, WM_CLOSE, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN,
    WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SETICON, WM_SETTINGCHANGE, WM_SIZE,
    WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::Bitmap;
use super::system::apps_use_dark_theme;

const CLASS: PCWSTR = w!("ShotEditor");
/// Posted by [`EditorNotifier::notify`], from any thread.
const WM_EDITOR_NOTIFY: u32 = WM_APP + 0x40;
/// Posted by [`EditorRef::set_theme`]: `wParam` bit 0 is dark, bit 1 follows the system.
const WM_EDITOR_THEME: u32 = WM_APP + 0x41;
/// Posted to look at the system theme again after a settings change arrived mid-event.
const WM_EDITOR_SYSTEM_THEME: u32 = WM_APP + 0x42;
/// windows-rs keeps it under `UI::Controls`, a feature Shot does not need otherwise.
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_ID: usize = 1;
/// The content never gets smaller than this, logical px: the toolbar fits at 960.
const MIN_SIZE: (f32, f32) = (960.0, 600.0);
/// How far a notch of the wheel scrolls, logical px.
const WHEEL_SCROLL: f32 = 48.0;
const WHEEL_DELTA: i32 = 120;

/// The app icon at the sizes a title bar and the taskbar use.
const ICONS: [(i32, &[u8]); 6] = [
    (
        16,
        include_bytes!("../../../../assets/app/windows/app-16.png"),
    ),
    (
        20,
        include_bytes!("../../../../assets/app/windows/app-20.png"),
    ),
    (
        24,
        include_bytes!("../../../../assets/app/windows/app-24.png"),
    ),
    (
        32,
        include_bytes!("../../../../assets/app/windows/app-32.png"),
    ),
    (
        40,
        include_bytes!("../../../../assets/app/windows/app-40.png"),
    ),
    (
        48,
        include_bytes!("../../../../assets/app/windows/app-48.png"),
    ),
];

pub enum WindowEvent {
    Input(Input),
    /// The time set with [`EditorWindow::set_timer`] has come.
    Timer,
    /// [`EditorNotifier::notify`] was called.
    Notify,
}

/// Owns the editor; gets every event of its window. After [`Input::Resize`] — also the first
/// event — the back buffer is new and has to be drawn whole.
pub trait EditorHandler {
    fn event(&mut self, window: &mut EditorWindow, event: WindowEvent);
}

/// What the window procedure changes, also while the handler holds the window.
struct Flags {
    busy: Cell<bool>,
    pending: RefCell<VecDeque<WindowEvent>>,
    /// A paint or a resize came while the handler ran.
    repaint: Cell<bool>,
    resize: Cell<bool>,
    scale: Cell<f32>,
    /// The client size and scale the handler was last told of.
    told: Cell<(i32, i32, f32)>,
    cursor: Cell<Cursor>,
    tracking: Cell<bool>,
    captured: Cell<bool>,
    /// Wheel movement toward the next zoom step.
    zoom_wheel: Cell<i32>,
    dark: Cell<bool>,
    follow_system: Cell<bool>,
    closing: Cell<bool>,
}

/// The window as its handler sees it.
pub struct EditorWindow {
    hwnd: HWND,
    back: Option<Bitmap>,
    flags: Rc<Flags>,
    icons: Vec<HICON>,
}

struct Host {
    window: EditorWindow,
    handler: Box<dyn EditorHandler>,
    flags: Rc<Flags>,
}

/// Wakes the window's handler with [`WindowEvent::Notify`] from any thread.
#[derive(Clone, Copy)]
pub struct EditorNotifier(isize);

impl EditorNotifier {
    pub fn notify(&self) {
        let hwnd = HWND(self.0 as *mut _);
        let _ = unsafe { PostMessageW(Some(hwnd), WM_EDITOR_NOTIFY, WPARAM(0), LPARAM(0)) };
    }
}

/// An open editor window, for the app.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EditorRef(isize);

impl EditorRef {
    fn hwnd(&self) -> HWND {
        HWND(self.0 as *mut _)
    }

    pub fn is_open(&self) -> bool {
        unsafe { IsWindow(Some(self.hwnd())) }.as_bool()
    }

    /// `follow_system`: the window follows the system theme from now on; `dark` is it now.
    pub fn set_theme(&self, dark: bool, follow_system: bool) {
        let wp = dark as usize | (follow_system as usize) << 1;
        let _ = unsafe { PostMessageW(Some(self.hwnd()), WM_EDITOR_THEME, WPARAM(wp), LPARAM(0)) };
    }

    /// Closes the window as its close button does, before returning: with edits it stays and
    /// asks first, in front.
    pub fn ask_close(&self) {
        unsafe {
            SendMessageW(self.hwnd(), WM_CLOSE, None, None);
            if self.is_open() {
                if IsIconic(self.hwnd()).as_bool() {
                    let _ = ShowWindow(self.hwnd(), SW_RESTORE);
                }
                let _ = SetForegroundWindow(self.hwnd());
            }
        }
    }
}

impl EditorWindow {
    /// Opens a window whose client area is `client` (physical px of the virtual screen, on a
    /// monitor at `scale`), and hands it to `handler`, which draws it before it is shown.
    pub fn open(
        title: &str,
        client: Rect,
        scale: f32,
        dark: bool,
        follow_system: bool,
        handler: Box<dyn EditorHandler>,
    ) -> windows::core::Result<EditorRef> {
        static REGISTER: Once = Once::new();
        let instance = unsafe { GetModuleHandleW(None)? };
        REGISTER.call_once(|| unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                ..Default::default()
            });
        });
        let dpi = (scale * 96.0).round() as u32;
        let mut r = RECT {
            left: client.x,
            top: client.y,
            right: client.right(),
            bottom: client.bottom(),
        };
        unsafe {
            AdjustWindowRectExForDpi(&mut r, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0), dpi)?;
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        set_dark_frame(hwnd, dark);
        let flags = Rc::new(Flags {
            busy: Cell::new(false),
            pending: RefCell::new(VecDeque::new()),
            repaint: Cell::new(false),
            resize: Cell::new(false),
            scale: Cell::new(unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0),
            told: Cell::new((0, 0, 0.0)),
            cursor: Cell::new(Cursor::Arrow),
            tracking: Cell::new(false),
            captured: Cell::new(false),
            zoom_wheel: Cell::new(0),
            dark: Cell::new(dark),
            follow_system: Cell::new(follow_system),
            closing: Cell::new(false),
        });
        let mut window = EditorWindow {
            hwnd,
            back: None,
            flags: flags.clone(),
            icons: Vec::new(),
        };
        window.set_icons();
        let host = Box::into_raw(Box::new(Host {
            window,
            handler,
            flags,
        }));
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, host as isize);
            resized(hwnd, host);
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(EditorRef(hwnd.0 as isize))
    }

    pub fn notifier(&self) -> EditorNotifier {
        EditorNotifier(self.hwnd.0 as isize)
    }

    /// The client area's size in physical px.
    pub fn size(&self) -> (u32, u32) {
        let (w, h, _) = self.flags.told.get();
        (w as u32, h as u32)
    }

    pub fn scale(&self) -> f32 {
        self.flags.scale.get()
    }

    /// The back buffer, the client area's size: draw into it, then [`Self::present`].
    pub fn canvas(&mut self) -> Option<BgrxMut<'_>> {
        self.back.as_mut().map(Bitmap::view_mut)
    }

    /// Puts `rect` of the back buffer on screen.
    pub fn present(&self, rect: Rect) {
        if let Some(back) = &self.back {
            unsafe { blit(self.hwnd, back, rect) };
        }
    }

    pub fn set_title(&self, title: &str) {
        let _ = unsafe { SetWindowTextW(self.hwnd, &HSTRING::from(title)) };
    }

    pub fn set_cursor(&self, cursor: Cursor) {
        if self.flags.cursor.replace(cursor) != cursor && self.flags.tracking.get() {
            unsafe { SetCursor(Some(load_cursor(cursor))) };
        }
    }

    /// Sends [`WindowEvent::Timer`] after `after`; `None` cancels it.
    pub fn set_timer(&self, after: Option<Duration>) {
        unsafe {
            match after {
                Some(d) => {
                    let ms = d.as_millis().clamp(1, u32::MAX as u128) as u32;
                    SetTimer(Some(self.hwnd), TIMER_ID, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_ID);
                }
            }
        }
    }

    /// Closes the window once the handler returns; the handler is dropped with it.
    pub fn close(&self) {
        self.flags.closing.set(true);
    }

    fn set_icons(&mut self) {
        self.icons = set_icons(self.hwnd, self.flags.scale.get());
    }
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        for icon in self.icons.drain(..) {
            let _ = unsafe { DestroyIcon(icon) };
        }
    }
}

/// Gives `hwnd` the app icon for its title bar and the taskbar; the window destroys them.
pub(super) fn set_icons(hwnd: HWND, scale: f32) -> Vec<HICON> {
    let mut icons = Vec::new();
    for (kind, size) in [(ICON_SMALL, 16.0), (ICON_BIG, 32.0)] {
        let px = (size * scale).round() as i32;
        let &(px, png) = ICONS
            .iter()
            .find(|(s, _)| *s >= px)
            .unwrap_or(&ICONS[ICONS.len() - 1]);
        if let Ok(icon) =
            unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, px, px, LR_DEFAULTCOLOR) }
        {
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(kind as usize)),
                    Some(LPARAM(icon.0 as isize)),
                );
            }
            icons.push(icon);
        }
    }
    icons
}

pub(super) fn set_dark_frame(hwnd: HWND, dark: bool) {
    let value = windows::core::BOOL::from(dark);
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const _ as _,
            size_of_val(&value) as u32,
        )
    };
}

fn load_cursor(cursor: Cursor) -> HCURSOR {
    let id = match cursor {
        Cursor::Arrow => IDC_ARROW,
        Cursor::Crosshair => IDC_CROSS,
        Cursor::Move | Cursor::Grabbing => IDC_SIZEALL,
        Cursor::Grab => IDC_HAND,
        Cursor::Resize(Axis::Vertical) => IDC_SIZENS,
        Cursor::Resize(Axis::Horizontal) => IDC_SIZEWE,
        Cursor::Resize(Axis::Falling) => IDC_SIZENWSE,
        Cursor::Resize(Axis::Rising) => IDC_SIZENESW,
    };
    unsafe { LoadCursorW(None, id) }.unwrap_or_default()
}

pub(super) unsafe fn blit(hwnd: HWND, back: &Bitmap, r: Rect) {
    unsafe {
        let dc = GetDC(Some(hwnd));
        let _ = BitBlt(
            dc,
            r.x,
            r.y,
            r.width,
            r.height,
            Some(back.dc()),
            r.x,
            r.y,
            SRCCOPY,
        );
        ReleaseDC(Some(hwnd), dc);
    }
}

/// Answers `WM_PAINT` from the back buffer.
pub(super) unsafe fn paint(hwnd: HWND, back: Option<&Bitmap>) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if let Some(back) = back {
            let r = ps.rcPaint;
            let _ = BitBlt(
                dc,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                Some(back.dc()),
                r.left,
                r.top,
                SRCCOPY,
            );
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Keys by their place on a US keyboard: letters and digits by scan code, so `Ctrl+Z` is the same
/// key in every layout.
fn key_of(wp: WPARAM, lp: LPARAM) -> Option<Key> {
    let vk = VIRTUAL_KEY(wp.0 as u16);
    Some(match vk {
        VK_DELETE => Key::Delete,
        VK_BACK => Key::Backspace,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Escape,
        VK_SPACE => Key::Space,
        VK_SHIFT => Key::Shift,
        _ if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&vk.0) => {
            Key::Char((b'0' + (vk.0 - VK_NUMPAD0.0) as u8) as char)
        }
        _ => {
            let extended = (lp.0 >> 24) & 1 != 0;
            if extended {
                return None;
            }
            let scan = ((lp.0 >> 16) & 0xFF) as u8;
            const ROWS: [(u8, &[u8]); 4] = [
                (0x02, b"1234567890"),
                (0x10, b"qwertyuiop"),
                (0x1E, b"asdfghjkl"),
                (0x2C, b"zxcvbnm"),
            ];
            let c = ROWS
                .iter()
                .find_map(|&(first, keys)| keys.get(scan.checked_sub(first)? as usize).copied())?;
            Key::Char(c as char)
        }
    })
}

pub(super) fn pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// The mouse position in a mouse message, client px.
pub(super) fn at(lp: LPARAM) -> (f32, f32) {
    (lp.0 as i16 as f32, (lp.0 >> 16) as i16 as f32)
}

/// Runs the handler for `event`, and for what arrived meanwhile; then closes the window if the
/// handler asked to.
unsafe fn dispatch(hwnd: HWND, host: *mut Host, event: WindowEvent) {
    unsafe {
        let flags = (*addr_of!((*host).flags)).clone();
        if flags.busy.get() {
            flags.pending.borrow_mut().push_back(event);
            return;
        }
        flags.busy.set(true);
        let mut next = Some(event);
        while let Some(event) = next {
            let window = &mut *addr_of_mut!((*host).window);
            let handler = &mut *addr_of_mut!((*host).handler);
            handler.event(window, event);
            next = flags.pending.borrow_mut().pop_front();
        }
        flags.busy.set(false);
        if flags.closing.get() {
            // Frees `host` in `WM_NCDESTROY`.
            let _ = DestroyWindow(hwnd);
            return;
        }
        if flags.resize.take() {
            resized(hwnd, host);
        }
        if flags.repaint.take() {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

/// The client area may have a new size or scale: a new back buffer, and the handler is told.
unsafe fn resized(hwnd: HWND, host: *mut Host) {
    unsafe {
        let flags = (*addr_of!((*host).flags)).clone();
        if flags.busy.get() {
            flags.resize.set(true);
            return;
        }
        let mut r = RECT::default();
        if GetClientRect(hwnd, &mut r).is_err() {
            return;
        }
        let (w, h, scale) = (r.right, r.bottom, flags.scale.get());
        if w <= 0 || h <= 0 || flags.told.get() == (w, h, scale) {
            return;
        }
        flags.told.set((w, h, scale));
        let window = &mut *addr_of_mut!((*host).window);
        window.back = None;
        window.back = Bitmap::new(Rect::new(0, 0, w, h)).ok();
        dispatch(
            hwnd,
            host,
            WindowEvent::Input(Input::Resize {
                width: w as u32,
                height: h as u32,
                scale,
            }),
        );
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_GETMINMAXINFO {
            // Before the host exists too: the window is created at least this large.
            let info = &mut *(lp.0 as *mut MINMAXINFO);
            let dpi = GetDpiForWindow(hwnd);
            let s = dpi as f32 / 96.0;
            let mut r = RECT {
                left: 0,
                top: 0,
                right: (MIN_SIZE.0 * s).round() as i32,
                bottom: (MIN_SIZE.1 * s).round() as i32,
            };
            if AdjustWindowRectExForDpi(&mut r, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0), dpi)
                .is_ok()
            {
                info.ptMinTrackSize = POINT {
                    x: r.right - r.left,
                    y: r.bottom - r.top,
                };
            }
            return LRESULT(0);
        }
        let host = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
        if host.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let flags = (*addr_of!((*host).flags)).clone();
        let send = |input: Input| dispatch(hwnd, host, WindowEvent::Input(input));
        let shift = || pressed(VK_SHIFT);
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                if flags.busy.get() {
                    flags.repaint.set(true);
                    return DefWindowProcW(hwnd, msg, wp, lp);
                }
                paint(hwnd, (*addr_of!((*host).window)).back.as_ref());
                LRESULT(0)
            }
            WM_SIZE => {
                if wp.0 as u32 != SIZE_MINIMIZED {
                    resized(hwnd, host);
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                flags.scale.set((wp.0 & 0xFFFF) as f32 / 96.0);
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                // The same size at another scale sends no `WM_SIZE`.
                resized(hwnd, host);
                LRESULT(0)
            }
            WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
                SetCursor(Some(load_cursor(flags.cursor.get())));
                LRESULT(1)
            }
            WM_MOUSEMOVE => {
                if !flags.tracking.replace(true) {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                }
                let (x, y) = at(lp);
                send(Input::Move {
                    x,
                    y,
                    shift: shift(),
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                flags.tracking.set(false);
                send(Input::Leave);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                flags.captured.set(true);
                let (x, y) = at(lp);
                send(Input::Down {
                    x,
                    y,
                    shift: shift(),
                });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                flags.captured.set(false);
                let _ = ReleaseCapture();
                let (x, y) = at(lp);
                send(Input::Up {
                    x,
                    y,
                    shift: shift(),
                });
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                if flags.captured.replace(false) {
                    send(Input::Blur);
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = (wp.0 >> 16) as u16 as i16 as i32;
                let mut p = POINT {
                    x: lp.0 as i16 as i32,
                    y: (lp.0 >> 16) as i16 as i32,
                };
                let _ = ScreenToClient(hwnd, &mut p);
                if msg == WM_MOUSEWHEEL && pressed(VK_CONTROL) {
                    let total = flags.zoom_wheel.get() + delta;
                    let steps = total / WHEEL_DELTA;
                    flags.zoom_wheel.set(total - steps * WHEEL_DELTA);
                    if steps != 0 {
                        send(Input::Zoom {
                            x: p.x as f32,
                            y: p.y as f32,
                            steps,
                        });
                    }
                } else {
                    let px = delta as f32 / WHEEL_DELTA as f32 * WHEEL_SCROLL * flags.scale.get();
                    let (dx, dy) = if msg == WM_MOUSEHWHEEL {
                        (-px, 0.0)
                    } else if shift() {
                        (px, 0.0)
                    } else {
                        (0.0, px)
                    };
                    send(Input::Scroll { dx, dy });
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if let Some(key) = key_of(wp, lp) {
                    send(Input::Key {
                        key,
                        ctrl: pressed(VK_CONTROL),
                        shift: shift(),
                    });
                }
                LRESULT(0)
            }
            WM_KEYUP => {
                if let Some(key @ (Key::Space | Key::Shift)) = key_of(wp, lp) {
                    send(Input::KeyUp {
                        key,
                        shift: shift(),
                    });
                }
                LRESULT(0)
            }
            WM_KILLFOCUS => {
                send(Input::Blur);
                LRESULT(0)
            }
            // The close button and Alt+F4: the editor decides.
            WM_CLOSE => {
                send(Input::Close);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_ID => {
                let _ = KillTimer(Some(hwnd), TIMER_ID);
                dispatch(hwnd, host, WindowEvent::Timer);
                LRESULT(0)
            }
            WM_EDITOR_NOTIFY => {
                dispatch(hwnd, host, WindowEvent::Notify);
                LRESULT(0)
            }
            WM_EDITOR_THEME => {
                flags.follow_system.set(wp.0 & 2 != 0);
                set_theme(hwnd, &flags, wp.0 & 1 != 0, send);
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_EDITOR_SYSTEM_THEME => {
                let theme = msg == WM_EDITOR_SYSTEM_THEME
                    || (lp.0 != 0
                        && PCWSTR(lp.0 as *const u16)
                            .to_string()
                            .is_ok_and(|s| s == "ImmersiveColorSet"));
                if theme && flags.follow_system.get() {
                    if flags.busy.get() {
                        let _ =
                            PostMessageW(Some(hwnd), WM_EDITOR_SYSTEM_THEME, WPARAM(0), LPARAM(0));
                    } else {
                        set_theme(hwnd, &flags, apps_use_dark_theme(), send);
                    }
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(host));
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn set_theme(hwnd: HWND, flags: &Flags, dark: bool, send: impl Fn(Input)) {
    if flags.dark.replace(dark) != dark {
        set_dark_frame(hwnd, dark);
        send(Input::Theme { dark });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(vk: VIRTUAL_KEY, scan: isize) -> Option<Key> {
        key_of(WPARAM(vk.0 as usize), LPARAM(scan << 16))
    }

    #[test]
    fn letters_and_digits_go_by_their_place() {
        // `Z` on a US keyboard is `Я` in the Russian layout and `W` in the French one.
        assert_eq!(key(VIRTUAL_KEY(0x5A), 0x2C), Some(Key::Char('z')));
        assert_eq!(key(VIRTUAL_KEY(0x57), 0x2C), Some(Key::Char('z')));
        assert_eq!(key(VIRTUAL_KEY(0x41), 0x10), Some(Key::Char('q')));
        assert_eq!(key(VIRTUAL_KEY(0x30), 0x0B), Some(Key::Char('0')));
        assert_eq!(key(VK_NUMPAD0, 0x52), Some(Key::Char('0')));
        assert_eq!(key(VK_DELETE, 0x53 | 0x100), Some(Key::Delete));
        assert_eq!(key(VIRTUAL_KEY(0xBD), 0x0C), None);
    }
}
