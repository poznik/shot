//! The settings and welcome windows (`docs/design/settings.md`): ordinary fixed-size top-level
//! windows showing what `shot_core::ui` draws. Like the editor window, each keeps a back buffer
//! the size of its client area that the handler draws into, and events that arrive while the
//! handler runs wait until it returns. The handler is dropped with the window.
//!
//! While a hotkey is recorded, a low-level keyboard hook takes the keys before the system does:
//! a combination another program registered would otherwise go to that program, and Shot could
//! neither record it nor say that it is taken.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::{addr_of, addr_of_mut};
use std::rc::Rc;
use std::sync::Once;

use shot_core::geom::Rect;
use shot_core::image::BgrxMut;
use shot_core::ui::{Input, Key, Mods};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetAsyncKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
    TrackMouseEvent, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SNAPSHOT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GWLP_USERDATA,
    GetClientRect, GetForegroundWindow, GetWindowLongPtrW, HC_ACTION, HHOOK, HICON, IDC_ARROW,
    IsIconic, IsWindow, KBDLLHOOKSTRUCT, LLKHF_UP, LoadCursorW, PostMessageW, RegisterClassW,
    SC_KEYMENU, SIZE_MINIMIZED, SW_RESTORE, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW,
    ShowWindow, UnhookWindowsHookEx, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT,
    WM_SETTINGCHANGE, WM_SIZE, WM_SYSCHAR, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW,
    WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::Bitmap;
use super::editor::{at, blit, paint, pressed, set_dark_frame, set_icons};
use super::hotkey;
use super::system::apps_use_dark_theme;

const CLASS: PCWSTR = w!("ShotForm");
/// Not resizable, no maximize button.
const STYLE: WINDOW_STYLE =
    WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);
/// Posted by [`FormRef::refresh`].
const WM_FORM_REFRESH: u32 = WM_APP + 0x50;
/// Posted by [`FormRef::set_theme`]: `wParam` bit 0 is dark, bit 1 follows the system.
const WM_FORM_THEME: u32 = WM_APP + 0x51;
/// Posted to look at the system theme again after a settings change arrived mid-event.
const WM_FORM_SYSTEM_THEME: u32 = WM_APP + 0x52;
/// Posted by the keyboard hook: `wParam` is the virtual key, bit 16 set when it goes up;
/// `lParam` the modifiers held, as [`Held`].
const WM_FORM_KEY: u32 = WM_APP + 0x53;
/// windows-rs keeps it under `UI::Controls`, a feature Shot does not need otherwise.
const WM_MOUSELEAVE: u32 = 0x02A3;

pub enum FormEvent {
    Input(Input),
    /// [`FormRef::refresh`] was called: what the window shows changed elsewhere.
    Refresh,
}

/// Owns the window's model; gets every event of its window. After [`Input::Resize`] — also the
/// first event — the back buffer is new and has to be drawn whole.
pub trait FormHandler {
    fn event(&mut self, window: &mut FormWindow, event: FormEvent);
}

/// What the window procedure changes, also while the handler holds the window.
struct Flags {
    busy: Cell<bool>,
    pending: RefCell<VecDeque<FormEvent>>,
    /// A paint or a resize came while the handler ran.
    repaint: Cell<bool>,
    resize: Cell<bool>,
    scale: Cell<f32>,
    /// The client size and scale the handler was last told of.
    told: Cell<(i32, i32, f32)>,
    tracking: Cell<bool>,
    captured: Cell<bool>,
    dark: Cell<bool>,
    follow_system: Cell<bool>,
    closing: Cell<bool>,
    /// `Alt`, `F10` and `PrintScreen` go to the handler, not to the system: a hotkey is being
    /// recorded.
    keys: Cell<bool>,
}

/// The window as its handler sees it.
pub struct FormWindow {
    hwnd: HWND,
    back: Option<Bitmap>,
    flags: Rc<Flags>,
    icons: Vec<HICON>,
}

struct Host {
    window: FormWindow,
    handler: Box<dyn FormHandler>,
    flags: Rc<Flags>,
}

/// An open form window, for the app.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FormRef(isize);

impl FormRef {
    pub(super) fn hwnd(&self) -> HWND {
        HWND(self.0 as *mut _)
    }

