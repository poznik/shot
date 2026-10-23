//! The annotations (`docs/design/editor.md`, "Objects"): what each one is, its sizes, where it
//! lies and what the mouse hits. Coordinates are image pixels; sizes are the tokens times the
//! scale of the monitor the capture came from (`k`), so an M stroke is 8 px in a 200% capture.

use std::ops::{Add, Mul, Sub};

use crate::text::{self, Weight};
use crate::theme::Color;

/// A point or a vector in image pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn len(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn dist(self, o: Pt) -> f32 {
        (self - o).len()
    }

    fn min(self, o: Pt) -> Pt {
        Pt::new(self.x.min(o.x), self.y.min(o.y))
    }

    fn max(self, o: Pt) -> Pt {
        Pt::new(self.x.max(o.x), self.y.max(o.y))
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Pt {
    type Output = Pt;
    fn mul(self, k: f32) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
}

/// An axis-aligned box in image pixels, `min` ≤ `max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: Pt,
    pub max: Pt,
}

impl Bounds {
    pub fn of(a: Pt, b: Pt) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    pub fn inflate(self, d: f32) -> Self {
        Self {
            min: self.min - Pt::new(d, d),
            max: self.max + Pt::new(d, d),
        }
    }

    pub fn union(self, o: Bounds) -> Self {
        Self {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Pt {
        (self.min + self.max) * 0.5
    }
}

/// One of the eight annotation colors (`tokens.md`, "Annotations").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swatch {
    pub color: Color,
    /// The counter's glyph on this color.
    pub glyph: Color,
    pub halo: Color,
}

pub const HALO_DARK: Color = Color::hex(0x0000_0059);
pub const HALO_LIGHT: Color = Color::hex(0xFFFF_FF99);
const ON_DARK: Color = Color::hex(0xFFFF_FFFF);
const ON_LIGHT: Color = Color::hex(0x1616_1AFF);

pub const SWATCHES: [Swatch; 8] = [
    swatch(0xE534_3EFF, ON_DARK, HALO_DARK),
    swatch(0xFF8A_1FFF, ON_LIGHT, HALO_DARK),
    swatch(0xFFD4_26FF, ON_LIGHT, HALO_DARK),
    swatch(0x1A98_50FF, ON_DARK, HALO_DARK),
    swatch(0x2A72_F0FF, ON_DARK, HALO_DARK),
    swatch(0x934B_F0FF, ON_DARK, HALO_DARK),
    swatch(0x1A1A_1EFF, ON_DARK, HALO_LIGHT),
    swatch(0xFFFF_FFFF, ON_LIGHT, HALO_DARK),
];

const fn swatch(rgba: u32, glyph: Color, halo: Color) -> Swatch {
    Swatch {
        color: Color::hex(rgba),
        glyph,
        halo,
    }
}

/// `selection`: the hover outline, the selection box and the handle ring.
pub const SELECTION: Color = Color::hex(0x4651_F6FF);
/// `selection.handle-fill`.
pub const HANDLE_FILL: Color = Color::hex(0xFFFF_FFFF);

/// The halo on each side of an object, at 100%.
const HALO: f32 = 1.0;
/// How far inside the head the arrow's shaft ends.
const SHAFT_INSIDE_HEAD: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Width {
    S,
    M,
    L,
}

impl Width {
    pub const ALL: [Width; 3] = [Width::S, Width::M, Width::L];

    /// The stroke at 100%.
    pub fn stroke(self) -> f32 {
        match self {
            Width::S => 2.0,
            Width::M => 4.0,
            Width::L => 8.0,
        }
    }

    fn counter(self) -> f32 {
        match self {
            Width::S => 22.0,
            Width::M => 28.0,
            Width::L => 36.0,
        }
    }

    fn glyph(self) -> f32 {
        match self {
            Width::S => 12.0,
            Width::M => 15.0,
            Width::L => 19.0,
        }
    }
}

/// How counters count: 1, 2, 3 or A, B, C.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Counting {
    Numbers,
    Letters,
}

