//! The editor window (SPEC §3.3, `docs/design/editor.md`, "Window"): an ordinary titled window
//! showing what `shot-core` draws, built as [`super::titled`] describes.
//!
//! The event pump hands the window's input to [`intercept`] before AppKit sees it, and the window's
//! delegate tells of resizes, the close button and the lost focus; both become [`WindowEvent`]s
//! for the [`EditorHandler`] that owns the editor. An event that arrives while the handler runs
//! waits until it returns. The handler is dropped with the window.
//!
//! While a file is dragged out ([`EditorWindow::drag_file`]), the mouse goes to AppKit, which
//! runs the drag.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{ClassType, MainThreadMarker, Message, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceCustomization, NSCursor, NSEvent, NSEventModifierFlags, NSEventType, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSSize, NSString};
use shot_core::editor::objects::Axis;
use shot_core::editor::{Cursor, Input, Key};
use shot_core::geom::{Point, Rect};
use shot_core::image::{BgrxMut, BgrxView};

use super::Error;
use super::drag::{self, DragImage};
use super::monitors::screens;
use super::pump::{Intercepted, register, set_timer, unregister};
use super::titled::{self, Note, Parts, Surface, appearance};

/// The content never gets smaller than this, points: the toolbar fits at 960.
const MIN_SIZE: (f64, f64) = (960.0, 600.0);
/// How far a line of a mouse wheel scrolls, points; Chromium's step on macOS.
const LINE_SCROLL: f64 = 40.0;
/// A zoom step: so far a trackpad scrolls with ⌘, points, or so much a pinch magnifies.
const ZOOM_SCROLL: f64 = 40.0;
const ZOOM_PINCH: f64 = 0.2;

const KEY_SHIFT: u16 = 0x38;
const KEY_RIGHT_SHIFT: u16 = 0x3C;

