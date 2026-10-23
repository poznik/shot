//! Top-level windows at the moment of the hotkey, for the overlay's window mode (SPEC §3.1).

use shot_core::geom::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetLayeredWindowAttributes, GetWindowLongW,
    GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, LWA_ALPHA, WS_EX_LAYERED,
    WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

/// Rects of the windows a user sees, from the top of the z-order down, in physical pixels.
///
/// Left out: hidden and minimized windows; cloaked ones (other virtual desktops, suspended store
/// apps); click-through and fully transparent ones (game and GPU overlays); the desktop itself;
/// and Shot's own windows. The rect is the visible frame from DWM, without the invisible resize
/// borders `GetWindowRect` includes.
pub fn window_rects() -> Vec<Rect> {
    struct Collect {
        own_process: u32,
        rects: Vec<Rect>,
    }

    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `&mut Collect` passed below, alive for the whole enumeration.
        let c = unsafe { &mut *(data.0 as *mut Collect) };
        if let Some(rect) = unsafe { visible_rect(hwnd, c.own_process) } {
            c.rects.push(rect);
        }
        TRUE
    }

    let mut c = Collect {
        own_process: unsafe { GetCurrentProcessId() },
        rects: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut c as *mut _ as isize));
    }
    c.rects
}

/// The desktop's windows: over the desktop the overlay targets the whole monitor.
const SKIPPED_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

unsafe fn visible_rect(hwnd: HWND, own_process: u32) -> Option<Rect> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TRANSPARENT.0 != 0 {
            return None;
        }
        if ex & WS_EX_LAYERED.0 != 0 {
            let (mut alpha, mut flags) = (0u8, Default::default());
            // Fails for windows drawn with `UpdateLayeredWindow`; those are kept.
            if GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), Some(&mut flags)).is_ok()
                && flags & LWA_ALPHA == LWA_ALPHA
                && alpha == 0
            {
                return None;
            }
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as _,
            size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return None;
        }
        let mut process = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process));
        if process == own_process {
            return None;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut buf) as usize;
        let class = &buf[..len];
        if SKIPPED_CLASSES
            .iter()
            .any(|name| class.iter().copied().eq(name.encode_utf16()))
        {
            return None;
        }

        let mut r = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as _,
            size_of::<RECT>() as u32,
        )
        .is_err()
        {
            GetWindowRect(hwnd, &mut r).ok()?;
        }
        let rect = Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
        (!rect.is_empty()).then_some(rect)
    }
}