/// The text of counter `n` (from 1): `12`, or `L`; after `Z` comes `AA`.
pub fn counter_label(counting: Counting, n: u32) -> String {
    match counting {
        Counting::Numbers => n.to_string(),
        Counting::Letters => {
            let mut n = n.max(1);
            let mut out = Vec::new();
            while n > 0 {
                n -= 1;
                out.push(b'A' + (n % 26) as u8);
                n /= 26;
            }
            out.reverse();
            String::from_utf8(out).unwrap_or_default()
        }
    }
}

/// The number the next counter of its kind gets: the largest on the image plus one. Deleting does
/// not renumber.
pub fn next_counter(objects: &[Object], counting: Counting) -> u32 {
    objects
        .iter()
        .filter_map(|o| match o.shape {
            Shape::Counter { counting: c, n, .. } if c == counting => Some(n),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        + 1
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Line {
        a: Pt,
        b: Pt,
    },
    /// From the tail `a` to the tip `b`.
    Arrow {
        a: Pt,
        b: Pt,
    },
    /// The drag rect, corners in the order dragged.
    Rect {
        a: Pt,
        b: Pt,
    },
    /// Inscribed in the drag rect.
    Ellipse {
        a: Pt,
        b: Pt,
    },
    Counter {
        at: Pt,
        counting: Counting,
        n: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Object {
    pub shape: Shape,
    /// Index into [`SWATCHES`].
    pub color: usize,
    pub width: Width,
}

/// The sizes of one object in image pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    pub stroke: f32,
    pub head: f32,
    pub counter: f32,
    pub glyph: f32,
    pub halo: f32,
}

/// A handle of a selected object: the end points of a line or an arrow (0 = tail, 1 = tip), or
/// the corners and edge midpoints of a drag rect clockwise from the top-left (0–7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(pub u8);

/// Which way a handle resizes, for the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Up and down.
    Vertical,
    Horizontal,
    /// Top-left to bottom-right.
    Falling,
    Rising,
}

impl Object {
    pub fn swatch(&self) -> &'static Swatch {
        &SWATCHES[self.color.min(SWATCHES.len() - 1)]
    }

    pub fn sizes(&self, k: f32) -> Sizes {
        let w = self.width;
        Sizes {
            stroke: w.stroke() * k,
            head: (3.0 * w.stroke() + 10.0) * k,
            counter: w.counter() * k,
            glyph: w.glyph() * k,
            halo: HALO * k,
        }
    }

    /// The counter's text and the size of its body: a circle for one or two characters, a
    /// capsule of height `d` and width `text + d/2` for more.
    pub fn counter_body(&self, k: f32) -> Option<(String, f32, f32)> {
        let Shape::Counter { counting, n, .. } = self.shape else {
            return None;
        };
        let s = self.sizes(k);
        let label = counter_label(counting, n);
        let w = if label.chars().count() <= 2 {
            s.counter
        } else {
            text::advance(Weight::SemiBold, s.glyph, &label) + s.counter / 2.0
        };
        Some((label, w.max(s.counter), s.counter))
    }

    /// Everything the object paints, halo included.
    pub fn bounds(&self, k: f32) -> Bounds {
        let s = self.sizes(k);
        match self.shape {
            Shape::Line { a, b } => Bounds::of(a, b).inflate(s.stroke / 2.0 + s.halo),
            Shape::Arrow { a, b } => {
                let reach = (s.head / 2.0 + s.stroke / 4.0).max(s.stroke / 2.0);
                Bounds::of(a, b).inflate(reach + s.halo)
            }
            Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
                Bounds::of(a, b).inflate(s.stroke / 2.0 + s.halo)
            }
            Shape::Counter { at, .. } => {
                let (_, w, h) = self.counter_body(k).unwrap_or_default();
                let half = Pt::new(w / 2.0, h / 2.0);
                Bounds::of(at - half, at + half).inflate(s.halo)
            }
        }
    }

    /// The point lies on what the object paints, or within `slack` of it. A rectangle or an
    /// ellipse is hit on its outline only: what is inside stays reachable.
    pub fn hit(&self, p: Pt, k: f32, slack: f32) -> bool {
        let s = self.sizes(k);
        let reach = s.stroke / 2.0 + s.halo + slack;
        match self.shape {
            Shape::Line { a, b } => segment_distance(p, a, b) <= reach,
            Shape::Arrow { a, b } => {
                if segment_distance(p, a, b) <= reach {
                    return true;
                }
                let (_, tri) = arrow_parts(a, b, s.head);
                inside_triangle(p, tri) || {
                    let edge = (0..3).map(|i| segment_distance(p, tri[i], tri[(i + 1) % 3]));
                    edge.fold(f32::MAX, f32::min) <= s.stroke / 4.0 + s.halo + slack
                }
            }
            Shape::Rect { a, b } => {
                let r = Bounds::of(a, b);
                let outside = Pt::new(
                    (r.min.x - p.x).max(p.x - r.max.x).max(0.0),
                    (r.min.y - p.y).max(p.y - r.max.y).max(0.0),
                );
                let inside = (p.x - r.min.x)
                    .min(r.max.x - p.x)
                    .min(p.y - r.min.y)
                    .min(r.max.y - p.y);
                let d = if outside.len() > 0.0 {
                    outside.len()
                } else {
                    inside
                };
                d <= reach
            }
            Shape::Ellipse { a, b } => {
                let r = Bounds::of(a, b);
                let c = r.center();
                let (rx, ry) = (r.width() / 2.0, r.height() / 2.0);
                if rx < 0.5 || ry < 0.5 {
                    return segment_distance(p, r.min, r.max) <= reach;
                }
                let d = p - c;
                let t = Pt::new(d.x / rx, d.y / ry).len();
                if t < 1e-6 {
                    return rx.min(ry) <= reach;
                }
                // Along the ray from the center: where it crosses the ellipse is d / t.
                d.len() * (1.0 - 1.0 / t).abs() <= reach
            }
            Shape::Counter { at, .. } => {
                let (_, w, h) = self.counter_body(k).unwrap_or_default();
                let r = h / 2.0;
                let half = (w / 2.0 - r).max(0.0);
                let a = at - Pt::new(half, 0.0);
                let b = at + Pt::new(half, 0.0);
                segment_distance(p, a, b) <= r + s.halo + slack
            }
        }
    }

    /// The handles of the selected object and where they are. A counter has none: its size
    /// comes from the width.
    pub fn handles(&self) -> Vec<(Handle, Pt)> {
        match self.shape {
            Shape::Line { a, b } | Shape::Arrow { a, b } => vec![(Handle(0), a), (Handle(1), b)],
            Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
                let r = Bounds::of(a, b);
                let c = r.center();
                let (l, t, rr, bb) = (r.min.x, r.min.y, r.max.x, r.max.y);
                [
                    Pt::new(l, t),
                    Pt::new(c.x, t),
                    Pt::new(rr, t),
                    Pt::new(rr, c.y),
                    Pt::new(rr, bb),
                    Pt::new(c.x, bb),
                    Pt::new(l, bb),
                    Pt::new(l, c.y),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, p)| (Handle(i as u8), p))
                .collect()
            }
            Shape::Counter { .. } => Vec::new(),
        }
    }

    /// Which way handle `h` resizes.
    pub fn handle_axis(&self, h: Handle) -> Axis {
        match self.shape {
            Shape::Line { a, b } | Shape::Arrow { a, b } => {
                // The cursor follows the line's direction, in 45° sectors.
                let d = b - a;
                let angle = d.y.atan2(d.x).to_degrees().rem_euclid(180.0);
                match angle {
                    a if !(22.5..157.5).contains(&a) => Axis::Horizontal,
                    a if a < 67.5 => Axis::Falling,
                    a if a < 112.5 => Axis::Vertical,
                    _ => Axis::Rising,
                }
            }
            _ => match h.0 % 4 {
                0 => Axis::Falling,
                1 => Axis::Vertical,
                2 => Axis::Rising,
                _ => Axis::Horizontal,
            },
        }
    }

    /// The object as `original` with handle `h` dragged to `to`.
    pub fn drag_handle(&mut self, original: &Object, h: Handle, to: Pt) {
        self.shape = match original.shape {
            Shape::Line { b, .. } if h.0 == 0 => Shape::Line { a: to, b },
            Shape::Line { a, .. } => Shape::Line { a, b: to },
            Shape::Arrow { b, .. } if h.0 == 0 => Shape::Arrow { a: to, b },
            Shape::Arrow { a, .. } => Shape::Arrow { a, b: to },
            Shape::Rect { a, b } => {
                let (a, b) = drag_box(a, b, h, to);
                Shape::Rect { a, b }
            }
            Shape::Ellipse { a, b } => {
                let (a, b) = drag_box(a, b, h, to);
                Shape::Ellipse { a, b }
            }
            shape @ Shape::Counter { .. } => shape,
        };
    }

    /// The object moved by `d`.
    pub fn translate(&mut self, d: Pt) {
        match &mut self.shape {
            Shape::Line { a, b }
            | Shape::Arrow { a, b }
            | Shape::Rect { a, b }
            | Shape::Ellipse { a, b } => {
                *a = *a + d;
                *b = *b + d;
            }
            Shape::Counter { at, .. } => *at = *at + d,
        }
    }

    /// Too small to keep after a drag: a click without a drag.
    pub fn is_degenerate(&self, min: f32) -> bool {
        match self.shape {
            Shape::Line { a, b } | Shape::Arrow { a, b } => a.dist(b) < min,
            Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
                (a.x - b.x).abs() < min || (a.y - b.y).abs() < min
            }
            Shape::Counter { .. } => false,
        }
    }
}

