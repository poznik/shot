//! The editor's chrome (`docs/design/editor.md`, "Toolbar", "Tooltip", "Zoom label", "Drag-out
//! button", "Notice", "Unsaved edits"): where each piece lies in the window, and each piece drawn
//! with its shadow into a sprite of its own, which the editor composes over the image.

use tiny_skia::{FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::Tool;
use super::objects::{Counting, SWATCHES, Width};
use crate::canvas::skia_color;
use crate::draw::{self, Area};
use crate::geom::{Point, Rect};
use crate::hotkey::Os;
use crate::icons::{self, Icon};
use crate::text;
use crate::theme::{
    Color, Palette, RADIUS_LG, RADIUS_MD, RADIUS_SM, SHADOW_LG, SHADOW_MD, SHADOW_SM, Shadow,
    TYPE_BODY, TYPE_BODY_STRONG, TYPE_CAPTION, TYPE_LABEL, TYPE_TITLE, scaled, stroke,
};

const TOOLBAR_W: f32 = 888.0;
const TOOLBAR_H: f32 = 44.0;
const TOOLBAR_TOP: f32 = 12.0;
const BUTTON: f32 = 32.0;
const CELL: f32 = 28.0;
const PAD: f32 = 6.0;
const ICON: f32 = 20.0;
const DIVIDER_H: f32 = 20.0;
/// Where each divider stands, logical px from the toolbar's left edge.
const DIVIDERS: [f32; 4] = [266.0, 543.0, 672.0, 765.0];
const SWATCH_R: f32 = 10.0;
const RING_GAP: f32 = 2.0;
const RING: f32 = 2.0;
const WIDTH_BAR: f32 = 16.0;

const TIP_GAP: f32 = 8.0;
const TIP_H: f32 = 24.0;
const TIP_PAD: f32 = 8.0;
const LABEL_INSET: f32 = 12.0;
const LABEL_H: f32 = 24.0;
const LABEL_PAD: f32 = 8.0;
const NOTICE_BOTTOM: f32 = 16.0;
const NOTICE_H: f32 = 32.0;
const NOTICE_PAD: f32 = 12.0;
const NOTICE_ICON: f32 = 16.0;
const NOTICE_GAP: f32 = 8.0;
const DIALOG_W: f32 = 400.0;
const DIALOG_PAD: f32 = 24.0;
const DIALOG_BUTTON_H: f32 = 32.0;
const DIALOG_BUTTON_PAD: f32 = 12.0;
const DIALOG_BUTTON_GAP: f32 = 8.0;

/// A control of the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Tool(Tool),
    Color(usize),
    Width(Width),
    Undo,
    Redo,
    Copy,
    Save,
    Close,
}

/// Every control and its left edge, logical px from the toolbar's left edge.
const BUTTONS: [(Button, f32); 23] = [
    (Button::Tool(Tool::Select), 6.0),
    (Button::Tool(Tool::Arrow), 42.0),
    (Button::Tool(Tool::Line), 78.0),
    (Button::Tool(Tool::Rect), 114.0),
    (Button::Tool(Tool::Ellipse), 150.0),
    (Button::Tool(Tool::Counter(Counting::Numbers)), 186.0),
    (Button::Tool(Tool::Counter(Counting::Letters)), 222.0),
    (Button::Color(0), 279.0),
    (Button::Color(1), 311.0),
    (Button::Color(2), 343.0),
    (Button::Color(3), 375.0),
    (Button::Color(4), 407.0),
    (Button::Color(5), 439.0),
    (Button::Color(6), 471.0),
    (Button::Color(7), 503.0),
    (Button::Width(Width::S), 556.0),
    (Button::Width(Width::M), 592.0),
    (Button::Width(Width::L), 628.0),
    (Button::Undo, 685.0),
    (Button::Redo, 721.0),
    (Button::Copy, 778.0),
    (Button::Save, 814.0),
    (Button::Close, 850.0),
];

impl Button {
    fn width(self) -> f32 {
        match self {
            Button::Color(_) => CELL,
            _ => BUTTON,
        }
    }

