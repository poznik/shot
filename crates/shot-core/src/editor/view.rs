//! Where the image lies in the editor window (`docs/design/editor.md`, "Window", "Image"): the
//! window's size, zoom, scroll, and the mapping between window and image pixels.

use tiny_skia::Transform;

use super::objects::Pt;
use crate::geom::Rect;

/// The zoom steps of `Ctrl` + wheel; 1.0 is one image pixel per physical screen pixel.
pub const ZOOM_STEPS: [f32; 11] = [0.10, 0.25, 0.33, 0.50, 0.67, 1.0, 1.5, 2.0, 3.0, 4.0, 8.0];

/// Image area margins, logical px: 12 + toolbar 44 + 12 on top.
const SIDE: f32 = 24.0;
const TOP: f32 = 68.0;
const BOTTOM: f32 = 24.0;
const MIN_WIDTH: f32 = 960.0;
const MIN_HEIGHT: f32 = 600.0;
/// The window takes at most this share of the work area.
const MAX_SHARE: f32 = 0.9;

/// The editor's content size for an image on a work area of a monitor at `scale`: the image at
/// fit scale plus the margins, at most 90% of the work area, at least 960 × 600.
pub fn content_size(image: (u32, u32), work: (u32, u32), scale: f32) -> (u32, u32) {
    let (mw, mh) = (2.0 * SIDE * scale, (TOP + BOTTOM) * scale);
    let (max_w, max_h) = (work.0 as f32 * MAX_SHARE, work.1 as f32 * MAX_SHARE);
    let zoom = fit_zoom(image, (max_w - mw, max_h - mh));
    let w = (image.0 as f32 * zoom + mw)
        .min(max_w)
        .max(MIN_WIDTH * scale);
    let h = (image.1 as f32 * zoom + mh)
        .min(max_h)
        .max(MIN_HEIGHT * scale);
    (
        (w.round() as u32).min(work.0),
        (h.round() as u32).min(work.1),
    )
}

