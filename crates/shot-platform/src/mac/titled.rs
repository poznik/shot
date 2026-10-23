//! What the editor and the form windows share: ordinary titled windows whose content is a layer
//! showing a back buffer that `shot-core` draws, one delegate that tells of the close button,
//! resizes and the lost focus, their theme, and Shot's activation.
//!
//! Shot stays an agent app (SPEC §5.4): a window takes the keyboard by activating Shot, and the
//! app the user worked in gets it back when Shot's last such window closes.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSApplicationActivationOptions, NSBackingStoreType, NSCursor, NSEvent,
    NSRunningApplication, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowDelegate, NSWindowStyleMask, NSWindowTabbingMode,
    NSWorkspace,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSPoint, NSRect, NSString, ns_string,
};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityTopLeft};
use shot_core::geom::Rect;
use shot_core::image::BgrxMut;

use super::overlay::contents_changed;
use super::system::apps_use_dark_theme;
use super::{Bitmap, Error, editor, form};

thread_local! {
    static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
    /// The app that was active when a window made Shot active.
    static PREVIOUS: RefCell<Option<Retained<NSRunningApplication>>> = const { RefCell::new(None) };
    static THEME_OBSERVER: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> =
        const { RefCell::new(None) };
    /// A window closed: the keyboard goes back once the app had its turn.
    static GIVE_BACK: Cell<bool> = const { Cell::new(false) };
}

/// What the delegate tells of a window.
#[derive(Clone, Copy)]
pub(super) enum Note {
    /// The close button, or ⌘W from a menu: the window's owner decides.
    Close,
    /// The content may have a new size or scale.
    Resized,
    /// The window lost the keyboard.
    Blur,
}

fn route(id: isize, note: Note) {
    if !editor::note(id, note) {
        form::note(id, note);
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ShotWindowDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, sender: &NSWindow) -> bool {
            route(sender.windowNumber(), Note::Close);
            false
        }

        #[unsafe(method(windowDidResize:))]
        fn did_resize(&self, notification: &NSNotification) {
            if let Some(id) = window_of(notification) {
                route(id, Note::Resized);
            }
        }

        /// Moved to a screen of another scale.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn did_change_backing(&self, notification: &NSNotification) {
            if let Some(id) = window_of(notification) {
                route(id, Note::Resized);
            }
        }

        #[unsafe(method(windowDidResignKey:))]
        fn did_resign_key(&self, notification: &NSNotification) {
            if let Some(id) = window_of(notification) {
                route(id, Note::Blur);
            }
        }
    }
);

fn window_of(notification: &NSNotification) -> Option<isize> {
    let window = notification.object()?.downcast::<NSWindow>().ok()?;
    Some(window.windowNumber())
}

/// A titled window whose content is a layer-hosting view: AppKit leaves the layer's contents to
/// Shot.
pub(super) struct Parts {
    pub ns: Retained<NSWindow>,
    /// The window number, which events carry.
    pub id: isize,
    pub view: Retained<NSView>,
    /// Moves arrive while another app is active, too, as on Windows.
    pub area: Retained<NSTrackingArea>,
}

/// A hidden window at `frame`, in points, with the buttons of `style`; its layer goes to a
/// [`Surface`].
pub(super) fn window(
    mtm: MainThreadMarker,
    frame: NSRect,
    style: NSWindowStyleMask,
    title: &str,
    dark: bool,
    follow_system: bool,
) -> Result<(Parts, Retained<CALayer>), Error> {
    let ns = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    let id = ns.windowNumber();
    if id <= 0 {
        return Err(Error::new("the window has no number"));
    }
    unsafe { ns.setReleasedWhenClosed(false) };
    ns.setRestorable(false);
    ns.setTabbingMode(NSWindowTabbingMode::Disallowed);
    // Opens at once, without the zoom of a new document window.
    ns.setAnimationBehavior(NSWindowAnimationBehavior::None);
    ns.setTitle(&NSString::from_str(title));
    ns.setAppearance(appearance(dark, follow_system).as_deref());

    let view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::ZERO, frame.size));
    let layer = CALayer::new();
    layer.setContentsGravity(unsafe { kCAGravityTopLeft });
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    ns.setContentView(Some(&view));
    let area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(&view),
            None,
        )
    };
    view.addTrackingArea(&area);
    let delegate = DELEGATE.with_borrow_mut(|d| {
        d.get_or_insert_with(|| unsafe { msg_send![Delegate::alloc(mtm), init] })
            .clone()
    });
    ns.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    watch_theme();
    Ok((Parts { ns, id, view, area }, layer))
}

/// Brings the window to the front with the keyboard.
pub(super) fn show(mtm: MainThreadMarker, ns: &NSWindow) {
    activate(mtm);
    if ns.isMiniaturized() {
        ns.deminiaturize(None);
    }
    ns.makeKeyAndOrderFront(None);
    CATransaction::flush();
}

/// Takes the window off the screen; when it was Shot's last, the app the user worked in gets the
/// keyboard back.
pub(super) fn close(parts: Parts, cursor_inside: bool) {
    parts.view.removeTrackingArea(&parts.area);
    parts.ns.setDelegate(None);
    parts.ns.orderOut(None);
    parts.ns.close();
    if cursor_inside {
        NSCursor::arrowCursor().set();
    }
    // AppKit may be inside a call to the window (its close button): it goes when the event is
    // handled.
    let _ = Retained::autorelease_ptr(parts.ns);
    GIVE_BACK.set(true);
}

