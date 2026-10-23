//! The Screen Recording window of macOS (`docs/design/settings.md`, "Screen Recording (macOS)"):
//! why Shot needs the permission, how to give it, and whether it is there now.

use tiny_skia::Pixmap;

use super::form::{Act, Form, Keyed, Kind, Slot};
use super::widgets;
use super::{Input, Key};
use crate::draw;
use crate::geom::Rect;
use crate::icons::{self, Icon};
use crate::image::BgrxMut;
use crate::text;
use crate::theme::{Palette, TYPE_BODY, TYPE_HEADLINE, TYPE_LABEL, scaled};

const WIDTH: f32 = 480.0;
const PAD: f32 = 24.0;
const ICON: f32 = 64.0;
const AFTER_ICON: f32 = 16.0;
const AFTER_TITLE: f32 = 8.0;
const AFTER_TEXT: f32 = 20.0;
/// A step's number sits in a circle this wide.
const STEP_DOT: f32 = 20.0;
/// Between the circle and the step's text.
const STEP_GAP: f32 = 12.0;
const BETWEEN_STEPS: f32 = 10.0;
const BEFORE_STATUS: f32 = 20.0;
const STATUS_ICON: f32 = 16.0;
const STATUS_GAP: f32 = 8.0;
const BEFORE_BUTTONS: f32 = 24.0;
const BUTTON_GAP: f32 = 8.0;

const TITLE: &str = "Разрешите Shot снимать экран";
const TEXT: &str = "macOS показывает экран только программам с разрешением «Запись экрана». \
                    Shot снимает только по хоткею и ничего не отправляет в сеть.";
/// Step 2 names the section by the start of its name: macOS 15 calls it «Запись экрана и
/// системного звука».
const STEPS: [&str; 3] = [
    "Нажмите «Открыть Системные настройки».",
    "Включите Shot в разделе «Запись экрана».",
    "Если macOS попросит перезапустить Shot, согласитесь.",
];
const MISSING: &str = "Разрешения пока нет";
const GRANTED: &str = "Разрешение есть";
const LATER: &str = "Позже";
const OPEN: &str = "Открыть Системные настройки";
const DONE: &str = "Готово";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctl {
    Later,
    Open,
    Done,
}

pub struct Permission {
    scale: f32,
    dark: bool,
    granted: bool,
    form: Form<Ctl>,
    closed: bool,
    /// `Открыть Системные настройки` was pressed and the app has not opened them yet.
    open_settings: bool,
    layout: Layout,
    icon: Option<Pixmap>,
    pixmap: Option<Pixmap>,
}

/// Where things lie, physical px.
#[derive(Clone, Debug, Default)]
struct Layout {
    title: i32,
    text: Vec<String>,
    text_top: i32,
    /// Each step's lines and the top of its first one.
    steps: Vec<(Vec<String>, i32)>,
    status: i32,
    height: i32,
}

impl Permission {
    /// `granted`: whether Shot has the permission now.
    pub fn new(granted: bool, scale: f32, dark: bool) -> Self {
        let mut w = Self {
            scale,
            dark,
            granted,
            form: Form::new(),
            closed: false,
            open_settings: false,
            layout: Layout::default(),
            icon: None,
            pixmap: None,
        };
        w.lay_out();
        w
    }

    /// The client area the content needs, physical px.
    pub fn content_size(&self) -> (u32, u32) {
        (scaled(WIDTH, self.scale) as u32, self.layout.height as u32)
    }

    /// The window is done with: `Позже`, `Готово`, `Esc` or the close button.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Whether `Открыть Системные настройки` was pressed since the last call.
    pub fn take_open_settings(&mut self) -> bool {
        std::mem::take(&mut self.open_settings)
    }

    /// Shows whether the permission is there now. Returns whether that changed the window.
    pub fn set_granted(&mut self, granted: bool) -> bool {
        if granted == self.granted {
            return false;
        }
        self.granted = granted;
        self.form.focus = None;
        self.lay_out();
        true
    }

    /// Returns whether the window looks different now: the pointer moving over nothing changes
    /// nothing.
    pub fn event(&mut self, input: Input) -> bool {
        if self.closed {
            return false;
        }
        if let Input::Move { .. } | Input::Leave = input {
            let before = (self.form.hover, self.form.pressed);
            self.form.pointer(input);
            return (self.form.hover, self.form.pressed) != before;
        }
        match input {
            Input::Resize { scale, .. } => {
                if scale != self.scale {
                    self.scale = scale;
                    self.icon = None;
                    self.lay_out();
                }
            }
            Input::Theme { dark } => self.dark = dark,
            Input::Close => self.closed = true,
            Input::Key { key, mods } => match self.form.key(key, mods) {
                Keyed::Used(Some(act)) => self.act(act),
                Keyed::Used(None) => {}
                Keyed::Unused => match key {
                    Key::Escape => self.closed = true,
                    // `Enter` is the primary button's.
                    Key::Hot(crate::hotkey::Key::Enter) if mods.plain() => {
                        self.act(Act::Press(self.primary()));
                    }
                    _ => {}
                },
            },
            Input::KeyUp { .. } => {}
            Input::Move { .. }
            | Input::Down { .. }
            | Input::Up { .. }
            | Input::Leave
            | Input::Blur => {
                if let Some(act) = self.form.pointer(input) {
                    self.act(act);
                }
            }
        }
        true
    }

