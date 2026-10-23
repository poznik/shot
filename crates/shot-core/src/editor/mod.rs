//! The editor (`docs/design/editor.md`, SPEC §3.3): annotations over a capture, drawn here and
//! shown by the platform in an ordinary window. The platform passes in input, the clock and the
//! window's size; it gets back the parts of the window to redraw, the cursor, and what to do:
//! copy, save, close.

mod backdrop;
pub mod chrome;
pub mod history;
pub mod objects;
pub mod render;
pub mod view;

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::mem::{discriminant, take};
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;

use self::backdrop::Backdrop;
use self::chrome::{Button, Dialog, DialogButton, Sprite, Toolbar, ToolbarState};
use self::history::History;
use self::objects::{Axis, Counting, Handle, Object, Pt, Shape, Width, constrain, next_counter};
use self::view::View;
use crate::canvas;
use crate::geom::{Point, Rect};
use crate::hotkey::Os;
use crate::icons::Icon;
use crate::image::BgrxMut;
use crate::settings::Destination;
use crate::theme::{FADE_MS, Palette};

const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
const NOTICE: Duration = Duration::from_secs(2);
const NOTICE_ERROR: Duration = Duration::from_secs(5);
const FADE: Duration = Duration::from_millis(FADE_MS);
/// How far from the edge of [`Editor::render`]'s canvas its antialiasing may be off: the platform
/// draws a part of the window with this margin around it, and shows the part only.
pub const EDGE: i32 = 2;
/// A frame of a fade.
const FRAME: Duration = Duration::from_millis(16);
/// How far from an object a click still hits it, logical px.
const HIT_SLACK: f32 = 4.0;
/// A drawing shorter than this, logical px, was a click: it is dropped.
const MIN_DRAG: f32 = 3.0;
const NUDGE: f32 = 1.0;
const NUDGE_SHIFT: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Select,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Counter(Counting),
}

/// What the editor opens with.
pub struct Open {
    /// Opaque BGRA, as `image::decode_png` reads it.
    pub image: Pixmap,
    /// The scale of the monitor the capture came from: annotation sizes are tokens times this.
    pub image_scale: f32,
    /// The window title: the file name.
    pub name: String,
    /// The capture is in a file that saving overwrites.
    pub has_file: bool,
    /// The content size in physical px, and the scale of its monitor.
    pub window: (u32, u32),
    pub scale: f32,
    pub dark: bool,
    /// Where `Enter` puts the result.
    pub destination: Destination,
    pub os: Os,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Move {
        x: f32,
        y: f32,
        shift: bool,
    },
    Down {
        x: f32,
        y: f32,
        shift: bool,
    },
    Up {
        x: f32,
        y: f32,
        shift: bool,
    },
    /// The cursor left the window.
    Leave,
    /// The wheel or the trackpad moves the content by (`dx`, `dy`) window px.
    Scroll {
        dx: f32,
        dy: f32,
    },
    /// `Ctrl` + wheel or a pinch: `steps` zoom steps in (positive) or out around (`x`, `y`).
    Zoom {
        x: f32,
        y: f32,
        steps: i32,
    },
    /// `ctrl` is `Cmd` on macOS.
    Key {
        key: Key,
        ctrl: bool,
        shift: bool,
    },
    KeyUp {
        key: Key,
        shift: bool,
    },
    Resize {
        width: u32,
        height: u32,
        scale: f32,
    },
    Theme {
        dark: bool,
    },
    /// The window's close button.
    Close,
    /// The window lost the keyboard focus or the mouse capture.
    Blur,
}

/// Keys by their place on a US keyboard, whatever the layout: letters lowercase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Delete,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    Space,
    Shift,
}

/// What the platform has to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Put [`Editor::result`] on the clipboard, then call [`Editor::copied`].
    Copy,
    /// Save the result over the file (into a new one when there is none) and copy it, then call
    /// [`Editor::saved`] or [`Editor::failed`].
    Save,
    /// Write the file if `file`, copy if `copy`, and close the window.
    SaveAndClose { file: bool, copy: bool },
    /// Close the window, dropping the edits.
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Crosshair,
    Move,
    Resize(Axis),
    Grab,
    Grabbing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Notice {
    Copied,
    Saved,
    /// Not saved, and why: `нет доступа к папке`.
    Failed(&'static str),
}

/// The objects and the selection: what undo goes back to.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    objects: Vec<Object>,
    selected: Option<usize>,
}

/// What a press started.
#[derive(Clone, Debug)]
enum Drag {
    None,
    /// Drawing object `index` from `start`.
    Draw {
        index: usize,
        start: Pt,
        before: Snapshot,
    },
    /// Moving object `index`, grabbed at image point `grab`. `created`: a counter just placed.
    Move {
        index: usize,
        grab: Pt,
        original: Object,
        before: Snapshot,
        created: bool,
    },
    Handle {
        index: usize,
        handle: Handle,
        original: Object,
        before: Snapshot,
    },
    Pan {
        last: (f32, f32),
    },
    Button(Button),
    ZoomLabel,
    Dialog(DialogButton),
}

/// What lies under the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Dialog(Option<DialogButton>),
    /// The toolbar: a control or its background.
    Toolbar(Option<Button>),
    ZoomLabel,
    Handle(Handle),
    Object(usize),
    Canvas,
}

/// A sprite of chrome where it is in the window, and the state it shows.
struct Placed {
    key: u64,
    sprite: Sprite,
    /// The piece's top-left corner in window px.
    at: Point,
}