    fn icon(self) -> Option<Icon> {
        Some(match self {
            Button::Tool(Tool::Select) => Icon::Select,
            Button::Tool(Tool::Arrow) => Icon::Arrow,
            Button::Tool(Tool::Line) => Icon::Line,
            Button::Tool(Tool::Rect) => Icon::Rectangle,
            Button::Tool(Tool::Ellipse) => Icon::Ellipse,
            Button::Tool(Tool::Counter(Counting::Numbers)) => Icon::CounterNumber,
            Button::Tool(Tool::Counter(Counting::Letters)) => Icon::CounterLetter,
            Button::Undo => Icon::Undo,
            Button::Redo => Icon::Redo,
            Button::Copy => Icon::Copy,
            Button::Save => Icon::Save,
            Button::Close => Icon::Close,
            Button::Color(_) | Button::Width(_) => return None,
        })
    }

    /// The tooltip: the name and the shortcut. Colors and widths have none.
    pub fn tooltip(self, os: Os) -> Option<(&'static str, &'static str)> {
        let mac = os == Os::MacOs;
        Some(match self {
            Button::Tool(Tool::Select) => ("Выделение", "V"),
            Button::Tool(Tool::Arrow) => ("Стрелка", "A"),
            Button::Tool(Tool::Line) => ("Линия", "L"),
            Button::Tool(Tool::Rect) => ("Прямоугольник", "R"),
            Button::Tool(Tool::Ellipse) => ("Эллипс", "O"),
            Button::Tool(Tool::Counter(Counting::Numbers)) => ("Счётчик 1, 2, 3", "N"),
            Button::Tool(Tool::Counter(Counting::Letters)) => ("Счётчик A, B, C", "N"),
            Button::Undo => ("Отменить", if mac { "Cmd+Z" } else { "Ctrl+Z" }),
            Button::Redo => (
                "Повторить",
                if mac { "Cmd+Shift+Z" } else { "Ctrl+Shift+Z" },
            ),
            Button::Copy => ("Копировать", if mac { "Cmd+C" } else { "Ctrl+C" }),
            Button::Save => ("Сохранить", if mac { "Cmd+S" } else { "Ctrl+S" }),
            Button::Close => ("Закрыть", "Esc"),
            Button::Color(_) | Button::Width(_) => return None,
        })
    }
}

/// A piece of chrome drawn with its shadow, and where the piece's own top-left corner lies in it.
pub struct Sprite {
    pub pixmap: Pixmap,
    pub inner: Point,
}

impl Sprite {
    /// The sprite's place in the window when the piece's top-left corner is at `at`.
    pub fn rect_at(&self, at: Point) -> Rect {
        Rect::new(
            at.x - self.inner.x,
            at.y - self.inner.y,
            self.pixmap.width() as i32,
            self.pixmap.height() as i32,
        )
    }
}

fn paint(color: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint.anti_alias = true;
    paint
}

/// A `w` × `h` rounded panel filled with `fill`, a 1 px border inside its edge, and its shadow.
fn panel(
    w: i32,
    h: i32,
    radius: f32,
    shadow: Shadow,
    fill: Color,
    palette: &Palette,
    s: f32,
) -> Option<Sprite> {
    let radius = radius * s;
    let (mut pixmap, x, y) = draw::shadow(w as f32, h as f32, radius, shadow, palette.dark, s)?;
    let a = Area::new(x as f32, y as f32, w as f32, h as f32);
    draw::fill_round_rect(&mut pixmap, a, radius, fill);
    draw::border_inside(
        &mut pixmap,
        a,
        radius,
        stroke(1.0, s) as f32,
        palette.border_default,
    );
    Some(Sprite {
        pixmap,
        inner: Point::new(x, y),
    })
}

/// The band between circles of radius `outer` and `inner` around (`cx`, `cy`).
fn ring(pixmap: &mut Pixmap, cx: f32, cy: f32, outer: f32, inner: f32, color: Color) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, outer);
    pb.push_circle(cx, cy, inner);
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::EvenOdd,
            Transform::identity(),
            None,
        );
    }
}

/// `text` with its line box centered vertically in a box of height `h` at `top`.
fn label(
    pixmap: &mut Pixmap,
    style: crate::text::TypeStyle,
    s: f32,
    text: &str,
    x: f32,
    (top, h): (f32, f32),
    color: Color,
) -> f32 {
    let line_top = top + ((h - style.line_height * s) / 2.0).round();
    text::draw(pixmap, style, s, text, x, line_top, &paint(color))
}

/// Where the toolbar and its controls lie in a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Toolbar {
    pub rect: Rect,
    scale: f32,
}

