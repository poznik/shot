//! Driving Shot over ssh, where no input can be sent and the screen must not be captured. With
//! `SHOT_DEV` set, the frames are synthetic (see `capture`) and Shot takes commands as the object
//! of the distributed notification `com.poznik.shot.dev`: `capture` stands for the hotkey, and
//! `move x y`, `press x y`, `release x y`, `space`, `cancel` for input to the overlay, in pixels of
//! the virtual desktop, and `card-enter n`, `card-leave n`, `card-click n` for the mouse on the
//! card in slot `n` (a click in its middle). Input to the newest editor window: `editor-down x y`,
//! `editor-drag x y`, `editor-up x y` in pixels of its content, `editor-key code [mods]` with a
//! macOS key code and modifiers `cmd`, `shift`, `alt`, `ctrl`, and `editor-close` for its close
//! button. `settings` opens the settings window; input to the newest form window: `form-move x y`,
//! `form-click x y`, `form-key code [mods]` (down and up), `form-close`; the folder dialog, in a
//! process of its own, takes `folder-choose` for the folder it shows and `folder-cancel`.
//! `menu n` chooses the `n`-th command of the menu-bar menu, which goes to stderr instead of the
//! screen. `permission on` and `permission off` give and take the Screen Recording permission, as
//! seen by Shot (see `permission`). How long the app took over each command goes to stderr.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::time::Instant;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSModalResponseCancel,
    NSModalResponseOK,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSOperationQueue, NSPoint, NSString, ns_string,
};
use shot_core::geom::Point;
use shot_core::overlay;

use super::pump::{Event, Intercepted, post, push_and_wake};
use super::tray::TrayEvent;
use super::{autostart, capture, editor, form, permission, preview, tray};

/// Which of Shot's windows a command stands for input to.
#[derive(Clone, Copy)]
enum Target {
    Editor,
    Form,
}

impl Target {
    /// The newest such window.
    fn front(self) -> Option<isize> {
        match self {
            Target::Editor => editor::front(),
            Target::Form => form::front(),
        }
    }

    fn intercept(self, event: &NSEvent) -> Intercepted {
        match self {
            Target::Editor => editor::intercept(event),
            Target::Form => form::intercept(event),
        }
    }
}

thread_local! {
    static OBSERVER: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> =
        const { RefCell::new(None) };
    /// The command handed to the app, and when.
    static PENDING: Cell<Option<(Instant, &'static str)>> = const { Cell::new(None) };
}

/// Starts taking commands when `SHOT_DEV` is set.
pub(crate) fn listen() {
    if !capture::dev() {
        return;
    }
    if autostart::registered() {
        eprintln!("dev: this Shot is a login item");
    }
    if std::env::var_os("SHOT_DEV_NO_PERMISSION").is_some() {
        permission::dev_grant(false);
    }
    let block = RcBlock::new(|n: NonNull<NSNotification>| {
        // SAFETY: the center hands a valid notification.
        let n = unsafe { n.as_ref() };
        let Some(command) = n.object().and_then(|o| o.downcast::<NSString>().ok()) else {
            return;
        };
        let t = Instant::now();
        let command = command.to_string();
        if command.starts_with("folder-") {
            // For the folder dialog's own process.
            return;
        }
        match parse(&command) {
            Some((what, event)) => {
                PENDING.set(Some((t, what)));
                push_and_wake(event);
            }
            None => eprintln!("dev: what is {command:?}?"),
        }
    });
    let observer = unsafe {
        NSDistributedNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(ns_string!("com.poznik.shot.dev")),
            None::<&AnyObject>,
            None,
            &block,
        )
    };
    OBSERVER.set(Some(observer));
}

fn parse(command: &str) -> Option<(&'static str, Event)> {
    let mut words = command.split_whitespace();
    let name = words.next()?;
    let mut at = || -> Option<Point> {
        Some(Point::new(
            words.next()?.parse().ok()?,
            words.next()?.parse().ok()?,
        ))
    };
    Some(match name {
        "capture" => ("capture", Event::Hotkey { queued_ms: 0 }),
        "move" => ("move", Event::Overlay(overlay::Event::Move(at()?))),
        "press" => ("press", Event::Overlay(overlay::Event::Press(at()?))),
        "release" => ("release", Event::Overlay(overlay::Event::Release(at()?))),
        "space" => ("space", Event::Overlay(overlay::Event::Space)),
        "cancel" => ("cancel", Event::Overlay(overlay::Event::Cancel)),
        "card-enter" => (
            "card-enter",
            card(words.next()?, NSEventType::MouseEntered)?,
        ),
        "card-leave" => ("card-leave", card(words.next()?, NSEventType::MouseExited)?),
        "card-click" => ("card-click", card(words.next()?, NSEventType::LeftMouseUp)?),
        "editor-down" => (
            "editor-down",
            editor_mouse(NSEventType::LeftMouseDown, at()?)?,
        ),
        "editor-drag" => (
            "editor-drag",
            editor_mouse(NSEventType::LeftMouseDragged, at()?)?,
        ),
        "editor-up" => ("editor-up", editor_mouse(NSEventType::LeftMouseUp, at()?)?),
        "editor-key" => {
            let code = words.next()?.parse().ok()?;
            let flags = modifiers(words)?;
            (
                "editor-key",
                key(Target::Editor, NSEventType::KeyDown, code, flags)?,
            )
        }
        "editor-close" => {
            editor::press_close();
            ("editor-close", Event::Other)
        }
        "settings" => ("settings", Event::Tray(TrayEvent::Wake)),
        "menu" => {
            tray::dev_pick(words.next()?.parse().ok()?);
            ("menu", Event::Tray(TrayEvent::Menu(Point::new(0, 0))))
        }
        "permission" => {
            permission::dev_grant(match words.next()? {
                "on" => true,
                "off" => false,
                _ => return None,
            });
            // As when the user comes back from the system settings.
            ("permission", Event::Activated)
        }
        "form-move" => (
            "form-move",
            mouse(Target::Form, NSEventType::MouseMoved, at()?)?,
        ),
        "form-click" => {
            let p = at()?;
            mouse(Target::Form, NSEventType::LeftMouseDown, p)?;
            (
                "form-click",
                mouse(Target::Form, NSEventType::LeftMouseUp, p)?,
            )
        }
        "form-key" => {
            let code = words.next()?.parse().ok()?;
            let flags = modifiers(words)?;
            let down = key(Target::Form, NSEventType::KeyDown, code, flags)?;
            // The key may have closed the window: then its release has nowhere to go.
            let _ = key(Target::Form, NSEventType::KeyUp, code, flags);
            ("form-key", down)
        }
        "form-close" => {
            form::press_close();
            ("form-close", Event::Other)
        }
        _ => return None,
    })
}

