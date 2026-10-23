//! Screens as one virtual desktop of physical pixels, top-left origin, like Windows'. AppKit counts
//! points from the bottom-left of the main screen; a screen's pixel origin is its point origin,
//! flipped, times the largest backing scale `S`, and its pixel size is its point size times its
//! own scale. Screens do not overlap in points, so they do not in pixels either, and every pixel
//! of a screen maps to one of its points.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen};
use objc2_foundation::{NSPoint, NSRect};
use shot_core::geom::Rect;

#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub rect: Rect,
    /// The part not covered by the menu bar and the Dock: where the preview goes.
    pub work: Rect,
    /// The backing scale: 2.0 on Retina.
    pub scale: f32,
    /// The screen with the menu bar.
    pub primary: bool,
}

/// A screen in points and in pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Screen {
    pub monitor: Monitor,
    /// AppKit's frame: points, bottom-left origin.
    pub frame: NSRect,
    /// `CGDirectDisplayID`.
    pub display: u32,
}

impl Screen {
    /// The point (bottom-left origin) of pixel `(x, y)` of this screen.
    #[allow(dead_code)] // The preview cards and the editor (MAC-005) place themselves with it.
    pub fn to_points(self, x: f64, y: f64) -> NSPoint {
        let s = f64::from(self.monitor.scale);
        let r = self.monitor.rect;
        NSPoint::new(
            self.frame.origin.x + (x - f64::from(r.x)) / s,
            self.frame.origin.y + self.frame.size.height - (y - f64::from(r.y)) / s,
        )
    }

    /// The pixel of point `p` (bottom-left origin), which may lie off this screen.
    pub fn to_pixels(self, p: NSPoint) -> (f64, f64) {
        let s = f64::from(self.monitor.scale);
        let r = self.monitor.rect;
        (
            f64::from(r.x) + (p.x - self.frame.origin.x) * s,
            f64::from(r.y) + (self.frame.origin.y + self.frame.size.height - p.y) * s,
        )
    }

    /// A rectangle of points (bottom-left origin) in pixels.
    pub fn rect_to_pixels(self, r: NSRect) -> Rect {
        let (x0, y0) = self.to_pixels(NSPoint::new(r.origin.x, r.origin.y + r.size.height));
        let (x1, y1) = self.to_pixels(NSPoint::new(r.origin.x + r.size.width, r.origin.y));
        let (x0, y0) = (x0.round() as i32, y0.round() as i32);
        Rect::new(x0, y0, x1.round() as i32 - x0, y1.round() as i32 - y0)
    }

    /// A rectangle of this screen's pixels in points (bottom-left origin).
    #[allow(dead_code)] // As `to_points`.
    pub fn rect_to_points(self, r: Rect) -> NSRect {
        let s = f64::from(self.monitor.scale);
        let top_left = self.to_points(f64::from(r.x), f64::from(r.y));
        let h = f64::from(r.height) / s;
        NSRect::new(
            NSPoint::new(top_left.x, top_left.y - h),
            objc2_foundation::NSSize::new(f64::from(r.width) / s, h),
        )
    }
}

/// Every screen, the main one first.
pub(crate) fn screens() -> Vec<Screen> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let list = NSScreen::screens(mtm);
    let Some(main) = list.firstObject() else {
        return Vec::new();
    };
    let top = main.frame().origin.y + main.frame().size.height;
    let max_scale = list
        .iter()
        .map(|s| s.backingScaleFactor())
        .fold(1.0, f64::max);
    list.iter()
        .enumerate()
        .map(|(i, screen)| {
            let frame = screen.frame();
            let scale = screen.backingScaleFactor();
            let rect = Rect::new(
                (frame.origin.x * max_scale).round() as i32,
                ((top - frame.origin.y - frame.size.height) * max_scale).round() as i32,
                (frame.size.width * scale).round() as i32,
                (frame.size.height * scale).round() as i32,
            );
            let mut s = Screen {
                monitor: Monitor {
                    rect,
                    work: rect,
                    scale: scale as f32,
                    primary: i == 0,
                },
                frame,
                display: display_id(&screen),
            };
            s.monitor.work = s.rect_to_pixels(screen.visibleFrame());
            s
        })
        .collect()
}

fn display_id(screen: &NSScreen) -> u32 {
    let key = objc2_foundation::ns_string!("NSScreenNumber");
    screen
        .deviceDescription()
        .objectForKey(key)
        .and_then(|n| n.downcast::<objc2_foundation::NSNumber>().ok())
        .map_or(0, |n| n.unsignedIntValue())
}

/// All monitors, left to right (then top to bottom): the order of `(1)`, `(2)` in file names.
pub fn monitors() -> Vec<Monitor> {
    let mut list: Vec<Monitor> = screens().into_iter().map(|s| s.monitor).collect();
    list.sort_by_key(|m| (m.rect.x, m.rect.y));
    list
}

/// The screen a point (bottom-left origin) is on, else the nearest.
pub(crate) fn screen_at(screens: &[Screen], p: NSPoint) -> Option<&Screen> {
    let distance = |s: &&Screen| {
        let f = s.frame;
        let dx = (f.origin.x - p.x)
            .max(p.x - (f.origin.x + f.size.width))
            .max(0.0);
        let dy = (f.origin.y - p.y)
            .max(p.y - (f.origin.y + f.size.height))
            .max(0.0);
        dx * dx + dy * dy
    };
    screens
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
}

/// The cursor in pixels of the virtual desktop.
pub fn cursor_pos() -> (i32, i32) {
    let p = NSEvent::mouseLocation();
    let screens = screens();
    match screen_at(&screens, p) {
        Some(s) => {
            let (x, y) = s.to_pixels(p);
            let r = s.monitor.rect;
            // The bottom and right edges are on the screen in points but one past it in pixels.
            (
                (x.floor() as i32).clamp(r.x, r.right() - 1),
                (y.floor() as i32).clamp(r.y, r.bottom() - 1),
            )
        }
        None => (0, 0),
    }
}