impl Toolbar {
    pub fn at(window: (u32, u32), scale: f32) -> Self {
        let w = scaled(TOOLBAR_W, scale);
        Self {
            rect: Rect::new(
                (window.0 as i32 - w) / 2,
                scaled(TOOLBAR_TOP, scale),
                w,
                scaled(TOOLBAR_H, scale),
            ),
            scale,
        }
    }

    /// A control's rect relative to the toolbar's top-left corner.
    fn local(&self, b: Button, x: f32) -> Rect {
        let s = self.scale;
        Rect::new(
            scaled(x, s),
            scaled(PAD, s),
            scaled(b.width(), s),
            scaled(BUTTON, s),
        )
    }

    pub fn button_rect(&self, button: Button) -> Rect {
        BUTTONS
            .iter()
            .find(|(b, _)| *b == button)
            .map(|&(b, x)| self.local(b, x).offset(self.rect.x, self.rect.y))
            .unwrap_or_default()
    }

    /// The control under window px (`x`, `y`): `None` off the toolbar, `Some(None)` on its
    /// background.
    pub fn hit(&self, x: i32, y: i32) -> Option<Option<Button>> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let (lx, ly) = (x - self.rect.x, y - self.rect.y);
        Some(
            BUTTONS
                .iter()
                .find(|&&(b, bx)| self.local(b, bx).contains(lx, ly))
                .map(|&(b, _)| b),
        )
    }
}

/// What the toolbar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToolbarState {
    pub tool: Tool,
    pub color: usize,
    pub width: Width,
    pub can_undo: bool,
    pub can_redo: bool,
    pub hover: Option<Button>,
    pub pressed: Option<Button>,
}

/// The toolbar without its controls: the panel, its shadow and the dividers. It changes with the
/// scale and the theme only, so the editor keeps it for [`toolbar`].
pub fn toolbar_base(t: &Toolbar, palette: &Palette) -> Option<Sprite> {
    let s = t.scale;
    let mut sprite = panel(
        t.rect.width,
        t.rect.height,
        RADIUS_MD,
        SHADOW_MD,
        palette.surface_base,
        palette,
        s,
    )?;
    let (ox, oy) = (sprite.inner.x, sprite.inner.y);
    let line = stroke(1.0, s);
    for x in DIVIDERS {
        let h = scaled(DIVIDER_H, s);
        let top = (t.rect.height - h) / 2;
        draw::fill_round_rect(
            &mut sprite.pixmap,
            Area::new(
                (ox + scaled(x, s)) as f32,
                (oy + top) as f32,
                line as f32,
                h as f32,
            ),
            0.0,
            palette.border_default,
        );
    }
    Some(sprite)
}

/// The toolbar showing `state`, drawn over its `base`.
pub fn toolbar(t: &Toolbar, base: &Sprite, state: &ToolbarState, palette: &Palette) -> Sprite {
    let s = t.scale;
    let mut sprite = Sprite {
        pixmap: base.pixmap.clone(),
        inner: base.inner,
    };
    let (ox, oy) = (sprite.inner.x, sprite.inner.y);
    let pixmap = &mut sprite.pixmap;
    let line = stroke(1.0, s);
    for &(b, x) in &BUTTONS {
        let r = t.local(b, x).offset(ox, oy);
        let a = Area::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32);
        let hover = state.hover == Some(b);
        let pressed = state.pressed == Some(b) && hover;
        let (cx, cy) = (a.x + a.w / 2.0, a.y + a.h / 2.0);
        match b {
            Button::Color(i) => {
                let rs = SWATCH_R * s;
                let selected = state.color == i;
                if selected || hover {
                    let color = if selected {
                        palette.accent_solid
                    } else {
                        palette.border_strong
                    };
                    let inner = rs + RING_GAP * s;
                    ring(pixmap, cx, cy, inner + RING * s, inner, color);
                }
                draw::fill_circle(pixmap, cx, cy, rs, SWATCHES[i].color);
                ring(pixmap, cx, cy, rs, rs - line as f32, palette.border_strong);
            }
            Button::Width(w) => {
                let fill = if state.width == w || pressed {
                    Some(palette.fill_pressed)
                } else if hover {
                    Some(palette.fill_hover)
                } else {
                    None
                };
                if let Some(fill) = fill {
                    draw::fill_round_rect(pixmap, a, RADIUS_SM * s, fill);
                }
                let (len, h) = (WIDTH_BAR * s, (w.stroke() * s).round().max(1.0));
                let mut pb = PathBuilder::new();
                pb.move_to(cx - (len - h) / 2.0, cy);
                pb.line_to(cx + (len - h) / 2.0, cy);
                if let Some(path) = pb.finish() {
                    let stroke = Stroke {
                        width: h,
                        line_cap: LineCap::Round,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(
                        &path,
                        &paint(palette.icon_primary),
                        &stroke,
                        Transform::identity(),
                        None,
                    );
                }
            }
            _ => {
                let active = b == Button::Tool(state.tool);
                let disabled = (b == Button::Undo && !state.can_undo)
                    || (b == Button::Redo && !state.can_redo);
                let fill = match (active, disabled) {
                    (_, true) => None,
                    (true, _) if pressed => Some(palette.accent_pressed),
                    (true, _) if hover => Some(palette.accent_hover),
                    (true, _) => Some(palette.accent_solid),
                    _ if pressed => Some(palette.fill_pressed),
                    _ if hover => Some(palette.fill_hover),
                    _ => None,
                };
                if let Some(fill) = fill {
                    draw::fill_round_rect(pixmap, a, RADIUS_SM * s, fill);
                }
                let color = if disabled {
                    palette.text_disabled
                } else if active {
                    palette.text_on_accent
                } else {
                    palette.icon_primary
                };
                if let Some(icon) = b.icon() {
                    let size = (ICON * s).round();
                    icons::draw(
                        pixmap,
                        icon,
                        (cx - size / 2.0).round(),
                        (cy - size / 2.0).round(),
                        size,
                        s,
                        color,
                    );
                }
            }
        }
    }
    sprite
}

