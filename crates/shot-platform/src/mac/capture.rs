//! Frames of the screen through ScreenCaptureKit (macOS 14+), into bitmaps in `IOSurface`s.
//!
//! The list of displays and windows SCK captures (`SCShareableContent`) costs tens of milliseconds
//! to fetch, so it is fetched ahead: at start, whenever the screens change and when Shot makes a
//! window to leave out of captures (see [`exclude`]). The shot itself is one `SCScreenshotManager`
//! request per display; the main thread waits for its answer, and the frame is copied out of SCK's
//! buffer on SCK's own queue.
//!
//! SCK converts the pixels to sRGB on the GPU, so files and the clipboard hold what they hold on
//! Windows and need no profile of the display. Every bitmap is tagged sRGB, and the compositor
//! matches the overlay back to the display: the frozen screen looks as the live one did, save for
//! the rare colors of a wide-gamut display that sRGB lacks.
//!
//! With `SHOT_DEV` set the frames are synthetic, never the screen: for driving Shot over ssh.

use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, msg_send};
use objc2_core_foundation::{CFString, CFType};
use objc2_core_graphics::{CGColorSpace, CGPreflightScreenCaptureAccess, kCGColorSpaceSRGB};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_io_surface::{
    IOSurface, IOSurfaceLockOptions, IOSurfacePropertyKeyBytesPerElement,
    IOSurfacePropertyKeyBytesPerRow, IOSurfacePropertyKeyHeight, IOSurfacePropertyKeyPixelFormat,
    IOSurfacePropertyKeyWidth, IOSurfacePurgeabilityState, kIOSurfaceColorSpace,
};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCDisplay, SCScreenshotManager, SCShareableContent,
    SCStreamConfiguration, SCWindow,
};
use shot_core::geom::Rect;
use shot_core::image::{BgrxMut, BgrxView};

use super::Error;
use super::monitors::screens;

const BGRA: u32 = u32::from_be_bytes(*b"BGRA");

/// How long a request to SCK may take before the capture gives up.
const PATIENCE: Duration = Duration::from_secs(3);

/// A 32-bit BGRX image placed at `rect` of the virtual desktop: a captured monitor, or a canvas
/// the overlay is drawn into. It lives in an `IOSurface`, which an overlay window shows without a
/// copy; the memory goes back to the OS on drop.
pub struct Bitmap {
    pub rect: Rect,
    surface: Retained<IOSurface>,
    base: NonNull<u8>,
    stride: usize,
    /// Locked for the CPU: from creation until a window shows it, and again while the overlay
    /// draws a change into it.
    locked: Cell<bool>,
}

// SAFETY: an `IOSurface` may be used from any thread; the bitmap is owned and used by one thread
// at a time (the saving thread gets it whole).
unsafe impl Send for Bitmap {}

impl Bitmap {
    /// A bitmap of `rect`'s size. The OS hands out zeroed pages on first touch.
    pub fn new(rect: Rect) -> Result<Self, Error> {
        let (w, h) = (rect.width.max(1) as usize, rect.height.max(1) as usize);
        let keys = unsafe {
            [
                IOSurfacePropertyKeyWidth,
                IOSurfacePropertyKeyHeight,
                IOSurfacePropertyKeyBytesPerElement,
                IOSurfacePropertyKeyBytesPerRow,
                IOSurfacePropertyKeyPixelFormat,
            ]
        };
        let values = [
            NSNumber::new_usize(w),
            NSNumber::new_usize(h),
            NSNumber::new_usize(4),
            NSNumber::new_usize(w * 4),
            NSNumber::new_u32(BGRA),
        ];
        let props = NSDictionary::from_retained_objects(&keys, &values);
        // SAFETY: the keys are `NSString`s; the values are what IOSurface expects for them.
        let props: Retained<NSDictionary<NSString, AnyObject>> =
            unsafe { Retained::cast_unchecked(props) };
        let surface = IOSurface::initWithProperties(IOSurface::alloc(), &props)
            .ok_or_else(|| Error::new(format!("no IOSurface of {w}x{h}")))?;
        if let Some(srgb) = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
            .and_then(|space| space.property_list())
        {
            // SAFETY: CF objects are Objective-C objects, the key a `CFString` bridged to
            // `NSString`, and the value the form IOSurface documents for it.
            unsafe {
                let key = &*(kIOSurfaceColorSpace as *const CFString).cast::<NSString>();
                surface.setAttachment_forKey(&*(&*srgb as *const CFType).cast::<AnyObject>(), key);
            }
        }
        surface.lockWithOptions_seed(IOSurfaceLockOptions(0), std::ptr::null_mut());
        let stride = surface.bytesPerRow() as usize;
        let base = surface.baseAddress().cast::<u8>();
        Ok(Bitmap {
            rect,
            surface,
            base,
            stride,
            locked: Cell::new(true),
        })
    }

    fn len(&self) -> usize {
        self.stride * self.rect.height.max(0) as usize
    }

