//! The windows on screen at the hotkey, for the overlay to snap to (SPEC §3.1). Their frames come
//! from the window server with no permission; their names would need Screen Recording and are not
//! read.

use objc2::runtime::AnyObject;
use objc2_core_foundation::CFArray;
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowAlpha,
    kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
use shot_core::geom::Rect;

use super::monitors::{Screen, screens};

/// Window levels kept: normal windows, floating ones, modal panels, the menu bar and open menus.
/// Left out: the Dock, which spreads one transparent window over the whole screen, the menu-bar
/// icons, and everything at the screen saver's level and above (Shot's own overlay among them).
const LEVELS: [i32; 5] = [0, 3, 8, 24, 101];

/// The first list a process asks for costs ~27 ms, the next ones under 1: the first is asked for
/// at start, on a thread of its own.
pub(crate) fn warm_up() {
    std::thread::spawn(|| {
        let _ = CGWindowListCopyWindowInfo(
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
            kCGNullWindowID,
        );
    });
}

/// Rects of the windows a user sees, from the front to the back, in physical pixels.
///
/// Left out besides the levels above: Shot's own windows, fully transparent ones, and windows
/// above the normal level that cover a whole screen (backdrops of system helpers).
pub fn window_rects() -> Vec<Rect> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return Vec::new();
    };
    // SAFETY: a CFArray of CFDictionary with CFString keys is toll-free bridged to these.
    let list: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*list as *const CFArray).cast() };
    let screens = screens();
    let Some(main) = screens.first() else {
        return Vec::new();
    };
    let top = main.frame.origin.y + main.frame.size.height;
    let own = std::process::id() as i32;
    let number = |dict: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        dict.objectForKey(key)
            .and_then(|v| v.downcast::<NSNumber>().ok())
    };
    let mut rects = Vec::new();
    for dict in list.iter() {
        // SAFETY: the keys are the window server's constant strings, toll-free bridged.
        let key = |k: &objc2_core_foundation::CFString| unsafe {
            &*(k as *const objc2_core_foundation::CFString).cast::<NSString>()
        };
        let layer = number(&dict, key(unsafe { kCGWindowLayer })).map_or(-1, |n| n.intValue());
        let pid = number(&dict, key(unsafe { kCGWindowOwnerPID })).map_or(0, |n| n.intValue());
        let alpha = number(&dict, key(unsafe { kCGWindowAlpha })).map_or(1.0, |n| n.doubleValue());
        if !LEVELS.contains(&layer) || pid == own || alpha <= 0.0 {
            continue;
        }
        let Some(frame) = frame_of(&dict, top) else {
            continue;
        };
        if frame.size.width < 1.0 || frame.size.height < 1.0 {
            continue;
        }
        let Some(screen) = screen_of(&screens, frame) else {
            continue;
        };
        if layer != 0 && covers(screen.frame, frame) {
            continue;
        }
        rects.push(screen.rect_to_pixels(frame));
    }
    rects
}

/// The frame of window `number`, of any process, in AppKit's points.
pub(crate) fn window_frame(number: isize) -> Option<NSRect> {
    let list = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionIncludingWindow,
        u32::try_from(number).ok()?,
    )?;
    // SAFETY: as in `window_rects`.
    let list: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*list as *const CFArray).cast() };
    let main = screens().first().copied()?;
    let top = main.frame.origin.y + main.frame.size.height;
    list.iter().next().and_then(|dict| frame_of(&dict, top))
}

/// Where the window `dict` tells of is, in AppKit's points; `top` is the top of the main screen.
fn frame_of(dict: &NSDictionary<NSString, AnyObject>, top: f64) -> Option<NSRect> {
    // SAFETY: the key is the window server's constant string, toll-free bridged.
    let key =
        unsafe { &*(kCGWindowBounds as *const objc2_core_foundation::CFString).cast::<NSString>() };
    let bounds = dict
        .objectForKey(key)
        .and_then(|v| v.downcast::<NSDictionary>().ok())?;
    let field = |name: &str| {
        bounds
            .objectForKey(&NSString::from_str(name))
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .map_or(0.0, |n| n.doubleValue())
    };
    let (x, y, w, h) = (field("X"), field("Y"), field("Width"), field("Height"));
    // The window server counts from the top-left of the main screen, AppKit from its bottom.
    Some(NSRect::new(NSPoint::new(x, top - y - h), NSSize::new(w, h)))
}

/// The screen with the largest part of `frame`.
fn screen_of(screens: &[Screen], frame: NSRect) -> Option<Screen> {
    let overlap = |s: &Screen| {
        let f = s.frame;
        let w = (f.origin.x + f.size.width).min(frame.origin.x + frame.size.width)
            - f.origin.x.max(frame.origin.x);
        let h = (f.origin.y + f.size.height).min(frame.origin.y + frame.size.height)
            - f.origin.y.max(frame.origin.y);
        w.max(0.0) * h.max(0.0)
    };
    screens
        .iter()
        .filter(|s| overlap(s) > 0.0)
        .max_by(|a, b| overlap(a).total_cmp(&overlap(b)))
        .copied()
}

fn covers(screen: NSRect, frame: NSRect) -> bool {
    frame.origin.x <= screen.origin.x
        && frame.origin.y <= screen.origin.y
        && frame.origin.x + frame.size.width >= screen.origin.x + screen.size.width
        && frame.origin.y + frame.size.height >= screen.origin.y + screen.size.height
}
