//! The settings window (`docs/design/settings.md`, "Settings window", "Hotkey recorder"): six rows
//! that change the settings at once, one of them recording a new hotkey.

use tiny_skia::Pixmap;

use super::form::{Act, Form, Keyed, Kind, Slot};
use super::widgets::{self, Field};
use super::{Input, Key, Mods};
use crate::geom::Rect;
use crate::hotkey::{Hotkey, Os, modifier_caps};
use crate::icons::{self, Icon};
use crate::image::BgrxMut;
use crate::settings::{Destination, ThemeChoice};
use crate::theme::{
    Palette, TYPE_BODY, TYPE_BODY_STRONG, TYPE_CAPTION, TYPE_HEADLINE, scaled, stroke,
};

const WIDTH: f32 = 560.0;
const PAD: f32 = 24.0;
const ICON: f32 = 40.0;
const ICON_GAP: f32 = 12.0;
const HEADER_H: f32 = 40.0;
const AFTER_HEADER: f32 = 16.0;
const ROW_H: f32 = 56.0;
/// What an error or hint line adds to its row.
const LINE_H: f32 = 24.0;
/// The line's top below its row's top: 6 below the controls.
const LINE_TOP: f32 = 50.0;
const LINE_ICON: f32 = 16.0;
const LINE_ICON_GAP: f32 = 8.0;
const LABEL_GAP: f32 = 2.0;
const CONTROL_GAP: f32 = 12.0;
/// Between a row's hint and its controls.
const HINT_GAP: f32 = 24.0;
const FOOTER_GAP: f32 = 12.0;

const TITLE: &str = "Shot";
const FOOTER: &str = "Изменения сохраняются сразу.";
const CHANGE_FOLDER: &str = "Изменить…";

const DESTINATIONS: [(&str, Destination); 3] = [
    ("Буфер", Destination::Clipboard),
    ("Файл", Destination::File),
    ("Буфер и файл", Destination::Both),
];
const SECONDS: [(&str, u32); 3] = [("3 с", 3), ("5 с", 5), ("10 с", 10)];
const THEMES: [(&str, ThemeChoice); 3] = [
    ("Как в системе", ThemeChoice::System),
    ("Светлая", ThemeChoice::Light),
    ("Тёмная", ThemeChoice::Dark),
];

/// The rows, top to bottom, with their labels and hints; the folder row's hint is the folder.
const ROWS: [(Row, &str, &str); 6] = [
    (Row::Hotkey, "Хоткей", "Снять область, окно или экран"),
    (Row::Destination, "После снимка", "Куда отправить снимок"),
    (Row::Folder, "Папка", ""),
    (Row::Preview, "Превью", "Карточка в углу после снимка"),
    (
        Row::Autostart,
        "Автозапуск",
        "Запускать Shot при входе в систему",
    ),
    (Row::Theme, "Тема", "Светлая или тёмная"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Hotkey,
    Destination,
    Folder,
    Preview,
    Autostart,
    Theme,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctl {
    Hotkey,
    Destination,
    Folder,
    PreviewSeconds,
    Preview,
    Autostart,
    Theme,
}

/// What the window shows: the settings and what the OS says about them.
#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    pub hotkey: Hotkey,
    /// The OS took `hotkey`.
    pub hotkey_works: bool,
    pub destination: Destination,
    /// The save folder as the row shows it.
    pub folder: String,
    pub preview: bool,
    pub preview_seconds: u32,
    pub autostart: bool,
    pub theme: ThemeChoice,
}

/// What the app has to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Recording started: let the hotkey go, so pressing it does not start a capture.
    PauseHotkey,
    /// Recording ended without a new hotkey: take the old one back.
    ResumeHotkey,
    /// Take this hotkey instead of the old one, then call [`SettingsWindow::hotkey_taken`] or
    /// [`SettingsWindow::hotkey_refused`].
    SetHotkey(Hotkey),
    Destination(Destination),
    /// Show the folder dialog; a chosen folder comes back with [`SettingsWindow::set_values`].
    PickFolder,
    Preview(bool),
    PreviewSeconds(u32),
    Autostart(bool),
    Theme(ThemeChoice),
}