/// Called when the app asks for its next event. The app may have opened the next window in its
/// turn (the welcome hands over to the settings): the keyboard goes back only if none is open.
pub(super) fn after_turn() {
    if GIVE_BACK.take()
        && !editor::any_open()
        && !form::any_open()
        && let Some(mtm) = MainThreadMarker::new()
    {
        give_back(mtm);
    }
}

/// The content's size in px and its scale.
pub(super) fn content_px(ns: &NSWindow, view: &NSView) -> (u32, u32, f32) {
    let scale = ns.backingScaleFactor();
    let bounds = view.bounds();
    let w = (bounds.size.width * scale).round() as u32;
    let h = (bounds.size.height * scale).round() as u32;
    (w, h, scale as f32)
}

/// Where `event` is in px of `view`'s content at `scale`, top-left origin, and whether it is over
/// the content.
pub(super) fn at(view: &NSView, event: &NSEvent, scale: f32) -> (f32, f32, bool) {
    let bounds = view.bounds();
    let p = view.convertPoint_fromView(event.locationInWindow(), None);
    let inside =
        (0.0..bounds.size.width).contains(&p.x) && (0.0..bounds.size.height).contains(&p.y);
    let scale = f64::from(scale);
    (
        (p.x * scale) as f32,
        ((bounds.size.height - p.y) * scale) as f32,
        inside,
    )
}

/// A click on a window that does not have the keyboard gives it the keyboard; the click also goes
/// to the content, as on Windows.
pub(super) fn focus_on_click(event: &NSEvent) {
    if let Some(mtm) = MainThreadMarker::new()
        && let Some(window) = event.window(mtm)
        && !window.isKeyWindow()
    {
        activate(mtm);
        window.makeKeyAndOrderFront(None);
    }
}

/// The back buffer, an `IOSurface` the size of the content in px, and the layer that shows it:
/// the handler draws the changed parts, and the compositor reads them in place, also while the
/// window is moved or sized.
pub(super) struct Surface {
    layer: Retained<CALayer>,
    back: Option<Bitmap>,
    scale: f32,
    /// The layer shows `back`; a new one is shown at its first [`Self::present`].
    shown: Cell<bool>,
}

impl Surface {
    pub fn new(layer: Retained<CALayer>) -> Self {
        Surface {
            layer,
            back: None,
            scale: 1.0,
            shown: Cell::new(false),
        }
    }

    /// A new back buffer of `width`×`height` px, shown at `scale`.
    pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
        self.back = None;
        self.back = Bitmap::new(Rect::new(0, 0, width as i32, height as i32)).ok();
        self.scale = scale;
        self.shown.set(false);
    }

    pub fn canvas(&mut self) -> Option<BgrxMut<'_>> {
        let back = self.back.as_mut()?;
        back.lock();
        Some(back.view_mut())
    }

    pub fn present(&self) {
        let Some(back) = &self.back else { return };
        back.unlock();
        if self.shown.replace(true) {
            contents_changed(&self.layer, back);
        } else {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            self.layer.setContentsScale(f64::from(self.scale));
            let surface: &AnyObject = back.surface();
            unsafe { self.layer.setContents(Some(surface)) };
            CATransaction::commit();
        }
        CATransaction::flush();
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe { self.layer.setContents(None) };
        CATransaction::commit();
    }
}

/// The window's own appearance: none when it follows the app's, which follows the system.
pub(super) fn appearance(dark: bool, follow_system: bool) -> Option<Retained<NSAppearance>> {
    if follow_system {
        return None;
    }
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    NSAppearance::appearanceNamed(name)
}

/// Starts listening for the system theme, once; the windows that follow it are told.
fn watch_theme() {
    if THEME_OBSERVER.with_borrow(Option::is_some) {
        return;
    }
    let block = RcBlock::new(|_: NonNull<NSNotification>| {
        let dark = apps_use_dark_theme();
        editor::system_theme(dark);
        form::system_theme(dark);
    });
    let observer = unsafe {
        NSDistributedNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(ns_string!("AppleInterfaceThemeChangedNotification")),
            None::<&AnyObject>,
            None,
            &block,
        )
    };
    THEME_OBSERVER.set(Some(observer));
}

/// Makes Shot the active app. A click on a card does not, the card being a panel that leaves the
/// user's app active, and `activate()` waits for that app to yield, which it does not.
#[allow(deprecated)]
fn activate(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    if !app.isActive() {
        let front = NSWorkspace::sharedWorkspace().frontmostApplication();
        // The folder dialog's own Shot hands the keyboard back here: the app before it stays.
        if !front.as_ref().is_some_and(|front| same_program(front)) {
            PREVIOUS.set(front);
        }
    }
    app.activateIgnoringOtherApps(true);
}

/// Whether `app` runs Shot's own executable.
fn same_program(app: &NSRunningApplication) -> bool {
    let own = NSRunningApplication::currentApplication().executableURL();
    app.executableURL().is_some_and(|url| Some(url) == own)
}

/// Gives the keyboard back to the app the user worked in. `deactivate()` alone leaves no app
/// active.
fn give_back(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    let previous = PREVIOUS.take();
    if !app.isActive() {
        return;
    }
    match previous {
        Some(previous) if !previous.isTerminated() => {
            previous.activateWithOptions(NSApplicationActivationOptions::empty());
        }
        _ => app.deactivate(),
    }
}