    /// The window as a number, to hand to another process.
    pub fn raw(&self) -> isize {
        self.0
    }

    /// A window another process handed over as [`FormRef::raw`]; it may be gone.
    pub fn from_raw(raw: isize) -> Self {
        FormRef(raw)
    }

    pub fn is_open(&self) -> bool {
        unsafe { IsWindow(Some(self.hwnd())) }.as_bool()
    }

    /// Lets the window take input again: a dialog of another process that disabled it may have
    /// died without giving it back.
    pub fn enable(&self) {
        let _ = unsafe { EnableWindow(self.hwnd(), true) };
    }

    /// Has the handler redraw the window with [`FormEvent::Refresh`].
    pub fn refresh(&self) {
        let _ = unsafe { PostMessageW(Some(self.hwnd()), WM_FORM_REFRESH, WPARAM(0), LPARAM(0)) };
    }

    /// `follow_system`: the window follows the system theme from now on; `dark` is it now.
    pub fn set_theme(&self, dark: bool, follow_system: bool) {
        let wp = dark as usize | (follow_system as usize) << 1;
        let _ = unsafe { PostMessageW(Some(self.hwnd()), WM_FORM_THEME, WPARAM(wp), LPARAM(0)) };
    }

    /// Restores the window if it is minimized and brings it to the front.
    pub fn show(&self) {
        unsafe {
            if IsIconic(self.hwnd()).as_bool() {
                let _ = ShowWindow(self.hwnd(), SW_RESTORE);
            }
            let _ = SetForegroundWindow(self.hwnd());
        }
    }
}

impl FormWindow {
    /// Opens a window titled `title` whose client area is `client` (physical px of the virtual
    /// screen, on a monitor at `scale`), and hands it to `handler`, which draws it before it is
    /// shown.
    pub fn open(
        title: &str,
        client: Rect,
        scale: f32,
        dark: bool,
        follow_system: bool,
        handler: Box<dyn FormHandler>,
    ) -> windows::core::Result<FormRef> {
        static REGISTER: Once = Once::new();
        let instance = unsafe { GetModuleHandleW(None)? };
        REGISTER.call_once(|| unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
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
            AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi)?;
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                &HSTRING::from(title),
                STYLE,
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
        let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
        let flags = Rc::new(Flags {
            busy: Cell::new(false),
            pending: RefCell::new(VecDeque::new()),
            repaint: Cell::new(false),
            resize: Cell::new(false),
            scale: Cell::new(scale),
            told: Cell::new((0, 0, 0.0)),
            tracking: Cell::new(false),
            captured: Cell::new(false),
            dark: Cell::new(dark),
            follow_system: Cell::new(follow_system),
            closing: Cell::new(false),
            keys: Cell::new(false),
        });
        let window = FormWindow {
            hwnd,
            back: None,
            flags: flags.clone(),
            icons: set_icons(hwnd, scale),
        };
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
        Ok(FormRef(hwnd.0 as isize))
    }

    pub fn this(&self) -> FormRef {
        FormRef(self.hwnd.0 as isize)
    }

    /// The client area's size in physical px.
    pub fn size(&self) -> (u32, u32) {
        let (w, h, _) = self.flags.told.get();
        (w as u32, h as u32)
    }

    pub fn scale(&self) -> f32 {
        self.flags.scale.get()
    }

    /// Resizes the client area, keeping the window's top-left corner; [`Input::Resize`] follows
    /// once the handler returns.
    pub fn set_client_size(&self, width: u32, height: u32) {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        let mut r = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        if unsafe { AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi) }
            .is_ok()
        {
            let _ = unsafe {
                SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
        }
    }

