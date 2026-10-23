//! The preview card (`docs/design/overlay-preview.md`, "Preview"): its geometry, how it looks in
//! each state, and the stack of cards with their timers. The platform shows each card in a window
//! that never takes the focus, and passes in the clock and the cursor.

use std::io;
use std::time::{Duration, Instant};

use tiny_skia::{Paint, Pixmap};

use crate::canvas::skia_color;
use crate::draw::{self, Area};
use crate::geom::{Point, Rect};
use crate::icons::{self, Icon};
use crate::image::BgrxView;
use crate::text;
use crate::theme::{
    Color, FADE_MS, OVERLAY_LABEL_BG, Palette, RADIUS_LG, RADIUS_MD, SHADOW_LG, TYPE_CAPTION,
    scaled, stroke,
};

const CARD_W: f32 = 248.0;
const PAD: f32 = 4.0;
const BOX_W: f32 = 240.0;
const BOX_MIN_H: f32 = 64.0;
const STATUS_H: f32 = 28.0;
const STATUS_LEFT: f32 = 8.0;
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 6.0;
const CLOSE: f32 = 24.0;
const CLOSE_INSET: f32 = 8.0;
const CLOSE_HIT: f32 = 32.0;
/// From the work area's right and bottom edges, and the closest a card may come to its top.
const MARGIN: f32 = 16.0;
const GAP: f32 = 8.0;
const WHITE: Color = Color::hex(0xFFFF_FFFF);

/// Where the parts of one card lie, in physical px from its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub width: i32,
    pub height: i32,
    pub scale: f32,
    /// The thumbnail box.
    pub frame: Rect,
    /// The image inside the box: contained, centered, never above 1 : 1.
    pub image: Rect,
}

impl Geometry {
    /// The card of a `w` × `h` capture on a monitor with `scale`. The box height follows the
    /// image at the box width, but no larger than 1 : 1, kept within 64–240.
    pub fn of(w: u32, h: u32, scale: f32) -> Self {
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let pad = scaled(PAD, scale);
        let box_w = scaled(BOX_W, scale);
        let shown_h = h * (box_w as f32 / w).min(1.0);
        let box_h = (shown_h.round() as i32).clamp(scaled(BOX_MIN_H, scale), scaled(BOX_W, scale));
        let k = (box_w as f32 / w).min(box_h as f32 / h).min(1.0);
        let (iw, ih) = (
            ((w * k).round() as i32).max(1),
            ((h * k).round() as i32).max(1),
        );
        Self {
            width: scaled(CARD_W, scale),
            height: pad + box_h + scaled(STATUS_H, scale),
            scale,
            frame: Rect::new(pad, pad, box_w, box_h),
            image: Rect::new(pad + (box_w - iw) / 2, pad + (box_h - ih) / 2, iw, ih),
        }
    }

    fn close_center(&self) -> (f32, f32) {
        let s = self.scale;
        let r = CLOSE / 2.0 * s;
        (self.width as f32 - CLOSE_INSET * s - r, CLOSE_INSET * s + r)
    }

    /// The point, in the card's own px, falls on the close button's hit area.
    pub fn on_close(&self, x: i32, y: i32) -> bool {
        let (cx, cy) = self.close_center();
        let half = CLOSE_HIT / 2.0 * self.scale;
        (x as f32 + 0.5 - cx).abs() <= half && (y as f32 + 0.5 - cy).abs() <= half
    }
}

/// What the card says under the thumbnail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    CopiedAndSaved,
    Copied,
    /// Saved into `folder`, as the user knows it (`Pictures\Shot`).
    Saved {
        folder: String,
    },
    Failed {
        reason: &'static str,
    },
}

impl Status {
    /// Only a failure stays until the user closes it.
    pub fn stays(&self) -> bool {
        matches!(self, Status::Failed { .. })
    }
}

/// The reason a save failed, as the card says it after `Не сохранено: `.
pub fn failure_reason(e: &io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::PermissionDenied => "нет доступа к папке",
        io::ErrorKind::NotFound => "папка не найдена",
        io::ErrorKind::StorageFull => "диск заполнен",
        io::ErrorKind::ReadOnlyFilesystem => "диск только для чтения",
        _ => "ошибка записи",
    }
}

/// One card's content.
pub struct Card {
    pub geometry: Geometry,
    /// The capture scaled to `geometry.image`.
    pub thumb: Pixmap,
    pub status: Status,
    /// What a click does, shown on hover instead of the status; `None` when a click only closes
    /// the card.
    pub action: Option<&'static str>,
}