/// The tooltip of a toolbar control: its sprite and where its top-left corner goes, centered under
/// the control and kept inside the window.
pub fn tooltip(
    t: &Toolbar,
    button: Button,
    os: Os,
    window: (u32, u32),
    palette: &Palette,
) -> Option<(Sprite, Point)> {
    let (name, keys) = button.tooltip(os)?;
    let s = t.scale;
    let pad = scaled(TIP_PAD, s) as f32;
    let name_w = text::width(TYPE_CAPTION, s, name).ceil();
    let keys_w = text::width(TYPE_CAPTION, s, keys).ceil();
    let w = (2.0 * pad + name_w + scaled(TIP_GAP, s) as f32 + keys_w) as i32;
    let h = scaled(TIP_H, s);
    let mut sprite = panel(
        w,
        h,
        RADIUS_SM,
        SHADOW_SM,
        palette.surface_raised,
        palette,
        s,
    )?;
    let (ox, oy) = (sprite.inner.x as f32, sprite.inner.y as f32);
    let pen = label(
        &mut sprite.pixmap,
        TYPE_CAPTION,
        s,
        name,
        ox + pad,
        (oy, h as f32),
        palette.text_primary,
    );
    label(
        &mut sprite.pixmap,
        TYPE_CAPTION,
        s,
        keys,
        pen.ceil() + scaled(TIP_GAP, s) as f32,
        (oy, h as f32),
        palette.text_secondary,
    );
    let b = t.button_rect(button);
    let edge = scaled(4.0, s);
    let x = (b.x + b.width / 2 - w / 2).clamp(edge, (window.0 as i32 - w - edge).max(edge));
    let y = t.rect.bottom() + scaled(TIP_GAP, s);
    Some((sprite, Point::new(x, y)))
}

/// Where the zoom label lies for a zoom of `percent`.
pub fn zoom_label_rect(window: (u32, u32), s: f32, percent: u32) -> Rect {
    let w = (text::width(TYPE_LABEL, s, &format!("{percent}%")).ceil() as i32)
        + 2 * scaled(LABEL_PAD, s);
    let h = scaled(LABEL_H, s);
    let inset = scaled(LABEL_INSET, s);
    Rect::new(
        window.0 as i32 - inset - w,
        window.1 as i32 - inset - h,
        w,
        h,
    )
}

