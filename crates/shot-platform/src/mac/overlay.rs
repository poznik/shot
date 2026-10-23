//! Overlay windows (SPEC §3.1, §5.4): one borderless panel per screen, created hidden at start and
//! shown at the hotkey over the frozen frame. Each shows a canvas `shot-core` draws, an
//! `IOSurface` the compositor reads in place: a change is drawn into it and announced, never
//! copied. The panels sit above the menu bar and the Dock, on every Space and over full-screen
//! apps.
//!
//! The overlay does not activate Shot: the app the user works in stays active, and the panel
//! takes the keyboard as a non-activating panel does. The mouse and the keys reach the app through
//! [`intercept`], which the event pump asks before AppKit sees an event; the system cursor is
//! hidden, since `shot-core` draws the crosshair.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDidChangeScreenParametersNotification, NSBackingStoreType, NSColor,
    NSEvent, NSEventType, NSPanel, NSResponder, NSScreenSaverWindowLevel, NSStatusWindowLevel,
    NSTrackingArea, NSTrackingAreaOptions, NSWindow, NSWindowAnimationBehavior,
    NSWindowCollectionBehavior, NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_core_graphics::{CGDisplayHideCursor, CGDisplayShowCursor};
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize,
    ns_string,
};
use objc2_quartz_core::CATransaction;
use shot_core::geom::{Point, Rect};
use shot_core::overlay::Event;

use super::monitors::{Screen, screen_at, screens};
use super::pump::{self, Intercepted, push_and_wake};
use super::{Bitmap, Error, capture};

const KEY_ESCAPE: u16 = 53;
const KEY_SPACE: u16 = 49;

thread_local! {
    /// The screens while the overlay is shown and waiting for a choice: input to the panels
    /// becomes overlay events only then.
    static WAITING: RefCell<Option<Vec<Screen>>> = const { RefCell::new(None) };
}

fn waiting() -> bool {
    WAITING.with_borrow(|w| w.is_some())
}

define_class!(
    #[unsafe(super(NSPanel, NSWindow, NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ShotOverlayPanel"]
    struct Panel;

    unsafe impl NSObjectProtocol for Panel {}

    impl Panel {
        /// A borderless window refuses the keyboard unless it says otherwise.
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        /// The keyboard went elsewhere (another app came forward): the overlay closes, as
        /// `Esc` would.
        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            let _: () = unsafe { msg_send![super(self), resignKeyWindow] };
            if waiting() {
                push_and_wake(pump::Event::Overlay(Event::Cancel));
            }
        }
    }
);

