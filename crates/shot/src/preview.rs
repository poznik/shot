//! The preview cards: `shot-core` lays them out and draws them, `shot-platform` shows
//! them; this ties both to the clock, the mouse and the results of the background saves. A click
//! on a card asks for its capture in the editor.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use shot_core::draw::Pixmap;
use shot_core::geom::{Point, Rect};
use shot_core::preview::{Card, Placed, Stack, Status, failure_reason};
use shot_core::theme::Palette;
use shot_platform::sys::{self, Notifier, PreviewEvent, PreviewWindows, Sprite};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::MSG;

use crate::editor::Source;
use crate::log::log;

pub struct Previews {
    stack: Stack,
    windows: PreviewWindows,
    shown: Vec<Shown>,
    /// Cards not on screen because previews are off: shown only if their save fails.
    waiting: Vec<(u64, Card, Rect)>,
    next_key: u64,
    dark: bool,
    saved_tx: Sender<(u64, io::Result<Source>)>,
    saved_rx: Receiver<(u64, io::Result<Source>)>,
    /// Cards clicked before their capture was ready, and when.
    clicked: Vec<(u64, Instant)>,
    /// Captures to open in the editor, and when they were clicked.
    open: Vec<(Source, Instant)>,
}

struct Shown {
    key: u64,
    /// The card's id in the stack.
    id: u64,
    card: Card,
    slot: usize,
    shadow: Option<(Pixmap, Point)>,
    /// What the editor opens: the saved file, or the capture kept in memory.
    source: Option<Source>,
    hover: bool,
    /// The look changed since the window got its pixels.
    stale: bool,
    drawn: Option<Placed>,
}

/// Sends what a card opens to it, from the thread that saves or keeps the capture.
pub struct Reporter {
    tx: Sender<(u64, io::Result<Source>)>,
    notifier: Notifier,
}

impl Reporter {
    /// The card's capture is saved, or failed to be.
    pub fn saved(&self, key: u64, result: io::Result<PathBuf>) {
        self.send(key, result.map(Source::File));
    }

    /// The card's capture, which went only to the clipboard, is kept for the editor.
    pub fn kept(&self, key: u64, source: Source) {
        self.send(key, Ok(source));
    }

    fn send(&self, key: u64, result: io::Result<Source>) {
        if self.tx.send((key, result)).is_ok() {
            self.notifier.notify();
        }
    }
}

impl Previews {
    /// `spares`: how many cards one capture may show at once.
    pub fn new(dark: bool, spares: usize) -> Result<Self, sys::Error> {
        let (saved_tx, saved_rx) = mpsc::channel();
        Ok(Self {
            stack: Stack::default(),
            windows: PreviewWindows::new(spares)?,
            shown: Vec::new(),
            waiting: Vec::new(),
            next_key: 0,
            dark,
            saved_tx,
            saved_rx,
            clicked: Vec::new(),
            open: Vec::new(),
        })
    }

    /// Wakes the main loop from another thread.
    pub fn notifier(&self) -> Notifier {
        self.windows.notifier()
    }

    pub fn reporter(&self) -> Reporter {
        Reporter {
            tx: self.saved_tx.clone(),
            notifier: self.windows.notifier(),
        }
    }