thread_local! {
    static EDITORS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

pub enum WindowEvent {
    Input(Input),
    /// The time set with [`EditorWindow::set_timer`] has come.
    Timer,
    /// [`EditorNotifier::notify`] was called.
    Notify,
    /// The drag of [`EditorWindow::drag_file`] is over, or did not start: tell the editor
    /// [`Input::DragEnded`].
    DragEnded(Result<(), Error>),
}

/// Owns the editor; gets every event of its window. After [`Input::Resize`] — also the first
/// event — the back buffer is new and has to be drawn whole.
pub trait EditorHandler {
    fn event(&mut self, window: &mut EditorWindow, event: WindowEvent);
}

/// An open window, by its window number.
struct Entry {
    parts: Parts,
    flags: Rc<Flags>,
    /// `None` while the handler runs.
    host: Option<Box<Host>>,
}

struct Host {
    window: EditorWindow,
    handler: Box<dyn EditorHandler>,
}

/// What changes while the handler holds the window.
struct Flags {
    pending: RefCell<VecDeque<WindowEvent>>,
    /// The window was sized while the handler ran.
    resize: Cell<bool>,
    /// The content size and scale the handler was last told of.
    told: Cell<(u32, u32, f32)>,
    cursor: Cell<Cursor>,
    /// The mouse is over the content.
    inside: Cell<bool>,
    /// Zoom input toward the next step.
    zoom: Cell<f64>,
    dark: Cell<bool>,
    follow_system: Cell<bool>,
    closing: Cell<bool>,
    /// The file to drag once the handler returns, and what the drag shows.
    drag: RefCell<Option<(PathBuf, Option<DragImage>)>>,
    /// The last press or drag of the mouse: a drag starts from it.
    mouse: RefCell<Option<Retained<NSEvent>>>,
    /// AppKit runs a drag out of the window.
    dragging: Cell<bool>,
}

/// The window as its handler sees it.
pub struct EditorWindow {
    ns: Retained<NSWindow>,
    surface: Surface,
    flags: Rc<Flags>,
    notify: isize,
    timer: isize,
}

/// Wakes the window's handler with [`WindowEvent::Notify`] from any thread.
#[derive(Clone, Copy)]
pub struct EditorNotifier(isize);

impl EditorNotifier {
    pub fn notify(&self) {
        super::pump::post(self.0);
    }
}

/// An open editor window, for the app.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EditorRef(isize);

impl EditorRef {
    pub fn is_open(&self) -> bool {
        EDITORS.with_borrow(|list| list.iter().any(|e| e.parts.id == self.0))
    }

    /// `follow_system`: the window follows the system theme from now on; `dark` is it now.
    pub fn set_theme(&self, dark: bool, follow_system: bool) {
        let Some((ns, flags)) = EDITORS.with_borrow(|list| {
            let e = list.iter().find(|e| e.parts.id == self.0)?;
            Some((e.parts.ns.clone(), e.flags.clone()))
        }) else {
            return;
        };
        flags.follow_system.set(follow_system);
        ns.setAppearance(appearance(dark, follow_system).as_deref());
        set_theme(self.0, &flags, dark);
    }

    /// Closes the window as its close button does, before returning: with edits it stays and
    /// asks first, in front.
    pub fn ask_close(&self) {
        dispatch(self.0, WindowEvent::Input(Input::Close));
        let ns = EDITORS.with_borrow(|list| {
            let e = list.iter().find(|e| e.parts.id == self.0)?;
            Some(e.parts.ns.clone())
        });
        if let Some(ns) = ns
            && let Some(mtm) = MainThreadMarker::new()
        {
            titled::show(mtm, &ns);
        }
    }
}

/// What the delegate tells of a window; false when it is not an editor.
pub(super) fn note(id: isize, note: Note) -> bool {
    if !EditorRef(id).is_open() {
        return false;
    }
    match note {
        Note::Close => dispatch(id, WindowEvent::Input(Input::Close)),
        Note::Resized => resized(id),
        Note::Blur => dispatch(id, WindowEvent::Input(Input::Blur)),
    }
    true
}

pub(super) fn any_open() -> bool {
    EDITORS.with_borrow(|list| !list.is_empty())
}

/// The system theme is now `dark` or not: the editors that follow it are told.
pub(super) fn system_theme(dark: bool) {
    let following: Vec<(isize, Rc<Flags>)> = EDITORS.with_borrow(|list| {
        (list.iter())
            .filter(|e| e.flags.follow_system.get())
            .map(|e| (e.parts.id, e.flags.clone()))
            .collect()
    });
    for (id, flags) in following {
        set_theme(id, &flags, dark);
    }
}

impl EditorWindow {
    /// Opens a window whose content is `client` (physical px of the virtual desktop, on a screen
    /// at `scale`), and hands it to `handler`, which draws it before it is shown.
    pub fn open(
        title: &str,
        client: Rect,
        _scale: f32,
        dark: bool,
        follow_system: bool,
        handler: Box<dyn EditorHandler>,
    ) -> Result<EditorRef, Error> {
        let mtm = MainThreadMarker::new().ok_or_else(|| Error::new("not the main thread"))?;
        let middle = Point::new(client.x + client.width / 2, client.y + client.height / 2);
        let screen = (screens().into_iter())
            .min_by_key(|s| s.monitor.rect.distance_sq(middle))
            .ok_or_else(|| Error::new("no screen"))?;
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let (parts, layer) = titled::window(
            mtm,
            screen.rect_to_points(client),
            style,
            title,
            dark,
            follow_system,
        )?;
        parts
            .ns
            .setContentMinSize(NSSize::new(MIN_SIZE.0, MIN_SIZE.1));
        let (id, ns) = (parts.id, parts.ns.clone());
        let flags = Rc::new(Flags {
            pending: RefCell::new(VecDeque::new()),
            resize: Cell::new(false),
            told: Cell::new((0, 0, 0.0)),
            cursor: Cell::new(Cursor::Arrow),
            inside: Cell::new(false),
            zoom: Cell::new(0.0),
            dark: Cell::new(dark),
            follow_system: Cell::new(follow_system),
            closing: Cell::new(false),
            drag: RefCell::new(None),
            mouse: RefCell::new(None),
            dragging: Cell::new(false),
        });
        let window = EditorWindow {
            ns: ns.clone(),
            surface: Surface::new(layer),
            flags: flags.clone(),
            notify: register(Rc::new(move || dispatch(id, WindowEvent::Notify))),
            timer: register(Rc::new(move || dispatch(id, WindowEvent::Timer))),
        };
        EDITORS.with_borrow_mut(|list| {
            list.push(Entry {
                parts,
                flags,
                host: Some(Box::new(Host { window, handler })),
            })
        });
        resized(id);
        titled::show(mtm, &ns);
        Ok(EditorRef(id))
    }

    pub fn notifier(&self) -> EditorNotifier {
        EditorNotifier(self.notify)
    }

    /// The content's size in physical px.
    pub fn size(&self) -> (u32, u32) {
        let (w, h, _) = self.flags.told.get();
        (w, h)
    }

    pub fn scale(&self) -> f32 {
        self.flags.told.get().2
    }

    /// The back buffer, the content's size: draw into it, then [`Self::present`].
    pub fn canvas(&mut self) -> Option<BgrxMut<'_>> {
        self.surface.canvas()
    }

    /// Puts what was drawn into the back buffer on screen; `_rect` is where, which the compositor
    /// finds out by itself.
    pub fn present(&self, _rect: Rect) {
        self.surface.present();
    }

    pub fn set_title(&self, title: &str) {
        self.ns.setTitle(&NSString::from_str(title));
    }

    pub fn set_cursor(&self, cursor: Cursor) {
        self.flags.cursor.set(cursor);
        if self.flags.inside.get() {
            ns_cursor(cursor).set();
        }
    }

    /// Sends [`WindowEvent::Timer`] after `after`; `None` cancels it.
    pub fn set_timer(&self, after: Option<Duration>) {
        set_timer(self.timer, after.map(|d| Instant::now() + d));
    }

    /// Closes the window once the handler returns; the handler is dropped with it.
    pub fn close(&self) {
        self.flags.closing.set(true);
    }

    /// Drags the file at `path` out of the window once the handler returns, as Finder does, with
    /// `image` under the cursor; ask while the left button is down. The handler gets
    /// [`WindowEvent::DragEnded`] when the drag is over; meanwhile its other events come as usual.
    pub fn drag_file(&self, path: &Path, image: Option<BgrxView>) {
        let image = image.and_then(|i| DragImage::new(i, self.scale()));
        *self.flags.drag.borrow_mut() = Some((path.to_path_buf(), image));
    }
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        unregister(self.notify);
        unregister(self.timer);
    }
}

