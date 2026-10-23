//! Preview cards (SPEC §3.2). Each card is a borderless panel that never takes the keyboard or
//! brings Shot forward, with its shadow in a second panel under it that the mouse passes through,
//! as on Windows. Both show a bitmap `shot-core` drew, as their layer's `IOSurface`; they float
//! over other apps' windows on every Space and are left out of Shot's captures.
//!
//! The mouse reaches the app through [`intercept`], which the event pump asks before AppKit sees
//! an event: AppKit never handles a click on a card, so Shot stays in the background. The timer is
//! a deadline of the pump, set only while cards are on screen.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSCursor, NSEvent, NSEventType, NSPanel, NSStatusWindowLevel,
    NSTrackingArea, NSTrackingAreaOptions, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowSharingType, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CATransaction;
use shot_core::geom::{Point, Rect};

use super::monitors::{Screen, screens};
use super::overlay::contents_changed;
use super::pump::{Event, Intercepted, post, push, register, set_timer};
use super::{Bitmap, Error, capture};

/// What happened to the cards. `slot` is the window pair from [`PreviewWindows::acquire`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewEvent {
    /// The cursor came onto the card.
    Enter(usize),
    Leave(usize),
    /// A click at `at`, in the card's own pixels.
    Click {
        slot: usize,
        at: Point,
    },
    /// The time asked for with [`PreviewWindows::set_timer`] has come.
    Timer,
    /// A [`Notifier`] posted.
    Notify,
    /// The windows handled an event of their own; nothing changed for the app.
    Internal,
}

/// Premultiplied BGRA pixels, rows top to bottom without padding.
#[derive(Clone, Copy, Debug)]
pub struct Sprite<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Wakes the app from another thread: a save finished.
#[derive(Clone, Debug)]
pub struct Notifier(isize);

impl Notifier {
    pub fn notify(&self) {
        post(self.0);
    }
}

thread_local! {
    /// The window number of each slot's card and the scale of its pixels, for [`intercept`].
    static CARDS: RefCell<Vec<(isize, f64)>> = const { RefCell::new(Vec::new()) };
}

pub struct PreviewWindows {
    pairs: Rc<RefCell<Pairs>>,
    notify: isize,
    timer: isize,
    /// Makes the spare pairs once the loop is idle.
    spare: isize,
}

struct Pairs {
    list: Vec<Pair>,
    /// Free pairs kept ready.
    spares: usize,
}

struct Pair {
    card: Retained<NSPanel>,
    shadow: Retained<NSPanel>,
    used: bool,
    shown: bool,
    /// Where the pair is, while it is used: its pixels are at this screen's scale.
    screen: Option<Screen>,
    /// What the panels show, kept until the pair is freed.
    card_pixels: Option<Bitmap>,
    shadow_pixels: Option<Bitmap>,
}

impl PreviewWindows {
    /// `spares` pairs are made ahead, off the capture path, once the loop is idle: as many as one
    /// capture shows cards at once.
    pub fn new(spares: usize) -> Result<Self, Error> {
        let pairs = Rc::new(RefCell::new(Pairs {
            list: Vec::new(),
            spares: spares.max(1),
        }));
        let spare = {
            let pairs = pairs.clone();
            register(Rc::new(move || {
                // A failure here is tried again on the capture path.
                let _ = pairs.borrow_mut().make_spares();
            }))
        };
        post(spare);
        Ok(PreviewWindows {
            pairs,
            notify: register(Rc::new(|| push(Event::Preview(PreviewEvent::Notify)))),
            timer: register(Rc::new(|| push(Event::Preview(PreviewEvent::Timer)))),
            spare,
        })
    }

    pub fn notifier(&self) -> Notifier {
        Notifier(self.notify)
    }

    /// A free window pair: a spare, or a new one when none is left.
    pub fn acquire(&mut self) -> Result<usize, Error> {
        let mut pairs = self.pairs.borrow_mut();
        let slot = match pairs.list.iter().position(|p| !p.used) {
            Some(i) => i,
            None => pairs.add()?,
        };
        pairs.list[slot].used = true;
        post(self.spare);
        Ok(slot)
    }

    /// Hides the pair, lets its pixels go and makes it free.
    pub fn release(&mut self, slot: usize) {
        let mut pairs = self.pairs.borrow_mut();
        let Some(p) = pairs.list.get_mut(slot) else {
            return;
        };
        p.card.orderOut(None);
        p.shadow.orderOut(None);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        for panel in [&p.card, &p.shadow] {
            if let Some(layer) = panel.contentView().and_then(|v| v.layer()) {
                unsafe { layer.setContents(None) };
            }
        }
        CATransaction::commit();
        p.card_pixels = None;
        p.shadow_pixels = None;
        p.used = false;
        p.shown = false;
        p.screen = None;
    }