/// The modifiers named by `words`.
fn modifiers<'a>(words: impl Iterator<Item = &'a str>) -> Option<NSEventModifierFlags> {
    let mut flags = NSEventModifierFlags(0);
    for word in words {
        flags |= match word {
            "cmd" => NSEventModifierFlags::Command,
            "shift" => NSEventModifierFlags::Shift,
            "alt" => NSEventModifierFlags::Option,
            "ctrl" => NSEventModifierFlags::Control,
            _ => return None,
        };
    }
    Some(flags)
}

/// The mouse event `kind` in the middle of the card in `slot`, through the cards' own handling of
/// the mouse.
fn card(slot: &str, kind: NSEventType) -> Option<Event> {
    let number = preview::card_window(slot.parse().ok()?)?;
    let mtm = MainThreadMarker::new()?;
    let window = NSApplication::sharedApplication(mtm).windowWithWindowNumber(number)?;
    let size = window.frame().size;
    let at = NSPoint::new(size.width / 2.0, size.height / 2.0);
    let flags = NSEventModifierFlags(0);
    let event = if kind == NSEventType::LeftMouseUp {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, at, flags, 0.0, number, None, 0, 1, 0.0,
        )
    } else {
        unsafe {
            NSEvent::enterExitEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_trackingNumber_userData(
                kind, at, flags, 0.0, number, None, 0, 0, std::ptr::null_mut(),
            )
        }
    }?;
    match preview::intercept(&event) {
        Intercepted::Event(e) => Some(e),
        _ => None,
    }
}

/// Hands `event` to `target` as the pump would.
fn deliver(target: Target, event: Option<Retained<NSEvent>>) -> Option<Event> {
    let event = event?;
    match target.intercept(&event) {
        Intercepted::Swallow => Some(Event::Other),
        _ => None,
    }
}

fn editor_mouse(kind: NSEventType, at: Point) -> Option<Event> {
    mouse(Target::Editor, kind, at)
}

/// The mouse event `kind` at pixel `at` of the content of the newest `target` window.
fn mouse(target: Target, kind: NSEventType, at: Point) -> Option<Event> {
    let number = target.front()?;
    let mtm = MainThreadMarker::new()?;
    let window = NSApplication::sharedApplication(mtm).windowWithWindowNumber(number)?;
    let scale = window.backingScaleFactor();
    let height = window.contentView()?.bounds().size.height;
    let p = NSPoint::new(f64::from(at.x) / scale, height - f64::from(at.y) / scale);
    deliver(target, NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        kind, p, NSEventModifierFlags(0), 0.0, number, None, 0, 1, 1.0,
    ))
}

/// Key `code` going down or up, `kind`, in the newest `target` window.
fn key(target: Target, kind: NSEventType, code: u16, flags: NSEventModifierFlags) -> Option<Event> {
    let number = target.front()?;
    let none = NSString::new();
    deliver(target, NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        kind, NSPoint::ZERO, flags, 0.0, number, None, &none, &none, false, code,
    ))
}

/// In the process that shows the folder dialog: `folder-choose` presses the button that chooses
/// the folder the panel shows, `folder-cancel` its "Cancel". Another process draws the panel, and
/// it takes no `ok:` here.
pub(crate) fn listen_panel() -> Option<Retained<ProtocolObject<dyn NSObjectProtocol>>> {
    if !capture::dev() {
        return None;
    }
    let block = RcBlock::new(|n: NonNull<NSNotification>| {
        // SAFETY: the center hands a valid notification.
        let n = unsafe { n.as_ref() };
        let Some(command) = n.object().and_then(|o| o.downcast::<NSString>().ok()) else {
            return;
        };
        let code = match command.to_string().as_str() {
            "folder-choose" => NSModalResponseOK,
            "folder-cancel" => NSModalResponseCancel,
            _ => return,
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        NSApplication::sharedApplication(mtm).stopModalWithCode(code);
        // The modal loop sees the stop with its next event.
        post(0);
    });
    Some(unsafe {
        NSDistributedNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(ns_string!("com.poznik.shot.dev")),
            None::<&AnyObject>,
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    })
}

/// Called when the app asks for its next event: the command before it is handled.
pub(crate) fn report() {
    if let Some((t, what)) = PENDING.take() {
        eprintln!(
            "dev: {what} handled in {:.2} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
}