/// The line under the hotkey row.
#[derive(Clone, Debug, PartialEq)]
enum Line {
    /// While recording: what the combination lacks.
    Hint(String),
    Error(String),
}

pub struct SettingsWindow {
    os: Os,
    version: String,
    values: Values,
    scale: f32,
    dark: bool,
    form: Form<Ctl>,
    /// Recording a hotkey, with these modifiers held.
    recording: Option<Mods>,
    /// The hint while recording, or why the last recording did not take.
    line: Option<Line>,
    requests: Vec<Request>,
    closed: bool,
    layout: Layout,
    icon: Option<Pixmap>,
    pixmap: Option<Pixmap>,
}

/// Where the rows lie, physical px.
#[derive(Clone, Debug, Default)]
struct Layout {
    /// Each row's top, below its divider.
    rows: [i32; 6],
    /// Where each row's controls begin.
    controls: [i32; 6],
    /// The footer's divider.
    footer: i32,
    height: i32,
}

impl SettingsWindow {
    /// `version` is shown under the name: `0.1.0`.
    pub fn new(values: Values, version: &str, os: Os, scale: f32, dark: bool) -> Self {
        let mut w = Self {
            os,
            version: format!("Версия {version}"),
            values,
            scale,
            dark,
            form: Form::new(),
            recording: None,
            line: None,
            requests: Vec::new(),
            closed: false,
            layout: Layout::default(),
            icon: None,
            pixmap: None,
        };
        w.lay_out();
        w
    }