    /// Puts the card at `at` and its shadow at `shadow_at`, both at opacity `alpha`. A sprite
    /// that is `None` keeps the pixels the panel has. Shows the pair if it is hidden.
    pub fn update(
        &mut self,
        slot: usize,
        card: Option<Sprite>,
        at: Point,
        shadow: Option<Sprite>,
        shadow_at: Point,
        alpha: u8,
    ) -> Result<(), Error> {
        let mut pairs = self.pairs.borrow_mut();
        let Some(p) = pairs.list.get_mut(slot) else {
            return Ok(());
        };
        let screen = match p.screen {
            Some(s) if s.monitor.rect.contains(at.x, at.y) => s,
            _ => {
                let s = screen_of(at).ok_or_else(|| Error::new("no screen for the card"))?;
                p.screen = Some(s);
                s
            }
        };
        let scale = f64::from(screen.monitor.scale);
        CARDS.with_borrow_mut(|c| c[slot].1 = scale);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let shown = place(&p.shadow, &mut p.shadow_pixels, shadow, shadow_at, screen).and_then(
            |old_shadow| {
                let old_card = place(&p.card, &mut p.card_pixels, card, at, screen)?;
                Ok((old_shadow, old_card))
            },
        );
        CATransaction::commit();
        // The bitmaps replaced go once the new ones are set: a bitmap empties itself as it goes.
        drop(shown?);
        let alpha = f64::from(alpha) / 255.0;
        p.shadow.setAlphaValue(alpha);
        p.card.setAlphaValue(alpha);
        if !p.shown {
            p.shown = true;
            // The levels keep every card over every shadow.
            p.shadow.orderFrontRegardless();
            p.card.orderFrontRegardless();
        }
        CATransaction::flush();
        Ok(())
    }

    /// Asks for [`PreviewEvent::Timer`] after `after`; `None` cancels.
    pub fn set_timer(&self, after: Option<Duration>) {
        set_timer(self.timer, after.map(|d| Instant::now() + d));
    }
}

impl Pairs {
    fn make_spares(&mut self) -> Result<(), Error> {
        let mut made = Vec::new();
        while self.list.iter().filter(|p| !p.used).count() < self.spares {
            made.push(self.add_one()?);
        }
        if !made.is_empty() {
            self.exclude(&made);
        }
        Ok(())
    }

    fn add(&mut self) -> Result<usize, Error> {
        let slot = self.add_one()?;
        self.exclude(&[slot]);
        Ok(slot)
    }

    fn add_one(&mut self) -> Result<usize, Error> {
        let mtm = MainThreadMarker::new().ok_or_else(|| Error::new("not the main thread"))?;
        let card = panel(mtm, true);
        let shadow = panel(mtm, false);
        CARDS.with_borrow_mut(|c| c.push((card.windowNumber(), 1.0)));
        self.list.push(Pair {
            card,
            shadow,
            used: false,
            shown: false,
            screen: None,
            card_pixels: None,
            shadow_pixels: None,
        });
        Ok(self.list.len() - 1)
    }

    /// Leaves the pairs in `slots` out of captures.
    fn exclude(&self, slots: &[usize]) {
        let numbers: Vec<u32> = (slots.iter().map(|&i| &self.list[i]))
            .flat_map(|p| [p.card.windowNumber() as u32, p.shadow.windowNumber() as u32])
            .collect();
        capture::exclude(&numbers);
    }
}