/// The largest zoom that fits `image` into `room`, never above 100%.
fn fit_zoom(image: (u32, u32), room: (f32, f32)) -> f32 {
    let z = (room.0 / image.0.max(1) as f32).min(room.1 / image.1.max(1) as f32);
    z.clamp(0.01, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    image: (u32, u32),
    window: (u32, u32),
    scale: f32,
    zoom: f32,
    /// Follows the window: a resize fits the image again.
    fit: bool,
    /// The image's top-left corner in window px.
    origin: (f32, f32),
}

impl View {
    pub fn new(image: (u32, u32), window: (u32, u32), scale: f32) -> Self {
        let mut view = Self {
            image,
            window,
            scale,
            zoom: 1.0,
            fit: true,
            origin: (0.0, 0.0),
        };
        view.fit();
        view
    }

    pub fn window(&self) -> (u32, u32) {
        self.window
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// The image area: left, top, right, bottom in window px.
    fn area(&self) -> (f32, f32, f32, f32) {
        let s = self.scale;
        (
            SIDE * s,
            TOP * s,
            self.window.0 as f32 - SIDE * s,
            self.window.1 as f32 - BOTTOM * s,
        )
    }

    pub fn fit_zoom(&self) -> f32 {
        let (l, t, r, b) = self.area();
        fit_zoom(self.image, (r - l, b - t))
    }

    pub fn fit(&mut self) {
        self.fit = true;
        self.zoom = self.fit_zoom();
        self.place();
    }

    pub fn is_fit(&self) -> bool {
        self.fit
    }

    /// The image's size on screen, whole pixels.
    fn shown_size(&self) -> (i32, i32) {
        let size = |n: u32| ((n as f32 * self.zoom).round() as i32).max(1);
        (size(self.image.0), size(self.image.1))
    }

    /// Where the image is on screen, window px.
    pub fn shown(&self) -> Rect {
        let (w, h) = self.shown_size();
        Rect::new(
            self.origin.0.round() as i32,
            self.origin.1.round() as i32,
            w,
            h,
        )
    }

    /// Image px to the px of a canvas whose top-left corner is at window px `at`.
    pub fn transform(&self, at: (i32, i32)) -> Transform {
        let r = self.shown();
        Transform::from_row(
            r.width as f32 / self.image.0 as f32,
            0.0,
            0.0,
            r.height as f32 / self.image.1 as f32,
            (r.x - at.0) as f32,
            (r.y - at.1) as f32,
        )
    }

    /// The image point under window px (`x`, `y`), not clamped to the image.
    pub fn to_image(&self, x: f32, y: f32) -> Pt {
        let r = self.shown();
        Pt::new(
            (x - r.x as f32) * self.image.0 as f32 / r.width as f32,
            (y - r.y as f32) * self.image.1 as f32 / r.height as f32,
        )
    }

    /// Window px of image point `p`.
    pub fn to_window(&self, p: Pt) -> (f32, f32) {
        let r = self.shown();
        (
            r.x as f32 + p.x * r.width as f32 / self.image.0 as f32,
            r.y as f32 + p.y * r.height as f32 / self.image.1 as f32,
        )
    }

    /// Image px per window px: how far a window distance reaches on the image.
    pub fn image_per_window(&self) -> f32 {
        1.0 / self.zoom
    }

    /// Zooms to `zoom`, keeping the image point under (`x`, `y`) in place.
    pub fn zoom_at(&mut self, zoom: f32, x: f32, y: f32) {
        let p = self.to_image(x, y);
        self.fit = false;
        self.zoom = zoom.clamp(ZOOM_STEPS[0], ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
        self.origin = (x - p.x * self.zoom, y - p.y * self.zoom);
        self.place();
    }

    /// `steps` zoom steps in (positive) or out from the current zoom, around (`x`, `y`).
    pub fn zoom_steps(&mut self, steps: i32, x: f32, y: f32) {
        let mut zoom = self.zoom;
        for _ in 0..steps.unsigned_abs() {
            zoom = if steps > 0 {
                ZOOM_STEPS.into_iter().find(|&z| z > zoom + 1e-3)
            } else {
                ZOOM_STEPS.into_iter().rev().find(|&z| z < zoom - 1e-3)
            }
            .unwrap_or(zoom);
        }
        if zoom != self.zoom {
            self.zoom_at(zoom, x, y);
        }
    }

    /// 100% around the middle of the image area.
    pub fn actual_size(&mut self) {
        let (l, t, r, b) = self.area();
        self.zoom_at(1.0, (l + r) / 2.0, (t + b) / 2.0);
    }

    /// Moves the image by (`dx`, `dy`) window px, as far as it can go.
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        self.origin.0 += dx;
        self.origin.1 += dy;
        self.place();
    }

    /// The image is larger than the image area along some axis: there is something to scroll.
    pub fn scrolls(&self) -> bool {
        let (l, t, r, b) = self.area();
        let (w, h) = self.shown_size();
        w as f32 > r - l + 0.5 || h as f32 > b - t + 0.5
    }

    pub fn resize(&mut self, window: (u32, u32), scale: f32) {
        self.window = window;
        self.scale = scale;
        if self.fit {
            self.fit();
        } else {
            self.place();
        }
    }

    /// Centers the image along an axis where it fits the image area; elsewhere keeps the area
    /// covered.
    fn place(&mut self) {
        let (l, t, r, b) = self.area();
        let (w, h) = self.shown_size();
        let axis = |o: f32, lo: f32, hi: f32, len: f32| {
            if len <= hi - lo {
                lo + (hi - lo - len) / 2.0
            } else {
                o.clamp(hi - len, lo)
            }
        };
        self.origin = (
            axis(self.origin.0, l, r, w as f32),
            axis(self.origin.1, t, b, h as f32),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_fits_the_image_with_margins() {
        // A small capture: 100%, but never below 960 × 600.
        assert_eq!(content_size((400, 300), (2560, 1400), 1.0), (960, 600));
        // 1200 × 700 at 100% plus margins.
        assert_eq!(content_size((1200, 700), (2560, 1400), 1.0), (1248, 792));
        // A full-screen capture: 90% of the work area.
        assert_eq!(content_size((2560, 1440), (2560, 1400), 1.0), (2124, 1260));
        // At 200% the minimum doubles.
        assert_eq!(content_size((400, 300), (3840, 2100), 2.0), (1920, 1200));
    }

    #[test]
    fn fit_centers_and_never_enlarges() {
        let v = View::new((400, 300), (960, 600), 1.0);
        assert_eq!(v.zoom(), 1.0);
        // Image area 24..936 × 68..576.
        assert_eq!(v.shown(), Rect::new(280, 172, 400, 300));

        let v = View::new((2560, 1440), (1248, 792), 1.0);
        // Area 1200 × 700: width limits, 1200 / 2560.
        assert!((v.zoom() - 1200.0 / 2560.0).abs() < 1e-6);
        assert_eq!(v.shown().x, 24);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut v = View::new((2000, 1000), (1248, 792), 1.0);
        let before = v.to_image(500.0, 400.0);
        v.zoom_steps(1, 500.0, 400.0);
        assert!(!v.is_fit());
        // From 60% the next step is 67%.
        assert_eq!(v.zoom(), 0.67);
        let after = v.to_image(500.0, 400.0);
        // Horizontally only: 670 px still fit the area's height, so the image stays centered.
        assert!((before.x - after.x).abs() < 2.0, "{before:?} {after:?}");

        v.zoom_steps(3, 500.0, 400.0);
        assert_eq!(v.zoom(), 2.0);
        v.zoom_steps(-1, 500.0, 400.0);
        assert_eq!(v.zoom(), 1.5);
        v.zoom_steps(20, 0.0, 0.0);
        assert_eq!(v.zoom(), 8.0);
        v.fit();
        assert!((v.zoom() - 0.6).abs() < 1e-6);
    }

    #[test]
    fn scrolling_stops_at_the_image_edges() {
        let mut v = View::new((2000, 1000), (1248, 792), 1.0);
        v.actual_size();
        assert!(v.scrolls());
        v.scroll(10_000.0, 10_000.0);
        // The top-left corner rests on the area's.
        assert_eq!((v.shown().x, v.shown().y), (24, 68));
        v.scroll(-10_000.0, -10_000.0);
        assert_eq!(v.shown().right(), 1248 - 24);
        assert_eq!(v.shown().bottom(), 792 - 24);
    }

    #[test]
    fn mapping_round_trips() {
        let mut v = View::new((2000, 1000), (1248, 792), 1.25);
        v.zoom_steps(2, 300.0, 300.0);
        let p = Pt::new(812.0, 333.0);
        let (x, y) = v.to_window(p);
        assert!(v.to_image(x, y).dist(p) < 1e-3);
        let t = v.transform((100, 50));
        let (tx, ty) = (p.x * t.sx + t.tx, p.y * t.sy + t.ty);
        assert!((tx - (x - 100.0)).abs() < 1e-3 && (ty - (y - 50.0)).abs() < 1e-3);
    }
}