pub fn zoom_label(
    rect: Rect,
    s: f32,
    percent: u32,
    hover: bool,
    palette: &Palette,
) -> Option<Sprite> {
    let mut sprite = panel(
        rect.width,
        rect.height,
        RADIUS_SM,
        SHADOW_SM,
        palette.surface_base,
        palette,
        s,
    )?;
    let (ox, oy) = (sprite.inner.x as f32, sprite.inner.y as f32);
    if hover {
        let a = Area::new(ox, oy, rect.width as f32, rect.height as f32);
        draw::fill_round_rect(&mut sprite.pixmap, a, RADIUS_SM * s, palette.fill_hover);
    }
    label(
        &mut sprite.pixmap,
        TYPE_LABEL,
        s,
        &format!("{percent}%"),
        ox + scaled(LABEL_PAD, s) as f32,
        (oy, rect.height as f32),
        palette.text_secondary,
    );
    Some(sprite)
}

const DRAG_TEXT: &str = "Перетащить";
const DRAG_PAD_LEFT: f32 = 6.0;
const DRAG_ICON: f32 = 16.0;
const DRAG_GAP: f32 = 6.0;

/// Where the drag-out button lies: bottom-left, across from the zoom label.
pub fn drag_out_rect(window: (u32, u32), s: f32) -> Rect {
    let w = scaled(DRAG_PAD_LEFT, s)
        + scaled(DRAG_ICON, s)
        + scaled(DRAG_GAP, s)
        + text::width(TYPE_LABEL, s, DRAG_TEXT).ceil() as i32
        + scaled(LABEL_PAD, s);
    let h = scaled(LABEL_H, s);
    let inset = scaled(LABEL_INSET, s);
    Rect::new(inset, window.1 as i32 - inset - h, w, h)
}

/// The drag-out button: the result as an object to drag where a file goes.
pub fn drag_out(
    rect: Rect,
    s: f32,
    hover: bool,
    pressed: bool,
    palette: &Palette,
) -> Option<Sprite> {
    let mut sprite = panel(
        rect.width,
        rect.height,
        RADIUS_SM,
        SHADOW_SM,
        palette.surface_base,
        palette,
        s,
    )?;
    let (ox, oy) = (sprite.inner.x, sprite.inner.y);
    let fill = if pressed {
        Some(palette.fill_pressed)
    } else if hover {
        Some(palette.fill_hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        let a = Area::new(ox as f32, oy as f32, rect.width as f32, rect.height as f32);
        draw::fill_round_rect(&mut sprite.pixmap, a, RADIUS_SM * s, fill);
    }
    let icon = scaled(DRAG_ICON, s);
    let x = ox + scaled(DRAG_PAD_LEFT, s);
    icons::draw(
        &mut sprite.pixmap,
        Icon::Image,
        x as f32,
        (oy + (rect.height - icon) / 2) as f32,
        icon as f32,
        s,
        palette.icon_primary,
    );
    label(
        &mut sprite.pixmap,
        TYPE_LABEL,
        s,
        DRAG_TEXT,
        (x + icon + scaled(DRAG_GAP, s)) as f32,
        (oy as f32, rect.height as f32),
        palette.text_primary,
    );
    Some(sprite)
}

/// A notice: its sprite and where its top-left corner goes, centered above the bottom edge.
pub fn notice(
    icon: Icon,
    icon_color: Color,
    message: &str,
    color: Color,
    window: (u32, u32),
    s: f32,
    palette: &Palette,
) -> Option<(Sprite, Point)> {
    let pad = scaled(NOTICE_PAD, s);
    let icon_size = scaled(NOTICE_ICON, s);
    let gap = scaled(NOTICE_GAP, s);
    let text_w = text::width(TYPE_BODY, s, message).ceil() as i32;
    let w = 2 * pad + icon_size + gap + text_w;
    let h = scaled(NOTICE_H, s);
    let mut sprite = panel(
        w,
        h,
        RADIUS_MD,
        SHADOW_MD,
        palette.surface_raised,
        palette,
        s,
    )?;
    let (ox, oy) = (sprite.inner.x, sprite.inner.y);
    icons::draw(
        &mut sprite.pixmap,
        icon,
        (ox + pad) as f32,
        (oy + (h - icon_size) / 2) as f32,
        icon_size as f32,
        s,
        icon_color,
    );
    label(
        &mut sprite.pixmap,
        TYPE_BODY,
        s,
        message,
        (ox + pad + icon_size + gap) as f32,
        (oy as f32, h as f32),
        color,
    );
    let at = Point::new(
        (window.0 as i32 - w) / 2,
        window.1 as i32 - scaled(NOTICE_BOTTOM, s) - h,
    );
    Some((sprite, at))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogButton {
    Discard,
    Cancel,
    Save,
}

impl DialogButton {
    fn text(self) -> &'static str {
        match self {
            DialogButton::Discard => "Не сохранять",
            DialogButton::Cancel => "Отмена",
            DialogButton::Save => "Сохранить",
        }
    }
}

/// The unsaved-edits dialog: where it lies, its text lines and its buttons, in window px.
#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub rect: Rect,
    lines: Vec<String>,
    pub buttons: [(DialogButton, Rect); 3],
    scale: f32,
}