    pub fn view(&self) -> BgrxView<'_> {
        // SAFETY: the surface is `stride * height` bytes at `base` and lives as long as `self`.
        let data = unsafe { std::slice::from_raw_parts(self.base.as_ptr(), self.len()) };
        BgrxView {
            width: self.rect.width as u32,
            height: self.rect.height as u32,
            stride: self.stride,
            data,
        }
    }

    pub fn view_mut(&mut self) -> BgrxMut<'_> {
        // SAFETY: as in `view`; `&mut self` makes the borrow unique.
        let data = unsafe { std::slice::from_raw_parts_mut(self.base.as_ptr(), self.len()) };
        BgrxMut {
            width: self.rect.width as u32,
            height: self.rect.height as u32,
            stride: self.stride,
            data,
        }
    }

    /// The surface, for a layer to show.
    pub(crate) fn surface(&self) -> &IOSurface {
        &self.surface
    }

    /// Before the CPU draws into it.
    pub(crate) fn lock(&self) {
        if !self.locked.replace(true) {
            self.surface
                .lockWithOptions_seed(IOSurfaceLockOptions(0), std::ptr::null_mut());
        }
    }

    /// After the CPU drew into it: the change is there for the compositor.
    pub(crate) fn unlock(&self) {
        if self.locked.replace(false) {
            self.surface
                .unlockWithOptions_seed(IOSurfaceLockOptions(0), std::ptr::null_mut());
        }
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        self.unlock();
        // The window server may keep the last surface a hidden overlay showed until the panel is
        // shown again, and the memory counts as Shot's: 30 MB for a Retina screen. Emptied, the
        // surface gives it back now, whoever still holds it.
        let _: i32 = unsafe {
            msg_send![&*self.surface, setPurgeable: IOSurfacePurgeabilityState::PurgeableEmpty,
                oldState: std::ptr::null_mut::<IOSurfacePurgeabilityState>()]
        };
    }
}

/// `SCShareableContent` handed between SCK's queue and the main thread.
struct Content(Retained<SCShareableContent>);

// SAFETY: SCK's objects are immutable snapshots, safe to pass between threads.
unsafe impl Send for Content {}

static CONTENT: Mutex<Option<Content>> = Mutex::new(None);

/// Shot's windows left out of every capture, by number: the preview cards.
static EXCLUDED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Leaves `windows` out of every capture, once the list of windows is fetched again: a card on
/// screen must not end up in the next shot. `NSWindowSharingNone` is not enough, since recent
/// macOS versions let ScreenCaptureKit capture such windows.
pub(crate) fn exclude(windows: &[u32]) {
    EXCLUDED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend_from_slice(windows);
    refresh_displays();
}

/// Whether Shot may capture the screen; asking does not prompt the user.
pub(crate) fn may_capture() -> bool {
    CGPreflightScreenCaptureAccess()
}

/// Fetches the list of displays and windows in the background, for the next capture; nothing
/// without the right to capture (fetching it would ask the user).
pub(crate) fn refresh_displays() {
    if dev() || !may_capture() {
        return;
    }
    fetch(|content| {
        if let Some(content) = content {
            *CONTENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(content);
        }
    });
}

fn fetch(done: impl Fn(Option<Content>) + 'static) {
    let block = RcBlock::new(move |content: *mut SCShareableContent, _: *mut NSError| {
        // SAFETY: SCK hands a valid object or null.
        let content = unsafe { Retained::retain(content) };
        done(content.map(Content));
    });
    unsafe {
        // Hidden windows too: the cards made ahead are hidden until a capture shows them.
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, false, &block,
        )
    };
}

/// The SCK display with `id` and the windows to leave out, from the list fetched ahead, else
/// fetched now.
fn display(id: u32) -> Result<(Retained<SCDisplay>, Retained<NSArray<SCWindow>>), Error> {
    let find = |content: &Content| {
        let displays = unsafe { content.0.displays() };
        let display = displays.iter().find(|d| unsafe { d.displayID() } == id)?;
        let excluded = EXCLUDED.lock().unwrap_or_else(|e| e.into_inner());
        let windows: Vec<Retained<SCWindow>> = unsafe { content.0.windows() }
            .iter()
            .filter(|w| excluded.contains(&unsafe { w.windowID() }))
            .collect();
        Some((display, NSArray::from_retained_slice(&windows)))
    };
    if let Some(content) = CONTENT.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && let Some(d) = find(content)
    {
        return Ok(d);
    }
    let (tx, rx) = mpsc::sync_channel(1);
    fetch(move |content| {
        let _ = tx.send(content);
    });
    let content = rx
        .recv_timeout(PATIENCE)
        .map_err(|_| Error::new("ScreenCaptureKit did not list the displays"))?
        .ok_or_else(|| Error::new("ScreenCaptureKit lists no displays"))?;
    let found = find(&content);
    *CONTENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(content);
    found.ok_or_else(|| Error::new(format!("display {id} is not among the capturable ones")))
}

