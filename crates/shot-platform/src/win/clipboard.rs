//! Clipboard through Win32 directly.

use std::thread::sleep;
use std::time::Duration;

use shot_core::image::BgrxView;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFOHEADER};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalReAlloc, GlobalUnlock,
};
use windows::Win32::System::Ole::CF_DIB;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::w;

/// Owner of clipboard data. `SetClipboardData` fails without an owner window, so this keeps a
/// message-only window.
pub struct Clipboard {
    owner: HWND,
}

impl Clipboard {
    pub fn new() -> windows::core::Result<Self> {
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )?
        };
        Ok(Self { owner })
    }

    /// Puts the image on the clipboard as `CF_DIB` with bottom-up rows and opaque alpha: the
    /// variant every application reads correctly (top-down DIBs and zero alpha confuse some).
    ///
    /// `SetClipboardData` copies the block into the system, which serves every other process
    /// from that copy, and keeps ours only for `GetClipboardData` in this process: 14 MB of a
    /// 1440p capture would stay in Shot until another program takes the clipboard. Shot never
    /// reads the clipboard, so the block is discarded once the clipboard is closed (WIN-006).
    /// Delayed rendering does not help: clipboard history asks for the data at once.
    ///
    /// `_dpi` stays out: Windows applications size a `CF_DIB` at 96 dpi whatever it says.
    pub fn set_image(&self, img: BgrxView, _dpi: f32) -> windows::core::Result<()> {
        let header = size_of::<BITMAPINFOHEADER>();
        let row = img.width as usize * 4;
        let size = header + row * img.height as usize;
        unsafe {
            let mem = GlobalAlloc(GMEM_MOVEABLE, size)?;
            let ptr = GlobalLock(mem) as *mut u8;
            let info = BITMAPINFOHEADER {
                biSize: header as u32,
                biWidth: img.width as i32,
                biHeight: img.height as i32, // positive: bottom-up
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: (row * img.height as usize) as u32,
                ..Default::default()
            };
            ptr.cast::<BITMAPINFOHEADER>().write_unaligned(info);
            let pixels = std::slice::from_raw_parts_mut(ptr.add(header), size - header);
            for (dst, y) in pixels.chunks_exact_mut(row).zip((0..img.height).rev()) {
                dst.copy_from_slice(img.row(y));
                for px in dst.as_chunks_mut::<4>().0 {
                    px[3] = 255;
                }
            }
            let _ = GlobalUnlock(mem);

            if let Err(e) = self.open() {
                let _ = GlobalFree(Some(mem));
                return Err(e);
            }
            let result = EmptyClipboard()
                .and_then(|_| SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(mem.0))));
            let _ = CloseClipboard();
            match result {
                // The handle stays valid, empty and marked discarded: the system frees it when
                // the clipboard changes.
                Ok(_) => _ = GlobalReAlloc(mem, 0, GMEM_MOVEABLE.0),
                // The clipboard took ownership only on success.
                Err(_) => _ = GlobalFree(Some(mem)),
            }
            result.map(|_| ())
        }
    }

    /// Another application may hold the clipboard for a moment; retry briefly.
    fn open(&self) -> windows::core::Result<()> {
        let mut attempt = 0;
        loop {
            match unsafe { OpenClipboard(Some(self.owner)) } {
                Ok(()) => return Ok(()),
                Err(e) if attempt >= 10 => return Err(e),
                Err(_) => {
                    attempt += 1;
                    sleep(Duration::from_millis(5));
                }
            }
        }
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.owner) };
    }
}