fn set_theme(id: isize, flags: &Flags, dark: bool) {
    if flags.dark.replace(dark) != dark {
        dispatch(id, WindowEvent::Input(Input::Theme { dark }));
    }
}

fn ns_cursor(cursor: Cursor) -> Retained<NSCursor> {
    match cursor {
        Cursor::Arrow => NSCursor::arrowCursor(),
        Cursor::Crosshair => NSCursor::crosshairCursor(),
        Cursor::Grab => NSCursor::openHandCursor(),
        Cursor::Grabbing => NSCursor::closedHandCursor(),
        Cursor::Move => private_cursor(sel!(_moveCursor)).unwrap_or_else(NSCursor::openHandCursor),
        Cursor::Resize(axis) => resize_cursor(axis),
    }
}

/// A cursor AppKit has but does not declare; `None` where it lacks it.
fn private_cursor(name: objc2::runtime::Sel) -> Option<Retained<NSCursor>> {
    let class = NSCursor::class();
    let has: bool = unsafe { msg_send![class, respondsToSelector: name] };
    if !has {
        return None;
    }
    unsafe { objc2::msg_send![class, performSelector: name] }
}

/// A two-headed arrow along `axis`. macOS 15 declares them all as frame-resize cursors; macOS 14
/// has the straight ones under older names and the diagonal ones undeclared.
fn resize_cursor(axis: Axis) -> Retained<NSCursor> {
    let class = NSCursor::class();
    let modern = sel!(frameResizeCursorFromPosition:inDirections:);
    let has: bool = unsafe { msg_send![class, respondsToSelector: modern] };
    if has {
        // `NSCursorFrameResizePosition`: top 1, left 2, bottom 4, right 8.
        let position: usize = match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => 8,
            Axis::Falling => 1 | 2,
            Axis::Rising => 1 | 8,
        };
        // In both directions.
        let cursor: Option<Retained<NSCursor>> = unsafe {
            msg_send![class, frameResizeCursorFromPosition: position, inDirections: 3usize]
        };
        if let Some(cursor) = cursor {
            return cursor;
        }
    }
    #[allow(deprecated)]
    match axis {
        Axis::Vertical => NSCursor::resizeUpDownCursor(),
        Axis::Horizontal => NSCursor::resizeLeftRightCursor(),
        Axis::Falling => private_cursor(sel!(_windowResizeNorthWestSouthEastCursor))
            .unwrap_or_else(NSCursor::crosshairCursor),
        Axis::Rising => private_cursor(sel!(_windowResizeNorthEastSouthWestCursor))
            .unwrap_or_else(NSCursor::crosshairCursor),
    }
}