pub struct OverlayWindows {
    windows: Vec<Window>,
    cursor_hidden: bool,
    _screens_changed: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

struct Window {
    panel: Retained<Panel>,
    /// What the panel shows, placed at its monitor. `Some` exactly while the panel is shown.
    canvas: Option<Bitmap>,
}

impl OverlayWindows {
    /// Creates a hidden panel for each of `monitors`, kept off the hotkey path.
    pub fn new(monitors: &[Rect]) -> Result<Self, Error> {
        allow_hiding_the_cursor();
        // A screen plugged, unplugged or rearranged: the overlay closes, and the displays are
        // listed again for the next capture.
        let block = RcBlock::new(|_: NonNull<NSNotification>| {
            if waiting() {
                push_and_wake(pump::Event::Overlay(Event::Cancel));
            }
            capture::refresh_displays();
        });
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidChangeScreenParametersNotification),
                None::<&AnyObject>,
                None,
                &block,
            )
        };
        capture::refresh_displays();
        super::window_list::warm_up();
        let mut overlay = OverlayWindows {
            windows: Vec::new(),
            cursor_hidden: false,
            _screens_changed: observer,
        };
        overlay.ensure(monitors)?;
        Ok(overlay)
    }

    /// Makes sure there is a panel for each of `monitors`, after a screen was plugged in.
    pub fn ensure(&mut self, monitors: &[Rect]) -> Result<(), Error> {
        let mtm = MainThreadMarker::new().ok_or_else(|| Error::new("not the main thread"))?;
        while self.windows.len() < monitors.len() {
            self.windows.push(Window {
                panel: create(mtm),
                canvas: None,
            });
        }
        Ok(())
    }

    /// Shows panel `i` with `canvas`, already drawn, over the canvas's rect (a monitor). The
    /// first panel shown takes the keyboard.
    pub fn show(&mut self, i: usize, canvas: Bitmap) {
        let all = screens();
        // The screens changed since the capture: there is nothing to show it on.
        let Some(screen) = all.iter().find(|s| s.monitor.rect == canvas.rect).copied() else {
            return;
        };
        let w = &mut self.windows[i];
        w.panel.setFrame_display(screen.frame, false);
        canvas.unlock();
        if let Some(view) = w.panel.contentView()
            && let Some(layer) = view.layer()
        {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            layer.setContentsScale(f64::from(screen.monitor.scale));
            let surface: &AnyObject = canvas.surface();
            unsafe { layer.setContents(Some(surface)) };
            CATransaction::commit();
        }
        w.panel.orderFrontRegardless();
        let first = !waiting();
        if first {
            w.panel.makeKeyWindow();
            WAITING.set(Some(all));
            if !self.cursor_hidden {
                let _ = CGDisplayHideCursor(0);
                self.cursor_hidden = true;
            }
        }
        CATransaction::flush();
        w.canvas = Some(canvas);
    }

    /// The canvas of panel `i` while it is shown, to draw changes into before [`Self::present`].
    pub fn canvas(&mut self, i: usize) -> Option<&mut Bitmap> {
        let canvas = self.windows.get_mut(i)?.canvas.as_mut()?;
        canvas.lock();
        Some(canvas)
    }

    /// Puts what was drawn into panel `i`'s canvas on screen; `_rect` is where, which the
    /// compositor finds out by itself.
    pub fn present(&self, i: usize, _rect: Rect) {
        let Some(w) = self.windows.get(i) else { return };
        let Some(canvas) = &w.canvas else { return };
        canvas.unlock();
        if let Some(view) = w.panel.contentView()
            && let Some(layer) = view.layer()
        {
            contents_changed(&layer, canvas);
        }
        CATransaction::flush();
    }

    /// Hides every panel, frees the canvases and shows the cursor again.
    pub fn hide(&mut self) {
        WAITING.set(None);
        if std::mem::take(&mut self.cursor_hidden) {
            let _ = CGDisplayShowCursor(0);
        }
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        for w in &mut self.windows {
            if w.canvas.is_some() {
                w.panel.orderOut(None);
                if let Some(view) = w.panel.contentView()
                    && let Some(layer) = view.layer()
                {
                    unsafe { layer.setContents(None) };
                }
            }
        }
        CATransaction::commit();
        CATransaction::flush();
        for w in &mut self.windows {
            w.canvas = None;
        }
    }
}

/// A hidden overlay panel.
fn create(mtm: MainThreadMarker) -> Retained<Panel> {
    let rect = NSRect::new(NSPoint::ZERO, NSSize::new(100.0, 100.0));
    let panel: Retained<Panel> = unsafe {
        msg_send![
            Panel::alloc(mtm),
            initWithContentRect: rect,
            styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setLevel(NSScreenSaverWindowLevel);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(false);
    panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
    panel.setOpaque(true);
    panel.setHasShadow(false);
    panel.setBackgroundColor(Some(&NSColor::blackColor()));
    panel.setAcceptsMouseMovedEvents(true);
    if let Some(view) = panel.contentView() {
        view.setWantsLayer(true);
        // Moves arrive while another app is active, too.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&view),
                None,
            )
        };
        view.addTrackingArea(&area);
    }
    panel
}

/// Tells the compositor the canvas changed in place. `setContentsChanged` is what WebKit and
/// Chromium call for that, though no header declares it; without it the contents are set anew.
pub(crate) fn contents_changed(layer: &objc2_quartz_core::CALayer, canvas: &Bitmap) {
    if layer.respondsToSelector(sel!(setContentsChanged)) {
        let _: () = unsafe { msg_send![layer, setContentsChanged] };
    } else {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe { layer.setContents(None) };
        let surface: &AnyObject = canvas.surface();
        unsafe { layer.setContents(Some(surface)) };
        CATransaction::commit();
    }
}

