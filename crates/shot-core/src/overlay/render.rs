//! The overlay as the platform drives it: input in; damaged rects and their pixels out.

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::canvas::{self, skia_color};
use crate::geom::{Point, Rect};
use crate::image::{BgrxMut, BgrxView};
use crate::text;
use crate::theme::{
    Color, OVERLAY_CROSSHAIR, OVERLAY_CROSSHAIR_HALO, OVERLAY_DIM, OVERLAY_FRAME,
    OVERLAY_FRAME_HALO, OVERLAY_LABEL_BG, OVERLAY_LABEL_TEXT, OVERLAY_LABEL_TEXT_DIM, RADIUS_MD,
    RADIUS_XS, TYPE_CAPTION, TYPE_LABEL, scaled,
};

use super::layout::{
    HINT, HINT_HEIGHT, HINT_KEY_GAP, HINT_PAD_X, HINT_PAD_Y, HINT_PAIR_GAP, LABEL_PAD_X,
    LABEL_PAD_Y, Label, Layout, hint_width, label_text,
};
use super::selection::{Event, Outcome, Selection};

/// A rect to redraw on one monitor, in that monitor's own pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Damage {
    pub monitor: usize,
    pub rect: Rect,
}

/// What one input changed.
#[derive(Debug, Default, PartialEq)]
pub struct Update {
    pub outcome: Option<Outcome>,
    pub damage: Vec<Damage>,
}

pub struct Overlay {
    selection: Selection,
    show_hint: bool,
    layouts: Vec<Layout>,
    label: Option<(Label, Pixmap)>,
    hint: Option<(f32, Pixmap)>,
}

