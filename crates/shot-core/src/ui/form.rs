//! What the settings and welcome windows share: their controls, which control the pointer is on,
//! the keyboard focus, and what a click or a key did to a control.

use super::{Input, Key, Mods};
use crate::geom::Rect;
use crate::hotkey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Button {
        primary: bool,
    },
    Toggle {
        on: bool,
    },
    /// `selected` is `None` when the value is none of the segments.
    Segmented {
        selected: Option<usize>,
    },
    Hotkey,
}

/// A control of a form, in window px.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Slot<Id> {
    pub id: Id,
    pub kind: Kind,
    /// The control; a segmented control's track.
    pub rect: Rect,
    /// A segmented control's segments.
    pub parts: Vec<Rect>,
    /// Where else a click works the control: a toggle's row label.
    pub label: Option<Rect>,
    pub enabled: bool,
}

impl<Id> Slot<Id> {
    pub fn new(id: Id, kind: Kind, rect: Rect) -> Self {
        Self {
            id,
            kind,
            rect,
            parts: Vec::new(),
            label: None,
            enabled: true,
        }
    }
}

/// A control and, for a segmented control, the segment.
pub(crate) type Target<Id> = (Id, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act<Id> {
    /// A button clicked, a toggle flipped or a hotkey field asked to record.
    Press(Id),
    /// A segment chosen.
    Select(Id, usize),
}

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keyed<Id> {
    /// The form used the key; maybe it worked a control.
    Used(Option<Act<Id>>),
    /// Not a key of the form: the window may use it.
    Unused,
}

#[derive(Clone, Debug)]
pub(crate) struct Form<Id> {
    pub slots: Vec<Slot<Id>>,
    pub hover: Option<Target<Id>>,
    pub pressed: Option<Target<Id>>,
    pub focus: Option<Id>,
    /// The focus moved by the keyboard: its ring shows.
    pub ring: bool,
}

impl<Id: Copy + Eq> Form<Id> {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            hover: None,
            pressed: None,
            focus: None,
            ring: false,
        }
    }

    pub fn slot(&self, id: Id) -> Option<&Slot<Id>> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// The enabled control under window px (`x`, `y`).
    pub fn hit(&self, x: f32, y: f32) -> Option<Target<Id>> {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        self.slots
            .iter()
            .filter(|s| s.enabled)
            .find_map(|s| match s.kind {
                Kind::Segmented { .. } => s
                    .parts
                    .iter()
                    .position(|r| r.contains(x, y))
                    .map(|i| (s.id, i)),
                _ => (s.rect.contains(x, y) || s.label.is_some_and(|l| l.contains(x, y)))
                    .then_some((s.id, 0)),
            })
    }

    fn act(&self, (id, part): Target<Id>) -> Option<Act<Id>> {
        let slot = self.slot(id)?;
        Some(match slot.kind {
            Kind::Segmented { .. } => Act::Select(id, part),
            _ => Act::Press(id),
        })
    }

    /// Follows the pointer. A press focuses its control, without the ring; a release on the
    /// control it pressed works it.
    pub fn pointer(&mut self, input: Input) -> Option<Act<Id>> {
        match input {
            Input::Move { x, y } => {
                self.hover = self.hit(x, y);
                None
            }
            Input::Leave => {
                self.hover = None;
                None
            }
            Input::Down { x, y } => {
                self.pressed = self.hit(x, y);
                self.hover = self.pressed;
                if let Some((id, _)) = self.pressed {
                    self.focus = Some(id);
                    self.ring = false;
                }
                None
            }
            Input::Up { x, y } => {
                let pressed = self.pressed.take();
                self.hover = self.hit(x, y);
                pressed
                    .filter(|p| Some(*p) == self.hover)
                    .and_then(|p| self.act(p))
            }
            Input::Blur => {
                self.pressed = None;
                None
            }
            _ => None,
        }
    }

    /// `Tab` and `Shift+Tab` move the focus; `Space` works the focused control, `Enter` a focused
    /// button or hotkey field; `←` `→` move a segmented control's choice.
    pub fn key(&mut self, key: Key, mods: Mods) -> Keyed<Id> {
        let Key::Hot(key) = key else {
            return Keyed::Unused;
        };
        if !mods.plain() {
            return Keyed::Unused;
        }
        if key == hotkey::Key::Tab {
            self.step_focus(mods.shift);
            return Keyed::Used(None);
        }
        let Some(slot) = self
            .focus
            .and_then(|id| self.slot(id))
            .filter(|s| s.enabled)
        else {
            return Keyed::Unused;
        };
        let (id, kind) = (slot.id, slot.kind);
        let last = slot.parts.len().saturating_sub(1);
        match (kind, key) {
            (Kind::Segmented { selected }, hotkey::Key::Left | hotkey::Key::Right) => {
                self.ring = true;
                let next = match (selected, key) {
                    (None, _) => 0,
                    (Some(i), hotkey::Key::Left) => i.saturating_sub(1),
                    (Some(i), _) => (i + 1).min(last),
                };
                Keyed::Used((Some(next) != selected).then_some(Act::Select(id, next)))
            }
            (Kind::Segmented { .. }, hotkey::Key::Space) => Keyed::Used(None),
            (Kind::Button { .. } | Kind::Toggle { .. } | Kind::Hotkey, hotkey::Key::Space)
            | (Kind::Button { .. } | Kind::Hotkey, hotkey::Key::Enter) => {
                Keyed::Used(Some(Act::Press(id)))
            }
            _ => Keyed::Unused,
        }
    }

    fn step_focus(&mut self, back: bool) {
        self.ring = true;
        let enabled: Vec<Id> = self
            .slots
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.id)
            .collect();
        if enabled.is_empty() {
            return;
        }
        let n = enabled.len();
        let next = match self
            .focus
            .and_then(|f| enabled.iter().position(|&id| id == f))
        {
            None if back => n - 1,
            None => 0,
            Some(i) if back => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
        };
        self.focus = Some(enabled[next]);
    }

    /// How a control looks now.
    pub fn look(&self, id: Id) -> Look {
        let hover = self.hover.filter(|(h, _)| *h == id).map(|(_, p)| p);
        let pressed = self.pressed.filter(|(p, _)| *p == id).map(|(_, p)| p);
        Look {
            hover,
            pressed: pressed.filter(|p| Some(*p) == hover),
            focus: self.ring && self.focus == Some(id),
            enabled: self.slot(id).is_some_and(|s| s.enabled),
        }
    }
}