/// Turns mouse and key events to the overlay panels into overlay events, in physical pixels of
/// the virtual desktop.
pub(crate) fn intercept(event: &NSEvent) -> Intercepted {
    WAITING.with_borrow(|screens| {
        let Some(screens) = screens else {
            return Intercepted::Pass;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return Intercepted::Pass;
        };
        let Some(window) = event.window(mtm) else {
            return Intercepted::Pass;
        };
        if !window.isKindOfClass(Panel::class()) {
            return Intercepted::Pass;
        }
        let at = || {
            let p = window.convertPointToScreen(event.locationInWindow());
            let Some(s) = screen_at(screens, p) else {
                return Point::new(0, 0);
            };
            let (x, y) = s.to_pixels(p);
            let r = s.monitor.rect;
            // The bottom and right edges are on the screen in points but one past it in pixels.
            let pixel = |v: f64, end: i32| (v.floor() as i32).min(end - 1);
            let f = s.frame;
            let on_screen = (f.origin.x..=f.origin.x + f.size.width).contains(&p.x)
                && (f.origin.y..=f.origin.y + f.size.height).contains(&p.y);
            if on_screen {
                Point::new(pixel(x, r.right()), pixel(y, r.bottom()))
            } else {
                // Dragged off every screen: as far as it went.
                Point::new(x.floor() as i32, y.floor() as i32)
            }
        };
        let e = match event.r#type() {
            NSEventType::MouseMoved | NSEventType::LeftMouseDragged => Event::Move(at()),
            NSEventType::LeftMouseDown => Event::Press(at()),
            NSEventType::LeftMouseUp => Event::Release(at()),
            NSEventType::RightMouseDown => Event::Cancel,
            NSEventType::KeyDown => match event.keyCode() {
                KEY_ESCAPE => Event::Cancel,
                KEY_SPACE if !event.isARepeat() => Event::Space,
                _ => return Intercepted::Swallow,
            },
            NSEventType::KeyUp
            | NSEventType::FlagsChanged
            | NSEventType::RightMouseUp
            | NSEventType::RightMouseDragged
            | NSEventType::OtherMouseDown
            | NSEventType::OtherMouseUp
            | NSEventType::OtherMouseDragged
            | NSEventType::ScrollWheel => return Intercepted::Swallow,
            _ => return Intercepted::Pass,
        };
        Intercepted::Event(pump::Event::Overlay(e))
    })
}

/// Waits until macOS reports every shown window of Shot visible, at most 300 ms; for `SHOT_PERF`
/// timings. Only the window server's own events are handled meanwhile.
pub fn wait_for_compose() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    CATransaction::flush();
    let app = NSApplication::sharedApplication(mtm);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
    // The menu-bar icon may be off screen for good, pushed there by a menu-bar manager, and the
    // overlay covers the cards.
    let covered = waiting();
    let visible = || {
        app.windows().iter().all(|w| {
            !w.isVisible()
                || w.level() == NSStatusWindowLevel
                || (covered && w.level() < NSScreenSaverWindowLevel)
                || w.occlusionState().contains(NSWindowOcclusionState::Visible)
        })
    };
    while !visible() && std::time::Instant::now() < deadline {
        let until = NSDate::dateWithTimeIntervalSinceNow(0.001);
        let event = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                objc2_app_kit::NSEventMask::AppKitDefined
                    | objc2_app_kit::NSEventMask::SystemDefined,
                Some(&until),
                NSDefaultRunLoopMode,
                true,
            )
        };
        if let Some(event) = event {
            app.sendEvent(&event);
        }
    }
}

/// Lets Shot hide the cursor while another app is active, which is the case under the overlay.
/// The switch is the window server's own (`SetsCursorInBackground`), not in any header: looked up
/// at run time, and without it the cursor stays visible.
fn allow_hiding_the_cursor() {
    type DefaultConnection = unsafe extern "C" fn() -> i32;
    type SetProperty = unsafe extern "C" fn(i32, i32, *const c_void, *const c_void) -> i32;
    unsafe {
        let default = libc::dlsym(libc::RTLD_DEFAULT, c"_CGSDefaultConnection".as_ptr());
        let set = libc::dlsym(libc::RTLD_DEFAULT, c"CGSSetConnectionProperty".as_ptr());
        if default.is_null() || set.is_null() {
            return;
        }
        let default: DefaultConnection = std::mem::transmute(default);
        let set: SetProperty = std::mem::transmute(set);
        let connection = default();
        let key: *const c_void =
            (ns_string!("SetsCursorInBackground") as *const objc2_foundation::NSString).cast();
        let yes = objc2_foundation::NSNumber::new_bool(true);
        let value: *const c_void = (&*yes as *const objc2_foundation::NSNumber).cast();
        set(connection, connection, key, value);
    }
}