    /// While `on`, every key comes to the handler, whichever program registered it, and `Alt`,
    /// `F10` and `PrintScreen` do not open the window menu or go to the system: for recording a
    /// hotkey.
    pub fn capture_keys(&self, on: bool) {
        if self.flags.keys.replace(on) != on {
            if on {
                take_keyboard(self.hwnd);
            } else {
                release_keyboard(self.hwnd);
            }
        }
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

    /// Closes the window once the handler returns; the handler is dropped with it.
    pub fn close(&self) {
        self.flags.closing.set(true);
    }
}

impl Drop for FormWindow {
    fn drop(&mut self) {
        for icon in self.icons.drain(..) {
            let _ = unsafe { DestroyIcon(icon) };
        }
    }
}

fn mods() -> Mods {
    Mods {
        ctrl: pressed(VK_CONTROL),
        alt: pressed(VK_MENU),
        shift: pressed(VK_SHIFT),
        meta: pressed(VK_LWIN) || pressed(VK_RWIN),
    }
}

/// The key of a virtual-key code: what a hotkey can use, `Esc`, or a modifier.
fn key_of(vk: u32) -> Option<Key> {
    match VIRTUAL_KEY(vk as u16) {
        VK_ESCAPE => Some(Key::Escape),
        VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN | VK_LSHIFT | VK_RSHIFT
        | VK_LCONTROL | VK_RCONTROL | VK_LMENU | VK_RMENU => Some(Key::Modifier),
        _ => hotkey::key_of(vk).map(Key::Hot),
    }
}

thread_local! {
    /// The keyboard hook and the window it takes the keys for.
    static KEYBOARD: Cell<Option<(HHOOK, HWND)>> = const { Cell::new(None) };
    /// The modifiers held, as the hook saw them go down and up.
    static HELD: Cell<Held> = const { Cell::new(Held(0)) };
}

/// The modifier keys held, left and right apart: a bit per key of [`Held::KEYS`].
#[derive(Clone, Copy)]
struct Held(u8);

impl Held {
    const KEYS: [VIRTUAL_KEY; 8] = [
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
    ];

    fn now() -> Self {
        let mut bits = 0;
        for (i, vk) in Self::KEYS.iter().enumerate() {
            if unsafe { GetAsyncKeyState(vk.0 as i32) } < 0 {
                bits |= 1 << i;
            }
        }
        Held(bits)
    }

    fn set(&mut self, vk: VIRTUAL_KEY, down: bool) {
        if let Some(i) = Self::KEYS.iter().position(|&k| k == vk) {
            if down {
                self.0 |= 1 << i;
            } else {
                self.0 &= !(1 << i);
            }
        }
    }

    fn mods(self) -> Mods {
        Mods {
            ctrl: self.0 & 0b11 != 0,
            alt: self.0 & 0b1100 != 0,
            shift: self.0 & 0b11_0000 != 0,
            meta: self.0 & 0b1100_0000 != 0,
        }
    }
}

fn take_keyboard(hwnd: HWND) {
    release_keyboard(hwnd);
    let instance = unsafe { GetModuleHandleW(None) }.ok();
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook),
            instance.map(Into::into),
            0,
        )
    };
    if let Ok(hook) = hook {
        HELD.set(Held::now());
        KEYBOARD.set(Some((hook, hwnd)));
    }
}

fn release_keyboard(hwnd: HWND) {
    if let Some((hook, owner)) = KEYBOARD.get()
        && owner == hwnd
    {
        let _ = unsafe { UnhookWindowsHookEx(hook) };
        KEYBOARD.set(None);
    }
}

/// Takes every key while the recording window is in front and posts it there as
/// `WM_FORM_KEY`. The system's hotkeys come after the hook, so a combination another program
/// registered reaches the window too, and does not set that program off.
unsafe extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION as i32
            && let Some((_, hwnd)) = KEYBOARD.get()
            && GetForegroundWindow() == hwnd
        {
            let event = &*(lp.0 as *const KBDLLHOOKSTRUCT);
            let up = event.flags.contains(LLKHF_UP);
            let mut held = HELD.get();
            held.set(VIRTUAL_KEY(event.vkCode as u16), !up);
            HELD.set(held);
            let key = (event.vkCode as usize & 0xFFFF) | (up as usize) << 16;
            let _ = PostMessageW(
                Some(hwnd),
                WM_FORM_KEY,
                WPARAM(key),
                LPARAM(held.0 as isize),
            );
            return LRESULT(1);
        }
        CallNextHookEx(None, code, wp, lp)
    }
}