impl Card {
    pub fn new(
        image: &BgrxView,
        scale: f32,
        status: Status,
        action: Option<&'static str>,
    ) -> Option<Self> {
        let geometry = Geometry::of(image.width, image.height, scale);
        let thumb = draw::thumbnail(
            image,
            geometry.image.width as u32,
            geometry.image.height as u32,
        )?;
        Some(Self {
            geometry,
            thumb,
            status,
            action,
        })
    }

    /// The card as its window shows it: premultiplied BGRA with transparent rounded corners.
    pub fn render(&self, palette: &Palette, hover: bool) -> Option<Pixmap> {
        let g = &self.geometry;
        let s = g.scale;
        let mut pixmap = Pixmap::new(g.width as u32, g.height as u32)?;
        let card = Area::new(0.0, 0.0, g.width as f32, g.height as f32);
        let radius = RADIUS_LG * s;
        draw::fill_round_rect(&mut pixmap, card, radius, palette.surface_base);

        let frame = area(g.frame);
        let inner_radius = RADIUS_MD * s;
        draw::fill_round_rect(&mut pixmap, frame, inner_radius, palette.surface_sunken);
        draw::draw_clipped(
            &mut pixmap,
            &self.thumb,
            g.image.x,
            g.image.y,
            frame,
            inner_radius,
        );
        let line = stroke(1.0, s) as f32;
        draw::border_inside(
            &mut pixmap,
            frame,
            inner_radius,
            line,
            palette.border_default,
        );

        let (icon, label, color, icon_color) =
            match (&self.status, hover.then_some(self.action).flatten()) {
                (Status::Failed { reason }, _) => (
                    Icon::Alert,
                    format!("Не сохранено: {reason}"),
                    palette.danger,
                    palette.danger,
                ),
                (_, Some(action)) => (
                    Icon::Edit,
                    action.to_string(),
                    palette.accent_text,
                    palette.accent_text,
                ),
                (Status::CopiedAndSaved, None) => (
                    Icon::Check,
                    "Скопировано и сохранено".to_string(),
                    palette.text_secondary,
                    palette.success,
                ),
                (Status::Copied, None) => (
                    Icon::Copy,
                    "Скопировано в буфер".to_string(),
                    palette.text_secondary,
                    palette.icon_secondary,
                ),
                (Status::Saved { folder }, None) => (
                    Icon::Save,
                    format!("Сохранено в {folder}"),
                    palette.text_secondary,
                    palette.icon_secondary,
                ),
            };
        let row_top = (g.frame.bottom()) as f32;
        let row_h = g.height as f32 - row_top;
        let icon_size = (ICON * s).round();
        let left = (STATUS_LEFT * s).round();
        icons::draw(
            &mut pixmap,
            icon,
            left,
            (row_top + (row_h - icon_size) / 2.0).round(),
            icon_size,
            s,
            icon_color,
        );
        let text_x = left + icon_size + (ICON_GAP * s).round();
        let room = g.width as f32 - text_x - left;
        let label = match &self.status {
            Status::Saved { folder } if !hover || self.action.is_none() => {
                fit_left("Сохранено в ", folder, room, s)
            }
            _ => fit_right(&label, room, s),
        };
        let mut paint = Paint::default();
        paint.set_color(skia_color(color));
        paint.anti_alias = true;
        let text_top = row_top + (row_h - TYPE_CAPTION.line_height * s) / 2.0;
        text::draw(
            &mut pixmap,
            TYPE_CAPTION,
            s,
            &label,
            text_x,
            text_top,
            &paint,
        );

        let border = if hover {
            palette.border_strong
        } else {
            palette.border_default
        };
        draw::border_inside(&mut pixmap, card, radius, line, border);

        if hover || self.status.stays() {
            let (cx, cy) = g.close_center();
            draw::fill_circle(&mut pixmap, cx, cy, CLOSE / 2.0 * s, OVERLAY_LABEL_BG);
            let size = (ICON * s).round();
            icons::draw(
                &mut pixmap,
                Icon::Close,
                (cx - size / 2.0).round(),
                (cy - size / 2.0).round(),
                size,
                s,
                WHITE,
            );
        }
        Some(pixmap)
    }