/// With `SHOT_DEV`, synthetic frames stand in for the screen.
pub(crate) fn dev() -> bool {
    std::env::var_os("SHOT_DEV").is_some()
}

/// Captures each rectangle of the virtual desktop, each a whole monitor. `_capture_blt` is for
/// Windows.
pub fn capture_rects(rects: &[Rect], _capture_blt: bool) -> Result<Vec<Bitmap>, Error> {
    if !super::permission::screen_permission() {
        return Err(Error::new("no Screen Recording permission"));
    }
    if dev() {
        return rects.iter().map(|r| synthetic(*r)).collect();
    }
    let screens = screens();
    rects
        .iter()
        .map(|r| {
            let screen = screens
                .iter()
                .find(|s| s.monitor.rect == *r)
                .ok_or_else(|| Error::new("no screen at that place"))?;
            let (display, excluded) = display(screen.display)?;
            shoot(&display, &excluded, *r)
        })
        .collect()
}

fn shoot(display: &SCDisplay, excluded: &NSArray<SCWindow>, rect: Rect) -> Result<Bitmap, Error> {
    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            display,
            excluded,
        )
    };
    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth(rect.width as usize);
        config.setHeight(rect.height as usize);
        config.setPixelFormat(BGRA);
        config.setShowsCursor(false);
        config.setCaptureResolution(SCCaptureResolutionType::Best);
        config.setColorSpaceName(kCGColorSpaceSRGB);
    }
    let bitmap = Bitmap::new(rect)?;
    let target = Target {
        base: bitmap.base,
        stride: bitmap.stride,
        width: rect.width as usize,
        height: rect.height as usize,
    };
    let (tx, rx) = mpsc::sync_channel::<Result<(), Error>>(1);
    let block = RcBlock::new(move |sample: *mut CMSampleBuffer, error: *mut NSError| {
        // SAFETY: SCK hands valid objects or null; the bitmap outlives the wait below.
        let result = match unsafe { sample.as_ref() } {
            Some(sample) => unsafe { target.copy_from(sample) },
            None => Err(Error::new(match unsafe { error.as_ref() } {
                Some(e) => e.localizedDescription().to_string(),
                None => "ScreenCaptureKit returned nothing".into(),
            })),
        };
        let _ = tx.send(result);
    });
    unsafe {
        SCScreenshotManager::captureSampleBufferWithFilter_configuration_completionHandler(
            &filter,
            &config,
            Some(&block),
        )
    };
    match rx.recv_timeout(PATIENCE) {
        Ok(result) => result.map(|()| bitmap),
        // The answer may still come and write into the bitmap: keep it alive.
        Err(_) => {
            std::mem::forget(bitmap);
            Err(Error::new("ScreenCaptureKit did not answer"))
        }
    }
}

/// Where SCK's answer is copied: the pixels of a [`Bitmap`].
#[derive(Clone, Copy)]
struct Target {
    base: NonNull<u8>,
    stride: usize,
    width: usize,
    height: usize,
}

// SAFETY: it points into a bitmap the main thread does not touch until the copy is done.
unsafe impl Send for Target {}

impl Target {
    /// # Safety
    /// The bitmap must be alive and not used elsewhere.
    unsafe fn copy_from(self, sample: &CMSampleBuffer) -> Result<(), Error> {
        let buffer =
            unsafe { sample.image_buffer() }.ok_or_else(|| Error::new("no image in the answer"))?;
        let read_only = CVPixelBufferLockFlags::ReadOnly;
        if unsafe { CVPixelBufferLockBaseAddress(&buffer, read_only) } != 0 {
            return Err(Error::new("cannot read the captured image"));
        }
        let (w, h) = (
            CVPixelBufferGetWidth(&buffer).min(self.width),
            CVPixelBufferGetHeight(&buffer).min(self.height),
        );
        let (src, src_stride) = (
            CVPixelBufferGetBaseAddress(&buffer).cast::<u8>(),
            CVPixelBufferGetBytesPerRow(&buffer),
        );
        if !src.is_null() {
            for y in 0..h {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        src.add(y * src_stride),
                        self.base.as_ptr().add(y * self.stride),
                        w * 4,
                    )
                };
            }
        }
        unsafe { CVPixelBufferUnlockBaseAddress(&buffer, read_only) };
        Ok(())
    }
}

/// A frame that is plainly not the screen: diagonal bands and a grid every 100 pixels.
fn synthetic(rect: Rect) -> Result<Bitmap, Error> {
    let mut bitmap = Bitmap::new(rect)?;
    let view = bitmap.view_mut();
    for y in 0..view.height as usize {
        let row = &mut view.data[y * view.stride..][..view.width as usize * 4];
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let grid = x % 100 == 0 || y % 100 == 0;
            let band = ((x + y) / 160 % 2) as u8;
            let v = if grid { 40 } else { 150 + 50 * band };
            *px = [v, v.saturating_sub(30), v.saturating_sub(70), 255];
        }
    }
    Ok(bitmap)
}