impl Placed {
    fn rect(&self) -> Rect {
        self.sprite.rect_at(self.at)
    }
}

/// Rebuilds the sprite in `slot` when `key` changed; `None` removes it.
fn keep(
    slot: &mut Option<Placed>,
    key: Option<u64>,
    build: impl FnOnce() -> Option<(Sprite, Point)>,
) {
    match key {
        None => *slot = None,
        Some(k) if slot.as_ref().is_some_and(|p| p.key == k) => {}
        Some(k) => *slot = build().map(|(sprite, at)| Placed { key: k, sprite, at }),
    }
}

fn hash(value: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn hash_object(o: &Object, h: &mut impl Hasher) {
    discriminant(&o.shape).hash(h);
    let points = match o.shape {
        Shape::Line { a, b }
        | Shape::Arrow { a, b }
        | Shape::Rect { a, b }
        | Shape::Ellipse { a, b } => [a, b],
        Shape::Counter { at, counting, n } => {
            (counting, n).hash(h);
            [at, at]
        }
    };
    for p in points {
        (p.x.to_bits(), p.y.to_bits()).hash(h);
    }
    (o.color, o.width).hash(h);
}

struct NoticeState {
    notice: Notice,
    since: Instant,
    until: Instant,
}

struct DialogState {
    layout: Dialog,
    hover: Option<DialogButton>,
}

/// The selected object's handles, and the box of a rect or an ellipse: left, top, right, bottom.
type Decorations = (Vec<(f32, f32)>, Option<[f32; 4]>);

/// A part of the window and what it shows: when either changes, the part is redrawn.
#[derive(Clone, Copy, PartialEq)]
struct Visual {
    id: u32,
    rect: Rect,
    key: u64,
}

pub struct Editor {
    image: Pixmap,
    k: f32,
    name: String,
    has_file: bool,
    destination: Destination,
    os: Os,
    palette: &'static Palette,
    view: View,
    toolbar: Toolbar,

    objects: Vec<Object>,
    selected: Option<usize>,
    hovered: Option<usize>,
    tool: Tool,
    /// What `N` picks.
    counting: Counting,
    color: usize,
    width: Width,
    history: History<Snapshot>,
    saved: Vec<Object>,
    /// The objects of a save under way.
    saving: Option<Vec<Object>>,

    drag: Drag,
    pointer: Option<(f32, f32)>,
    shift: bool,
    space: bool,
    hover_button: Option<Button>,
    zoom_hover: bool,
    /// The control under the cursor, since when, and whether a press hid its tooltip.
    tip: Option<(Button, Instant, bool)>,
    notice: Option<NoticeState>,
    dialog: Option<DialogState>,
    requests: Vec<Request>,

    now: Instant,
    backdrop: Option<(u64, Backdrop)>,
    toolbar_base: Option<(u64, Sprite)>,
    toolbar_sprite: Option<Placed>,
    tooltip_sprite: Option<Placed>,
    zoom_sprite: Option<Placed>,
    notice_sprite: Option<Placed>,
    notice_alpha: u8,
    dialog_sprite: Option<Placed>,
    visuals: Vec<Visual>,
    damage: Vec<Rect>,
}

impl Editor {
    pub fn new(open: Open, now: Instant) -> Self {
        let size = (open.image.width(), open.image.height());
        let view = View::new(size, open.window, open.scale);
        let mut editor = Self {
            image: open.image,
            k: open.image_scale,
            name: open.name,
            has_file: open.has_file,
            destination: open.destination,
            os: open.os,
            palette: Palette::of(open.dark),
            toolbar: Toolbar::at(open.window, open.scale),
            view,
            objects: Vec::new(),
            selected: None,
            hovered: None,
            tool: Tool::Arrow,
            counting: Counting::Numbers,
            color: 0,
            width: Width::M,
            history: History::default(),
            saved: Vec::new(),
            saving: None,
            drag: Drag::None,
            pointer: None,
            shift: false,
            space: false,
            hover_button: None,
            zoom_hover: false,
            tip: None,
            notice: None,
            dialog: None,
            requests: Vec::new(),
            now,
            backdrop: None,
            toolbar_base: None,
            toolbar_sprite: None,
            tooltip_sprite: None,
            zoom_sprite: None,
            notice_sprite: None,
            notice_alpha: 0,
            dialog_sprite: None,
            visuals: Vec::new(),
            damage: Vec::new(),
        };
        editor.refresh(now);
        editor
    }

    pub fn window(&self) -> (u32, u32) {
        self.view.window()
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn zoom(&self) -> f32 {
        self.view.zoom()
    }

    /// There are edits since the last save.
    pub fn dirty(&self) -> bool {
        self.objects != self.saved
    }

    /// The dialog asks whether to save the edits before closing.
    pub fn asking(&self) -> bool {
        self.dialog.is_some()
    }

    /// The window title: the file name, with `*` before it on Windows while there are unsaved
    /// edits (macOS shows them with `setDocumentEdited`).
    pub fn title(&self) -> String {
        if self.os == Os::Windows && self.dirty() {
            format!("* {}", self.name)
        } else {
            self.name.clone()
        }
    }

    /// The image with the annotations drawn into it: what is copied and saved.
    pub fn result(&self) -> Pixmap {
        render::flatten(&self.image, &self.objects, self.k)
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        take(&mut self.requests)
    }

    /// The parts of the window to redraw since the last call: inside the window, apart.
    pub fn take_damage(&mut self) -> Vec<Rect> {
        let (w, h) = self.window();
        let whole = Rect::new(0, 0, w as i32, h as i32);
        merge(take(&mut self.damage))
            .into_iter()
            .filter_map(|r| r.intersect(&whole))
            .collect()
    }

    /// The result is on the clipboard.
    pub fn copied(&mut self, now: Instant) {
        self.show(Notice::Copied, now);
    }

    /// The save finished; `name` when it went into a new file.
    pub fn saved(&mut self, name: Option<String>, now: Instant) {
        if let Some(objects) = self.saving.take() {
            self.saved = objects;
        }
        if let Some(name) = name {
            self.name = name;
            self.has_file = true;
        }
        self.show(Notice::Saved, now);
    }

    pub fn failed(&mut self, reason: &'static str, now: Instant) {
        self.saving = None;
        self.show(Notice::Failed(reason), now);
    }

    fn show(&mut self, notice: Notice, now: Instant) {
        let shown = if matches!(notice, Notice::Failed(_)) {
            NOTICE_ERROR
        } else {
            NOTICE
        };
        self.notice = Some(NoticeState {
            notice,
            since: now,
            until: now + shown,
        });
        self.refresh(now);
    }

    /// When [`Editor::tick`] has to run next: a tooltip to show, a notice to fade.
    pub fn next_deadline(&self) -> Option<Instant> {
        let tip = self
            .tip
            .filter(|&(b, since, hidden)| {
                !hidden && self.now < since + TOOLTIP_DELAY && b.tooltip(self.os).is_some()
            })
            .map(|(_, since, _)| since + TOOLTIP_DELAY);
        let notice = self.notice.as_ref().map(|n| {
            let fading = self.now < n.since + FADE || self.now >= n.until - FADE;
            if fading {
                (self.now + FRAME).min(n.until)
            } else {
                n.until - FADE
            }
        });
        [tip, notice].into_iter().flatten().min()
    }

    pub fn tick(&mut self, now: Instant) {
        self.refresh(now);
    }

    pub fn handle(&mut self, input: Input, now: Instant) {
        match input {
            Input::Move { x, y, shift } => {
                self.pointer = Some((x, y));
                self.shift = shift;
                self.on_move(x, y, now);
            }
            Input::Down { x, y, shift } => {
                self.pointer = Some((x, y));
                self.shift = shift;
                self.on_down(x, y, now);
            }
            Input::Up { x, y, shift } => {
                self.pointer = Some((x, y));
                self.shift = shift;
                self.on_up(x, y, now);
            }
            Input::Leave => {
                if matches!(self.drag, Drag::None) {
                    self.pointer = None;
                    self.hover_button = None;
                    self.zoom_hover = false;
                    self.hovered = None;
                    self.tip = None;
                }
            }
            Input::Scroll { dx, dy } => {
                if self.dialog.is_none() && self.view.scrolls() {
                    self.view.scroll(dx, dy);
                    self.follow_pointer(now);
                }
            }
            Input::Zoom { x, y, steps } => {
                if self.dialog.is_none() {
                    self.view.zoom_steps(steps, x, y);
                    self.follow_pointer(now);
                }
            }
            Input::Key { key, ctrl, shift } => {
                self.shift = shift;
                self.on_key(key, ctrl, shift, now);
            }
            Input::KeyUp { key, shift } => {
                self.shift = shift;
                if key == Key::Space {
                    self.space = false;
                }
                if key == Key::Shift {
                    self.follow_pointer(now);
                }
            }
            Input::Resize {
                width,
                height,
                scale,
            } => {
                self.view.resize((width, height), scale);
                self.toolbar = Toolbar::at((width, height), scale);
                if let Some(d) = &mut self.dialog {
                    d.layout = Dialog::of((width, height), scale, &self.name);
                }
                self.follow_pointer(now);
            }
            Input::Theme { dark } => self.palette = Palette::of(dark),
            Input::Close => {
                if self.dialog.is_none() {
                    self.close_or_ask();
                }
            }
            Input::Blur => {
                self.space = false;
                if let Some((x, y)) = self.pointer
                    && !matches!(self.drag, Drag::None)
                {
                    self.on_up(x, y, now);
                }
            }
        }
        self.refresh(now);
    }

    /// After the view changed under a still cursor: the drag and the hover follow.
    fn follow_pointer(&mut self, now: Instant) {
        if let Some((x, y)) = self.pointer {
            self.on_move(x, y, now);
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            objects: self.objects.clone(),
            selected: self.selected,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.objects = s.objects;
        self.selected = s.selected.filter(|&i| i < self.objects.len());
        self.hovered = None;
    }

    fn scale(&self) -> f32 {
        self.view.scale()
    }

    fn clamp_to_image(&self, p: Pt) -> Pt {
        Pt::new(
            p.x.clamp(0.0, self.image.width() as f32),
            p.y.clamp(0.0, self.image.height() as f32),
        )
    }

    fn zoom_percent(&self) -> u32 {
        (self.view.zoom() * 100.0).round() as u32
    }

    fn target(&self, x: f32, y: f32) -> Target {
        let (xi, yi) = (x.floor() as i32, y.floor() as i32);
        if let Some(d) = &self.dialog {
            return Target::Dialog(d.layout.hit(xi, yi));
        }
        if let Some(hit) = self.toolbar.hit(xi, yi) {
            return Target::Toolbar(hit);
        }
        let s = self.scale();
        if chrome::zoom_label_rect(self.window(), s, self.zoom_percent()).contains(xi, yi) {
            return Target::ZoomLabel;
        }
        if let Some(i) = self.selected {
            let reach = render::handle_reach(s);
            let nearest = self.objects[i]
                .handles()
                .into_iter()
                .map(|(h, p)| {
                    let (hx, hy) = self.view.to_window(p);
                    (h, (hx - x).abs().max((hy - y).abs()))
                })
                .filter(|&(_, d)| d <= reach)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((h, _)) = nearest {
                return Target::Handle(h);
            }
        }
        if self.tool == Tool::Select {
            let p = self.view.to_image(x, y);
            let slack = HIT_SLACK * s * self.view.image_per_window();
            if let Some(i) = (0..self.objects.len())
                .rev()
                .find(|&i| self.objects[i].hit(p, self.k, slack))
            {
                return Target::Object(i);
            }
        }
        Target::Canvas
    }

    fn on_move(&mut self, x: f32, y: f32, now: Instant) {
        let target = self.target(x, y);
        self.hover_button = match target {
            Target::Toolbar(b) => b,
            _ => None,
        };
        self.zoom_hover = target == Target::ZoomLabel;
        if let Some(d) = &mut self.dialog {
            d.hover = match target {
                Target::Dialog(b) => b,
                _ => None,
            };
        }
        self.tip = match (self.tip, self.hover_button) {
            (Some(tip), Some(b)) if tip.0 == b => Some(tip),
            (_, Some(b)) => Some((b, now, !matches!(self.drag, Drag::None))),
            (_, None) => None,
        };
        self.hovered = match (&self.drag, target) {
            (Drag::Move { index, .. }, _) => Some(*index),
            (Drag::None, Target::Object(i)) => Some(i),
            _ => None,
        };

        match self.drag.clone() {
            Drag::Pan { last } => {
                self.view.scroll(x - last.0, y - last.1);
                self.drag = Drag::Pan { last: (x, y) };
            }
            Drag::Draw { index, start, .. } => {
                let mut p = self.clamp_to_image(self.view.to_image(x, y));
                let o = &mut self.objects[index];
                let line = matches!(o.shape, Shape::Line { .. } | Shape::Arrow { .. });
                if self.shift {
                    p = constrain(line, start, p);
                }
                let (a, b) = (start, p);
                o.shape = match o.shape {
                    Shape::Line { .. } => Shape::Line { a, b },
                    Shape::Arrow { .. } => Shape::Arrow { a, b },
                    Shape::Rect { .. } => Shape::Rect { a, b },
                    Shape::Ellipse { .. } => Shape::Ellipse { a, b },
                    shape => shape,
                };
            }
            Drag::Move {
                index,
                grab,
                original,
                ..
            } => {
                let mut d = self.view.to_image(x, y) - grab;
                // The object's middle stays on the image, so it cannot be lost off its edge.
                let c = original.bounds(self.k).center();
                d = self.clamp_to_image(c + d) - c;
                let mut o = original;
                o.translate(d);
                self.objects[index] = o;
            }
            Drag::Handle {
                index,
                handle,
                original,
                ..
            } => {
                let mut to = self.clamp_to_image(self.view.to_image(x, y));
                if self.shift
                    && let Shape::Line { a, b } | Shape::Arrow { a, b } = original.shape
                {
                    let fixed = if handle.0 == 0 { b } else { a };
                    to = constrain(true, fixed, to);
                }
                let mut o = original;
                o.drag_handle(&original, handle, to);
                self.objects[index] = o;
            }
            Drag::None | Drag::Button(_) | Drag::ZoomLabel | Drag::Dialog(_) => {}
        }
    }

    fn on_down(&mut self, x: f32, y: f32, now: Instant) {
        if let Some(tip) = &mut self.tip {
            tip.2 = true;
        }
        if !matches!(self.drag, Drag::None) {
            return;
        }
        let target = self.target(x, y);
        self.drag = match target {
            Target::Dialog(Some(b)) => Drag::Dialog(b),
            Target::Dialog(None) | Target::Toolbar(None) => Drag::None,
            Target::Toolbar(Some(b)) if self.enabled(b) => Drag::Button(b),
            Target::Toolbar(Some(_)) => Drag::None,
            Target::ZoomLabel => Drag::ZoomLabel,
            _ if self.space => Drag::Pan { last: (x, y) },
            Target::Handle(handle) => match self.selected {
                Some(index) => Drag::Handle {
                    index,
                    handle,
                    original: self.objects[index],
                    before: self.snapshot(),
                },
                None => Drag::None,
            },
            Target::Object(index) => {
                self.selected = Some(index);
                Drag::Move {
                    index,
                    grab: self.view.to_image(x, y),
                    original: self.objects[index],
                    before: self.snapshot(),
                    created: false,
                }
            }
            Target::Canvas => self.start_drawing(x, y),
        };
        self.on_move(x, y, now);
    }

    fn start_drawing(&mut self, x: f32, y: f32) -> Drag {
        let p = self.clamp_to_image(self.view.to_image(x, y));
        let before = self.snapshot();
        let shape = match self.tool {
            Tool::Select => {
                self.selected = None;
                return Drag::None;
            }
            Tool::Arrow => Shape::Arrow { a: p, b: p },
            Tool::Line => Shape::Line { a: p, b: p },
            Tool::Rect => Shape::Rect { a: p, b: p },
            Tool::Ellipse => Shape::Ellipse { a: p, b: p },
            Tool::Counter(counting) => Shape::Counter {
                at: p,
                counting,
                n: next_counter(&self.objects, counting),
            },
        };
        let object = Object {
            shape,
            color: self.color,
            width: self.width,
        };
        self.objects.push(object);
        let index = self.objects.len() - 1;
        self.selected = Some(index);
        if matches!(shape, Shape::Counter { .. }) {
            // Placed at once; dragging moves it.
            Drag::Move {
                index,
                grab: p,
                original: object,
                before,
                created: true,
            }
        } else {
            Drag::Draw {
                index,
                start: p,
                before,
            }
        }
    }

    fn on_up(&mut self, x: f32, y: f32, now: Instant) {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        let target = self.target(x, y);
        match drag {
            Drag::Draw { index, before, .. } => {
                let min = MIN_DRAG * self.scale() * self.view.image_per_window();
                if self.objects[index].is_degenerate(min) {
                    // A click: nothing drawn, and the selection is let go.
                    self.objects.remove(index);
                    self.selected = None;
                } else {
                    self.history.record(before, None);
                }
            }
            Drag::Move {
                index,
                original,
                before,
                created,
                ..
            } => {
                if created || self.objects[index] != original {
                    self.history.record(before, None);
                }
            }
            Drag::Handle {
                index,
                original,
                before,
                ..
            } => {
                if self.objects[index] != original {
                    self.history.record(before, None);
                }
            }
            Drag::Button(b) => {
                if target == Target::Toolbar(Some(b)) {
                    self.press(b);
                }
            }
            Drag::ZoomLabel => {
                if target == Target::ZoomLabel {
                    if (self.view.zoom() - 1.0).abs() < 1e-3 {
                        self.view.fit();
                    } else {
                        self.view.actual_size();
                    }
                }
            }
            Drag::Dialog(b) => {
                if target == Target::Dialog(Some(b)) {
                    self.choose(b);
                }
            }
            Drag::Pan { .. } | Drag::None => {}
        }
        self.on_move(x, y, now);
    }

    fn enabled(&self, b: Button) -> bool {
        match b {
            Button::Undo => self.history.can_undo(),
            Button::Redo => self.history.can_redo(),
            _ => true,
        }
    }

    fn press(&mut self, b: Button) {
        match b {
            Button::Tool(t) => self.set_tool(t),
            Button::Color(c) => self.set_color(c),
            Button::Width(w) => self.set_width(w),
            Button::Undo => self.undo(),
            Button::Redo => self.redo(),
            Button::Copy => self.requests.push(Request::Copy),
            Button::Save => self.save(),
            Button::Close => self.close_or_ask(),
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if let Tool::Counter(c) = tool {
            self.counting = c;
        }
    }

    /// Changes the selected object, as one step of history.
    fn edit_selected(&mut self, change: impl FnOnce(&mut Object)) {
        let Some(i) = self.selected else { return };
        let before = self.snapshot();
        change(&mut self.objects[i]);
        if self.objects[i] != before.objects[i] {
            self.history.record(before, None);
        }
    }

    fn set_color(&mut self, color: usize) {
        self.color = color;
        self.edit_selected(|o| o.color = color);
    }

    fn set_width(&mut self, width: Width) {
        self.width = width;
        self.edit_selected(|o| o.width = width);
    }

    fn editing(&self) -> bool {
        matches!(
            self.drag,
            Drag::Draw { .. } | Drag::Move { .. } | Drag::Handle { .. }
        )
    }

    fn undo(&mut self) {
        if !self.editing()
            && let Some(prev) = self.history.undo(self.snapshot())
        {
            self.restore(prev);
        }
    }

    fn redo(&mut self) {
        if !self.editing()
            && let Some(next) = self.history.redo(self.snapshot())
        {
            self.restore(next);
        }
    }

    fn save(&mut self) {
        self.saving = Some(self.objects.clone());
        self.requests.push(Request::Save);
    }

    fn close_or_ask(&mut self) {
        if self.dirty() {
            self.dialog = Some(DialogState {
                layout: Dialog::of(self.window(), self.scale(), &self.name),
                hover: None,
            });
            self.tip = None;
        } else {
            self.requests.push(Request::Close);
        }
    }

    fn choose(&mut self, b: DialogButton) {
        self.dialog = None;
        match b {
            DialogButton::Discard => self.requests.push(Request::Close),
            DialogButton::Cancel => {}
            DialogButton::Save => self.requests.push(Request::SaveAndClose {
                file: true,
                copy: true,
            }),
        }
    }

    fn on_key(&mut self, key: Key, ctrl: bool, shift: bool, now: Instant) {
        if self.dialog.is_some() {
            match key {
                Key::Enter => self.choose(DialogButton::Save),
                Key::Escape => self.choose(DialogButton::Cancel),
                _ => {}
            }
            return;
        }
        match key {
            Key::Shift => return self.follow_pointer(now),
            Key::Space => {
                self.space = true;
                return;
            }
            Key::Escape if self.editing() => {
                // Drops the drag in progress.
                if let Drag::Draw { before, .. }
                | Drag::Move { before, .. }
                | Drag::Handle { before, .. } = std::mem::replace(&mut self.drag, Drag::None)
                {
                    self.restore(before);
                }
                return;
            }
            _ if self.editing() => return,
            _ => {}
        }
        match (key, ctrl) {
            (Key::Escape, _) => self.close_or_ask(),
            (Key::Enter, _) => {
                let file = self.destination.file() && (self.dirty() || !self.has_file);
                self.requests.push(Request::SaveAndClose {
                    file,
                    copy: self.destination.clipboard(),
                });
            }
            (Key::Delete | Key::Backspace, false) => {
                if let Some(i) = self.selected {
                    let before = self.snapshot();
                    self.objects.remove(i);
                    self.selected = None;
                    self.hovered = None;
                    self.history.record(before, None);
                }
            }
            (Key::Left | Key::Right | Key::Up | Key::Down, false) => {
                let step = if shift { NUDGE_SHIFT } else { NUDGE };
                let d = match key {
                    Key::Left => Pt::new(-step, 0.0),
                    Key::Right => Pt::new(step, 0.0),
                    Key::Up => Pt::new(0.0, -step),
                    _ => Pt::new(0.0, step),
                };
                if let Some(i) = self.selected {
                    let before = self.snapshot();
                    self.objects[i].translate(d);
                    // A run of nudges of one object is one step.
                    self.history.record(before, Some(i as u64));
                }
            }
            (Key::Char(c), true) => match c {
                'z' if shift => self.redo(),
                'z' => self.undo(),
                'y' if self.os == Os::Windows => self.redo(),
                'c' => self.requests.push(Request::Copy),
                's' => self.save(),
                '0' => self.view.fit(),
                '1' => self.view.actual_size(),
                _ => {}
            },
            (Key::Char(c), false) => match c {
                'v' => self.set_tool(Tool::Select),
                'a' => self.set_tool(Tool::Arrow),
                'l' => self.set_tool(Tool::Line),
                'r' => self.set_tool(Tool::Rect),
                'o' => self.set_tool(Tool::Ellipse),
                'n' => {
                    let counting = match self.tool {
                        Tool::Counter(Counting::Numbers) => Counting::Letters,
                        Tool::Counter(Counting::Letters) => Counting::Numbers,
                        _ => self.counting,
                    };
                    self.set_tool(Tool::Counter(counting));
                }
                _ => {}
            },
            _ => {}
        }
        self.follow_pointer(now);
    }

    /// The cursor for where the mouse is and what it is doing.
    pub fn cursor(&self) -> Cursor {
        match &self.drag {
            Drag::Pan { .. } => return Cursor::Grabbing,
            Drag::Draw { .. } => return Cursor::Crosshair,
            Drag::Move { created: true, .. } => return Cursor::Crosshair,
            Drag::Move { .. } => return Cursor::Move,
            Drag::Handle { index, handle, .. } => {
                return Cursor::Resize(self.objects[*index].handle_axis(*handle));
            }
            Drag::Button(_) | Drag::ZoomLabel | Drag::Dialog(_) => return Cursor::Arrow,
            Drag::None => {}
        }
        let Some((x, y)) = self.pointer else {
            return Cursor::Arrow;
        };
        match self.target(x, y) {
            Target::Dialog(_) | Target::Toolbar(_) | Target::ZoomLabel => Cursor::Arrow,
            _ if self.space => Cursor::Grab,
            Target::Handle(h) => match self.selected {
                Some(i) => Cursor::Resize(self.objects[i].handle_axis(h)),
                None => Cursor::Arrow,
            },
            Target::Object(_) => Cursor::Move,
            Target::Canvas if self.tool == Tool::Select => Cursor::Arrow,
            Target::Canvas => Cursor::Crosshair,
        }
    }

    /// The object shows the `selection` outline in place of its halo.
    fn outlined(&self, i: usize) -> bool {
        let boxed = matches!(
            self.objects[i].shape,
            Shape::Rect { .. } | Shape::Ellipse { .. }
        );
        self.hovered == Some(i) || (self.selected == Some(i) && !boxed)
    }

    /// The selected object's handles and box, window px.
    fn decorations(&self) -> Option<Decorations> {
        let o = &self.objects[self.selected?];
        let handles = o
            .handles()
            .into_iter()
            .map(|(_, p)| self.view.to_window(p))
            .collect();
        let frame = match o.shape {
            Shape::Rect { a, b } | Shape::Ellipse { a, b } => {
                let (l, t) = self.view.to_window(Pt::new(a.x.min(b.x), a.y.min(b.y)));
                let (r, bb) = self.view.to_window(Pt::new(a.x.max(b.x), a.y.max(b.y)));
                Some([l, t, r, bb])
            }
            _ => None,
        };
        Some((handles, frame))
    }

    fn decorations_visual(&self) -> Option<Visual> {
        let (handles, frame) = self.decorations()?;
        let reach = render::handle_reach(self.scale()) / 2.0 + 2.0;
        let mut h = DefaultHasher::new();
        let mut bounds: Option<Rect> = None;
        let mut add = |l: f32, t: f32, r: f32, b: f32| {
            let rect = Rect::new(
                l.floor() as i32,
                t.floor() as i32,
                (r.ceil() - l.floor()) as i32 + 1,
                (b.ceil() - t.floor()) as i32 + 1,
            );
            bounds = Some(bounds.map_or(rect, |u| u.union(&rect)));
        };
        for &(x, y) in &handles {
            (x.to_bits(), y.to_bits()).hash(&mut h);
            add(x - reach, y - reach, x + reach, y + reach);
        }
        if let Some([l, t, r, b]) = frame {
            [l, t, r, b].map(f32::to_bits).hash(&mut h);
            add(l - 2.0, t - 2.0, r + 2.0, b + 2.0);
        }
        Some(Visual {
            id: 1,
            rect: bounds?,
            key: h.finish(),
        })
    }

    /// Brings the caches up to the state, and adds what changed on screen to the damage.
    fn refresh(&mut self, now: Instant) {
        self.now = now;
        if self.notice.as_ref().is_some_and(|n| now >= n.until) {
            self.notice = None;
        }
        let palette = self.palette;
        let s = self.scale();
        let window = self.window();
        let shown = self.view.shown();

        let size = (shown.width, shown.height);
        let prepared = hash((size, s.to_bits(), palette.dark));
        if self.backdrop.as_ref().is_none_or(|(k, _)| *k != prepared) {
            let backdrop = Backdrop::new(&self.image, size, s, palette.dark);
            self.backdrop = Some((prepared, backdrop));
        }

        let state = ToolbarState {
            tool: self.tool,
            color: self.selected.map_or(self.color, |i| self.objects[i].color),
            width: self.selected.map_or(self.width, |i| self.objects[i].width),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            hover: self.hover_button,
            pressed: match self.drag {
                Drag::Button(b) => Some(b),
                _ => None,
            },
        };
        let common = hash((window, s.to_bits(), palette.dark));
        let toolbar = self.toolbar;
        if self.toolbar_base.as_ref().is_none_or(|(k, _)| *k != common) {
            self.toolbar_base = chrome::toolbar_base(&toolbar, palette).map(|b| (common, b));
        }
        let base = self.toolbar_base.as_ref().map(|(_, b)| b);
        keep(
            &mut self.toolbar_sprite,
            Some(hash((common, state))),
            || {
                let sprite = chrome::toolbar(&toolbar, base?, &state, palette);
                Some((sprite, Point::new(toolbar.rect.x, toolbar.rect.y)))
            },
        );

        let tip = self
            .tip
            .filter(|&(_, since, hidden)| {
                !hidden && now >= since + TOOLTIP_DELAY && self.dialog.is_none()
            })
            .map(|(b, ..)| b)
            .filter(|b| b.tooltip(self.os).is_some());
        let os = self.os;
        keep(
            &mut self.tooltip_sprite,
            tip.map(|b| hash((common, b))),
            || chrome::tooltip(&toolbar, tip?, os, window, palette),
        );

        let percent = self.zoom_percent();
        let zoom_hover = self.zoom_hover;
        keep(
            &mut self.zoom_sprite,
            Some(hash((common, percent, zoom_hover))),
            || {
                let r = chrome::zoom_label_rect(window, s, percent);
                chrome::zoom_label(r, s, percent, zoom_hover, palette)
                    .map(|sprite| (sprite, Point::new(r.x, r.y)))
            },
        );

        let notice = self.notice.as_ref().map(|n| n.notice);
        keep(
            &mut self.notice_sprite,
            notice.map(|n| hash((common, n))),
            || {
                let (icon, icon_color, text, color) = match notice? {
                    Notice::Copied => (
                        Icon::Check,
                        palette.success,
                        "Скопировано".to_string(),
                        palette.text_primary,
                    ),
                    Notice::Saved => (
                        Icon::Check,
                        palette.success,
                        "Сохранено и скопировано".to_string(),
                        palette.text_primary,
                    ),
                    Notice::Failed(reason) => (
                        Icon::Alert,
                        palette.danger,
                        format!("Не сохранено: {reason}"),
                        palette.danger,
                    ),
                };
                chrome::notice(icon, icon_color, &text, color, window, s, palette)
            },
        );
        self.notice_alpha = self.notice.as_ref().map_or(0, |n| {
            let fade = FADE.as_secs_f32();
            let t_in = now.saturating_duration_since(n.since).as_secs_f32() / fade;
            let t_out = n.until.saturating_duration_since(now).as_secs_f32() / fade;
            // Sixteen levels: a fade is a few frames, and a still notice is no frame at all.
            let level = (t_in.min(t_out).min(1.0) * 16.0).round();
            (level * 255.0 / 16.0) as u8
        });

        let dialog = self.dialog.as_ref().map(|d| (d.layout.clone(), d.hover));
        let pressed = match self.drag {
            Drag::Dialog(b) => Some(b),
            _ => None,
        };
        keep(
            &mut self.dialog_sprite,
            dialog
                .as_ref()
                .map(|(layout, hover)| hash((common, layout.rect, *hover, pressed))),
            || {
                let (layout, hover) = dialog?;
                let sprite = layout.render(hover, pressed, palette)?;
                Some((sprite, Point::new(layout.rect.x, layout.rect.y)))
            },
        );

        let visuals = self.visuals();
        self.damage.extend(diff(&self.visuals, &visuals));
        self.visuals = visuals;
    }

    fn visuals(&self) -> Vec<Visual> {
        let (w, h) = self.window();
        let whole = Rect::new(0, 0, w as i32, h as i32);
        let mut v = vec![Visual {
            id: 0,
            rect: whole,
            key: hash((
                self.view.shown(),
                self.window(),
                self.backdrop.as_ref().map(|(k, _)| *k),
            )),
        }];
        let t = self.view.transform((0, 0));
        for (i, o) in self.objects.iter().enumerate() {
            if let Some(r) = render::object_rect(o, self.k, t) {
                let mut h = DefaultHasher::new();
                hash_object(o, &mut h);
                self.outlined(i).hash(&mut h);
                v.push(Visual {
                    id: 100 + i as u32,
                    rect: Rect::new(r.x(), r.y(), r.width() as i32, r.height() as i32),
                    key: h.finish(),
                });
            }
        }
        v.extend(self.decorations_visual());
        for (id, slot) in [
            (2, &self.toolbar_sprite),
            (3, &self.zoom_sprite),
            (4, &self.tooltip_sprite),
        ] {
            if let Some(p) = slot {
                v.push(Visual {
                    id,
                    rect: p.rect(),
                    key: p.key,
                });
            }
        }
        if let Some(p) = &self.notice_sprite {
            v.push(Visual {
                id: 5,
                rect: p.rect(),
                key: p.key ^ self.notice_alpha as u64,
            });
        }
        if let Some(p) = &self.dialog_sprite {
            v.push(Visual {
                id: 6,
                rect: whole,
                key: p.key,
            });
        }
        v
    }

    /// Draws `region` of the window into `out`, which is the region's size: what lies there is
    /// overwritten, so `out` needs no clearing. Shapes crossing the edge of `out` may be off by
    /// a shade within [`EDGE`] px of it.
    pub fn render(&self, out: &mut Pixmap, region: Rect) {
        debug_assert_eq!(
            (out.width() as i32, out.height() as i32),
            (region.width, region.height)
        );
        let Some((_, backdrop)) = &self.backdrop else {
            return;
        };
        let shown = self.view.shown();
        let local = Rect::new(0, 0, region.width, region.height);
        let at = |p: Point| Point::new(p.x - region.x, p.y - region.y);

        backdrop.draw_image(&mut canvas_of(out), region, &self.image, shown);
        let t = self.view.transform((region.x, region.y));
        let t0 = self.view.transform((0, 0));
        for (i, o) in self.objects.iter().enumerate() {
            let visible = render::object_rect(o, self.k, t0).is_some_and(|r| {
                Rect::new(r.x(), r.y(), r.width() as i32, r.height() as i32)
                    .intersect(&region)
                    .is_some()
            });
            if visible {
                render::draw_object(out, o, self.k, t, self.outlined(i));
            }
        }
        {
            let mut dst = canvas_of(out);
            for piece in region.subtract(&shown) {
                backdrop.draw_surround(&mut dst, region, piece, shown, self.palette);
            }
        }
        if let Some((handles, frame)) = self.decorations() {
            let s = self.scale();
            let (dx, dy) = (region.x as f32, region.y as f32);
            if let Some([l, t, r, b]) = frame {
                render::draw_selection_box(out, l - dx, t - dy, r - dx, b - dy, s);
            }
            let points: Vec<(f32, f32)> = handles.iter().map(|&(x, y)| (x - dx, y - dy)).collect();
            render::draw_handles(out, &points, s);
        }
        let mut dst = canvas_of(out);
        for p in [
            &self.toolbar_sprite,
            &self.zoom_sprite,
            &self.tooltip_sprite,
        ]
        .into_iter()
        .flatten()
        {
            let r = p.rect();
            canvas::draw_sprite(&mut dst, &p.sprite.pixmap, at(Point::new(r.x, r.y)), local);
        }
        if let Some(p) = &self.notice_sprite {
            let r = p.rect();
            canvas::draw_sprite_faded(
                &mut dst,
                &p.sprite.pixmap,
                at(Point::new(r.x, r.y)),
                local,
                self.notice_alpha,
            );
        }
        if let Some(p) = &self.dialog_sprite {
            canvas::fill(&mut dst, local, self.palette.scrim);
            let r = p.rect();
            canvas::draw_sprite(&mut dst, &p.sprite.pixmap, at(Point::new(r.x, r.y)), local);
        }
    }
}

/// A sprite-sized canvas over `out`; its alpha bytes are left as they come.
fn canvas_of(out: &mut Pixmap) -> BgrxMut<'_> {
    BgrxMut {
        width: out.width(),
        height: out.height(),
        stride: out.width() as usize * 4,
        data: out.data_mut(),
    }
}

/// What to redraw between two lists of visuals: where any part appeared, went or changed.
fn diff(old: &[Visual], new: &[Visual]) -> Vec<Rect> {
    let before: HashMap<u32, &Visual> = old.iter().map(|v| (v.id, v)).collect();
    let mut out = Vec::new();
    for v in new {
        match before.get(&v.id) {
            Some(o) if **o == *v => {}
            Some(o) => {
                out.push(o.rect);
                out.push(v.rect);
            }
            None => out.push(v.rect),
        }
    }
    let ids: Vec<u32> = new.iter().map(|v| v.id).collect();
    out.extend(old.iter().filter(|o| !ids.contains(&o.id)).map(|o| o.rect));
    merge(out)
}

/// Overlapping rects joined, so no pixel is drawn twice.
fn merge(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.retain(|r| !r.is_empty());
    let mut out: Vec<Rect> = Vec::new();
    while let Some(mut r) = rects.pop() {
        let mut i = 0;
        while i < out.len() {
            if out[i].intersect(&r).is_some() {
                r = r.union(&out.swap_remove(i));
                i = 0;
            } else {
                i += 1;
            }
        }
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests;