    /// The client area the content needs, physical px: 560 logical wide, as high as the rows.
    pub fn content_size(&self) -> (u32, u32) {
        (scaled(WIDTH, self.scale) as u32, self.layout.height as u32)
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// The window asked to close: `Esc`, or the close button.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// A hotkey is being recorded: every key goes to the window, `Alt` and `Win` too.
    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn values(&self) -> &Values {
        &self.values
    }

    /// The settings changed elsewhere: the file, the tray menu, the folder dialog.
    pub fn set_values(&mut self, values: Values) {
        self.values = values;
        self.lay_out();
    }

    /// The OS took the recorded hotkey.
    pub fn hotkey_taken(&mut self, hotkey: Hotkey) {
        self.values.hotkey = hotkey;
        self.values.hotkey_works = true;
        self.line = None;
        self.lay_out();
    }

    /// The OS refused `wanted`; the old hotkey works again if `old_works`.
    pub fn hotkey_refused(&mut self, wanted: Hotkey, old_works: bool) {
        self.values.hotkey_works = old_works;
        let text = if old_works {
            format!(
                "{} занято другой программой. Работает прежнее: {}.",
                wanted.display(self.os),
                self.values.hotkey.display(self.os)
            )
        } else {
            format!(
                "{} занято другой программой. Запишите другое сочетание.",
                wanted.display(self.os)
            )
        };
        self.line = Some(Line::Error(text));
        self.lay_out();
    }

    /// Returns whether the window looks different now: the pointer moving over nothing changes
    /// nothing.
    pub fn event(&mut self, input: Input) -> bool {
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
                }
            }
            Input::Theme { dark } => self.dark = dark,
            Input::Close => self.close(),
            Input::Blur => {
                self.form.pointer(input);
                self.stop_recording();
            }
            Input::Key { key, mods } => self.key(key, mods),
            Input::KeyUp { mods } => {
                if self.recording.is_some() {
                    self.recording = Some(mods);
                }
            }
            Input::Down { x, y } => {
                // A click outside the field ends the recording.
                if self.recording.is_some() && self.form.hit(x, y).map(|t| t.0) != Some(Ctl::Hotkey)
                {
                    self.stop_recording();
                }
                self.form.pointer(input);
            }
            Input::Move { .. } | Input::Up { .. } | Input::Leave => {
                if let Some(act) = self.form.pointer(input) {
                    self.act(act);
                }
            }
        }
        self.lay_out();
        true
    }

    fn close(&mut self) {
        self.stop_recording();
        self.closed = true;
    }

    fn key(&mut self, key: Key, mods: Mods) {
        if self.recording.is_some() {
            return self.record(key, mods);
        }
        match self.form.key(key, mods) {
            Keyed::Used(Some(act)) => self.act(act),
            Keyed::Used(None) => {}
            Keyed::Unused => {
                if key == Key::Escape {
                    self.close();
                }
            }
        }
    }

    fn act(&mut self, act: Act<Ctl>) {
        let v = &mut self.values;
        let request = match act {
            Act::Press(Ctl::Hotkey) => {
                if self.recording.is_none() {
                    self.recording = Some(Mods::default());
                    self.line = None;
                    self.requests.push(Request::PauseHotkey);
                }
                return;
            }
            Act::Press(Ctl::Folder) => Request::PickFolder,
            Act::Press(Ctl::Preview) => {
                v.preview = !v.preview;
                Request::Preview(v.preview)
            }
            Act::Press(Ctl::Autostart) => {
                v.autostart = !v.autostart;
                Request::Autostart(v.autostart)
            }
            Act::Select(Ctl::Destination, i) => match DESTINATIONS.get(i) {
                Some(&(_, d)) if d != v.destination => {
                    v.destination = d;
                    Request::Destination(d)
                }
                _ => return,
            },
            Act::Select(Ctl::PreviewSeconds, i) => match SECONDS.get(i) {
                Some(&(_, n)) if n != v.preview_seconds => {
                    v.preview_seconds = n;
                    Request::PreviewSeconds(n)
                }
                _ => return,
            },
            Act::Select(Ctl::Theme, i) => match THEMES.get(i) {
                Some(&(_, t)) if t != v.theme => {
                    v.theme = t;
                    Request::Theme(t)
                }
                _ => return,
            },
            _ => return,
        };
        self.requests.push(request);
    }

    /// A key while recording: modifiers show as caps, a key with a required modifier ends it.
    fn record(&mut self, key: Key, mods: Mods) {
        let key = match key {
            Key::Escape => return self.stop_recording(),
            Key::Modifier => {
                self.recording = Some(mods);
                return;
            }
            Key::Hot(key) => key,
        };
        let hotkey = Hotkey {
            ctrl: mods.ctrl,
            alt: mods.alt,
            shift: mods.shift,
            meta: mods.meta,
            key,
        };
        if !hotkey.has_required_modifier(self.os) {
            self.recording = Some(mods);
            self.line = Some(Line::Hint(
                match self.os {
                    Os::Windows => "Нужен Ctrl или Alt",
                    Os::MacOs => "Нужен \u{2318} или \u{2303}",
                }
                .to_string(),
            ));
            return;
        }
        self.recording = None;
        if hotkey.is_reserved(self.os) {
            self.line = Some(Line::Error(format!(
                "{} — системный снимок экрана macOS. Выберите другое сочетание.",
                hotkey.display(self.os)
            )));
            self.requests.push(Request::ResumeHotkey);
        } else if hotkey == self.values.hotkey && self.values.hotkey_works {
            self.line = None;
            self.requests.push(Request::ResumeHotkey);
        } else {
            self.line = None;
            self.requests.push(Request::SetHotkey(hotkey));
        }
    }

    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            if matches!(self.line, Some(Line::Hint(_))) {
                self.line = None;
            }
            self.requests.push(Request::ResumeHotkey);
        }
    }

    /// The line under the hotkey row, if any: `(error, text)`.
    fn line(&self) -> Option<(bool, String)> {
        match &self.line {
            Some(Line::Hint(t)) => Some((false, t.clone())),
            Some(Line::Error(t)) => Some((true, t.clone())),
            None if !self.values.hotkey_works && self.recording.is_none() => Some((
                true,
                format!(
                    "{} занято другой программой. Запишите другое сочетание.",
                    self.values.hotkey.display(self.os)
                ),
            )),
            None => None,
        }
    }

    fn lay_out(&mut self) {
        let s = self.scale;
        let pad = scaled(PAD, s);
        let right = scaled(WIDTH, s) - pad;
        let line = stroke(1.0, s);
        let row_h = scaled(ROW_H, s);
        let gap = scaled(CONTROL_GAP, s);
        let has_line = self.line().is_some();
        let v = &self.values;
        let mut slots = Vec::new();
        let mut y = pad + scaled(HEADER_H, s) + scaled(AFTER_HEADER, s);
        for (i, &(row, _, _)) in ROWS.iter().enumerate() {
            y += line;
            let top = y;
            self.layout.rows[i] = top;
            // A control `w` × `h` ending at `x`, centered in the row.
            let at = |x: i32, w: i32, h: i32| Rect::new(x - w, top + (row_h - h) / 2, w, h);
            let segmented = |id, x: i32, labels: &[&str], selected| {
                let r = at(
                    x,
                    widgets::segmented_width(labels, s),
                    scaled(widgets::SEGMENTED_H, s),
                );
                let mut slot = Slot::new(id, Kind::Segmented { selected }, r);
                slot.parts = widgets::segments(r, labels, s);
                slot
            };
            let toggle = |id, on| {
                Slot::new(
                    id,
                    Kind::Toggle { on },
                    at(
                        right,
                        scaled(widgets::TOGGLE_W, s),
                        scaled(widgets::TOGGLE_H, s),
                    ),
                )
            };
            let first = slots.len();
            match row {
                Row::Hotkey => slots.push(Slot::new(
                    Ctl::Hotkey,
                    Kind::Hotkey,
                    at(
                        right,
                        scaled(widgets::FIELD_W, s),
                        scaled(widgets::FIELD_H, s),
                    ),
                )),
                Row::Destination => slots.push(segmented(
                    Ctl::Destination,
                    right,
                    &labels(&DESTINATIONS),
                    DESTINATIONS.iter().position(|d| d.1 == v.destination),
                )),
                Row::Folder => slots.push(Slot::new(
                    Ctl::Folder,
                    Kind::Button { primary: false },
                    at(
                        right,
                        widgets::button_width(CHANGE_FOLDER, s),
                        scaled(widgets::BUTTON_H, s),
                    ),
                )),
                Row::Preview => {
                    let flip = toggle(Ctl::Preview, v.preview);
                    let mut seconds = segmented(
                        Ctl::PreviewSeconds,
                        flip.rect.x - gap,
                        &labels(&SECONDS),
                        SECONDS.iter().position(|d| d.1 == v.preview_seconds),
                    );
                    seconds.enabled = v.preview;
                    slots.push(seconds);
                    slots.push(flip);
                }
                Row::Autostart => slots.push(toggle(Ctl::Autostart, v.autostart)),
                Row::Theme => slots.push(segmented(
                    Ctl::Theme,
                    right,
                    &labels(&THEMES),
                    THEMES.iter().position(|d| d.1 == v.theme),
                )),
            }
            let left = slots[first..]
                .iter()
                .map(|s| s.rect.x)
                .min()
                .unwrap_or(right);
            self.layout.controls[i] = left;
            // A toggle also flips by its row's label.
            if let Some(flip) = slots[first..]
                .iter_mut()
                .find(|s| matches!(s.kind, Kind::Toggle { .. }))
            {
                flip.label = Some(Rect::new(pad, top, left - gap - pad, row_h));
            }
            y += row_h;
            if row == Row::Hotkey && has_line {
                y += scaled(LINE_H, s);
            }
        }
        self.layout.footer = y;
        y += line + scaled(FOOTER_GAP, s) + scaled(TYPE_CAPTION.line_height, s) + pad;
        self.layout.height = y;
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
        let width = scaled(WIDTH, s);
        let line_w = stroke(1.0, s);

        // Header.
        let icon_px = scaled(ICON, s);
        if self.icon.as_ref().map(Pixmap::width) != Some(icon_px as u32) {
            self.icon = widgets::app_icon(icon_px as u32);
        }
        if let Some(icon) = &self.icon {
            widgets::sprite(pixmap, icon, pad, pad);
        }
        let x = pad + icon_px + scaled(ICON_GAP, s);
        widgets::label(
            pixmap,
            TYPE_HEADLINE,
            s,
            TITLE,
            x,
            pad,
            palette.text_primary,
        );
        let top = pad + scaled(TYPE_HEADLINE.line_height, s);
        widgets::label(
            pixmap,
            TYPE_CAPTION,
            s,
            &self.version,
            x,
            top,
            palette.text_secondary,
        );

        // Rows.
        let row_h = scaled(ROW_H, s);
        let block = scaled(TYPE_BODY_STRONG.line_height, s)
            + scaled(LABEL_GAP, s)
            + scaled(TYPE_CAPTION.line_height, s);
        for (i, &(row, label, hint)) in ROWS.iter().enumerate() {
            let top = self.layout.rows[i];
            widgets::divider(pixmap, pad, top - line_w, width - 2 * pad, palette, s);
            let label_top = top + (row_h - block) / 2;
            widgets::label(
                pixmap,
                TYPE_BODY_STRONG,
                s,
                label,
                pad,
                label_top,
                palette.text_primary,
            );
            let room = (self.layout.controls[i] - scaled(HINT_GAP, s) - pad) as f32;
            let hint = match row {
                Row::Folder => widgets::elide_middle(TYPE_CAPTION, s, &self.values.folder, room),
                _ => hint.to_string(),
            };
            let hint_top =
                label_top + scaled(TYPE_BODY_STRONG.line_height, s) + scaled(LABEL_GAP, s);
            widgets::label(
                pixmap,
                TYPE_CAPTION,
                s,
                &hint,
                pad,
                hint_top,
                palette.text_secondary,
            );
        }
        let line = self.line();
        for slot in &self.form.slots {
            let look = self.form.look(slot.id);
            match (slot.id, slot.kind) {
                (Ctl::Hotkey, _) => {
                    let caps = match self.recording {
                        Some(mods) => modifier_caps(mods.array(), self.os),
                        None => self.values.hotkey.caps(self.os),
                    };
                    let field = match (self.recording, &line) {
                        (Some(_), _) => Field::Recording(&caps),
                        (None, Some((true, _))) => Field::Error(&caps),
                        _ => Field::Shows(&caps),
                    };
                    widgets::hotkey_field(pixmap, slot.rect, field, look, palette, s);
                }
                (id, Kind::Segmented { selected }) => {
                    let names = match id {
                        Ctl::Destination => labels(&DESTINATIONS),
                        Ctl::PreviewSeconds => labels(&SECONDS),
                        _ => labels(&THEMES),
                    };
                    widgets::segmented(
                        pixmap,
                        slot.rect,
                        &slot.parts,
                        &names,
                        selected,
                        look,
                        palette,
                        s,
                    );
                }
                (_, Kind::Toggle { on }) => {
                    widgets::toggle(pixmap, slot.rect, on, look, palette, s)
                }
                (_, Kind::Button { primary }) => {
                    widgets::button(pixmap, slot.rect, CHANGE_FOLDER, primary, look, palette, s)
                }
                (_, Kind::Hotkey) => {}
            }
        }

        // The hotkey row's line.
        if let Some((error, text)) = line {
            let top = self.layout.rows[0] + scaled(LINE_TOP, s);
            let line_h = scaled(TYPE_BODY.line_height, s);
            let mut x = pad;
            let color = if error {
                let icon = scaled(LINE_ICON, s);
                icons::draw(
                    pixmap,
                    Icon::Alert,
                    x as f32,
                    (top + (line_h - icon) / 2) as f32,
                    icon as f32,
                    s,
                    palette.danger,
                );
                x += icon + scaled(LINE_ICON_GAP, s);
                palette.danger
            } else {
                palette.text_secondary
            };
            widgets::label(pixmap, TYPE_BODY, s, &text, x, top, color);
        }

        // Footer.
        let footer = self.layout.footer;
        widgets::divider(pixmap, pad, footer, width - 2 * pad, palette, s);
        let top = footer + line_w + scaled(FOOTER_GAP, s);
        widgets::label(
            pixmap,
            TYPE_CAPTION,
            s,
            FOOTER,
            pad,
            top,
            palette.text_secondary,
        );
    }
}

