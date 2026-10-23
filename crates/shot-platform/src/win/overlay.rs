//! Overlay windows (SPEC §3.1, §5.4): one borderless topmost popup per monitor, created hidden at
//! start and shown at the hotkey over the frozen frame — showing a ready window is the fast path
//! (WIN-001). Each shows a canvas `shot-core` draws; the app reads the mouse and the keys from
//! their messages with [`OverlayWindows::event`].
//!
//! The overlay does not take the focus: the window the user works in keeps it, menus included,
//! and `Esc` and `Space` are hotkeys while the overlay is up. Taking the focus with
//! `SetForegroundWindow` cost 6–8 ms on the hotkey path (WIN-003).

use std::sync::atomic::{AtomicBool, Ordering};

use shot_core::geom::{Point, Rect};
use shot_core::overlay::Event;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_TRANSITIONS_FORCEDISABLED, DwmFlush, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, EndPaint, GetDC, HDC, PAINTSTRUCT, ReleaseDC, SRCCOPY, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, ReleaseCapture, SetCapture, UnregisterHotKey, VIRTUAL_KEY,
    VK_ESCAPE, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetForegroundWindow, GetWindowLongPtrW,
    HWND_TOPMOST, MA_NOACTIVATE, MSG, PostMessageW, RegisterClassW, SWP_HIDEWINDOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetCursor, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, WM_ACTIVATEAPP, WM_APP, WM_CLOSE, WM_DISPLAYCHANGE,
    WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WM_SETCURSOR, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use super::Bitmap;

const CLASS: windows::core::PCWSTR = w!("ShotOverlay");

/// Posted to an overlay window when the overlay has to close without a capture: the displays
/// changed, or, when it had to take the focus, the focus went elsewhere or Alt+F4 was pressed.
const WM_OVERLAY_CANCEL: u32 = WM_APP + 1;

/// Hotkey ids of `Esc` and `Space` while the overlay is up; the app's own are below.
const HOTKEY_ESC: i32 = 0xB001;
const HOTKEY_SPACE: i32 = 0xB002;

/// True while the overlay is shown and waiting for a choice. Only then do the window messages
/// above cancel it: hiding the overlay gives the focus back on purpose.
static WAITING: AtomicBool = AtomicBool::new(false);

pub struct OverlayWindows {
    windows: Vec<Window>,
    /// The window `Esc` and `Space` are registered to while the overlay is up.
    keys: Option<HWND>,
    /// The focused window before the overlay, when the overlay had to take the focus because
    /// another program holds `Esc` or `Space` as a hotkey. It gets the focus back.
    previous: Option<HWND>,
}

struct Window {
    hwnd: HWND,
    /// What the window shows, placed at its monitor. `Some` exactly while the window is shown.
    canvas: Option<Bitmap>,
}

