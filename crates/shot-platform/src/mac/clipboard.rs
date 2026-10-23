//! The clipboard: `NSPasteboard`. An image goes on as TIFF at once, since an uncompressed file is
//! a copy with the bytes reordered, and as PNG a moment later, encoded in the background from the
//! TIFF's own pixels: AppKit reads either, Chromium and the web prefer PNG. Both carry the
//! density, so a capture pastes into documents at its size on screen, and the TIFF carries the
//! sRGB profile, without which AppKit would take it for generic RGB.
//!
//! The pasteboard server keeps its own copy, and AppKit keeps the data it was given until the
//! clipboard changes: 22 MB of a full-screen TIFF would stay in Shot (as on Windows, WIN-006), and
//! megabytes of its PNG. So both are written into files mapped into memory and deleted at once:
//! the pages of a file do not count as Shot's memory, and the system takes them back when it
//! needs them.
//!
//! With `SHOT_DEV` set the image goes to a pasteboard of its own, `com.poznik.shot.dev`, and the
//! user's stays as it was.

use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

use block2::RcBlock;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};
use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};
use objc2_foundation::{NSArray, NSData, ns_string};
use shot_core::image::{
    BgrxView, Compression, TIFF_PIXELS_AT, encode_rgb_png, encode_tiff, tiff_len, write_tiff,
};

use super::pump::{post, register};
use super::{Error, capture};

pub struct Clipboard {
    srgb: Vec<u8>,
    /// Puts the PNG on, on the main thread.
    key: isize,
}

/// A PNG for the pasteboard as it was at the change count.
static PNG: Mutex<Option<(isize, Data)>> = Mutex::new(None);

/// Data handed between the main thread and the one that encodes the PNG.
struct Data(Retained<NSData>);

// SAFETY: an `NSData` is immutable; Apple documents immutable Foundation objects as safe to use
// from any thread.
unsafe impl Send for Data {}

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        let srgb = unsafe { CGColorSpace::with_name(Some(kCGColorSpaceSRGB)) }
            .and_then(|space| CGColorSpace::icc_data(Some(&space)))
            .map(|data| data.to_vec())
            .unwrap_or_default();
        let key = register(Rc::new(add_png));
        Ok(Clipboard { srgb, key })
    }

    /// Puts the image on the clipboard; `dpi` is the monitor's scale times 72.
    pub fn set_image(&self, img: BgrxView, dpi: f32) -> Result<(), Error> {
        let len = tiff_len(img.width, img.height, self.srgb.len());
        let tiff = mapped(len, |bytes| write_tiff(img, dpi, Some(&self.srgb), bytes))
            .unwrap_or_else(|| NSData::from_vec(encode_tiff(img, dpi, Some(&self.srgb))));
        let board = board();
        let count = board.clearContents();
        if !board.setData_forType(Some(&tiff), unsafe { NSPasteboardTypeTIFF }) {
            return Err(Error::new("the pasteboard refused the image"));
        }
        let (width, height, key) = (img.width, img.height, self.key);
        let tiff = Data(tiff);
        thread::spawn(move || {
            let tiff = tiff;
            // SAFETY: nothing mutates the data.
            let bytes = unsafe { tiff.0.as_bytes_unchecked() };
            let rgb = &bytes[TIFF_PIXELS_AT..TIFF_PIXELS_AT + width as usize * height as usize * 3];
            let mut png = Vec::new();
            if encode_rgb_png(width, height, rgb, &mut png, Compression::Fast, Some(dpi)).is_ok() {
                let png = mapped(png.len(), |bytes| bytes.copy_from_slice(&png))
                    .unwrap_or_else(|| NSData::from_vec(png));
                *PNG.lock().unwrap_or_else(|e| e.into_inner()) = Some((count, Data(png)));
                post(key);
            }
        });
        Ok(())
    }
}

/// Adds the PNG unless something else went on the clipboard meanwhile.
fn add_png() {
    let Some((count, png)) = PNG.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let board = board();
    if board.changeCount() != count {
        return;
    }
    let png_type = unsafe { NSPasteboardTypePNG };
    unsafe { board.addTypes_owner(&NSArray::from_slice(&[png_type]), None::<&AnyObject>) };
    board.setData_forType(Some(&png.0), png_type);
}

/// `len` bytes that `fill` writes, in a mapped file; `None` when there is no file to map, and the
/// data stays in memory.
fn mapped(len: usize, fill: impl FnOnce(&mut [u8])) -> Option<Retained<NSData>> {
    match Mapping::new(len) {
        Ok(mut map) => {
            fill(map.bytes());
            Some(map.into_data())
        }
        Err(e) => {
            eprintln!("clipboard: no mapped file ({e}); the image stays in memory");
            None
        }
    }
}

fn board() -> Retained<NSPasteboard> {
    if capture::dev() {
        NSPasteboard::pasteboardWithName(ns_string!("com.poznik.shot.dev"))
    } else {
        NSPasteboard::generalPasteboard()
    }
}

/// A deleted file mapped into memory.
struct Mapping {
    ptr: NonNull<c_void>,
    len: usize,
}

impl Mapping {
    fn new(len: usize) -> io::Result<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "shot-clipboard-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        let _ = fs::remove_file(&path);
        file.set_len(len as u64)?;
        // SAFETY: a fresh shared mapping of the whole file, which outlives `file`.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let ptr = NonNull::new(ptr).ok_or_else(|| io::Error::other("null mapping"))?;
        Ok(Mapping { ptr, len })
    }

    fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: the mapping is `len` bytes, readable and writable, and only `self` refers to it.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast(), self.len) }
    }

    /// An `NSData` over the mapping, which unmaps it when freed.
    fn into_data(self) -> Retained<NSData> {
        let (ptr, len) = (self.ptr, self.len);
        std::mem::forget(self);
        let unmap = RcBlock::new(|ptr: NonNull<c_void>, len: usize| {
            // SAFETY: the mapping made in `new`, freed once, by the data that owned it.
            unsafe { libc::munmap(ptr.as_ptr(), len) };
        });
        // SAFETY: the bytes stay valid until the deallocator runs.
        unsafe {
            NSData::initWithBytesNoCopy_length_deallocator(NSData::alloc(), ptr, len, Some(&unmap))
        }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: the mapping made in `new`, not handed to an `NSData`.
        unsafe { libc::munmap(self.ptr.as_ptr(), self.len) };
    }
}
