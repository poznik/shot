//! Where each overlay element sits on one monitor (DES-002 "Geometry" and "Placement"), and
//! which pixels change between two layouts.

use crate::geom::{Point, Rect};
use crate::text;
use crate::theme::{TYPE_CAPTION, TYPE_LABEL, scaled, stroke};

use super::selection::{Selection, TargetKind};

// Logical px at 100% scale.
const AREA_FRAME: f32 = 1.0;
const WINDOW_FRAME: f32 = 2.0;
const FRAME_HALO: f32 = 1.0;
const CROSSHAIR_GAP: f32 = 3.0;
const CROSSHAIR_ARM: f32 = 8.0;
const CROSSHAIR_WIDTH: f32 = 1.0;
const CROSSHAIR_HALO: f32 = 1.0;
const LABEL_OFFSET: f32 = 16.0;
const LABEL_EDGE: f32 = 8.0;
pub(super) const LABEL_HEIGHT: f32 = 20.0;
pub(super) const LABEL_PAD_X: f32 = 6.0;
pub(super) const LABEL_PAD_Y: f32 = 2.0;
pub(super) const HINT_HEIGHT: f32 = 32.0;
pub(super) const HINT_PAD_X: f32 = 14.0;
pub(super) const HINT_PAD_Y: f32 = 8.0;
pub(super) const HINT_KEY_GAP: f32 = 6.0;
pub(super) const HINT_PAIR_GAP: f32 = 16.0;
const HINT_EDGE: f32 = 24.0;
const HINT_AVOID: f32 = 96.0;

/// Key hint pairs: the key in `type.label`, the action in `type.caption`.
pub(super) const HINT: [(&str, &str); 4] = [
    ("Клик", "окно"),
    ("Протянуть", "область"),
    ("Space", "весь экран"),
    ("Esc", "отмена"),
];

/// Everything drawn on one monitor, in that monitor's own pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    /// The undimmed rect. It may lie partly or wholly off this monitor.
    pub target: Rect,
    /// Frame width directly outside the target, then the halo outside the frame.
    pub frame: i32,
    pub halo: i32,
    pub crosshair: Option<Crosshair>,
    pub label: Option<Label>,
    pub hint: Option<Hint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Crosshair {
    pub hot: Point,
    pub gap: i32,
    pub arm: i32,
    pub width: i32,
    pub halo: i32,
}

impl Crosshair {
    /// Right, left, down, up.
    pub fn arms(&self) -> [Rect; 4] {
        let Crosshair {
            hot,
            gap,
            arm,
            width,
            ..
        } = *self;
        let top = hot.y - (width - 1) / 2;
        let left = hot.x - (width - 1) / 2;
        [
            Rect::new(hot.x + gap, top, arm, width),
            Rect::new(hot.x - gap - arm + 1, top, arm, width),
            Rect::new(left, hot.y + gap, width, arm),
            Rect::new(left, hot.y - gap - arm + 1, width, arm),
        ]
    }

    pub fn bounds(&self) -> Rect {
        let r = self.gap + self.arm + self.width + self.halo;
        Rect::new(self.hot.x - r, self.hot.y - r, 2 * r + 1, 2 * r + 1)
    }
}

/// The size label: what it says and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Label {
    pub rect: Rect,
    pub size: (i32, i32),
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Hint {
    pub rect: Rect,
    pub scale: f32,
}