impl OverlayWindows {
    /// Creates a hidden window over each of `monitors`: about 15 ms each, kept off the hotkey path.
    pub fn new(monitors: &[Rect]) -> windows::core::Result<Self> {
        unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: GetModuleHandleW(None)?.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassW(&class);
        }
        let mut overlay = Self {
            windows: Vec::new(),
            keys: None,
            previous: None,
        };
        for r in monitors {
            overlay.create(*r)?;
        }
        Ok(overlay)
    }

    /// Makes sure there is a window for each of `monitors`, after a monitor was plugged in.
    pub fn ensure(&mut self, monitors: &[Rect]) -> windows::core::Result<()> {
        for r in monitors.iter().skip(self.windows.len()) {
            self.create(*r)?;
        }
        Ok(())
    }

    fn create(&mut self, r: Rect) -> windows::core::Result<()> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                None,
                WS_POPUP,
                r.x,
                r.y,
                r.width,
                r.height,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        unsafe {
            // No fade: the overlay has to be there at once.
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &TRUE as *const _ as _,
                size_of_val(&TRUE) as u32,
            );
        }
        self.windows.push(Window { hwnd, canvas: None });
        Ok(())
    }

    /// Shows window `i` with `canvas`, already drawn, over the canvas's rect (a monitor). The
    /// first window shown receives `Esc` and `Space`.
    pub fn show(&mut self, i: usize, canvas: Bitmap) {
        let w = &mut self.windows[i];
        let r = canvas.rect;
        unsafe {
            SetWindowLongPtrW(w.hwnd, GWLP_USERDATA, canvas.dc().0 as isize);
            let _ = SetWindowPos(
                w.hwnd,
                Some(HWND_TOPMOST),
                r.x,
                r.y,
                r.width,
                r.height,
                SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            blit(w.hwnd, &canvas, Rect::new(0, 0, r.width, r.height));
            // Painted already: no WM_PAINT for the same pixels.
            let _ = ValidateRect(Some(w.hwnd), None);
        }
        w.canvas = Some(canvas);
        if !WAITING.swap(true, Ordering::Relaxed) {
            let hwnd = w.hwnd;
            self.take_keys(hwnd);
            // The crosshair is the cursor.
            unsafe { SetCursor(None) };
        }
    }

    fn take_keys(&mut self, hwnd: HWND) {
        unsafe {
            let esc = RegisterHotKey(Some(hwnd), HOTKEY_ESC, MOD_NOREPEAT, VK_ESCAPE.0 as u32);
            let space = RegisterHotKey(Some(hwnd), HOTKEY_SPACE, MOD_NOREPEAT, VK_SPACE.0 as u32);
            self.keys = Some(hwnd);
            if esc.is_err() || space.is_err() {
                // Then the keys can only arrive as key presses, to the focused window.
                self.previous = Some(GetForegroundWindow());
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }

    /// The canvas of window `i` while it is shown, to draw changes into before [`Self::present`].
    pub fn canvas(&mut self, i: usize) -> Option<&mut Bitmap> {
        self.windows.get_mut(i)?.canvas.as_mut()
    }

    /// Puts `rect` of window `i`'s canvas, in the monitor's own pixels, on screen.
    pub fn present(&self, i: usize, rect: Rect) {
        if let Some(w) = self.windows.get(i)
            && let Some(canvas) = &w.canvas
        {
            unsafe { blit(w.hwnd, canvas, rect) };
        }
    }

    /// Hides every window, frees the canvases and gives the focus back.
    pub fn hide(&mut self) {
        WAITING.store(false, Ordering::Relaxed);
        unsafe {
            if let Some(hwnd) = self.keys.take() {
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ESC);
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_SPACE);
            }
            let _ = ReleaseCapture();
            if let Some(previous) = self.previous.take() {
                let _ = SetForegroundWindow(previous);
            }
        }
        for w in &mut self.windows {
            if w.canvas.take().is_some() {
                unsafe {
                    SetWindowLongPtrW(w.hwnd, GWLP_USERDATA, 0);
                    let _ = SetWindowPos(
                        w.hwnd,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_HIDEWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
                    );
                }
            }
        }
    }

    /// The overlay input a message to one of these windows carries, in physical pixels of the
    /// virtual screen.
    pub fn event(&self, msg: &MSG) -> Option<Event> {
        let w = self.windows.iter().find(|w| w.hwnd == msg.hwnd)?;
        let origin = w.canvas.as_ref()?.rect;
        // Client coordinates, signed: under mouse capture the cursor may be left of or above the
        // window that holds the capture.
        let at = || {
            let (x, y) = (msg.lParam.0 as i16, (msg.lParam.0 >> 16) as i16);
            Point::new(origin.x + x as i32, origin.y + y as i32)
        };
        match msg.message {
            WM_MOUSEMOVE => Some(Event::Move(at())),
            WM_LBUTTONDOWN => Some(Event::Press(at())),
            WM_LBUTTONUP => Some(Event::Release(at())),
            WM_RBUTTONDOWN | WM_OVERLAY_CANCEL => Some(Event::Cancel),
            WM_HOTKEY => match msg.wParam.0 as i32 {
                HOTKEY_ESC => Some(Event::Cancel),
                HOTKEY_SPACE => Some(Event::Space),
                _ => None,
            },
            WM_KEYDOWN => match VIRTUAL_KEY(msg.wParam.0 as u16) {
                VK_ESCAPE => Some(Event::Cancel),
                VK_SPACE => Some(Event::Space),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Waits until DWM has composed a frame: for measuring when the overlay is on screen.
pub fn wait_for_compose() {
    let _ = unsafe { DwmFlush() };
}

unsafe fn blit(hwnd: HWND, canvas: &Bitmap, r: Rect) {
    unsafe {
        let dc = GetDC(Some(hwnd));
        let _ = BitBlt(
            dc,
            r.x,
            r.y,
            r.width,
            r.height,
            Some(canvas.dc()),
            r.x,
            r.y,
            SRCCOPY,
        );
        ReleaseDC(Some(hwnd), dc);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            // Never paint a background over the frozen frame.
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let canvas = HDC(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
                if !canvas.is_invalid() {
                    let r = ps.rcPaint;
                    let _ = BitBlt(
                        dc,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        Some(canvas),
                        r.left,
                        r.top,
                        SRCCOPY,
                    );
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_SETCURSOR => {
                SetCursor(None);
                LRESULT(1)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // A drag keeps reporting the cursor when it crosses to another monitor's window.
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            WM_ACTIVATEAPP if wp.0 == 0 => cancel(hwnd),
            WM_DISPLAYCHANGE => cancel(hwnd),
            // Alt+F4 cancels the overlay instead of destroying the window.
            WM_CLOSE => cancel(hwnd),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn cancel(hwnd: HWND) -> LRESULT {
    if WAITING.load(Ordering::Relaxed) {
        let _ = unsafe { PostMessageW(Some(hwnd), WM_OVERLAY_CANCEL, WPARAM(0), LPARAM(0)) };
    }
    LRESULT(0)
}