    /// The card's shadow in its own sprite, and the card's top-left corner in it.
    pub fn render_shadow(&self, palette: &Palette) -> Option<(Pixmap, Point)> {
        let g = &self.geometry;
        let (pixmap, x, y) = draw::shadow(
            g.width as f32,
            g.height as f32,
            RADIUS_LG * g.scale,
            SHADOW_LG,
            palette.dark,
            g.scale,
        )?;
        Some((pixmap, Point::new(x, y)))
    }
}

fn area(r: Rect) -> Area {
    Area::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32)
}

fn fits(text: &str, room: f32, scale: f32) -> bool {
    text::width(TYPE_CAPTION, scale, text) <= room
}

/// `prefix` and `tail`, the tail cut from the left with `…` until it fits in `room`.
fn fit_left(prefix: &str, tail: &str, room: f32, scale: f32) -> String {
    let whole = format!("{prefix}{tail}");
    if fits(&whole, room, scale) {
        return whole;
    }
    let chars: Vec<char> = tail.chars().collect();
    (1..chars.len())
        .map(|cut| format!("{prefix}…{}", chars[cut..].iter().collect::<String>()))
        .find(|t| fits(t, room, scale))
        .unwrap_or_else(|| format!("{prefix}…"))
}

/// `text` cut from the right with `…` until it fits in `room`.
fn fit_right(text: &str, room: f32, scale: f32) -> String {
    if fits(text, room, scale) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    (1..chars.len())
        .rev()
        .map(|n| format!("{}…", chars[..n].iter().collect::<String>().trim_end()))
        .find(|t| fits(t, room, scale))
        .unwrap_or_else(|| "…".to_string())
}

/// The cards on screen, oldest first, and their timers.
#[derive(Default)]
pub struct Stack {
    cards: Vec<Entry>,
    /// The cursor is over one of the cards: timers and the layout wait.
    hover: bool,
    next_id: u64,
}

struct Entry {
    id: u64,
    width: i32,
    height: i32,
    work: Rect,
    scale: f32,
    /// The card's top-left corner on screen.
    at: Point,
    shown: Instant,
    timer: Timer,
    /// When the card began to fade out.
    leaving: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Timer {
    Runs { until: Instant },
    Paused { left: Duration },
    Never,
}

/// A card as it is now on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub id: u64,
    pub at: Point,
    /// Opacity of the whole card, for the fades.
    pub alpha: u8,
}

const FADE: Duration = Duration::from_millis(FADE_MS);
/// How often a fade redraws.
pub const FRAME: Duration = Duration::from_millis(16);

impl Stack {
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// Adds a `width` × `height` card in the bottom-right corner of `work` and moves the others up.
    /// It hides after `timeout`, or stays when `None`. When the column reaches the top of the
    /// work area, the oldest cards there are dropped.
    pub fn add(
        &mut self,
        now: Instant,
        (width, height): (i32, i32),
        work: Rect,
        scale: f32,
        timeout: Option<Duration>,
    ) -> u64 {
        self.next_id += 1;
        let timer = match timeout {
            None => Timer::Never,
            Some(t) if self.hover => Timer::Paused { left: t },
            Some(t) => Timer::Runs { until: now + t },
        };
        self.cards.push(Entry {
            id: self.next_id,
            width,
            height,
            work,
            scale,
            at: Point::new(0, 0),
            shown: now,
            timer,
            leaving: None,
        });
        self.layout();
        self.next_id
    }

    /// The card stays until it is closed: its save failed.
    pub fn keep(&mut self, id: u64) {
        if let Some(e) = self.cards.iter_mut().find(|e| e.id == id) {
            e.timer = Timer::Never;
        }
    }

    /// Fades the card out.
    pub fn close(&mut self, now: Instant, id: u64) {
        if let Some(e) = self.cards.iter_mut().find(|e| e.id == id) {
            e.leaving.get_or_insert(now);
        }
    }

    /// The cursor came onto a card or left the stack: timers pause or resume with the time they
    /// had left, and the stack closes the gaps once the cursor is gone.
    pub fn hover(&mut self, now: Instant, hover: bool) {
        if hover == self.hover {
            return;
        }
        self.hover = hover;
        for e in &mut self.cards {
            e.timer = match e.timer {
                Timer::Runs { until } if hover => Timer::Paused {
                    left: until.saturating_duration_since(now),
                },
                Timer::Paused { left } if !hover => Timer::Runs { until: now + left },
                t => t,
            };
        }
        if !hover {
            self.layout();
        }
    }

