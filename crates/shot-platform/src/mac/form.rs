//! The settings and welcome windows (`docs/design/settings.md`): ordinary fixed-size titled
//! windows showing what `shot_core::ui` draws, built as [`super::titled`] describes. Like the
//! editor, each keeps a back buffer the size of its content that the handler draws into, and
//! events that arrive while the handler runs wait until it returns. The handler is dropped with
//! the window.
//!
//! The keys reach the window through the event pump before AppKit sees them, ⌘ combinations too.
//! A combination the system or another app holds as a hotkey goes to its holder first: while a
//! hotkey is recorded, Shot's own is let go, but a taken combination never reaches the window, as
//! there is no way around that without the Accessibility permission.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearanceCustomization, NSEvent, NSEventModifierFlags, NSEventType, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use shot_core::geom::{Point, Rect};
use shot_core::image::BgrxMut;
use shot_core::ui::{Input, Key, Mods};

use super::Error;
use super::hotkey;
use super::monitors::screens;
use super::pump::{Intercepted, post, register, unregister};
use super::titled::{self, Note, Parts, Surface, appearance};

const KEY_ESCAPE: u16 = 0x35;
const KEY_W: u16 = 0x0D;

thread_local! {
    static FORMS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

pub enum FormEvent {
    Input(Input),
    /// [`FormRef::refresh`] was called: what the window shows changed elsewhere.
    Refresh,
}

/// Owns the window's model; gets every event of its window. After [`Input::Resize`] — also the
/// first event — the back buffer is new and has to be drawn whole.
pub trait FormHandler {
    fn event(&mut self, window: &mut FormWindow, event: FormEvent);
}

/// An open window, by its window number.
struct Entry {
    parts: Parts,
    flags: Rc<Flags>,
    /// The pump's key that has the handler redraw.
    refresh: isize,
    /// `None` while the handler runs.
    host: Option<Box<Host>>,
}

struct Host {
    window: FormWindow,
    handler: Box<dyn FormHandler>,
}

/// What changes while the handler holds the window.
struct Flags {
    pending: RefCell<VecDeque<FormEvent>>,
    /// The window was sized while the handler ran.
    resize: Cell<bool>,
    /// The content size and scale the handler was last told of.
    told: Cell<(u32, u32, f32)>,
    /// The mouse is over the content.
    inside: Cell<bool>,
    dark: Cell<bool>,
    follow_system: Cell<bool>,
    closing: Cell<bool>,
    /// A hotkey is being recorded: ⌘W goes to the handler instead of closing the window.
    keys: Cell<bool>,
}

/// The window as its handler sees it.
pub struct FormWindow {
    ns: Retained<NSWindow>,
    id: isize,
    surface: Surface,
    flags: Rc<Flags>,
    refresh: isize,
}

/// An open form window, for the app.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FormRef(isize);

impl FormRef {
    /// The window number, to hand to another process.
    pub fn raw(&self) -> isize {
        self.0
    }

    /// A window another process handed over as [`FormRef::raw`]; it may be gone.
    pub fn from_raw(raw: isize) -> Self {
        FormRef(raw)
    }

    /// Lets the window take input again: a dialog of another process had the keyboard, and the
    /// window gets it back.
    pub fn enable(&self) {
        self.show();
    }

    pub fn is_open(&self) -> bool {
        FORMS.with_borrow(|list| list.iter().any(|e| e.parts.id == self.0))
    }

    /// Has the handler redraw the window with [`FormEvent::Refresh`].
    pub fn refresh(&self) {
        let key = FORMS.with_borrow(|list| {
            let e = list.iter().find(|e| e.parts.id == self.0)?;
            Some(e.refresh)
        });
        if let Some(key) = key {
            post(key);
        }
    }

    /// `follow_system`: the window follows the system theme from now on; `dark` is it now.
    pub fn set_theme(&self, dark: bool, follow_system: bool) {
        let Some((ns, flags)) = FORMS.with_borrow(|list| {
            let e = list.iter().find(|e| e.parts.id == self.0)?;
            Some((e.parts.ns.clone(), e.flags.clone()))
        }) else {
            return;
        };
        flags.follow_system.set(follow_system);
        ns.setAppearance(appearance(dark, follow_system).as_deref());
        set_theme(self.0, &flags, dark);
    }

    /// Brings the window back from the Dock if it is there, and to the front.
    pub fn show(&self) {
        if let Some(ns) = self.ns()
            && let Some(mtm) = MainThreadMarker::new()
        {
            titled::show(mtm, &ns);
        }
    }

    fn ns(&self) -> Option<Retained<NSWindow>> {
        FORMS.with_borrow(|list| {
            let e = list.iter().find(|e| e.parts.id == self.0)?;
            Some(e.parts.ns.clone())
        })
    }
}

/// What the delegate tells of a window; false when it is not a form.
pub(super) fn note(id: isize, note: Note) -> bool {
    if !FormRef(id).is_open() {
        return false;
    }
    match note {
        Note::Close => dispatch(id, FormEvent::Input(Input::Close)),
        Note::Resized => resized(id),
        Note::Blur => dispatch(id, FormEvent::Input(Input::Blur)),
    }
    true
}

pub(super) fn any_open() -> bool {
    FORMS.with_borrow(|list| !list.is_empty())
}

/// The system theme is now `dark` or not: the forms that follow it are told.
pub(super) fn system_theme(dark: bool) {
    let following: Vec<(isize, Rc<Flags>)> = FORMS.with_borrow(|list| {
        (list.iter())
            .filter(|e| e.flags.follow_system.get())
            .map(|e| (e.parts.id, e.flags.clone()))
            .collect()
    });
    for (id, flags) in following {
        set_theme(id, &flags, dark);
    }
}

impl FormWindow {
    /// Opens a window titled `title` whose content is `client` (physical px of the virtual
    /// desktop, on a screen at `scale`), and hands it to `handler`, which draws it before it is
    /// shown.
    pub fn open(
        title: &str,
        client: Rect,
        _scale: f32,
        dark: bool,
        follow_system: bool,
        handler: Box<dyn FormHandler>,
    ) -> Result<FormRef, Error> {
        let mtm = MainThreadMarker::new().ok_or_else(|| Error::new("not the main thread"))?;
        let middle = Point::new(client.x + client.width / 2, client.y + client.height / 2);
        let screen = (screens().into_iter())
            .min_by_key(|s| s.monitor.rect.distance_sq(middle))
            .ok_or_else(|| Error::new("no screen"))?;
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        let (parts, layer) = titled::window(
            mtm,
            screen.rect_to_points(client),
            style,
            title,
            dark,
            follow_system,
        )?;
        let (id, ns) = (parts.id, parts.ns.clone());
        let flags = Rc::new(Flags {
            pending: RefCell::new(VecDeque::new()),
            resize: Cell::new(false),
            told: Cell::new((0, 0, 0.0)),
            inside: Cell::new(false),
            dark: Cell::new(dark),
            follow_system: Cell::new(follow_system),
            closing: Cell::new(false),
            keys: Cell::new(false),
        });
        let refresh = register(Rc::new(move || dispatch(id, FormEvent::Refresh)));
        let window = FormWindow {
            ns: ns.clone(),
            id,
            surface: Surface::new(layer),
            flags: flags.clone(),
            refresh,
        };
        FORMS.with_borrow_mut(|list| {
            list.push(Entry {
                parts,
                flags,
                refresh,
                host: Some(Box::new(Host { window, handler })),
            })
        });
        resized(id);
        if FormRef(id).is_open() {
            titled::show(mtm, &ns);
        }
        Ok(FormRef(id))
    }

    pub fn this(&self) -> FormRef {
        FormRef(self.id)
    }

    /// The content's size in physical px.
    pub fn size(&self) -> (u32, u32) {
        let (w, h, _) = self.flags.told.get();
        (w, h)
    }

    pub fn scale(&self) -> f32 {
        self.flags.told.get().2
    }

    /// Resizes the content to `width`×`height` px, keeping the window's top-left corner;
    /// [`Input::Resize`] follows once the handler returns.
    pub fn set_client_size(&self, width: u32, height: u32) {
        let scale = self.ns.backingScaleFactor();
        let content = self.ns.contentRectForFrameRect(self.ns.frame());
        let size = NSSize::new(f64::from(width) / scale, f64::from(height) / scale);
        let top = content.origin.y + content.size.height;
        let content = NSRect::new(NSPoint::new(content.origin.x, top - size.height), size);
        self.ns
            .setFrame_display(self.ns.frameRectForContentRect(content), true);
    }

    /// While `on`, ⌘W goes to the handler, as every other key does: for recording a hotkey.
    pub fn capture_keys(&self, on: bool) {
        self.flags.keys.set(on);
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

    /// Closes the window once the handler returns; the handler is dropped with it.
    pub fn close(&self) {
        self.flags.closing.set(true);
    }
}

impl Drop for FormWindow {
    fn drop(&mut self) {
        unregister(self.refresh);
    }
}

fn set_theme(id: isize, flags: &Flags, dark: bool) {
    if flags.dark.replace(dark) != dark {
        dispatch(id, FormEvent::Input(Input::Theme { dark }));
    }
}

/// Runs the handler for `event`, and for what arrived meanwhile; then closes the window if the
/// handler asked to.
fn dispatch(id: isize, event: FormEvent) {
    let mut event = Some(event);
    let Some((mut host, flags)) = FORMS.with_borrow_mut(|list| {
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
        let entry = FORMS.with_borrow_mut(|list| {
            let i = list.iter().position(|e| e.parts.id == id)?;
            Some(list.remove(i))
        });
        drop(host);
        if let Some(entry) = entry {
            titled::close(entry.parts, entry.flags.inside.get());
        }
        return;
    }
    FORMS.with_borrow_mut(|list| {
        if let Some(e) = list.iter_mut().find(|e| e.parts.id == id) {
            e.host = Some(host);
        }
    });
    if flags.resize.take() {
        resized(id);
    }
}

/// The content may have a new size or scale: a new back buffer, and the handler is told.
fn resized(id: isize) {
    let resize = FORMS.with_borrow_mut(|list| {
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
        dispatch(id, FormEvent::Input(input));
    }
}

fn mods(flags: NSEventModifierFlags) -> Mods {
    Mods {
        ctrl: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        shift: flags.contains(NSEventModifierFlags::Shift),
        meta: flags.contains(NSEventModifierFlags::Command),
    }
}

/// The modifier of a modifier key's code.
fn modifier_of(code: u16) -> Option<NSEventModifierFlags> {
    Some(match code {
        0x37 | 0x36 => NSEventModifierFlags::Command,
        0x38 | 0x3C => NSEventModifierFlags::Shift,
        0x3A | 0x3D => NSEventModifierFlags::Option,
        0x3B | 0x3E => NSEventModifierFlags::Control,
        _ => return None,
    })
}

/// The key of a key code: what a hotkey can use, or `Esc`.
fn key_of(code: u16) -> Option<Key> {
    match code {
        KEY_ESCAPE => Some(Key::Escape),
        _ => hotkey::key_of(u32::from(code)).map(Key::Hot),
    }
}

/// Turns the input to a form window into form events and keeps it from AppKit, except what goes
/// to the title bar.
pub(crate) fn intercept(event: &NSEvent) -> Intercepted {
    let id = event.windowNumber();
    let Some((view, area, flags)) = FORMS.with_borrow(|list| {
        let e = list.iter().find(|e| e.parts.id == id && id > 0)?;
        Some((e.parts.view.clone(), e.parts.area.clone(), e.flags.clone()))
    }) else {
        return Intercepted::Pass;
    };
    let scale = flags.told.get().2;
    let at = || titled::at(&view, event, scale);
    let send = |input: Input| {
        dispatch(id, FormEvent::Input(input));
        Intercepted::Swallow
    };
    let held = event.modifierFlags();
    let mods = mods(held);
    match event.r#type() {
        NSEventType::MouseMoved | NSEventType::LeftMouseDragged => {
            let (x, y, inside) = at();
            if !inside && event.r#type() == NSEventType::MouseMoved {
                return Intercepted::Pass;
            }
            send(Input::Move { x, y })
        }
        NSEventType::LeftMouseDown => {
            let (x, y, inside) = at();
            if !inside {
                return Intercepted::Pass;
            }
            titled::focus_on_click(event);
            send(Input::Down { x, y })
        }
        NSEventType::LeftMouseUp => {
            let (x, y, _) = at();
            send(Input::Up { x, y })
        }
        NSEventType::MouseEntered | NSEventType::MouseExited => {
            let ours = event
                .trackingArea()
                .is_none_or(|a| std::ptr::eq(&*a, &*area));
            if !ours {
                return Intercepted::Pass;
            }
            let entered = event.r#type() == NSEventType::MouseEntered;
            flags.inside.set(entered);
            if entered {
                Intercepted::Swallow
            } else {
                send(Input::Leave)
            }
        }
        NSEventType::KeyDown => {
            let code = event.keyCode();
            let only_cmd = Mods {
                meta: true,
                ..Mods::default()
            };
            if code == KEY_W && mods == only_cmd && !flags.keys.get() {
                return send(Input::Close);
            }
            match key_of(code) {
                Some(key) => send(Input::Key { key, mods }),
                None => Intercepted::Swallow,
            }
        }
        NSEventType::KeyUp => send(Input::KeyUp { mods }),
        NSEventType::FlagsChanged => match modifier_of(event.keyCode()) {
            Some(modifier) if held.contains(modifier) => send(Input::Key {
                key: Key::Modifier,
                mods,
            }),
            Some(_) => send(Input::KeyUp { mods }),
            None => Intercepted::Pass,
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

/// The newest form window, for the dev commands that stand for input.
pub(crate) fn front() -> Option<isize> {
    FORMS.with_borrow(|list| list.last().map(|e| e.parts.id))
}

/// Closes the newest form window as its close button does.
pub(crate) fn press_close() {
    let ns = FORMS.with_borrow(|list| list.last().map(|e| e.parts.ns.clone()));
    if let Some(ns) = ns {
        ns.performClose(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_a_form_tells_apart() {
        assert_eq!(key_of(0x35), Some(Key::Escape));
        assert_eq!(key_of(0x30), Some(Key::Hot(shot_core::hotkey::Key::Tab)));
        assert_eq!(
            key_of(0x01),
            Some(Key::Hot(shot_core::hotkey::Key::Char('S')))
        );
        assert_eq!(modifier_of(0x36), Some(NSEventModifierFlags::Command));
        // Caps Lock is no modifier of a hotkey.
        assert_eq!(modifier_of(0x39), None);
    }
}