/// Runs the handler for `event`, and for what arrived meanwhile; then closes the window if the
/// handler asked to.
unsafe fn dispatch(hwnd: HWND, host: *mut Host, event: FormEvent) {
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
            FormEvent::Input(Input::Resize {
                width: w as u32,
                height: h as u32,
                scale,
            }),
        );
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let host = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Host;
        if host.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let flags = (*addr_of!((*host).flags)).clone();
        let send = |input: Input| dispatch(hwnd, host, FormEvent::Input(input));
        let keys = flags.keys.get();
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
                send(Input::Move { x, y });
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
                send(Input::Down { x, y });
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                flags.captured.set(false);
                let _ = ReleaseCapture();
                let (x, y) = at(lp);
                send(Input::Up { x, y });
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                if flags.captured.replace(false) {
                    send(Input::Blur);
                }
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if msg == WM_SYSKEYDOWN && !keys {
                    // `Alt+F4`, `Alt+Space` and the window menu.
                    return DefWindowProcW(hwnd, msg, wp, lp);
                }
                if let Some(key) = key_of(wp.0 as u32) {
                    send(Input::Key { key, mods: mods() });
                }
                LRESULT(0)
            }
            WM_KEYUP | WM_SYSKEYUP => {
                // `PrintScreen` comes only as a key going up.
                if keys && wp.0 as u32 == VK_SNAPSHOT.0 as u32 {
                    send(Input::Key {
                        key: Key::Hot(shot_core::hotkey::Key::PrintScreen),
                        mods: mods(),
                    });
                }
                send(Input::KeyUp { mods: mods() });
                if msg == WM_SYSKEYUP && !keys {
                    return DefWindowProcW(hwnd, msg, wp, lp);
                }
                LRESULT(0)
            }
            // While recording, `Alt` and a letter would beep and `Alt` or `F10` alone would open
            // the window menu.
            WM_SYSCHAR | WM_CHAR if keys => LRESULT(0),
            WM_SYSCOMMAND if keys && (wp.0 & 0xFFF0) as u32 == SC_KEYMENU => LRESULT(0),
            WM_FORM_KEY => {
                let mods = Held(lp.0 as u8).mods();
                if wp.0 >> 16 & 1 == 0 {
                    if let Some(key) = key_of(wp.0 as u32 & 0xFFFF) {
                        send(Input::Key { key, mods });
                    }
                } else {
                    send(Input::KeyUp { mods });
                }
                LRESULT(0)
            }
            WM_KILLFOCUS => {
                send(Input::Blur);
                LRESULT(0)
            }
            // The close button and `Alt+F4`: the model decides.
            WM_CLOSE => {
                send(Input::Close);
                LRESULT(0)
            }
            WM_FORM_REFRESH => {
                dispatch(hwnd, host, FormEvent::Refresh);
                LRESULT(0)
            }
            WM_FORM_THEME => {
                flags.follow_system.set(wp.0 & 2 != 0);
                set_theme(hwnd, &flags, wp.0 & 1 != 0, send);
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_FORM_SYSTEM_THEME => {
                let theme = msg == WM_FORM_SYSTEM_THEME
                    || (lp.0 != 0
                        && PCWSTR(lp.0 as *const u16)
                            .to_string()
                            .is_ok_and(|s| s == "ImmersiveColorSet"));
                if theme && flags.follow_system.get() {
                    if flags.busy.get() {
                        let _ =
                            PostMessageW(Some(hwnd), WM_FORM_SYSTEM_THEME, WPARAM(0), LPARAM(0));
                    } else {
                        set_theme(hwnd, &flags, apps_use_dark_theme(), send);
                    }
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_NCDESTROY => {
                release_keyboard(hwnd);
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

    #[test]
    fn keys_a_form_tells_apart() {
        assert_eq!(key_of(0x1B), Some(Key::Escape));
        assert_eq!(key_of(0x12), Some(Key::Modifier), "Alt");
        assert_eq!(key_of(0x5B), Some(Key::Modifier), "Win");
        assert_eq!(key_of(0x09), Some(Key::Hot(shot_core::hotkey::Key::Tab)));
        assert_eq!(
            key_of(0x53),
            Some(Key::Hot(shot_core::hotkey::Key::Char('S')))
        );
        assert_eq!(key_of(0x60), None, "numpad 0");
    }
}