/// The state a control is drawn in; `hover` and `pressed` name the segment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Look {
    pub hover: Option<usize>,
    pub pressed: Option<usize>,
    pub focus: bool,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form<u8> {
        let mut f = Form::new();
        f.slots.push(Slot::new(
            1,
            Kind::Button { primary: false },
            Rect::new(0, 0, 10, 10),
        ));
        let mut seg = Slot::new(
            2,
            Kind::Segmented { selected: Some(0) },
            Rect::new(20, 0, 20, 10),
        );
        seg.parts = vec![Rect::new(20, 0, 10, 10), Rect::new(30, 0, 10, 10)];
        f.slots.push(seg);
        let mut off = Slot::new(3, Kind::Toggle { on: false }, Rect::new(50, 0, 10, 10));
        off.enabled = false;
        f.slots.push(off);
        f
    }

    #[test]
    fn click_works_the_control_it_pressed() {
        let mut f = form();
        f.pointer(Input::Down { x: 5.0, y: 5.0 });
        assert_eq!(f.pointer(Input::Up { x: 5.0, y: 5.0 }), Some(Act::Press(1)));
        f.pointer(Input::Down { x: 5.0, y: 5.0 });
        assert_eq!(
            f.pointer(Input::Up { x: 25.0, y: 5.0 }),
            None,
            "released elsewhere"
        );
        f.pointer(Input::Down { x: 35.0, y: 5.0 });
        assert_eq!(
            f.pointer(Input::Up { x: 35.0, y: 5.0 }),
            Some(Act::Select(2, 1))
        );
        f.pointer(Input::Down { x: 55.0, y: 5.0 });
        assert_eq!(f.pointer(Input::Up { x: 55.0, y: 5.0 }), None, "disabled");
        assert!(!f.ring, "a click shows no focus ring");
    }

    #[test]
    fn tab_skips_disabled_controls_and_wraps() {
        let mut f = form();
        let tab = |f: &mut Form<u8>, shift| {
            f.key(
                Key::Hot(hotkey::Key::Tab),
                Mods {
                    shift,
                    ..Mods::default()
                },
            )
        };
        tab(&mut f, false);
        assert_eq!(f.focus, Some(1));
        tab(&mut f, false);
        assert_eq!(f.focus, Some(2));
        tab(&mut f, false);
        assert_eq!(f.focus, Some(1));
        tab(&mut f, true);
        assert_eq!(f.focus, Some(2));
        assert!(f.look(2).focus);
        let right = f.key(Key::Hot(hotkey::Key::Right), Mods::default());
        assert_eq!(right, Keyed::Used(Some(Act::Select(2, 1))));
    }
}