    fn primary(&self) -> Ctl {
        if self.granted { Ctl::Done } else { Ctl::Open }
    }

    fn act(&mut self, act: Act<Ctl>) {
        match act {
            Act::Press(Ctl::Open) => self.open_settings = true,
            Act::Press(Ctl::Later | Ctl::Done) => self.closed = true,
            Act::Select(..) => {}
        }
    }

    fn lay_out(&mut self) {
        let s = self.scale;
        let pad = scaled(PAD, s);
        let right = scaled(WIDTH, s) - pad;
        let body_h = scaled(TYPE_BODY.line_height, s);
        let l = &mut self.layout;
        let mut y = pad + scaled(ICON, s) + scaled(AFTER_ICON, s);
        l.title = y;
        y += scaled(TYPE_HEADLINE.line_height, s) + scaled(AFTER_TITLE, s);
        l.text_top = y;
        l.text = widgets::wrap(TYPE_BODY, s, TEXT, (right - pad) as f32);
        y += l.text.len() as i32 * body_h + scaled(AFTER_TEXT, s);
        let dot = scaled(STEP_DOT, s);
        let step_x = pad + dot + scaled(STEP_GAP, s);
        l.steps.clear();
        for (i, step) in STEPS.iter().enumerate() {
            if i > 0 {
                y += scaled(BETWEEN_STEPS, s);
            }
            let lines = widgets::wrap(TYPE_BODY, s, step, (right - step_x) as f32);
            let h = (lines.len() as i32 * body_h).max(dot);
            // The first line is centered on the circle.
            l.steps.push((lines, y + (dot - body_h) / 2));
            y += h;
        }
        y += scaled(BEFORE_STATUS, s);
        l.status = y;
        y += body_h.max(scaled(STATUS_ICON, s)) + scaled(BEFORE_BUTTONS, s);
        let button_h = scaled(widgets::BUTTON_H, s);
        let slots = if self.granted {
            let w = widgets::button_width(DONE, s);
            vec![Slot::new(
                Ctl::Done,
                Kind::Button { primary: true },
                Rect::new(right - w, y, w, button_h),
            )]
        } else {
            let open_w = widgets::button_width(OPEN, s);
            let later_w = widgets::button_width(LATER, s);
            let open_x = right - open_w;
            vec![
                Slot::new(
                    Ctl::Later,
                    Kind::Button { primary: false },
                    Rect::new(
                        open_x - scaled(BUTTON_GAP, s) - later_w,
                        y,
                        later_w,
                        button_h,
                    ),
                ),
                Slot::new(
                    Ctl::Open,
                    Kind::Button { primary: true },
                    Rect::new(open_x, y, open_w, button_h),
                ),
            ]
        };
        l.height = y + button_h + pad;
        self.form.slots = slots;
    }

    /// Draws the window into `dst`, its client area.
    pub fn draw(&mut self, dst: &mut BgrxMut) {
        let (w, h) = self.content_size();
        let palette = Palette::of(self.dark);
        let Some(mut pixmap) = widgets::canvas(&mut self.pixmap, w, h, palette) else {
            return;
        };
        self.render(&mut pixmap, palette);
        widgets::present(dst, &pixmap, palette);
        self.pixmap = Some(pixmap);
    }