const DIALOG_TITLE: &str = "Сохранить правки?";

impl Dialog {
    pub fn of(window: (u32, u32), s: f32, name: &str) -> Self {
        let pad = scaled(DIALOG_PAD, s);
        let w = scaled(DIALOG_W, s);
        let room = (w - 2 * pad) as f32;
        let lines = wrap(&format!("В «{name}» есть несохранённые правки."), room, s);
        let text_h = scaled(TYPE_TITLE.line_height, s)
            + scaled(8.0, s)
            + lines.len() as i32 * scaled(TYPE_BODY.line_height, s);
        let button_h = scaled(DIALOG_BUTTON_H, s);
        let h = pad + text_h + scaled(24.0, s) + button_h + pad;
        let rect = Rect::new((window.0 as i32 - w) / 2, (window.1 as i32 - h) / 2, w, h);
        let button_w = |b: DialogButton| {
            text::width(TYPE_BODY_STRONG, s, b.text()).ceil() as i32
                + 2 * scaled(DIALOG_BUTTON_PAD, s)
        };
        let y = rect.bottom() - pad - button_h;
        let gap = scaled(DIALOG_BUTTON_GAP, s);
        let save_w = button_w(DialogButton::Save);
        let cancel_w = button_w(DialogButton::Cancel);
        let save_x = rect.right() - pad - save_w;
        let cancel_x = save_x - gap - cancel_w;
        let buttons = [
            (
                DialogButton::Discard,
                Rect::new(rect.x + pad, y, button_w(DialogButton::Discard), button_h),
            ),
            (
                DialogButton::Cancel,
                Rect::new(cancel_x, y, cancel_w, button_h),
            ),
            (DialogButton::Save, Rect::new(save_x, y, save_w, button_h)),
        ];
        Self {
            rect,
            lines,
            buttons,
            scale: s,
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<DialogButton> {
        self.buttons
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(b, _)| *b)
    }

    pub fn render(
        &self,
        hover: Option<DialogButton>,
        pressed: Option<DialogButton>,
        palette: &Palette,
    ) -> Option<Sprite> {
        let s = self.scale;
        let mut sprite = panel(
            self.rect.width,
            self.rect.height,
            RADIUS_LG,
            SHADOW_LG,
            palette.surface_base,
            palette,
            s,
        )?;
        let (dx, dy) = (sprite.inner.x - self.rect.x, sprite.inner.y - self.rect.y);
        let pixmap = &mut sprite.pixmap;
        let pad = scaled(DIALOG_PAD, s);
        let x = (self.rect.x + pad + dx) as f32;
        let mut top = (self.rect.y + pad + dy) as f32;
        text::draw(
            pixmap,
            TYPE_TITLE,
            s,
            DIALOG_TITLE,
            x,
            top,
            &paint(palette.text_primary),
        );
        top += (scaled(TYPE_TITLE.line_height, s) + scaled(8.0, s)) as f32;
        for line in &self.lines {
            text::draw(
                pixmap,
                TYPE_BODY,
                s,
                line,
                x,
                top,
                &paint(palette.text_secondary),
            );
            top += scaled(TYPE_BODY.line_height, s) as f32;
        }
        for &(b, r) in &self.buttons {
            let r = r.offset(dx, dy);
            let a = Area::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32);
            let down = pressed == Some(b) && hover == Some(b);
            let radius = RADIUS_SM * s;
            let text_color = if b == DialogButton::Save {
                let fill = if down {
                    palette.accent_pressed
                } else if hover == Some(b) {
                    palette.accent_hover
                } else {
                    palette.accent_solid
                };
                draw::fill_round_rect(pixmap, a, radius, fill);
                palette.text_on_accent
            } else {
                if down {
                    draw::fill_round_rect(pixmap, a, radius, palette.fill_pressed);
                } else if hover == Some(b) {
                    draw::fill_round_rect(pixmap, a, radius, palette.fill_hover);
                }
                draw::border_inside(
                    pixmap,
                    a,
                    radius,
                    stroke(1.0, s) as f32,
                    palette.border_strong,
                );
                palette.text_primary
            };
            label(
                pixmap,
                TYPE_BODY_STRONG,
                s,
                b.text(),
                a.x + scaled(DIALOG_BUTTON_PAD, s) as f32,
                (a.y, a.h),
                text_color,
            );
        }
        Some(sprite)
    }
}