    /// Starts the fades that are due and drops the cards that have faded out.
    pub fn update(&mut self, now: Instant) {
        for e in &mut self.cards {
            if let Timer::Runs { until } = e.timer
                && until <= now
            {
                e.leaving.get_or_insert(until);
            }
        }
        let before = self.cards.len();
        self.cards.retain(|e| {
            e.leaving
                .is_none_or(|t| now.saturating_duration_since(t) < FADE)
        });
        if self.cards.len() != before && !self.hover {
            self.layout();
        }
    }

    /// When [`Self::update`] has something to do next: the next fade frame or hide time.
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        self.cards
            .iter()
            .filter_map(|e| {
                if e.leaving.is_some() || now.saturating_duration_since(e.shown) + FRAME < FADE {
                    return Some(now + FRAME);
                }
                match e.timer {
                    Timer::Runs { until } => Some(until),
                    _ => None,
                }
            })
            .min()
    }

    pub fn placed(&self, now: Instant) -> impl Iterator<Item = Placed> + '_ {
        self.cards.iter().map(move |e| {
            // The first frame is already one step in: a card shown at opacity 0 would wait a
            // timer tick to become visible.
            let fade_in =
                (now.saturating_duration_since(e.shown) + FRAME).as_secs_f32() / FADE.as_secs_f32();
            let fade_out = e.leaving.map_or(1.0, |t| {
                1.0 - now.saturating_duration_since(t).as_secs_f32() / FADE.as_secs_f32()
            });
            Placed {
                id: e.id,
                at: e.at,
                alpha: (fade_in.min(fade_out).clamp(0.0, 1.0) * 255.0).round() as u8,
            }
        })
    }

    /// Stacks each work area's cards upward from its bottom-right corner, newest at the bottom,
    /// and drops the oldest ones that do not fit.
    fn layout(&mut self) {
        let mut keep = vec![true; self.cards.len()];
        let mut bottoms: Vec<(Rect, i32)> = Vec::new();
        for i in (0..self.cards.len()).rev() {
            let e = &mut self.cards[i];
            let s = e.scale;
            let slot = match bottoms.iter_mut().find(|(w, _)| *w == e.work) {
                Some(slot) => slot,
                None => {
                    bottoms.push((e.work, e.work.bottom() - scaled(MARGIN, s)));
                    bottoms.last_mut().expect("just pushed")
                }
            };
            let first = slot.1 == e.work.bottom() - scaled(MARGIN, s);
            let top = slot.1 - e.height;
            if !first && top < e.work.y + scaled(MARGIN, s) {
                keep[i] = false;
                continue;
            }
            e.at = Point::new(e.work.right() - scaled(MARGIN, s) - e.width, top);
            slot.1 = top - scaled(GAP, s);
        }
        let mut keep = keep.into_iter();
        self.cards.retain(|_| keep.next().unwrap_or(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(w: u32, h: u32) -> ((i32, i32), (i32, i32)) {
        let g = Geometry::of(w, h, 1.0);
        (
            (g.frame.width, g.frame.height),
            (g.image.width, g.image.height),
        )
    }

    #[test]
    fn thumbnail_rule_matches_the_design_table() {
        assert_eq!(sizes(1920, 1080), ((240, 135), (240, 135)));
        assert_eq!(sizes(1080, 1920), ((240, 240), (135, 240)));
        assert_eq!(sizes(3440, 600), ((240, 64), (240, 42)));
        assert_eq!(sizes(90, 40), ((240, 64), (90, 40)));
        let g = Geometry::of(1920, 1080, 1.0);
        assert_eq!((g.width, g.height), (248, 167));
        // Centered in its box.
        assert_eq!(Geometry::of(90, 40, 1.0).image, Rect::new(79, 16, 90, 40));
    }

    #[test]
    fn scales_with_the_monitor() {
        let g = Geometry::of(1920, 1080, 1.25);
        assert_eq!((g.width, g.frame.width), (310, 300));
        assert!(g.on_close(310 - 10 - 15, 10 + 15));
        assert!(!g.on_close(150, 100));
    }

    #[test]
    fn renders_every_state() {
        let data = vec![128u8; 64 * 32 * 4];
        let view = BgrxView {
            width: 64,
            height: 32,
            stride: 256,
            data: &data,
        };
        for status in [
            Status::CopiedAndSaved,
            Status::Copied,
            Status::Saved {
                folder: r"C:\a very long folder name\that does not fit\into the card\Shot".into(),
            },
            Status::Failed {
                reason: "нет доступа к папке",
            },
        ] {
            let card = Card::new(&view, 1.5, status, Some("Открыть в редакторе")).unwrap();
            for hover in [false, true] {
                let p = card.render(&crate::theme::LIGHT, hover).unwrap();
                // Rounded corners are clear, the body is opaque.
                assert_eq!(p.pixel(0, 0).unwrap().alpha(), 0);
                assert_eq!(p.pixel(p.width() / 2, p.height() - 3).unwrap().alpha(), 255);
            }
        }
    }

    #[test]
    fn long_texts_are_cut() {
        let room = 150.0;
        let t = fit_left("Сохранено в ", r"C:\Users\someone\Pictures\Shot", room, 1.0);
        assert!(
            t.starts_with("Сохранено в …") && t.ends_with(r"\Shot"),
            "{t}"
        );
        assert!(fits(&t, room, 1.0));
        let t = fit_right("Не сохранено: диск только для чтения", room, 1.0);
        assert!(t.ends_with('…') && fits(&t, room, 1.0), "{t}");
    }

    const WORK: Rect = Rect::new(0, 0, 1000, 600);

    fn ats(stack: &Stack, now: Instant) -> Vec<(u64, Point)> {
        stack.placed(now).map(|p| (p.id, p.at)).collect()
    }

    #[test]
    fn stacks_upward_and_drops_the_oldest() {
        let now = Instant::now();
        let mut s = Stack::default();
        let five = Some(Duration::from_secs(5));
        let a = s.add(now, (248, 167), WORK, 1.0, five);
        assert_eq!(ats(&s, now), [(a, Point::new(736, 417))]);
        let b = s.add(now, (248, 167), WORK, 1.0, five);
        assert_eq!(
            ats(&s, now),
            [(a, Point::new(736, 242)), (b, Point::new(736, 417))]
        );
        let c = s.add(now, (248, 167), WORK, 1.0, five);
        let d = s.add(now, (248, 167), WORK, 1.0, five);
        // A fourth card would cross 16 px from the top: the oldest goes.
        assert_eq!(
            ats(&s, now),
            [
                (b, Point::new(736, 67)),
                (c, Point::new(736, 242)),
                (d, Point::new(736, 417))
            ]
        );
        assert!(a < b);
    }

    #[test]
    fn hover_pauses_timers_and_the_layout() {
        let t0 = Instant::now();
        let mut s = Stack::default();
        let a = s.add(t0, (248, 167), WORK, 1.0, Some(Duration::from_secs(5)));
        let b = s.add(t0, (248, 100), WORK, 1.0, Some(Duration::from_secs(1)));
        assert_eq!(s.next_deadline(t0), Some(t0 + FRAME));
        assert_eq!(
            s.placed(t0 + FADE).map(|p| p.alpha).collect::<Vec<_>>(),
            [255, 255]
        );

        s.hover(t0 + Duration::from_millis(500), true);
        s.close(t0 + Duration::from_millis(600), b);
        s.update(t0 + Duration::from_secs(10));
        // b is gone; a waits where it was, its timer paused.
        assert_eq!(ats(&s, t0), [(a, Point::new(736, 309))]);
        assert_eq!(s.next_deadline(t0 + Duration::from_secs(10)), None);

        let back = t0 + Duration::from_secs(20);
        s.hover(back, false);
        assert_eq!(ats(&s, back), [(a, Point::new(736, 417))]);
        assert_eq!(
            s.next_deadline(back),
            Some(back + Duration::from_millis(4500))
        );
        s.update(back + Duration::from_millis(4500) + FADE);
        assert!(s.is_empty());
    }

    #[test]
    fn a_failed_card_stays() {
        let t0 = Instant::now();
        let mut s = Stack::default();
        let a = s.add(t0, (248, 167), WORK, 1.0, Some(Duration::from_secs(5)));
        s.keep(a);
        s.update(t0 + Duration::from_secs(60));
        assert_eq!(s.placed(t0).count(), 1);
        assert_eq!(s.next_deadline(t0 + Duration::from_secs(60)), None);
    }

    #[test]
    fn failure_reasons() {
        let e = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(failure_reason(&e), "нет доступа к папке");
    }
}