/// The new corners of a drag rect `a`–`b` with handle `h` at `to`. Corners move both edges they
/// touch; edge midpoints one.
fn drag_box(a: Pt, b: Pt, h: Handle, to: Pt) -> (Pt, Pt) {
    let r = Bounds::of(a, b);
    let (mut l, mut t, mut rr, mut bb) = (r.min.x, r.min.y, r.max.x, r.max.y);
    match h.0 {
        0 => (l, t) = (to.x, to.y),
        1 => t = to.y,
        2 => (rr, t) = (to.x, to.y),
        3 => rr = to.x,
        4 => (rr, bb) = (to.x, to.y),
        5 => bb = to.y,
        6 => (l, bb) = (to.x, to.y),
        _ => l = to.x,
    }
    (Pt::new(l, t), Pt::new(rr, bb))
}

/// The end point of a drag from `a` to `b` with `Shift`: lines and arrows snap to 15° steps,
/// keeping their length; a rect becomes a square of the longer side.
pub fn constrain(shape_is_line: bool, a: Pt, b: Pt) -> Pt {
    let d = b - a;
    if shape_is_line {
        let step = 15f32.to_radians();
        let angle = (d.y.atan2(d.x) / step).round() * step;
        let len = d.len();
        a + Pt::new(angle.cos(), angle.sin()) * len
    } else {
        let side = d.x.abs().max(d.y.abs());
        a + Pt::new(side.copysign(d.x), side.copysign(d.y))
    }
}