/// Runs the handler for `event`, and for what arrived meanwhile; then closes the window if the
/// handler asked to.
fn dispatch(id: isize, event: WindowEvent) {
    let mut event = Some(event);
    let Some((mut host, flags)) = EDITORS.with_borrow_mut(|list| {
        let e = list.iter_mut().find(|e| e.parts.id == id)?;
        match e.host.take() {
            Some(host) => Some((host, e.flags.clone())),
            None => {
                // The handler runs: after it.
                e.flags.pending.borrow_mut().extend(event.take());
                None
            }
        }
    }) else {
        return;
    };
    while let Some(e) = event
        .take()
        .or_else(|| flags.pending.borrow_mut().pop_front())
    {
        host.handler.event(&mut host.window, e);
    }
    if flags.closing.get() {
        let entry = EDITORS.with_borrow_mut(|list| {
            let i = list.iter().position(|e| e.parts.id == id)?;
            Some(list.remove(i))
        });
        drop(host);
        if let Some(entry) = entry {
            titled::close(entry.parts, entry.flags.inside.get());
        }
        return;
    }
    EDITORS.with_borrow_mut(|list| {
        if let Some(e) = list.iter_mut().find(|e| e.parts.id == id) {
            e.host = Some(host);
        }
    });
    if flags.resize.take() {
        resized(id);
    }
    let drag = flags.drag.borrow_mut().take();
    if let Some((path, image)) = drag {
        start_drag(id, &flags, &path, image.as_ref());
    }
}

/// Starts the drag the handler asked for; it ends in [`drag_ended`].
fn start_drag(id: isize, flags: &Flags, path: &Path, image: Option<&DragImage>) {
    let view = EDITORS.with_borrow(|list| {
        let e = list.iter().find(|e| e.parts.id == id)?;
        Some(e.parts.view.clone())
    });
    let event = flags.mouse.borrow().clone();
    let (Some(view), Some(event)) = (view, event) else {
        return dispatch(
            id,
            WindowEvent::DragEnded(Err(Error::new("no press to drag from"))),
        );
    };
    flags.dragging.set(true);
    if let Err(e) = drag::drag_file(id, &view, &event, path, image) {
        drag::abandon(id);
        drag_ended(id, Err(e));
    }
}

/// Window `id`'s drag is over: its handler is told, then where the pointer is now, which the
/// drag kept from it.
pub(super) fn drag_ended(id: isize, result: Result<(), Error>) {
    let Some((ns, view, flags)) = EDITORS.with_borrow(|list| {
        let e = list.iter().find(|e| e.parts.id == id)?;
        Some((e.parts.ns.clone(), e.parts.view.clone(), e.flags.clone()))
    }) else {
        return;
    };
    flags.dragging.set(false);
    dispatch(id, WindowEvent::DragEnded(result));
    let p = view.convertPoint_fromView(ns.mouseLocationOutsideOfEventStream(), None);
    let bounds = view.bounds();
    let inside =
        (0.0..bounds.size.width).contains(&p.x) && (0.0..bounds.size.height).contains(&p.y);
    flags.inside.set(inside);
    let input = if inside {
        let scale = f64::from(flags.told.get().2);
        Input::Move {
            x: (p.x * scale) as f32,
            y: ((bounds.size.height - p.y) * scale) as f32,
            shift: false,
        }
    } else {
        Input::Leave
    };
    dispatch(id, WindowEvent::Input(input));
}

/// The content may have a new size or scale: a new back buffer, and the handler is told.
fn resized(id: isize) {
    let resize = EDITORS.with_borrow_mut(|list| {
        let e = list.iter_mut().find(|e| e.parts.id == id)?;
        let Some(host) = e.host.as_mut() else {
            e.flags.resize.set(true);
            return None;
        };
        let told = titled::content_px(&e.parts.ns, &e.parts.view);
        let (width, height, scale) = told;
        if width == 0 || height == 0 || e.flags.told.get() == told {
            return None;
        }
        e.flags.told.set(told);
        host.window.surface.resize(width, height, scale);
        Some(Input::Resize {
            width,
            height,
            scale,
        })
    });
    if let Some(input) = resize {
        dispatch(id, WindowEvent::Input(input));
    }
}