fn labels<T>(options: &[(&'static str, T)]) -> Vec<&'static str> {
    options.iter().map(|o| o.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey;

    fn values() -> Values {
        Values {
            hotkey: Hotkey::default_for(Os::Windows),
            hotkey_works: true,
            destination: Destination::Both,
            folder: r"C:\Users\Someone\Pictures\Shot".to_string(),
            preview: true,
            preview_seconds: 5,
            autostart: false,
            theme: ThemeChoice::System,
        }
    }

    fn window(scale: f32) -> SettingsWindow {
        SettingsWindow::new(values(), "0.1.0", Os::Windows, scale, false)
    }

    fn key(w: &mut SettingsWindow, key: Key, mods: Mods) {
        w.event(Input::Key { key, mods });
    }

    fn click(w: &mut SettingsWindow, r: Rect) {
        let (x, y) = ((r.x + r.width / 2) as f32, (r.y + r.height / 2) as f32);
        w.event(Input::Down { x, y });
        w.event(Input::Up { x, y });
    }

    fn rect(w: &SettingsWindow, id: Ctl) -> Rect {
        w.form.slot(id).unwrap().rect
    }

    fn press(w: &mut SettingsWindow, id: Ctl) {
        let r = rect(w, id);
        click(w, r);
    }

    const CTRL_ALT: Mods = Mods {
        ctrl: true,
        alt: true,
        shift: false,
        meta: false,
    };

    #[test]
    fn height_follows_the_rows() {
        let mut w = window(1.0);
        assert_eq!(w.content_size(), (560, 473));
        w.hotkey_refused(hotkey::Hotkey::parse("Ctrl+Alt+S").unwrap(), true);
        assert_eq!(w.content_size(), (560, 497));
        assert_eq!(window(1.5).content_size().0, 840);
    }

    #[test]
    fn controls_sit_on_the_right_in_their_rows() {
        let w = window(1.0);
        let field = rect(&w, Ctl::Hotkey);
        // 24 padding, 40 header, 16, a 1 px divider, then centered in 56.
        assert_eq!(field, Rect::new(352, 93, 184, 32));
        let flip = rect(&w, Ctl::Preview);
        assert_eq!((flip.right(), flip.width, flip.height), (536, 40, 20));
        assert_eq!(rect(&w, Ctl::PreviewSeconds).right(), flip.x - 12);
    }

    #[test]
    fn records_a_hotkey() {
        let mut w = window(1.0);
        press(&mut w, Ctl::Hotkey);
        assert!(w.recording());
        assert_eq!(w.take_requests(), [Request::PauseHotkey]);

        // Shift alone is not enough on Windows: recording goes on with a hint.
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        key(&mut w, Key::Modifier, shift);
        key(&mut w, Key::Hot(hotkey::Key::Char('2')), shift);
        assert!(w.recording());
        assert_eq!(w.line().map(|l| l.0), Some(false));
        assert!(w.take_requests().is_empty());

        key(&mut w, Key::Hot(hotkey::Key::Char('S')), CTRL_ALT);
        assert!(!w.recording());
        let wanted = Hotkey::parse("Ctrl+Alt+S").unwrap();
        assert_eq!(w.take_requests(), [Request::SetHotkey(wanted)]);
        w.hotkey_taken(wanted);
        assert_eq!(w.values().hotkey, wanted);
        assert_eq!(w.line(), None);
    }

    #[test]
    fn a_refused_hotkey_says_which_works() {
        let mut w = window(1.0);
        w.hotkey_refused(Hotkey::parse("Ctrl+Alt+S").unwrap(), true);
        assert_eq!(
            w.line().unwrap().1,
            "Ctrl+Alt+S занято другой программой. Работает прежнее: Ctrl+Shift+2."
        );
        let mut broken = values();
        broken.hotkey_works = false;
        w.set_values(broken);
        w.line = None;
        assert_eq!(
            w.line().unwrap().1,
            "Ctrl+Shift+2 занято другой программой. Запишите другое сочетание."
        );
    }

    #[test]
    fn escape_cancels_recording_then_closes() {
        let mut w = window(1.0);
        press(&mut w, Ctl::Hotkey);
        key(&mut w, Key::Escape, Mods::default());
        assert!(!w.recording() && !w.closed());
        assert_eq!(
            w.take_requests(),
            [Request::PauseHotkey, Request::ResumeHotkey]
        );
        key(&mut w, Key::Escape, Mods::default());
        assert!(w.closed());
    }

    #[test]
    fn a_click_elsewhere_or_blur_cancels_recording() {
        let mut w = window(1.0);
        press(&mut w, Ctl::Hotkey);
        press(&mut w, Ctl::Autostart);
        assert_eq!(
            w.take_requests(),
            [
                Request::PauseHotkey,
                Request::ResumeHotkey,
                Request::Autostart(true)
            ]
        );
        press(&mut w, Ctl::Hotkey);
        w.event(Input::Blur);
        assert_eq!(
            w.take_requests(),
            [Request::PauseHotkey, Request::ResumeHotkey]
        );
    }

    #[test]
    fn macos_keeps_its_screenshot_keys() {
        let mut w = SettingsWindow::new(values(), "0.1.0", Os::MacOs, 1.0, false);
        press(&mut w, Ctl::Hotkey);
        let mods = Mods {
            shift: true,
            meta: true,
            ..Mods::default()
        };
        key(&mut w, Key::Hot(hotkey::Key::Char('4')), mods);
        assert_eq!(
            w.take_requests(),
            [Request::PauseHotkey, Request::ResumeHotkey]
        );
        assert_eq!(
            w.line().unwrap().1,
            "\u{21E7}\u{2318}4 — системный снимок экрана macOS. Выберите другое сочетание."
        );
    }

    #[test]
    fn rows_change_their_settings() {
        let mut w = window(1.0);
        let parts = w.form.slot(Ctl::Destination).unwrap().parts.clone();
        click(&mut w, parts[0]);
        click(&mut w, parts[0]);
        let theme = w.form.slot(Ctl::Theme).unwrap().parts.clone();
        click(&mut w, theme[2]);
        press(&mut w, Ctl::Folder);
        // The preview label flips the preview, which disables its seconds.
        let label = w.form.slot(Ctl::Preview).unwrap().label.unwrap();
        click(&mut w, Rect::new(label.x, label.y, 20, label.height));
        assert!(!w.form.slot(Ctl::PreviewSeconds).unwrap().enabled);
        assert_eq!(
            w.take_requests(),
            [
                Request::Destination(Destination::Clipboard),
                Request::Theme(ThemeChoice::Dark),
                Request::PickFolder,
                Request::Preview(false),
            ]
        );
    }

    #[test]
    fn tab_walks_the_rows_and_arrows_choose() {
        let mut w = window(1.0);
        let tab = Key::Hot(hotkey::Key::Tab);
        for _ in 0..2 {
            key(&mut w, tab, Mods::default());
        }
        assert_eq!(w.form.focus, Some(Ctl::Destination));
        key(&mut w, Key::Hot(hotkey::Key::Left), Mods::default());
        assert_eq!(w.take_requests(), [Request::Destination(Destination::File)]);
        for _ in 0..3 {
            key(&mut w, tab, Mods::default());
        }
        assert_eq!(w.form.focus, Some(Ctl::Preview));
        key(&mut w, Key::Hot(hotkey::Key::Space), Mods::default());
        assert_eq!(w.take_requests(), [Request::Preview(false)]);
    }

    #[test]
    fn draws_at_any_scale_and_theme() {
        for (scale, dark) in [(1.0, false), (1.25, true), (1.5, false)] {
            let mut w = SettingsWindow::new(values(), "0.1.0", Os::Windows, scale, dark);
            w.hotkey_refused(Hotkey::parse("Ctrl+Alt+S").unwrap(), true);
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
            assert_eq!(&data[..3], &[bg.b, bg.g, bg.r]);
        }
    }
}