    /// The theme changed: cards drawn from now on use it.
    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            for s in &mut self.shown {
                s.stale = true;
                s.shadow = None;
            }
            self.sync(Instant::now());
        }
    }

    /// Adds `card` to the stack in the corner of `work`, or, when `visible` is false, keeps it
    /// until its save reports back. Returns the key the save reports with.
    pub fn add(&mut self, card: Card, work: Rect, timeout: Duration, visible: bool) -> u64 {
        self.next_key += 1;
        let key = self.next_key;
        if visible {
            self.put(key, card, work, Some(timeout));
        } else {
            self.waiting.push((key, card, work));
        }
        key
    }

    fn put(&mut self, key: u64, card: Card, work: Rect, timeout: Option<Duration>) {
        let slot = match self.windows.acquire() {
            Ok(slot) => slot,
            Err(e) => return log!("preview window: {e}"),
        };
        let now = Instant::now();
        let g = card.geometry;
        let timeout = timeout.filter(|_| !card.status.stays());
        let id = self
            .stack
            .add(now, (g.width, g.height), work, g.scale, timeout);
        self.shown.push(Shown {
            key,
            id,
            card,
            slot,
            shadow: None,
            source: None,
            hover: false,
            stale: true,
            drawn: None,
        });
        self.sync(now);
    }

    /// Handles a message to the preview windows; false when it is not theirs.
    #[cfg(windows)]
    pub fn event(&mut self, msg: &MSG) -> bool {
        let Some(event) = self.windows.event(msg) else {
            return false;
        };
        self.handle(event);
        true
    }

    /// Applies what happened to the cards.
    pub fn handle(&mut self, event: PreviewEvent) {
        let now = Instant::now();
        match event {
            PreviewEvent::Enter(slot) | PreviewEvent::Leave(slot) => {
                let enter = matches!(event, PreviewEvent::Enter(_));
                if let Some(s) = self.shown.iter_mut().find(|s| s.slot == slot) {
                    s.hover = enter;
                    s.stale = true;
                }
                let hover = self.shown.iter().any(|s| s.hover);
                self.stack.hover(now, hover);
            }
            PreviewEvent::Click { slot, at } => {
                if let Some(s) = self.shown.iter_mut().find(|s| s.slot == slot) {
                    if !s.card.geometry.on_close(at.x, at.y) && s.card.action.is_some() {
                        match s.source.take() {
                            Some(source) => self.open.push((source, now)),
                            // The save is still running: the editor opens when it is done.
                            None => self.clicked.push((s.key, now)),
                        }
                    }
                    // The cursor stays over a card that fades: it no longer holds the stack.
                    s.hover = false;
                    self.stack.close(now, s.id);
                    let hover = self.shown.iter().any(|s| s.hover);
                    self.stack.hover(now, hover);
                }
            }
            PreviewEvent::Internal => return,
            PreviewEvent::Timer => {}
            PreviewEvent::Notify => self.take_results(),
        }
        self.sync(now);
    }

    /// The captures clicked since the last call, and when, to open in the editor.
    pub fn take_open(&mut self) -> Vec<(Source, Instant)> {
        std::mem::take(&mut self.open)
    }

    fn take_results(&mut self) {
        while let Ok((key, result)) = self.saved_rx.try_recv() {
            let clicked =
                (self.clicked.iter().position(|c| c.0 == key)).map(|i| self.clicked.remove(i).1);
            let result = match (result, clicked) {
                (Ok(source), Some(at)) => {
                    self.open.push((source, at));
                    Ok(())
                }
                (Ok(source), None) => {
                    if let Some(s) = self.shown.iter_mut().find(|s| s.key == key) {
                        s.source = Some(source);
                    }
                    Ok(())
                }
                (Err(e), _) => Err(e),
            };
            if let Some(s) = self.shown.iter_mut().find(|s| s.key == key) {
                match result {
                    Ok(()) => {}
                    Err(e) => {
                        s.card.status = Status::Failed {
                            reason: failure_reason(&e),
                        };
                        s.card.action = None;
                        s.stale = true;
                        self.stack.keep(s.id);
                    }
                }
            } else if let Some(i) = self.waiting.iter().position(|w| w.0 == key) {
                let (key, mut card, work) = self.waiting.remove(i);
                if let Err(e) = result {
                    card.status = Status::Failed {
                        reason: failure_reason(&e),
                    };
                    card.action = None;
                    self.put(key, card, work, None);
                }
            }
        }
    }

    /// Brings the windows in line with the stack and sets the timer for its next change.
    fn sync(&mut self, now: Instant) {
        self.stack.update(now);
        let placed: Vec<Placed> = self.stack.placed(now).collect();
        let windows = &mut self.windows;
        self.shown.retain(|s| {
            let keep = placed.iter().any(|p| p.id == s.id);
            if !keep {
                windows.release(s.slot);
            }
            keep
        });
        let palette = Palette::of(self.dark);
        for p in &placed {
            let Some(s) = self.shown.iter_mut().find(|s| s.id == p.id) else {
                continue;
            };
            if !s.stale && s.drawn == Some(*p) {
                continue;
            }
            let card = s.stale.then(|| s.card.render(palette, s.hover)).flatten();
            let new_shadow = s.shadow.is_none();
            if new_shadow {
                s.shadow = s.card.render_shadow(palette);
            }
            let Some((shadow, offset)) = &s.shadow else {
                continue;
            };
            let shadow_at = Point::new(p.at.x - offset.x, p.at.y - offset.y);
            let result = windows.update(
                s.slot,
                card.as_ref().map(sprite),
                p.at,
                new_shadow.then(|| sprite(shadow)),
                shadow_at,
                p.alpha,
            );
            if let Err(e) = result {
                log!("preview: {e}");
            }
            s.stale = false;
            s.drawn = Some(*p);
        }
        self.windows
            .set_timer(self.stack.next_deadline(now).map(|t| t - now));
    }
}

fn sprite(p: &Pixmap) -> Sprite<'_> {
    Sprite {
        data: p.data(),
        width: p.width(),
        height: p.height(),
    }
}
