//! Monitors and the cursor, in physical pixels.

use shot_core::geom::Rect;
use windows::Win32::Foundation::{LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, MONITORINFOF_PRIMARY};
use windows::core::BOOL;

#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub rect: Rect,
    /// The part not covered by the taskbar: where the preview goes.
    pub work: Rect,
    /// 1.0 at 96 DPI, 1.25 at 125%, and so on.
    pub scale: f32,
    pub primary: bool,
}

/// All monitors, left to right (then top to bottom): the order of `(1)`, `(2)` in file names.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `&mut Vec<Monitor>` passed below, alive for the whole enumeration.
        let list = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(hmon, &mut info) }.as_bool() {
            let (mut dpi_x, mut dpi_y) = (96, 96);
            let _ = unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
            list.push(Monitor {
                rect: to_rect(info.rcMonitor),
                work: to_rect(info.rcWork),
                scale: dpi_x as f32 / 96.0,
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        TRUE
    }

    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut list as *mut _ as isize),
        );
    }
    list.sort_by_key(|m| (m.rect.x, m.rect.y));
    list
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

fn to_rect(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}
