//! The first-run welcome (`docs/design/settings.md`, "Welcome"): where Shot lives, its hotkey and
//! keys, and whether it starts with the system.

use tiny_skia::Pixmap;

use super::form::{Act, Form, Keyed, Kind, Slot};
use super::widgets::{self, Caps};
use super::{Input, Key};
use crate::geom::Rect;
use crate::hotkey::{Hotkey, Os};
use crate::image::BgrxMut;
use crate::text;
use crate::theme::{Palette, TYPE_BODY, TYPE_BODY_STRONG, TYPE_HEADLINE, scaled, stroke};

const WIDTH: f32 = 480.0;
const PAD: f32 = 24.0;
const ICON: f32 = 64.0;
const AFTER_ICON: f32 = 16.0;
const AFTER_TITLE: f32 = 8.0;
const AFTER_TEXT: f32 = 20.0;
/// Between `Снять экран:` and the hotkey's caps.
const HOTKEY_GAP: f32 = 12.0;
const AFTER_HOTKEY: f32 = 16.0;
const ROW_H: f32 = 24.0;
/// The rows' left column.
const ROW_LEFT: f32 = 112.0;
const AFTER_ROWS: f32 = 20.0;
const AFTER_DIVIDER: f32 = 16.0;
const BEFORE_BUTTONS: f32 = 24.0;

const TITLE: &str = "Shot работает в фоне";
/// A no-break space keeps `^.` with its word.
const TEXT_WINDOWS: &str =
    "Значок Shot — в трее у часов. Если его не видно, он под стрелкой\u{a0}^.";
const TEXT_MACOS: &str = "Значок Shot — в меню-баре справа.";
const HOTKEY: &str = "Снять экран:";
const AUTOSTART: &str = "Запускать Shot при входе в систему";
const SETTINGS: &str = "Настройки…";
const DONE: &str = "Готово";

/// The rows: a word or a key cap on the left, what it does on the right.
const ROWS: [(Left, &str); 4] = [
    (Left::Word("Клик"), "окно под курсором"),
    (Left::Word("Протянуть"), "область"),
    (Left::Cap("Space"), "весь экран"),
    (Left::Cap("Esc"), "отмена"),
];