impl Layout {
    pub fn new(selection: &Selection, monitor: usize, show_hint: bool) -> Self {
        let monitors = selection.monitors();
        let m = &monitors[monitor];
        let origin = (-m.rect.x, -m.rect.y);
        let target = selection.target();
        // The frame belongs to the target, so it keeps one width where it spills onto a
        // neighbour with another scale.
        let ts = monitors[target.monitor].scale;
        let frame = match target.kind {
            TargetKind::Area => AREA_FRAME,
            TargetKind::Window | TargetKind::Monitor => WINDOW_FRAME,
        };
        let mut layout = Layout {
            target: target.rect.offset(origin.0, origin.1),
            frame: stroke(frame, ts),
            halo: stroke(FRAME_HALO, ts),
            crosshair: None,
            label: None,
            hint: None,
        };
        if selection.cursor_monitor() != monitor {
            return layout;
        }

        let s = m.scale;
        let hot = selection.hot();
        let hot = Point::new(hot.x + origin.0, hot.y + origin.1);
        layout.crosshair = Some(Crosshair {
            hot,
            gap: scaled(CROSSHAIR_GAP, s),
            arm: scaled(CROSSHAIR_ARM, s),
            width: stroke(CROSSHAIR_WIDTH, s),
            halo: stroke(CROSSHAIR_HALO, s),
        });

        let size = (target.rect.width, target.rect.height);
        let (w, h) = (label_width(size, s), scaled(LABEL_HEIGHT, s));
        let (offset, edge) = (scaled(LABEL_OFFSET, s), scaled(LABEL_EDGE, s));
        let mut x = hot.x + offset;
        if x + w > m.rect.width - edge {
            x = hot.x - offset - w;
        }
        let mut y = hot.y + offset;
        if y + h > m.rect.height - edge {
            y = hot.y - offset - h;
        }
        layout.label = Some(Label {
            rect: Rect::new(x, y, w, h),
            size,
            scale: s,
        });

        if show_hint {
            let (w, h) = (hint_width(s), scaled(HINT_HEIGHT, s));
            let work = m.work;
            let x = work.x + (work.width - w) / 2;
            let edge = scaled(HINT_EDGE, s);
            let bottom = Rect::new(x, work.bottom() - edge - h, w, h);
            let avoid = scaled(HINT_AVOID, s) as i64;
            let rect = if bottom.distance_sq(selection.hot()) <= avoid * avoid {
                Rect::new(x, work.y + edge, w, h)
            } else {
                bottom
            };
            layout.hint = Some(Hint {
                rect: rect.offset(origin.0, origin.1),
                scale: s,
            });
        }
        layout
    }

    /// The pixels that differ between `self` and `new`, as non-empty rects inside `bounds`.
    pub fn damage(&self, new: &Layout, bounds: Rect) -> Vec<Rect> {
        let mut out = Vec::new();
        if (self.target, self.frame, self.halo) != (new.target, new.frame, new.halo) {
            // Inside both targets the frame shows through in both; outside both outer edges
            // it is dimmed in both. Everything else may change.
            let keep = self.target.intersect(&new.target).unwrap_or_default();
            for l in [self, new] {
                out.extend(l.target.inflate(l.frame + l.halo).subtract(&keep));
            }
        }
        if self.crosshair != new.crosshair {
            out.extend(self.crosshair.map(|c| c.bounds()));
            out.extend(new.crosshair.map(|c| c.bounds()));
        }
        if self.label != new.label {
            out.extend(self.label.map(|l| l.rect));
            out.extend(new.label.map(|l| l.rect));
        }
        if self.hint != new.hint {
            out.extend(self.hint.map(|h| h.rect));
            out.extend(new.hint.map(|h| h.rect));
        }
        let mut out: Vec<Rect> = out.iter().filter_map(|r| r.intersect(&bounds)).collect();
        coalesce(&mut out);
        out
    }
}