/// `text` broken at spaces into lines no wider than `room`; a word wider than `room` gets a line
/// of its own.
fn wrap(text: &str, room: f32, s: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if line.is_empty() || text::width(TYPE_BODY, s, &candidate) <= room {
            line = candidate;
        } else {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};

    #[test]
    fn toolbar_layout_matches_the_design() {
        let t = Toolbar::at((960, 600), 1.0);
        assert_eq!(t.rect, Rect::new(36, 12, 888, 44));
        assert_eq!(
            t.button_rect(Button::Tool(Tool::Select)),
            Rect::new(42, 18, 32, 32)
        );
        assert_eq!(
            t.button_rect(Button::Color(7)),
            Rect::new(36 + 503, 18, 28, 32)
        );
        assert_eq!(t.button_rect(Button::Close).right(), 36 + 882);
        assert_eq!(t.hit(36 + 850 + 5, 30), Some(Some(Button::Close)));
        // Between buttons: the toolbar, no control.
        assert_eq!(t.hit(36 + 39, 30), Some(None));
        assert_eq!(t.hit(10, 30), None);
        // Each group ends 12 before its divider.
        for (end, divider) in [
            (254.0, 266.0),
            (531.0, 543.0),
            (660.0, 672.0),
            (753.0, 765.0),
        ] {
            assert_eq!(end + 12.0, divider);
        }
    }

    #[test]
    fn renders_every_piece() {
        for (palette, s) in [(&LIGHT, 1.0), (&DARK, 1.5)] {
            let t = Toolbar::at((1440, 900), s);
            let state = ToolbarState {
                tool: Tool::Arrow,
                color: 0,
                width: Width::M,
                can_undo: true,
                can_redo: false,
                hover: Some(Button::Color(3)),
                pressed: None,
            };
            let base = toolbar_base(&t, palette).unwrap();
            let sprite = toolbar(&t, &base, &state, palette);
            assert!(sprite.pixmap.width() as i32 > t.rect.width);
            let (tip, at) = tooltip(&t, Button::Undo, Os::Windows, (1440, 900), palette).unwrap();
            assert!(at.y > t.rect.bottom());
            assert!(tip.pixmap.width() > 0);
            let r = zoom_label_rect((1440, 900), s, 100);
            assert!(zoom_label(r, s, 100, true, palette).is_some());
            let r = drag_out_rect((1440, 900), s);
            assert!(drag_out(r, s, true, false, palette).is_some());
            assert!(
                notice(
                    Icon::Check,
                    palette.success,
                    "Скопировано",
                    palette.text_primary,
                    (1440, 900),
                    s,
                    palette
                )
                .is_some()
            );
            let d = Dialog::of((1440, 900), s, "Shot 2026-10-22 17-03-15.png");
            assert!(d.render(Some(DialogButton::Save), None, palette).is_some());
        }
    }

    #[test]
    fn dialog_wraps_and_places_buttons() {
        let d = Dialog::of((960, 600), 1.0, "Shot 2026-10-22 17-03-15.png");
        assert_eq!(d.rect.width, 400);
        assert!(d.lines.len() >= 2, "{:?}", d.lines);
        let [(_, discard), (_, cancel), (_, save)] = d.buttons;
        assert_eq!(discard.x, d.rect.x + 24);
        assert_eq!(save.right(), d.rect.right() - 24);
        assert_eq!(cancel.right() + 8, save.x);
        assert_eq!(d.hit(save.x + 2, save.y + 2), Some(DialogButton::Save));
    }

    #[test]
    fn drag_out_sits_across_from_the_zoom_label() {
        for s in [1.0, 1.25, 2.0] {
            let window = (1200, 800);
            let drag = drag_out_rect(window, s);
            let zoom = zoom_label_rect(window, s, 100);
            assert_eq!(drag.x, window.0 as i32 - zoom.right());
            assert_eq!((drag.y, drag.height), (zoom.y, zoom.height));
        }
    }
}