#[derive(Clone, Copy)]
enum Left {
    Word(&'static str),
    Cap(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctl {
    Autostart,
    Settings,
    Done,
}

/// How the welcome ended: `Готово`, `Esc` or the close button, or `Настройки…`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Finish {
    pub autostart: bool,
    /// Open the settings window next.
    pub settings: bool,
}

pub struct Welcome {
    os: Os,
    hotkey: Hotkey,
    scale: f32,
    dark: bool,
    form: Form<Ctl>,
    autostart: bool,
    finished: Option<Finish>,
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
    hotkey: i32,
    rows: i32,
    divider: i32,
    autostart: i32,
    height: i32,
}

impl Welcome {
    pub fn new(hotkey: Hotkey, os: Os, scale: f32, dark: bool) -> Self {
        let mut w = Self {
            os,
            hotkey,
            scale,
            dark,
            form: Form::new(),
            autostart: true,
            finished: None,
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

    /// The welcome is over: the app applies the choice, records it and closes the window.
    pub fn finished(&self) -> Option<Finish> {
        self.finished
    }

    /// Returns whether the window looks different now: the pointer moving over nothing changes
    /// nothing.
    pub fn event(&mut self, input: Input) -> bool {
        if self.finished.is_some() {
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
            Input::Close => self.finish(false),
            Input::Key { key, mods } => match self.form.key(key, mods) {
                Keyed::Used(Some(act)) => self.act(act),
                Keyed::Used(None) => {}
                Keyed::Unused => {
                    if matches!(key, Key::Escape | Key::Hot(crate::hotkey::Key::Enter)) {
                        self.finish(false);
                    }
                }
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

    fn act(&mut self, act: Act<Ctl>) {
        match act {
            Act::Press(Ctl::Autostart) => {
                self.autostart = !self.autostart;
                self.lay_out();
            }
            Act::Press(Ctl::Settings) => self.finish(true),
            Act::Press(Ctl::Done) => self.finish(false),
            Act::Select(..) => {}
        }
    }

    fn finish(&mut self, settings: bool) {
        self.finished = Some(Finish {
            autostart: self.autostart,
            settings,
        });
    }

    fn lay_out(&mut self) {
        let s = self.scale;
        let pad = scaled(PAD, s);
        let width = scaled(WIDTH, s);
        let right = width - pad;
        let text = match self.os {
            Os::Windows => TEXT_WINDOWS,
            Os::MacOs => TEXT_MACOS,
        };
        let l = &mut self.layout;
        let mut y = pad + scaled(ICON, s) + scaled(AFTER_ICON, s);
        l.title = y;
        y += scaled(TYPE_HEADLINE.line_height, s) + scaled(AFTER_TITLE, s);
        l.text_top = y;
        l.text = widgets::wrap(TYPE_BODY, s, text, (right - pad) as f32);
        y += l.text.len() as i32 * scaled(TYPE_BODY.line_height, s) + scaled(AFTER_TEXT, s);
        l.hotkey = y;
        y += Caps::Large.height(s) + scaled(AFTER_HOTKEY, s);
        l.rows = y;
        y += ROWS.len() as i32 * scaled(ROW_H, s) + scaled(AFTER_ROWS, s);
        l.divider = y;
        y += stroke(1.0, s) + scaled(AFTER_DIVIDER, s);
        l.autostart = y;
        let toggle_h = scaled(widgets::TOGGLE_H, s);
        let toggle_w = scaled(widgets::TOGGLE_W, s);
        let mut flip = Slot::new(
            Ctl::Autostart,
            Kind::Toggle { on: self.autostart },
            Rect::new(right - toggle_w, y, toggle_w, toggle_h),
        );
        flip.label = Some(Rect::new(pad, y, right - toggle_w - pad, toggle_h));
        y += toggle_h + scaled(BEFORE_BUTTONS, s);
        let button_h = scaled(widgets::BUTTON_H, s);
        let settings = Slot::new(
            Ctl::Settings,
            Kind::Button { primary: false },
            Rect::new(pad, y, widgets::button_width(SETTINGS, s), button_h),
        );
        let done_w = widgets::button_width(DONE, s);
        let done = Slot::new(
            Ctl::Done,
            Kind::Button { primary: true },
            Rect::new(right - done_w, y, done_w, button_h),
        );
        l.height = y + button_h + pad;
        self.form.slots = vec![flip, settings, done];
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
        let width = scaled(WIDTH, s);
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

        // `Снять экран:` and the hotkey, centered on one line.
        let caps_h = Caps::Large.height(s);
        let strong_h = scaled(TYPE_BODY_STRONG.line_height, s);
        let top = l.hotkey + (caps_h - strong_h) / 2;
        widgets::label(
            pixmap,
            TYPE_BODY_STRONG,
            s,
            HOTKEY,
            pad,
            top,
            palette.text_primary,
        );
        let x =
            pad + text::width(TYPE_BODY_STRONG, s, HOTKEY).ceil() as i32 + scaled(HOTKEY_GAP, s);
        let caps = self.hotkey.caps(self.os);
        widgets::key_caps(
            pixmap,
            x,
            l.hotkey + caps_h / 2,
            &caps,
            Caps::Large,
            palette,
            s,
        );

        let row_h = scaled(ROW_H, s);
        let text_dy = (row_h - body_h) / 2;
        for (i, &(left, what)) in ROWS.iter().enumerate() {
            let top = l.rows + i as i32 * row_h;
            match left {
                Left::Word(word) => {
                    let dy = (row_h - strong_h) / 2;
                    widgets::label(
                        pixmap,
                        TYPE_BODY_STRONG,
                        s,
                        word,
                        pad,
                        top + dy,
                        palette.text_primary,
                    );
                }
                Left::Cap(cap) => {
                    let caps = [cap.to_string()];
                    widgets::key_caps(pixmap, pad, top + row_h / 2, &caps, Caps::Small, palette, s);
                }
            }
            let x = pad + scaled(ROW_LEFT, s);
            widgets::label(
                pixmap,
                TYPE_BODY,
                s,
                what,
                x,
                top + text_dy,
                palette.text_secondary,
            );
        }

        widgets::divider(pixmap, pad, l.divider, width - 2 * pad, palette, s);
        let toggle_h = scaled(widgets::TOGGLE_H, s);
        let top = l.autostart + (toggle_h - body_h) / 2;
        widgets::label(
            pixmap,
            TYPE_BODY,
            s,
            AUTOSTART,
            pad,
            top,
            palette.text_primary,
        );

        for slot in &self.form.slots {
            let look = self.form.look(slot.id);
            match (slot.id, slot.kind) {
                (_, Kind::Toggle { on }) => {
                    widgets::toggle(pixmap, slot.rect, on, look, palette, s)
                }
                (id, Kind::Button { primary }) => {
                    let text = if id == Ctl::Done { DONE } else { SETTINGS };
                    widgets::button(pixmap, slot.rect, text, primary, look, palette, s);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey;
    use crate::ui::Mods;

    fn welcome(os: Os, scale: f32) -> Welcome {
        Welcome::new(Hotkey::default_for(os), os, scale, false)
    }

    fn click(w: &mut Welcome, id: Ctl) {
        let r = w.form.slot(id).unwrap().rect;
        let (x, y) = ((r.x + r.width / 2) as f32, (r.y + r.height / 2) as f32);
        w.event(Input::Down { x, y });
        w.event(Input::Up { x, y });
    }

    #[test]
    fn buttons_close_the_content() {
        let w = welcome(Os::Windows, 1.0);
        let done = w.form.slot(Ctl::Done).unwrap().rect;
        let settings = w.form.slot(Ctl::Settings).unwrap().rect;
        assert_eq!(done.right(), 456);
        assert_eq!(settings.x, 24);
        assert_eq!(done.y, settings.y);
        assert_eq!(w.content_size(), (480, (done.bottom() + 24) as u32));
        assert!(w.layout.text.len() >= 2, "the Windows text wraps");
        assert_eq!(welcome(Os::MacOs, 1.0).layout.text.len(), 1);
    }

    #[test]
    fn done_keeps_autostart_on() {
        let mut w = welcome(Os::Windows, 1.0);
        assert_eq!(w.finished(), None);
        click(&mut w, Ctl::Done);
        assert_eq!(
            w.finished(),
            Some(Finish {
                autostart: true,
                settings: false
            })
        );
    }

    #[test]
    fn the_label_flips_autostart_and_settings_opens_the_settings() {
        let mut w = welcome(Os::Windows, 1.0);
        let label = w.form.slot(Ctl::Autostart).unwrap().label.unwrap();
        let (x, y) = ((label.x + 10) as f32, (label.y + 5) as f32);
        w.event(Input::Down { x, y });
        w.event(Input::Up { x, y });
        click(&mut w, Ctl::Settings);
        assert_eq!(
            w.finished(),
            Some(Finish {
                autostart: false,
                settings: true
            })
        );
    }

    #[test]
    fn enter_and_escape_finish_like_done() {
        for key in [Key::Escape, Key::Hot(hotkey::Key::Enter)] {
            let mut w = welcome(Os::MacOs, 2.0);
            w.event(Input::Key {
                key,
                mods: Mods::default(),
            });
            assert_eq!(w.finished().map(|f| f.settings), Some(false));
        }
        let mut w = welcome(Os::Windows, 1.0);
        w.event(Input::Close);
        assert!(w.finished().is_some());
    }

    #[test]
    fn draws_at_any_scale_and_theme() {
        for (scale, dark) in [(1.0, false), (1.5, true), (2.0, false)] {
            let mut w = Welcome::new(Hotkey::default_for(Os::Windows), Os::Windows, scale, dark);
            let (cw, ch) = w.content_size();
            // A client area larger than the content gets the background around it.
            let (dw, dh) = (cw + 8, ch + 8);
            let mut data = vec![0u8; dw as usize * dh as usize * 4];
            let mut dst = BgrxMut {
                width: dw,
                height: dh,
                stride: dw as usize * 4,
                data: &mut data,
            };
            w.draw(&mut dst);
            let bg = Palette::of(dark).surface_base;
            let last = data.len() - 4;
            assert_eq!(&data[last..last + 3], &[bg.b, bg.g, bg.r]);
        }
    }
}