/// Merges rects whose union costs no more pixels than drawing both.
fn coalesce(rects: &mut Vec<Rect>) {
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let u = rects[i].union(&rects[j]);
                if u.area() <= rects[i].area() + rects[j].area() {
                    rects[i] = u;
                    rects.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
}

pub(super) fn label_text(size: (i32, i32)) -> [String; 3] {
    [size.0.to_string(), " × ".to_string(), size.1.to_string()]
}

fn label_width(size: (i32, i32), s: f32) -> i32 {
    let text: f32 = label_text(size)
        .iter()
        .map(|t| text::width(TYPE_LABEL, s, t))
        .sum();
    text.ceil() as i32 + 2 * scaled(LABEL_PAD_X, s)
}

pub(super) fn hint_width(s: f32) -> i32 {
    let pairs: f32 = HINT
        .iter()
        .map(|(key, action)| {
            text::width(TYPE_LABEL, s, key)
                + scaled(HINT_KEY_GAP, s) as f32
                + text::width(TYPE_CAPTION, s, action)
        })
        .sum();
    let gaps = (HINT.len() - 1) as i32 * scaled(HINT_PAIR_GAP, s);
    pairs.ceil() as i32 + gaps + 2 * scaled(HINT_PAD_X, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::{Event, Monitor};

    const SCREEN: Rect = Rect::new(0, 0, 1920, 1080);

    fn selection(cursor: Point, scale: f32) -> Selection {
        let monitor = Monitor {
            rect: SCREEN,
            work: Rect::new(0, 0, 1920, 1032),
            scale,
        };
        Selection::new(
            vec![monitor],
            vec![Rect::new(200, 200, 600, 400)],
            4,
            cursor,
        )
    }

    #[test]
    fn crosshair_leaves_the_hot_spot_clear() {
        let l = Layout::new(&selection(Point::new(100, 100), 1.0), 0, false);
        let [right, left, down, up] = l.crosshair.unwrap().arms();
        assert_eq!(right, Rect::new(103, 100, 8, 1));
        assert_eq!(left, Rect::new(90, 100, 8, 1));
        assert_eq!(down, Rect::new(100, 103, 1, 8));
        assert_eq!(up, Rect::new(100, 90, 1, 8));
    }

    #[test]
    fn label_flips_near_the_right_and_bottom_edges() {
        let label = |p: Point| Layout::new(&selection(p, 1.0), 0, false).label.unwrap();
        let l = label(Point::new(100, 100));
        assert_eq!((l.rect.x, l.rect.y, l.rect.height), (116, 116, 20));
        assert_eq!(l.size, (1920, 1080));

        let l = label(Point::new(1900, 1070));
        assert_eq!(l.rect.right(), 1900 - 16);
        assert_eq!(l.rect.bottom(), 1070 - 16);
    }

    #[test]
    fn frame_widths_follow_the_target_and_scale() {
        let mut s = selection(Point::new(300, 300), 1.5);
        let l = Layout::new(&s, 0, false);
        assert_eq!((l.frame, l.halo), (3, 2));
        s.handle(Event::Press(Point::new(300, 300)));
        s.handle(Event::Move(Point::new(400, 400)));
        let l = Layout::new(&s, 0, false);
        assert_eq!((l.frame, l.halo), (2, 2));
        assert_eq!(l.target, Rect::new(300, 300, 101, 101));
        assert_eq!(l.label.unwrap().rect.height, 30);
    }

    #[test]
    fn hint_moves_up_when_the_cursor_comes_near() {
        let hint = |p: Point| Layout::new(&selection(p, 1.0), 0, true).hint.unwrap().rect;
        let low = hint(Point::new(960, 500));
        assert_eq!(low.bottom(), 1032 - 24);
        assert_eq!(low.height, 32);
        assert_eq!(low.x, (1920 - low.width) / 2);

        assert_eq!(hint(Point::new(960, low.y - 97)), low);
        let high = hint(Point::new(960, low.y - 96));
        assert_eq!(high.y, 24);
        assert_eq!(hint(Point::new(low.right() + 50, 1079)), high);
    }

    #[test]
    fn a_small_drag_step_damages_little() {
        let mut s = selection(Point::new(500, 500), 1.0);
        s.handle(Event::Press(Point::new(500, 500)));
        s.handle(Event::Move(Point::new(1500, 900)));
        let before = Layout::new(&s, 0, true);
        s.handle(Event::Move(Point::new(1503, 902)));
        let after = Layout::new(&s, 0, true);
        let damage = before.damage(&after, SCREEN);
        let area: i64 = damage.iter().map(Rect::area).sum();
        assert!(area < 20_000, "{area} px in {damage:?}");
        assert_eq!(after.damage(&after, SCREEN), []);
    }
}
