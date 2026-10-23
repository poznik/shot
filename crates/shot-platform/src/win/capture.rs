//! Pixel buffers in GDI DIB sections, and screen capture into them with `BitBlt`.

use std::ptr::null_mut;

use shot_core::geom::Rect;
use shot_core::image::{BgrxMut, BgrxView};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ROP_CODE, ReleaseDC,
    SRCCOPY, SelectObject,
};

/// A 32-bit top-down DIB section with a memory DC, placed at `rect` of the virtual screen: a
/// captured monitor, or a canvas the overlay is drawn into. The memory belongs to the OS and goes
/// back to it on drop; the memory DC can be blitted from directly (no extra copy to paint it).
pub struct Bitmap {
    pub rect: Rect,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
}

// SAFETY: a memory DC and a DIB section are not bound to the creating thread; `Bitmap` is owned
// and used by one thread at a time.
unsafe impl Send for Bitmap {}

impl Bitmap {
    /// A bitmap of `rect`'s size. The OS hands out zeroed pages on first touch.
    pub fn new(rect: Rect) -> windows::core::Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rect.width,
                biHeight: -rect.height, // negative: rows top to bottom
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = null_mut();
            let bitmap = match CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let old = SelectObject(dc, bitmap.into());
            Ok(Self {
                rect,
                dc,
                bitmap,
                old,
                bits: bits as *mut u8,
            })
        }
    }

    fn stride(&self) -> usize {
        self.rect.width as usize * 4
    }

    pub fn view(&self) -> BgrxView<'_> {
        let stride = self.stride();
        // SAFETY: the DIB section is `stride * height` bytes and lives as long as `self`.
        let data =
            unsafe { std::slice::from_raw_parts(self.bits, stride * self.rect.height as usize) };
        BgrxView {
            width: self.rect.width as u32,
            height: self.rect.height as u32,
            stride,
            data,
        }
    }

    pub fn view_mut(&mut self) -> BgrxMut<'_> {
        let stride = self.stride();
        // SAFETY: as in `view`; `&mut self` makes the borrow unique.
        let data = unsafe {
            std::slice::from_raw_parts_mut(self.bits, stride * self.rect.height as usize)
        };
        BgrxMut {
            width: self.rect.width as u32,
            height: self.rect.height as u32,
            stride,
            data,
        }
    }

    /// Memory DC with the bitmap selected, for `BitBlt` into a window.
    pub fn dc(&self) -> HDC {
        self.dc
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Captures each rectangle of the virtual screen. `capture_blt` also asks GDI for layered windows;
/// under DWM they are in the composed image anyway, and the flag makes the cursor flicker.
pub fn capture_rects(rects: &[Rect], capture_blt: bool) -> windows::core::Result<Vec<Bitmap>> {
    let rop = if capture_blt {
        SRCCOPY | CAPTUREBLT
    } else {
        SRCCOPY
    };
    unsafe {
        let screen = GetDC(None);
        let result = rects
            .iter()
            .map(|r| capture_rect(screen, *r, rop))
            .collect();
        ReleaseDC(None, screen);
        result
    }
}

unsafe fn capture_rect(screen: HDC, rect: Rect, rop: ROP_CODE) -> windows::core::Result<Bitmap> {
    let capture = Bitmap::new(rect)?;
    unsafe {
        BitBlt(
            capture.dc,
            0,
            0,
            rect.width,
            rect.height,
            Some(screen),
            rect.x,
            rect.y,
            rop,
        )?;
    }
    Ok(capture)
}