impl Overlay {
    /// `show_hint` is true for the first five captures.
    pub fn new(selection: Selection, show_hint: bool) -> Self {
        let layouts = (0..selection.monitors().len())
            .map(|i| Layout::new(&selection, i, show_hint))
            .collect();
        Self {
            selection,
            show_hint,
            layouts,
            label: None,
            hint: None,
        }
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Applies one input and reports the rects to redraw with [`Overlay::render`].
    pub fn handle(&mut self, event: Event) -> Update {
        let outcome = self.selection.handle(event);
        let mut damage = Vec::new();
        for (i, m) in self.selection.monitors().iter().enumerate() {
            let new = Layout::new(&self.selection, i, self.show_hint);
            let bounds = Rect::new(0, 0, m.rect.width, m.rect.height);
            let rects = self.layouts[i].damage(&new, bounds);
            damage.extend(rects.into_iter().map(|rect| Damage { monitor: i, rect }));
            self.layouts[i] = new;
        }
        Update { outcome, damage }
    }

    /// Draws the `clip` part of one monitor into `canvas`: the frozen `frame` of that monitor,
    /// which has the canvas's size, and every overlay layer over it.
    pub fn render(&mut self, monitor: usize, frame: &BgrxView, canvas: &mut BgrxMut, clip: Rect) {
        debug_assert_eq!((frame.width, frame.height), (canvas.width, canvas.height));
        let bounds = Rect::new(0, 0, canvas.width as i32, canvas.height as i32);
        let Some(clip) = clip.intersect(&bounds) else {
            return;
        };
        let l = self.layouts[monitor];

        // Frozen frame, dimmed outside the target.
        if let Some(inner) = l.target.intersect(&clip) {
            canvas::copy(canvas, frame, inner);
        }
        for part in clip.subtract(&l.target) {
            canvas::copy_under(canvas, frame, part, OVERLAY_DIM);
        }

        // Frame and halo outside the target.
        for r in ring(l.target, l.frame) {
            fill(canvas, r, clip, OVERLAY_FRAME);
        }
        for r in ring(l.target.inflate(l.frame), l.halo) {
            fill(canvas, r, clip, OVERLAY_FRAME_HALO);
        }

        if let Some(hint) = l.hint {
            let at = Point::new(hint.rect.x, hint.rect.y);
            canvas::draw_sprite(canvas, self.hint_sprite(hint.scale), at, clip);
        }
        if let Some(label) = l.label {
            let at = Point::new(label.rect.x, label.rect.y);
            canvas::draw_sprite(canvas, self.label_sprite(label), at, clip);
        }
        if let Some(c) = l.crosshair {
            // Arms and their halos never overlap each other, so the order is free.
            for arm in c.arms() {
                fill(canvas, arm.inflate(c.halo), clip, OVERLAY_CROSSHAIR_HALO);
                fill(canvas, arm, clip, OVERLAY_CROSSHAIR);
            }
        }
    }

    fn label_sprite(&mut self, label: Label) -> &Pixmap {
        if self
            .label
            .as_ref()
            .is_none_or(|(l, _)| (l.size, l.scale) != (label.size, label.scale))
        {
            self.label = Some((label, draw_label(label)));
        }
        &self.label.as_ref().expect("just drawn").1
    }

    fn hint_sprite(&mut self, scale: f32) -> &Pixmap {
        if self.hint.as_ref().is_none_or(|(s, _)| *s != scale) {
            self.hint = Some((scale, draw_hint(scale)));
        }
        &self.hint.as_ref().expect("just drawn").1
    }
}

/// The band of width `w` directly outside `r`.
fn ring(r: Rect, w: i32) -> impl Iterator<Item = Rect> {
    r.inflate(w).subtract(&r)
}

fn fill(canvas: &mut BgrxMut, rect: Rect, clip: Rect, color: Color) {
    if let Some(r) = rect.intersect(&clip) {
        canvas::fill(canvas, r, color);
    }
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint
}

/// Cubic Bézier control distance for a quarter circle of radius 1.
const KAPPA: f32 = 0.552_284_8;

fn fill_rounded(pixmap: &mut Pixmap, radius: f32, color: Color) {
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    let r = radius.min(w / 2.0).min(h / 2.0);
    let c = r * (1.0 - KAPPA);
    let mut p = PathBuilder::new();
    p.move_to(r, 0.0);
    p.line_to(w - r, 0.0);
    p.cubic_to(w - c, 0.0, w, c, w, r);
    p.line_to(w, h - r);
    p.cubic_to(w, h - c, w - c, h, w - r, h);
    p.line_to(r, h);
    p.cubic_to(c, h, 0.0, h - c, 0.0, h - r);
    p.line_to(0.0, r);
    p.cubic_to(0.0, c, c, 0.0, r, 0.0);
    p.close();
    if let Some(path) = p.finish() {
        let paint = paint(color);
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn draw_label(label: Label) -> Pixmap {
    let s = label.scale;
    let (w, h) = (label.rect.width as u32, label.rect.height as u32);
    let mut pixmap = Pixmap::new(w, h).expect("label has a size");
    fill_rounded(&mut pixmap, scaled(RADIUS_XS, s) as f32, OVERLAY_LABEL_BG);
    let top = scaled(LABEL_PAD_Y, s) as f32;
    let mut x = scaled(LABEL_PAD_X, s) as f32;
    let [width, times, height] = label_text(label.size);
    for (text, color) in [
        (width, OVERLAY_LABEL_TEXT),
        (times, OVERLAY_LABEL_TEXT_DIM),
        (height, OVERLAY_LABEL_TEXT),
    ] {
        x = text::draw(&mut pixmap, TYPE_LABEL, s, &text, x, top, &paint(color));
    }
    pixmap
}

fn draw_hint(s: f32) -> Pixmap {
    let (w, h) = (hint_width(s) as u32, scaled(HINT_HEIGHT, s) as u32);
    let mut pixmap = Pixmap::new(w, h).expect("hint has a size");
    fill_rounded(&mut pixmap, scaled(RADIUS_MD, s) as f32, OVERLAY_LABEL_BG);
    let top = scaled(HINT_PAD_Y, s) as f32;
    // The caption's shorter line box is centered on the key's.
    let lines = scaled(TYPE_LABEL.line_height, s) - scaled(TYPE_CAPTION.line_height, s);
    let caption_top = top + lines as f32 / 2.0;
    let (key, action) = (paint(OVERLAY_LABEL_TEXT), paint(OVERLAY_LABEL_TEXT_DIM));
    let mut x = scaled(HINT_PAD_X, s) as f32;
    for (i, (key_text, action_text)) in HINT.iter().enumerate() {
        if i > 0 {
            x += scaled(HINT_PAIR_GAP, s) as f32;
        }
        x = text::draw(&mut pixmap, TYPE_LABEL, s, key_text, x, top, &key);
        x += scaled(HINT_KEY_GAP, s) as f32;
        x = text::draw(
            &mut pixmap,
            TYPE_CAPTION,
            s,
            action_text,
            x,
            caption_top,
            &action,
        );
    }
    pixmap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::Monitor;

    /// A frame with a different value in every channel of nearby pixels.
    fn pattern(width: i32, height: i32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                data.extend([(x * 3) as u8, (y * 5) as u8, (x ^ y) as u8, 0]);
            }
        }
        data
    }

    struct Screen {
        frame: Vec<u8>,
        canvas: Vec<u8>,
        width: i32,
        height: i32,
    }

    impl Screen {
        fn new(rect: Rect) -> Self {
            let frame = pattern(rect.width, rect.height);
            let canvas = vec![0; frame.len()];
            Self {
                frame,
                canvas,
                width: rect.width,
                height: rect.height,
            }
        }

        fn draw(&mut self, overlay: &mut Overlay, monitor: usize, clip: Rect) {
            let (w, h) = (self.width as u32, self.height as u32);
            let frame = BgrxView {
                width: w,
                height: h,
                stride: w as usize * 4,
                data: &self.frame,
            };
            let mut canvas = BgrxMut {
                width: w,
                height: h,
                stride: w as usize * 4,
                data: &mut self.canvas,
            };
            overlay.render(monitor, &frame, &mut canvas, clip);
        }

        fn full(&mut self, overlay: &mut Overlay, monitor: usize) {
            self.draw(overlay, monitor, Rect::new(0, 0, self.width, self.height));
        }
    }

    #[test]
    fn redrawing_damage_matches_a_full_redraw() {
        let monitors = vec![
            Monitor {
                rect: Rect::new(0, 0, 800, 500),
                work: Rect::new(0, 0, 800, 460),
                scale: 1.0,
            },
            Monitor {
                rect: Rect::new(-400, -100, 400, 640),
                work: Rect::new(-400, -100, 400, 640),
                scale: 1.25,
            },
        ];
        let windows = vec![Rect::new(50, 40, 300, 200), Rect::new(-150, 100, 500, 300)];
        let p = Point::new;
        let events = [
            Event::Move(p(60, 50)),
            Event::Move(p(200, 300)),
            Event::Move(p(500, 450)),
            Event::Move(p(-100, 200)),
            Event::Move(p(-300, 500)),
            Event::Move(p(780, 480)),
            Event::Press(p(700, 100)),
            Event::Move(p(702, 101)),
            Event::Move(p(650, 140)),
            Event::Move(p(300, 420)),
            Event::Move(p(-200, 440)),
            Event::Move(p(10, 495)),
            Event::Move(p(795, 5)),
        ];
        let mut overlay = Overlay::new(
            Selection::new(monitors.clone(), windows, 4, p(400, 30)),
            true,
        );
        let mut screens: Vec<Screen> = monitors.iter().map(|m| Screen::new(m.rect)).collect();
        for (i, screen) in screens.iter_mut().enumerate() {
            screen.full(&mut overlay, i);
        }
        for event in events {
            let update = overlay.handle(event);
            for d in &update.damage {
                screens[d.monitor].draw(&mut overlay, d.monitor, d.rect);
            }
            for (i, screen) in screens.iter_mut().enumerate() {
                let mut fresh = Screen::new(monitors[i].rect);
                fresh.full(&mut overlay, i);
                assert!(
                    screen.canvas == fresh.canvas,
                    "monitor {i} differs after {event:?}"
                );
            }
        }
    }

    #[test]
    fn dim_keeps_the_target_clear() {
        let rect = Rect::new(0, 0, 64, 48);
        let monitor = Monitor {
            rect,
            work: rect,
            scale: 1.0,
        };
        let window = Rect::new(10, 10, 20, 20);
        let selection = Selection::new(vec![monitor], vec![window], 4, Point::new(15, 15));
        let mut overlay = Overlay::new(selection, false);
        let mut screen = Screen::new(rect);
        screen.full(&mut overlay, 0);

        let px = |s: &Screen, x: i32, y: i32| {
            let i = ((y * s.width + x) * 4) as usize;
            [s.canvas[i], s.canvas[i + 1], s.canvas[i + 2]]
        };
        let src = |s: &Screen, x: i32, y: i32| {
            let i = ((y * s.width + x) * 4) as usize;
            [s.frame[i], s.frame[i + 1], s.frame[i + 2]]
        };
        // Inside the window, away from the crosshair: untouched.
        assert_eq!(px(&screen, 28, 28), src(&screen, 28, 28));
        // The 2 px frame, then the halo, then the dim.
        assert_eq!(px(&screen, 9, 20), [255, 255, 255]);
        assert_eq!(px(&screen, 8, 20), [255, 255, 255]);
        let dim = |v: u8| (v as f32 * 140.0 / 255.0).round() as u8;
        let halo = |v: u8| (dim(v) as f32 * 153.0 / 255.0).round() as u8;
        assert_eq!(px(&screen, 7, 20), src(&screen, 7, 20).map(halo));
        assert_eq!(px(&screen, 50, 40), src(&screen, 50, 40).map(dim));
    }
}