/// Turns the input to an editor window into editor events and keeps it from AppKit, except what
/// goes to the title bar and the window's edges.
pub(crate) fn intercept(event: &NSEvent) -> Intercepted {
    let id = event.windowNumber();
    let Some((view, area, flags)) = EDITORS.with_borrow(|list| {
        let e = list.iter().find(|e| e.parts.id == id && id > 0)?;
        Some((e.parts.view.clone(), e.parts.area.clone(), e.flags.clone()))
    }) else {
        return Intercepted::Pass;
    };
    let kind = event.r#type();
    if flags.dragging.get() {
        match kind {
            NSEventType::MouseMoved | NSEventType::LeftMouseDragged | NSEventType::LeftMouseUp => {
                return Intercepted::Pass;
            }
            // A press while the drag still runs: AppKit never said it ended.
            NSEventType::LeftMouseDown if drag::abandon(id) => {
                drag_ended(id, Err(Error::new("the drag did not end")));
            }
            _ => {}
        }
    }
    if matches!(
        kind,
        NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged
    ) {
        flags.mouse.replace(Some(event.retain()));
    }
    let mods = event.modifierFlags();
    let shift = mods.contains(NSEventModifierFlags::Shift);
    let cmd = mods.contains(NSEventModifierFlags::Command);
    let scale = flags.told.get().2;
    let at = || titled::at(&view, event, scale);
    let send = |input: Input| {
        dispatch(id, WindowEvent::Input(input));
        Intercepted::Swallow
    };
    match event.r#type() {
        NSEventType::MouseMoved | NSEventType::LeftMouseDragged => {
            let (x, y, inside) = at();
            if !inside && event.r#type() == NSEventType::MouseMoved {
                return Intercepted::Pass;
            }
            send(Input::Move { x, y, shift })
        }
        NSEventType::LeftMouseDown => {
            let (x, y, inside) = at();
            if !inside {
                return Intercepted::Pass;
            }
            titled::focus_on_click(event);
            send(Input::Down { x, y, shift })
        }
        NSEventType::LeftMouseUp => {
            let (x, y, _) = at();
            send(Input::Up { x, y, shift })
        }
        NSEventType::MouseEntered | NSEventType::MouseExited => {
            let ours = event
                .trackingArea()
                .is_none_or(|a| std::ptr::eq(&*a, &*area));
            if !ours {
                return Intercepted::Pass;
            }
            if event.r#type() == NSEventType::MouseEntered {
                flags.inside.set(true);
                ns_cursor(flags.cursor.get()).set();
                Intercepted::Swallow
            } else {
                flags.inside.set(false);
                NSCursor::arrowCursor().set();
                send(Input::Leave)
            }
        }
        NSEventType::ScrollWheel => {
            let (x, y, _) = at();
            let precise = event.hasPreciseScrollingDeltas();
            let (dx, dy) = (event.scrollingDeltaX(), event.scrollingDeltaY());
            if cmd {
                let step = if precise { ZOOM_SCROLL } else { 1.0 };
                return zoom(&flags, x, y, dy / step).map_or(Intercepted::Swallow, send);
            }
            let scale = f64::from(scale);
            let k = if precise { scale } else { LINE_SCROLL * scale };
            send(Input::Scroll {
                dx: (dx * k) as f32,
                dy: (dy * k) as f32,
            })
        }
        NSEventType::Magnify => {
            let (x, y, _) = at();
            zoom(&flags, x, y, event.magnification() / ZOOM_PINCH)
                .map_or(Intercepted::Swallow, send)
        }
        NSEventType::KeyDown => match key_of(event.keyCode()) {
            Some(key) => send(Input::Key {
                key,
                ctrl: cmd,
                shift,
            }),
            None => Intercepted::Swallow,
        },
        NSEventType::KeyUp => match key_of(event.keyCode()) {
            Some(key @ Key::Space) => send(Input::KeyUp { key, shift }),
            _ => Intercepted::Swallow,
        },
        NSEventType::FlagsChanged => match event.keyCode() {
            KEY_SHIFT | KEY_RIGHT_SHIFT if shift => send(Input::Key {
                key: Key::Shift,
                ctrl: cmd,
                shift,
            }),
            KEY_SHIFT | KEY_RIGHT_SHIFT => send(Input::KeyUp {
                key: Key::Shift,
                shift,
            }),
            _ => Intercepted::Pass,
        },
        NSEventType::RightMouseDown
        | NSEventType::RightMouseUp
        | NSEventType::RightMouseDragged
        | NSEventType::OtherMouseDown
        | NSEventType::OtherMouseUp
        | NSEventType::OtherMouseDragged => {
            if at().2 {
                Intercepted::Swallow
            } else {
                Intercepted::Pass
            }
        }
        _ => Intercepted::Pass,
    }
}