    fn render(&mut self, pixmap: &mut Pixmap, palette: &Palette) {
        let s = self.scale;
        let pad = scaled(PAD, s);
        let l = &self.layout;

        let icon_px = scaled(ICON, s) as u32;
        if self.icon.as_ref().map(Pixmap::width) != Some(icon_px) {
            self.icon = widgets::app_icon(icon_px);
        }
        if let Some(icon) = &self.icon {
            widgets::sprite(pixmap, icon, pad, pad);
        }
        widgets::label(
            pixmap,
            TYPE_HEADLINE,
            s,
            TITLE,
            pad,
            l.title,
            palette.text_primary,
        );
        let body_h = scaled(TYPE_BODY.line_height, s);
        for (i, line) in l.text.iter().enumerate() {
            let top = l.text_top + i as i32 * body_h;
            widgets::label(pixmap, TYPE_BODY, s, line, pad, top, palette.text_secondary);
        }

        let dot = scaled(STEP_DOT, s);
        let step_x = pad + dot + scaled(STEP_GAP, s);
        for (i, (lines, top)) in l.steps.iter().enumerate() {
            let cy = top + body_h / 2;
            let r = dot as f32 / 2.0;
            draw::fill_circle(pixmap, pad as f32 + r, cy as f32, r, palette.accent_subtle);
            let number = (i + 1).to_string();
            let label_h = scaled(TYPE_LABEL.line_height, s);
            let x = pad as f32 + (dot as f32 - text::width(TYPE_LABEL, s, &number)) / 2.0;
            widgets::label(
                pixmap,
                TYPE_LABEL,
                s,
                &number,
                x.round() as i32,
                cy - label_h / 2,
                palette.accent_text,
            );
            for (j, line) in lines.iter().enumerate() {
                let y = top + j as i32 * body_h;
                widgets::label(pixmap, TYPE_BODY, s, line, step_x, y, palette.text_primary);
            }
        }

        let (icon, text, icon_color, color) = if self.granted {
            (Icon::Check, GRANTED, palette.success, palette.text_primary)
        } else {
            (Icon::Alert, MISSING, palette.warning, palette.warning)
        };
        let icon_px = scaled(STATUS_ICON, s);
        let row_h = body_h.max(icon_px);
        icons::draw(
            pixmap,
            icon,
            pad as f32,
            (l.status + (row_h - icon_px) / 2) as f32,
            icon_px as f32,
            s,
            icon_color,
        );
        widgets::label(
            pixmap,
            TYPE_BODY,
            s,
            text,
            pad + icon_px + scaled(STATUS_GAP, s),
            l.status + (row_h - body_h) / 2,
            color,
        );

        for slot in &self.form.slots {
            let look = self.form.look(slot.id);
            if let Kind::Button { primary } = slot.kind {
                let text = match slot.id {
                    Ctl::Later => LATER,
                    Ctl::Open => OPEN,
                    Ctl::Done => DONE,
                };
                widgets::button(pixmap, slot.rect, text, primary, look, palette, s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey;
    use crate::ui::Mods;

    fn click(w: &mut Permission, id: Ctl) {
        let r = w.form.slot(id).unwrap().rect;
        let (x, y) = ((r.x + r.width / 2) as f32, (r.y + r.height / 2) as f32);
        w.event(Input::Down { x, y });
        w.event(Input::Up { x, y });
    }

    fn press(w: &mut Permission, key: Key) {
        w.event(Input::Key {
            key,
            mods: Mods::default(),
        });
    }

    #[test]
    fn buttons_sit_right_and_close_the_content() {
        let w = Permission::new(false, 1.0, false);
        let open = w.form.slot(Ctl::Open).unwrap().rect;
        let later = w.form.slot(Ctl::Later).unwrap().rect;
        assert_eq!(open.right(), 456);
        assert_eq!(later.right() + 8, open.x);
        assert_eq!(later.y, open.y);
        assert_eq!(w.content_size(), (480, (open.bottom() + 24) as u32));
        assert!(w.layout.text.len() >= 2, "the text wraps");
        assert!(w.layout.steps.iter().all(|(lines, _)| lines.len() == 1));
    }

    #[test]
    fn open_asks_for_the_system_settings_and_stays() {
        let mut w = Permission::new(false, 1.0, false);
        click(&mut w, Ctl::Open);
        assert!(w.take_open_settings());
        assert!(!w.take_open_settings());
        assert!(!w.closed());
        press(&mut w, Key::Hot(hotkey::Key::Enter));
        assert!(w.take_open_settings());
        click(&mut w, Ctl::Later);
        assert!(w.closed());
    }

    #[test]
    fn granted_leaves_one_done_button() {
        let mut w = Permission::new(false, 2.0, true);
        let height = w.content_size().1;
        assert!(w.set_granted(true));
        assert!(!w.set_granted(true));
        assert_eq!(w.form.slots.len(), 1);
        assert_eq!(w.content_size().1, height);
        assert_eq!(w.form.slot(Ctl::Done).unwrap().rect.right(), 912);
        press(&mut w, Key::Hot(hotkey::Key::Enter));
        assert!(!w.take_open_settings());
        assert!(w.closed());
    }

    #[test]
    fn escape_and_the_close_button_close() {
        let mut w = Permission::new(false, 1.0, false);
        press(&mut w, Key::Escape);
        assert!(w.closed());
        let mut w = Permission::new(true, 1.0, false);
        w.event(Input::Close);
        assert!(w.closed());
    }

    #[test]
    fn draws_at_any_scale_and_theme() {
        for (granted, scale, dark) in [(false, 1.0, false), (true, 1.5, true), (false, 2.0, true)] {
            let mut w = Permission::new(granted, scale, dark);
            let (cw, ch) = w.content_size();
            let mut data = vec![0u8; cw as usize * ch as usize * 4];
            let mut dst = BgrxMut {
                width: cw,
                height: ch,
                stride: cw as usize * 4,
                data: &mut data,
            };
            w.draw(&mut dst);
            let bg = Palette::of(dark).surface_base;
            let last = data.len() - 4;
            assert_eq!(&data[last..last + 3], &[bg.b, bg.g, bg.r]);
        }
    }
}
