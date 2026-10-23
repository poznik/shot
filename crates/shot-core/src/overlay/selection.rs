//! What the user points at and what an input captures (DES-002, states A and B and the keys).
//! A pure state machine: no drawing, no timing.

use crate::geom::{Point, Rect};

/// One monitor, in physical pixels of the virtual screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub rect: Rect,
    /// The part not covered by the taskbar or the Dock and menu bar.
    pub work: Rect,
    /// Interface scale: 1.0 at 96 dpi on Windows and on a non-Retina Mac.
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// A window from the list taken at the hotkey, clipped to the monitor under the cursor.
    Window,
    /// A whole monitor: what the cursor points at over the desktop.
    Monitor,
    /// A dragged area, clamped to the monitor where the drag started.
    Area,
}

/// The rect that would be captured now. There is always one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub kind: TargetKind,
    pub rect: Rect,
    /// The monitor the rect lies on, whose frozen frame it is cut from.
    pub monitor: usize,
}

/// Input in physical pixels of the virtual screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Move(Point),
    /// Left button down.
    Press(Point),
    /// Left button up.
    Release(Point),
    Space,
    /// `Esc` or the right button.
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Capture(Target),
    /// Every monitor, one file each.
    AllMonitors,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Hover,
    /// The button is down but has not moved past the drag threshold: a release is a click.
    Pressed {
        at: Point,
    },
    Drag {
        from: Point,
        monitor: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Selection {
    monitors: Vec<Monitor>,
    /// Window rects from the top of the z-order down.
    windows: Vec<Rect>,
    drag_threshold: i32,
    hot: Point,
    phase: Phase,
    target: Target,
}

impl Selection {
    /// `windows` go from the top of the z-order down. A drag starts once the cursor moves more
    /// than `drag_threshold` px from the press point along either axis.
    pub fn new(
        monitors: Vec<Monitor>,
        windows: Vec<Rect>,
        drag_threshold: i32,
        cursor: Point,
    ) -> Self {
        assert!(!monitors.is_empty(), "the overlay needs a monitor");
        let mut selection = Self {
            monitors,
            windows,
            drag_threshold,
            hot: cursor,
            phase: Phase::Hover,
            target: Target {
                kind: TargetKind::Monitor,
                rect: Rect::default(),
                monitor: 0,
            },
        };
        selection.target = selection.hover_target(cursor);
        selection
    }

    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    /// The cursor's hot spot.
    pub fn hot(&self) -> Point {
        self.hot
    }

    /// The monitor under the hot spot.
    pub fn cursor_monitor(&self) -> usize {
        self.monitor_at(self.hot)
    }

    pub fn target(&self) -> Target {
        self.target
    }

    pub fn dragging(&self) -> bool {
        matches!(self.phase, Phase::Drag { .. })
    }

    /// Applies one input. Returns what to capture once the choice is made.
    pub fn handle(&mut self, event: Event) -> Option<Outcome> {
        match event {
            Event::Move(p) => {
                self.move_to(p);
                None
            }
            Event::Press(p) => {
                self.move_to(p);
                if self.phase == Phase::Hover {
                    self.phase = Phase::Pressed { at: p };
                }
                None
            }
            Event::Release(p) => {
                self.move_to(p);
                match std::mem::replace(&mut self.phase, Phase::Hover) {
                    Phase::Hover => None,
                    Phase::Pressed { .. } | Phase::Drag { .. } => {
                        Some(Outcome::Capture(self.target))
                    }
                }
            }
            Event::Space => Some(Outcome::AllMonitors),
            Event::Cancel => Some(Outcome::Cancel),
        }
    }

    fn move_to(&mut self, p: Point) {
        self.hot = p;
        match self.phase {
            Phase::Hover => self.target = self.hover_target(p),
            // The target stays what was under the press point until the drag starts.
            Phase::Pressed { at } => {
                let t = self.drag_threshold;
                if (p.x - at.x).abs() > t || (p.y - at.y).abs() > t {
                    let monitor = self.monitor_at(at);
                    self.phase = Phase::Drag { from: at, monitor };
                    self.target = self.area(at, monitor);
                }
            }
            Phase::Drag { from, monitor } => self.target = self.area(from, monitor),
        }
    }

    fn area(&self, from: Point, monitor: usize) -> Target {
        let bounds = self.monitors[monitor].rect;
        Target {
            kind: TargetKind::Area,
            rect: Rect::from_corners(bounds.clamp(from), bounds.clamp(self.hot)),
            monitor,
        }
    }

    fn hover_target(&self, p: Point) -> Target {
        let monitor = self.monitor_at(p);
        let bounds = self.monitors[monitor].rect;
        let window = self
            .windows
            .iter()
            .find(|w| w.contains(p.x, p.y))
            .and_then(|w| w.intersect(&bounds));
        match window {
            Some(rect) => Target {
                kind: TargetKind::Window,
                rect,
                monitor,
            },
            None => Target {
                kind: TargetKind::Monitor,
                rect: bounds,
                monitor,
            },
        }
    }

    /// The monitor containing the point or, in a gap between monitors, the nearest one.
    fn monitor_at(&self, p: Point) -> usize {
        (0..self.monitors.len())
            .min_by_key(|&i| self.monitors[i].rect.distance_sq(p))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = Rect::new(0, 0, 2560, 1440);
    const SIDE: Rect = Rect::new(-1440, -851, 1440, 2560);
    /// An editor on top, a browser under it reaching onto the side monitor.
    const EDITOR: Rect = Rect::new(100, 100, 800, 600);
    const BROWSER: Rect = Rect::new(-300, 50, 1500, 1000);

    fn selection(cursor: Point) -> Selection {
        let monitor = |rect: Rect| Monitor {
            rect,
            work: Rect::new(rect.x, rect.y, rect.width, rect.height - 48),
            scale: 1.0,
        };
        Selection::new(
            vec![monitor(MAIN), monitor(SIDE)],
            vec![EDITOR, BROWSER],
            2,
            cursor,
        )
    }

    fn target(kind: TargetKind, rect: Rect, monitor: usize) -> Target {
        Target {
            kind,
            rect,
            monitor,
        }
    }

    #[test]
    fn hover_picks_the_topmost_window_clipped_to_the_monitor() {
        let mut s = selection(Point::new(500, 500));
        assert_eq!(s.target(), target(TargetKind::Window, EDITOR, 0));

        s.handle(Event::Move(Point::new(1000, 500)));
        assert_eq!(
            s.target(),
            target(TargetKind::Window, Rect::new(0, 50, 1200, 1000), 0)
        );

        s.handle(Event::Move(Point::new(-100, 500)));
        assert_eq!(
            s.target(),
            target(TargetKind::Window, Rect::new(-300, 50, 300, 1000), 1)
        );
    }

    #[test]
    fn hover_over_the_desktop_targets_the_monitor() {
        let mut s = selection(Point::new(2000, 1200));
        assert_eq!(s.target(), target(TargetKind::Monitor, MAIN, 0));
        s.handle(Event::Move(Point::new(-1000, -800)));
        assert_eq!(s.target(), target(TargetKind::Monitor, SIDE, 1));
        assert_eq!(s.cursor_monitor(), 1);
    }

    #[test]
    fn click_captures_the_hovered_window_or_monitor() {
        let mut s = selection(Point::new(500, 500));
        assert_eq!(s.handle(Event::Press(Point::new(500, 500))), None);
        let out = s.handle(Event::Release(Point::new(500, 500)));
        assert_eq!(
            out,
            Some(Outcome::Capture(target(TargetKind::Window, EDITOR, 0)))
        );

        let mut s = selection(Point::new(2000, 1200));
        s.handle(Event::Press(Point::new(2000, 1200)));
        let out = s.handle(Event::Release(Point::new(2000, 1200)));
        assert_eq!(
            out,
            Some(Outcome::Capture(target(TargetKind::Monitor, MAIN, 0)))
        );
    }

    #[test]
    fn a_jitter_within_the_threshold_is_still_a_click() {
        // Press on the editor's edge and wobble across it: the target stays the editor.
        let mut s = selection(Point::new(899, 300));
        s.handle(Event::Press(Point::new(899, 300)));
        s.handle(Event::Move(Point::new(901, 302)));
        assert!(!s.dragging());
        assert_eq!(s.target(), target(TargetKind::Window, EDITOR, 0));
        let out = s.handle(Event::Release(Point::new(901, 298)));
        assert_eq!(
            out,
            Some(Outcome::Capture(target(TargetKind::Window, EDITOR, 0)))
        );
    }

    #[test]
    fn drag_spans_press_to_hot_spot_inclusive() {
        let mut s = selection(Point::new(500, 500));
        s.handle(Event::Press(Point::new(500, 500)));
        s.handle(Event::Move(Point::new(503, 500)));
        assert!(s.dragging());
        assert_eq!(
            s.target(),
            target(TargetKind::Area, Rect::new(500, 500, 4, 1), 0)
        );

        s.handle(Event::Move(Point::new(400, 450)));
        assert_eq!(s.target().rect, Rect::new(400, 450, 101, 51));
        let out = s.handle(Event::Release(Point::new(409, 460)));
        let area = target(TargetKind::Area, Rect::new(409, 460, 92, 41), 0);
        assert_eq!(out, Some(Outcome::Capture(area)));
        assert!(!s.dragging());
    }

    #[test]
    fn a_fast_release_past_the_threshold_is_a_drag() {
        let mut s = selection(Point::new(500, 500));
        s.handle(Event::Press(Point::new(500, 500)));
        let out = s.handle(Event::Release(Point::new(600, 650)));
        let area = target(TargetKind::Area, Rect::new(500, 500, 101, 151), 0);
        assert_eq!(out, Some(Outcome::Capture(area)));
    }

    #[test]
    fn drag_stops_at_the_edge_of_its_monitor() {
        let mut s = selection(Point::new(100, 1000));
        s.handle(Event::Press(Point::new(100, 1000)));
        s.handle(Event::Move(Point::new(-200, 1600)));
        assert_eq!(
            s.target(),
            target(TargetKind::Area, Rect::new(0, 1000, 101, 440), 0)
        );
        assert_eq!(s.cursor_monitor(), 1);
    }

    #[test]
    fn keys_end_the_selection() {
        let mut s = selection(Point::new(500, 500));
        assert_eq!(s.handle(Event::Space), Some(Outcome::AllMonitors));
        s.handle(Event::Press(Point::new(500, 500)));
        s.handle(Event::Move(Point::new(700, 700)));
        assert_eq!(s.handle(Event::Cancel), Some(Outcome::Cancel));
    }

    #[test]
    fn release_without_press_does_nothing() {
        let mut s = selection(Point::new(500, 500));
        assert_eq!(s.handle(Event::Release(Point::new(510, 510))), None);
    }
}