/// Adds `amount` steps of zoom input; a zoom event once it makes whole steps.
fn zoom(flags: &Flags, x: f32, y: f32, amount: f64) -> Option<Input> {
    let total = flags.zoom.get() + amount;
    let steps = total.trunc();
    flags.zoom.set(total - steps);
    (steps != 0.0).then_some(Input::Zoom {
        x,
        y,
        steps: steps as i32,
    })
}

/// Keys by their place on a US keyboard: macOS key codes name places, so `⌘Z` is the same key in
/// every layout.
fn key_of(code: u16) -> Option<Key> {
    const PLACES: [(u16, char); 36] = [
        (0x00, 'a'),
        (0x0B, 'b'),
        (0x08, 'c'),
        (0x02, 'd'),
        (0x0E, 'e'),
        (0x03, 'f'),
        (0x05, 'g'),
        (0x04, 'h'),
        (0x22, 'i'),
        (0x26, 'j'),
        (0x28, 'k'),
        (0x25, 'l'),
        (0x2E, 'm'),
        (0x2D, 'n'),
        (0x1F, 'o'),
        (0x23, 'p'),
        (0x0C, 'q'),
        (0x0F, 'r'),
        (0x01, 's'),
        (0x11, 't'),
        (0x20, 'u'),
        (0x09, 'v'),
        (0x0D, 'w'),
        (0x07, 'x'),
        (0x10, 'y'),
        (0x06, 'z'),
        (0x1D, '0'),
        (0x12, '1'),
        (0x13, '2'),
        (0x14, '3'),
        (0x15, '4'),
        (0x17, '5'),
        (0x16, '6'),
        (0x1A, '7'),
        (0x1C, '8'),
        (0x19, '9'),
    ];
    Some(match code {
        0x75 => Key::Delete,
        0x33 => Key::Backspace,
        0x7B => Key::Left,
        0x7C => Key::Right,
        0x7E => Key::Up,
        0x7D => Key::Down,
        0x24 | 0x4C => Key::Enter,
        0x35 => Key::Escape,
        0x31 => Key::Space,
        KEY_SHIFT | KEY_RIGHT_SHIFT => Key::Shift,
        // The keypad's digits: 0–7, then 8 and 9 after a gap.
        0x52..=0x59 => Key::Char((b'0' + (code - 0x52) as u8) as char),
        0x5B => Key::Char('8'),
        0x5C => Key::Char('9'),
        _ => Key::Char(PLACES.iter().find(|(c, _)| *c == code)?.1),
    })
}

/// The newest editor window, for the dev commands that stand for input.
pub(crate) fn front() -> Option<isize> {
    EDITORS.with_borrow(|list| list.last().map(|e| e.parts.id))
}

/// Closes the newest editor window as its close button does.
pub(crate) fn press_close() {
    let ns = EDITORS.with_borrow(|list| list.last().map(|e| e.parts.ns.clone()));
    if let Some(ns) = ns {
        ns.performClose(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits_go_by_their_place() {
        // `Z` on a US keyboard is `Я` in the Russian layout and `W` in the French one.
        assert_eq!(key_of(0x06), Some(Key::Char('z')));
        assert_eq!(key_of(0x0C), Some(Key::Char('q')));
        assert_eq!(key_of(0x1D), Some(Key::Char('0')));
        assert_eq!(key_of(0x52), Some(Key::Char('0')));
        assert_eq!(key_of(0x59), Some(Key::Char('7')));
        assert_eq!(key_of(0x5C), Some(Key::Char('9')));
        assert_eq!(key_of(0x75), Some(Key::Delete));
        // `-` is not a key the editor knows.
        assert_eq!(key_of(0x1B), None);
    }
}
