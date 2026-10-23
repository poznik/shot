//! Geometry in physical pixels of the virtual screen.

/// A pixel position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Axis-aligned rectangle in physical pixels. The virtual screen spans all monitors, so the
/// origin may be negative (a monitor left of or above the primary one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// The rectangle spanning two corner pixels, both included.
    pub fn from_corners(a: Point, b: Point) -> Rect {
        Rect::new(
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x - b.x).abs() + 1,
            (a.y - b.y).abs() + 1,
        )
    }

    pub const fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    pub const fn area(&self) -> i64 {
        if self.is_empty() {
            0
        } else {
            self.width as i64 * self.height as i64
        }
    }

    pub const fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Grows the rectangle by `d` on every side.
    pub const fn inflate(&self, d: i32) -> Rect {
        Rect::new(
            self.x - d,
            self.y - d,
            self.width + 2 * d,
            self.height + 2 * d,
        )
    }

    /// The nearest pixel inside the rectangle, which must not be empty.
    pub fn clamp(&self, p: Point) -> Point {
        Point::new(
            p.x.clamp(self.x, self.right() - 1),
            p.y.clamp(self.y, self.bottom() - 1),
        )
    }

    /// Squared distance from the point to the nearest pixel of the rectangle; 0 inside.
    pub fn distance_sq(&self, p: Point) -> i64 {
        let q = self.clamp(p);
        let (dx, dy) = ((p.x - q.x) as i64, (p.y - q.y) as i64);
        dx * dx + dy * dy
    }

    /// The common part, or `None` if the rectangles do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = Rect::new(
            x,
            y,
            self.right().min(other.right()) - x,
            self.bottom().min(other.bottom()) - y,
        );
        (!r.is_empty()).then_some(r)
    }

    /// The smallest rectangle covering both.
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    /// The parts of `self` outside `hole`: up to four rectangles (top, bottom, left, right
    /// strips), none of them empty.
    pub fn subtract(&self, hole: &Rect) -> impl Iterator<Item = Rect> + use<> {
        let pieces = match self.intersect(hole) {
            None => [Some(*self), None, None, None],
            Some(h) => [
                Rect::new(self.x, self.y, self.width, h.y - self.y),
                Rect::new(self.x, h.bottom(), self.width, self.bottom() - h.bottom()),
                Rect::new(self.x, h.y, h.x - self.x, h.height),
                Rect::new(h.right(), h.y, self.right() - h.right(), h.height),
            ]
            .map(|r| (!r.is_empty()).then_some(r)),
        };
        pieces.into_iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(-1440, -851, 1440, 2560);
        assert!(r.contains(-1440, -851));
        assert!(r.contains(-1, 1708));
        assert!(!r.contains(0, 0));
        assert!(!r.contains(-1, 1709));
    }

    #[test]
    fn union_spans_both() {
        let a = Rect::new(0, 0, 2560, 1440);
        let b = Rect::new(-1440, -851, 1440, 2560);
        assert_eq!(a.union(&b), Rect::new(-1440, -851, 4000, 2560));
    }

    #[test]
    fn corners_are_inclusive_in_any_order() {
        let r = Rect::new(10, 20, 6, 1);
        assert_eq!(
            Rect::from_corners(Point::new(10, 20), Point::new(15, 20)),
            r
        );
        assert_eq!(
            Rect::from_corners(Point::new(15, 20), Point::new(10, 20)),
            r
        );
        assert_eq!(
            Rect::from_corners(Point::new(3, 3), Point::new(3, 3)),
            Rect::new(3, 3, 1, 1)
        );
    }

    #[test]
    fn intersect_and_clamp() {
        let a = Rect::new(0, 0, 100, 50);
        assert_eq!(
            a.intersect(&Rect::new(90, 40, 20, 20)),
            Some(Rect::new(90, 40, 10, 10))
        );
        assert_eq!(a.intersect(&Rect::new(100, 0, 5, 5)), None);
        assert_eq!(a.clamp(Point::new(-5, 70)), Point::new(0, 49));
        assert_eq!(a.distance_sq(Point::new(103, 53)), 4 * 4 + 4 * 4);
        assert_eq!(a.distance_sq(Point::new(5, 5)), 0);
    }

    #[test]
    fn subtract_covers_the_rest_exactly() {
        let a = Rect::new(0, 0, 10, 10);
        let hole = Rect::new(3, 4, 4, 2);
        let parts: Vec<Rect> = a.subtract(&hole).collect();
        assert_eq!(parts.len(), 4);
        let area: i64 = parts.iter().map(Rect::area).sum();
        assert_eq!(area, a.area() - hole.area());
        for (i, p) in parts.iter().enumerate() {
            assert!(p.intersect(&hole).is_none());
            assert!(parts[i + 1..].iter().all(|q| p.intersect(q).is_none()));
        }
        assert_eq!(a.subtract(&a).count(), 0);
        assert_eq!(
            a.subtract(&Rect::new(50, 50, 1, 1)).collect::<Vec<_>>(),
            [a]
        );
    }
}