/// A hidden panel for a card, or for a shadow, which the mouse passes through.
fn panel(mtm: MainThreadMarker, card: bool) -> Retained<NSPanel> {
    let rect = NSRect::new(NSPoint::ZERO, NSSize::new(1.0, 1.0));
    let panel: Retained<NSPanel> = unsafe {
        msg_send![
            NSPanel::alloc(mtm),
            initWithContentRect: rect,
            styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    unsafe { panel.setReleasedWhenClosed(false) };
    // Over other apps' windows and the Dock, under the menus; every card over every shadow.
    panel.setLevel(NSStatusWindowLevel + if card { 2 } else { 1 });
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setHidesOnDeactivate(false);
    // The fades are Shot's own.
    panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
    panel.setOpaque(false);
    panel.setHasShadow(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setSharingType(NSWindowSharingType::None);
    if !card {
        panel.setIgnoresMouseEvents(true);
    }
    if let Some(view) = panel.contentView() {
        view.setWantsLayer(true);
        if card {
            // The cursor comes and goes while another app is active.
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    NSRect::ZERO,
                    NSTrackingAreaOptions::MouseEnteredAndExited
                        | NSTrackingAreaOptions::ActiveAlways
                        | NSTrackingAreaOptions::InVisibleRect,
                    Some(&view),
                    None,
                )
            };
            view.addTrackingArea(&area);
        }
    }
    panel
}

/// Moves `panel` to `at`, a pixel of `screen`, and gives it `sprite` if there is one: drawn into
/// the bitmap it shows when the size is the same, else into a new one. Returns the bitmap the new
/// one replaced.
fn place(
    panel: &NSPanel,
    pixels: &mut Option<Bitmap>,
    sprite: Option<Sprite>,
    at: Point,
    screen: Screen,
) -> Result<Option<Bitmap>, Error> {
    let Some(sprite) = sprite else {
        panel.setFrameTopLeftPoint(screen.to_points(f64::from(at.x), f64::from(at.y)));
        return Ok(None);
    };
    let (w, h) = (sprite.width as i32, sprite.height as i32);
    panel.setFrame_display(screen.rect_to_points(Rect::new(at.x, at.y, w, h)), false);
    let layer = (panel.contentView().and_then(|v| v.layer()))
        .ok_or_else(|| Error::new("a preview panel has no layer"))?;
    if let Some(b) = pixels
        && b.rect.width == w
        && b.rect.height == h
    {
        b.lock();
        copy(sprite, b);
        b.unlock();
        contents_changed(&layer, b);
        return Ok(None);
    }
    let mut b = Bitmap::new(Rect::new(0, 0, w, h))?;
    copy(sprite, &mut b);
    b.unlock();
    layer.setContentsScale(f64::from(screen.monitor.scale));
    let surface: &AnyObject = b.surface();
    unsafe { layer.setContents(Some(surface)) };
    Ok(pixels.replace(b))
}

fn copy(sprite: Sprite, to: &mut Bitmap) {
    let row = sprite.width as usize * 4;
    let view = to.view_mut();
    for (y, src) in sprite.data.chunks_exact(row).enumerate() {
        view.data[y * view.stride..][..row].copy_from_slice(src);
    }
}

/// The screen with pixel `at`, else the nearest.
fn screen_of(at: Point) -> Option<Screen> {
    screens()
        .into_iter()
        .min_by_key(|s| s.monitor.rect.distance_sq(at))
}

/// Turns mouse events to the cards into card events, and keeps the rest of the input to them
/// from AppKit.
pub(crate) fn intercept(event: &NSEvent) -> Intercepted {
    let number = event.windowNumber();
    let found = CARDS.with_borrow(|c| {
        c.iter()
            .position(|card| card.0 == number)
            .map(|i| (i, c[i].1))
    });
    let Some((slot, scale)) = found.filter(|_| number > 0) else {
        return Intercepted::Pass;
    };
    let e = match event.r#type() {
        NSEventType::MouseEntered => {
            NSCursor::pointingHandCursor().set();
            PreviewEvent::Enter(slot)
        }
        NSEventType::MouseExited => {
            NSCursor::arrowCursor().set();
            PreviewEvent::Leave(slot)
        }
        NSEventType::LeftMouseUp => {
            let Some(window) = MainThreadMarker::new().and_then(|mtm| event.window(mtm)) else {
                return Intercepted::Swallow;
            };
            let p = event.locationInWindow();
            let height = window.frame().size.height;
            PreviewEvent::Click {
                slot,
                at: Point::new(
                    (p.x * scale).floor() as i32,
                    ((height - p.y) * scale).floor() as i32,
                ),
            }
        }
        NSEventType::LeftMouseDown
        | NSEventType::LeftMouseDragged
        | NSEventType::RightMouseDown
        | NSEventType::RightMouseUp
        | NSEventType::RightMouseDragged
        | NSEventType::OtherMouseDown
        | NSEventType::OtherMouseUp
        | NSEventType::OtherMouseDragged
        | NSEventType::MouseMoved
        | NSEventType::ScrollWheel
        | NSEventType::CursorUpdate => return Intercepted::Swallow,
        _ => return Intercepted::Pass,
    };
    Intercepted::Event(Event::Preview(e))
}

/// The window number of the card in `slot`, for the dev commands that stand for the mouse.
pub(crate) fn card_window(slot: usize) -> Option<isize> {
    CARDS.with_borrow(|c| c.get(slot).map(|card| card.0))
}