/// The arrow's shaft (from the tail to 2 px inside the head), if any shows outside the head, and
/// its head: tip, then the two base corners. A head longer than the arrow shrinks to it.
pub fn arrow_parts(a: Pt, b: Pt, head: f32) -> (Option<(Pt, Pt)>, [Pt; 3]) {
    let d = b - a;
    let len = d.len();
    if len < 1e-3 {
        return (None, [b, b, b]);
    }
    let u = d * (1.0 / len);
    let n = Pt::new(-u.y, u.x);
    let l = head.min(len);
    let base = b - u * l;
    let tri = [b, base + n * (l / 2.0), base - n * (l / 2.0)];
    let shaft = (len > l).then(|| (a, b - u * (l - SHAFT_INSIDE_HEAD).max(0.0)));
    (shaft, tri)
}

fn segment_distance(p: Pt, a: Pt, b: Pt) -> f32 {
    let ab = b - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 < 1e-9 {
        return p.dist(a);
    }
    let ap = p - a;
    let t = ((ap.x * ab.x + ap.y * ab.y) / len2).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

fn inside_triangle(p: Pt, t: [Pt; 3]) -> bool {
    let cross = |a: Pt, b: Pt, c: Pt| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let d = [
        cross(t[0], t[1], p),
        cross(t[1], t[2], p),
        cross(t[2], t[0], p),
    ];
    d.iter().all(|&v| v >= 0.0) || d.iter().all(|&v| v <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(shape: Shape) -> Object {
        Object {
            shape,
            color: 0,
            width: Width::M,
        }
    }

    #[test]
    fn letters_run_past_z() {
        let l = |n| counter_label(Counting::Letters, n);
        assert_eq!(
            [l(1), l(2), l(26), l(27), l(28), l(52), l(53)],
            ["A", "B", "Z", "AA", "AB", "AZ", "BA"]
        );
        assert_eq!(l(26 + 26 * 26), "ZZ");
        assert_eq!(l(26 + 26 * 26 + 1), "AAA");
        assert_eq!(counter_label(Counting::Numbers, 12), "12");
    }

    #[test]
    fn numbering_takes_the_largest_of_its_kind_plus_one() {
        let counter = |counting, n| {
            object(Shape::Counter {
                at: Pt::default(),
                counting,
                n,
            })
        };
        let mut objects = vec![
            counter(Counting::Numbers, 1),
            counter(Counting::Numbers, 4),
            counter(Counting::Letters, 2),
        ];
        assert_eq!(next_counter(&objects, Counting::Numbers), 5);
        assert_eq!(next_counter(&objects, Counting::Letters), 3);
        // Deleting the middle one does not renumber, nor reuse its number.
        objects.remove(0);
        assert_eq!(next_counter(&objects, Counting::Numbers), 5);
        assert_eq!(next_counter(&[], Counting::Letters), 1);
    }

    #[test]
    fn sizes_follow_the_tokens_and_the_capture_scale() {
        let o = object(Shape::Line {
            a: Pt::default(),
            b: Pt::new(10.0, 0.0),
        });
        let s = o.sizes(1.0);
        assert_eq!(
            (s.stroke, s.head, s.counter, s.glyph),
            (4.0, 22.0, 28.0, 15.0)
        );
        let s = o.sizes(2.0);
        assert_eq!((s.stroke, s.head, s.halo), (8.0, 44.0, 2.0));
        for (w, head) in [(Width::S, 16.0), (Width::M, 22.0), (Width::L, 34.0)] {
            assert_eq!(Object { width: w, ..o }.sizes(1.0).head, head);
        }
    }

    #[test]
    fn three_characters_make_a_capsule() {
        let counter = |n| {
            object(Shape::Counter {
                at: Pt::default(),
                counting: Counting::Numbers,
                n,
            })
        };
        let (_, w, h) = counter(99).counter_body(1.0).unwrap();
        assert_eq!((w, h), (28.0, 28.0));
        let (label, w, h) = counter(100).counter_body(1.0).unwrap();
        assert_eq!(label, "100");
        assert_eq!(h, 28.0);
        assert!(w > 28.0, "{w}");
    }

    #[test]
    fn the_arrow_shaft_ends_inside_the_head() {
        let (shaft, tri) = arrow_parts(Pt::new(0.0, 0.0), Pt::new(100.0, 0.0), 22.0);
        assert_eq!(shaft, Some((Pt::new(0.0, 0.0), Pt::new(80.0, 0.0))));
        assert_eq!(
            tri,
            [
                Pt::new(100.0, 0.0),
                Pt::new(78.0, 11.0),
                Pt::new(78.0, -11.0)
            ]
        );
        // Shorter than its head: the head shrinks to the arrow and there is no shaft.
        let (shaft, tri) = arrow_parts(Pt::new(0.0, 0.0), Pt::new(10.0, 0.0), 22.0);
        assert_eq!(shaft, None);
        assert_eq!(tri[1], Pt::new(0.0, 5.0));
    }

    #[test]
    fn hits_land_on_the_outline() {
        let line = object(Shape::Line {
            a: Pt::new(0.0, 0.0),
            b: Pt::new(100.0, 0.0),
        });
        assert!(line.hit(Pt::new(50.0, 3.0), 1.0, 0.0));
        assert!(!line.hit(Pt::new(50.0, 4.0), 1.0, 0.0));
        assert!(line.hit(Pt::new(50.0, 6.0), 1.0, 3.0));

        let rect = object(Shape::Rect {
            a: Pt::new(100.0, 100.0),
            b: Pt::new(0.0, 0.0),
        });
        assert!(rect.hit(Pt::new(0.0, 50.0), 1.0, 0.0));
        assert!(rect.hit(Pt::new(97.0, 50.0), 1.0, 0.0));
        assert!(!rect.hit(Pt::new(50.0, 50.0), 1.0, 4.0));
        assert!(!rect.hit(Pt::new(104.0, 50.0), 1.0, 0.0));

        let ellipse = object(Shape::Ellipse {
            a: Pt::new(0.0, 0.0),
            b: Pt::new(200.0, 100.0),
        });
        assert!(ellipse.hit(Pt::new(100.0, 1.0), 1.0, 0.0));
        assert!(ellipse.hit(Pt::new(199.0, 50.0), 1.0, 0.0));
        assert!(!ellipse.hit(Pt::new(100.0, 50.0), 1.0, 4.0));
        assert!(!ellipse.hit(Pt::new(5.0, 5.0), 1.0, 2.0));

        let arrow = object(Shape::Arrow {
            a: Pt::new(0.0, 0.0),
            b: Pt::new(100.0, 0.0),
        });
        // The head is wider than the shaft.
        assert!(arrow.hit(Pt::new(85.0, 7.0), 1.0, 0.0));
        assert!(!arrow.hit(Pt::new(40.0, 7.0), 1.0, 0.0));

        let counter = object(Shape::Counter {
            at: Pt::new(50.0, 50.0),
            counting: Counting::Numbers,
            n: 1,
        });
        assert!(counter.hit(Pt::new(50.0, 64.0), 1.0, 0.0));
        assert!(!counter.hit(Pt::new(50.0, 66.0), 1.0, 0.0));
    }

    #[test]
    fn handles_resize_their_edges() {
        let mut rect = object(Shape::Rect {
            a: Pt::new(10.0, 10.0),
            b: Pt::new(110.0, 60.0),
        });
        let original = rect;
        assert_eq!(rect.handles().len(), 8);
        assert_eq!(rect.handles()[3].1, Pt::new(110.0, 35.0));
        // The right edge's midpoint moves only the right edge.
        rect.drag_handle(&original, Handle(3), Pt::new(150.0, 0.0));
        assert_eq!(
            rect.shape,
            Shape::Rect {
                a: Pt::new(10.0, 10.0),
                b: Pt::new(150.0, 60.0)
            }
        );
        // The top-left corner moves two edges.
        rect.drag_handle(&original, Handle(0), Pt::new(0.0, 5.0));
        assert_eq!(
            rect.shape,
            Shape::Rect {
                a: Pt::new(0.0, 5.0),
                b: Pt::new(110.0, 60.0)
            }
        );
        assert_eq!(rect.handle_axis(Handle(1)), Axis::Vertical);
        assert_eq!(rect.handle_axis(Handle(6)), Axis::Rising);

        let mut arrow = object(Shape::Arrow {
            a: Pt::new(0.0, 0.0),
            b: Pt::new(10.0, 10.0),
        });
        let original = arrow;
        arrow.drag_handle(&original, Handle(1), Pt::new(20.0, 0.0));
        assert_eq!(
            arrow.shape,
            Shape::Arrow {
                a: Pt::new(0.0, 0.0),
                b: Pt::new(20.0, 0.0)
            }
        );
        assert_eq!(arrow.handle_axis(Handle(0)), Axis::Horizontal);
        assert_eq!(original.handle_axis(Handle(0)), Axis::Falling);
    }

    #[test]
    fn shift_snaps_to_15_degrees_and_squares() {
        let a = Pt::new(0.0, 0.0);
        let b = constrain(true, a, Pt::new(100.0, 10.0));
        assert!((b.y).abs() < 1e-3 && (b.x - 100.5).abs() < 0.1, "{b:?}");
        let b = constrain(true, a, Pt::new(50.0, 30.0));
        let angle = b.y.atan2(b.x).to_degrees();
        assert!((angle - 30.0).abs() < 1e-3, "{angle}");
        assert_eq!(
            constrain(false, a, Pt::new(-40.0, 25.0)),
            Pt::new(-40.0, 40.0)
        );
    }
}
