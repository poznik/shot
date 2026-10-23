//! The main thread's event loop. AppKit's events are pulled one at a time and sent to their
//! windows; what concerns the app goes into a queue that [`next_event`] hands out. Other threads
//! wake the loop by posting an application-defined event, and timers are deadlines the wait ends
//! at: nothing runs while Shot is idle.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Instant;

use objc2::ffi::{objc_autoreleasePoolPop, objc_autoreleasePoolPush};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, class, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSEvent, NSEventMask,
    NSEventModifierFlags, NSEventType,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSNotification, NSPoint};
use shot_core::overlay;

use super::preview::{self, PreviewEvent};
use super::tray::TrayEvent;

/// What the app gets from [`next_event`].
#[derive(Debug)]
pub enum Event {
    /// The capture hotkey, which waited about `queued_ms` before it got here.
    Hotkey {
        queued_ms: u32,
    },
    Tray(TrayEvent),
    Preview(PreviewEvent),
    Overlay(overlay::Event),
    /// A file in the watched folder changed.
    Folder,
    /// Shot became the active app: the user may come back from the system settings.
    Activated,
    /// An event went to a window or the menu bar; nothing changed for the app.
    Other,
    Quit,
}

/// What the pump does with an input event, as Shot's windows decide before AppKit sees it.
pub(crate) enum Intercepted {
    /// Not theirs: AppKit handles it.
    Pass,
    /// Input to them that means nothing.
    Swallow,
    Event(Event),
}

/// Marks Shot's own application-defined events; `data1` is the callback to run.
const SUBTYPE: i16 = 0x5348;

/// Shot's windows that take their input before AppKit sees it, asked in this order.
const INTERCEPTS: [fn(&NSEvent) -> Intercepted; 4] = [
    super::overlay::intercept,
    preview::intercept,
    super::editor::intercept,
    super::form::intercept,
];

thread_local! {
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    static CALLBACKS: RefCell<HashMap<isize, Rc<dyn Fn()>>> = RefCell::new(HashMap::new());
    static NEXT_KEY: Cell<isize> = const { Cell::new(1) };
    static TIMERS: RefCell<HashMap<isize, Instant>> = RefCell::new(HashMap::new());
    static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
    /// The autorelease pool the app's handling of the last event runs in.
    static POOL: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        /// Shot.app opened again while it runs: as a second launch on Windows.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            push(Event::Tray(TrayEvent::Wake));
            false
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _notification: &NSNotification) {
            push_and_wake(Event::Activated);
        }
    }
);

/// Makes Shot an agent app (no Dock icon, no menu of its own) and finishes AppKit's launch. Call
/// first, on the main thread.
pub fn init() {
    let mtm = MainThreadMarker::new().expect("Shot runs on the main thread");
    // The start, up to the first event, runs inside a pool as the handling of each event does:
    // AppKit keeps the first window it shows alive otherwise.
    // SAFETY: popped by the first call to `next_event`, on this thread.
    POOL.set(unsafe { objc_autoreleasePoolPush() });
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(mtm), init] };
    app.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|d| *d.borrow_mut() = Some(delegate));
    app.finishLaunching();
    super::dev::listen();
}

/// Queues an event for the app; on the main thread.
pub(crate) fn push(event: Event) {
    QUEUE.with(|q| q.borrow_mut().push_back(event));
}

/// Queues an event from a callback that AppKit runs outside of an event (a Carbon handler, a
/// notification): the wait has to end for the app to see it.
pub(crate) fn push_and_wake(event: Event) {
    push(event);
    post(0);
}

/// Registers `f` to run on the main thread when [`post`] is called with the key returned, from any
/// thread.
pub(crate) fn register(f: Rc<dyn Fn()>) -> isize {
    let key = NEXT_KEY.with(|k| {
        let key = k.get();
        k.set(key + 1);
        key
    });
    CALLBACKS.with(|c| c.borrow_mut().insert(key, f));
    key
}

pub(crate) fn unregister(key: isize) {
    CALLBACKS.with(|c| c.borrow_mut().remove(&key));
    set_timer(key, None);
}

/// Has the main thread run the callback of `key`; any thread. Key 0 only ends the wait.
pub(crate) fn post(key: isize) {
    let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::ZERO,
        NSEventModifierFlags(0),
        0.0,
        0,
        None,
        SUBTYPE,
        key,
        0,
    );
    let Some(event) = event else { return };
    // SAFETY: `postEvent:atStart:` may be called from any thread (AppKit documents it); the shared
    // application exists since `init`.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, postEvent: &*event, atStart: false];
    }
}

/// Runs the callback of `key` at `at`, or never with `None`.
pub(crate) fn set_timer(key: isize, at: Option<Instant>) {
    TIMERS.with(|t| match at {
        Some(at) => t.borrow_mut().insert(key, at),
        None => t.borrow_mut().remove(&key),
    });
}

fn call(key: isize) {
    let f = CALLBACKS.with(|c| c.borrow().get(&key).cloned());
    if let Some(f) = f {
        f();
    }
}

/// Runs the callbacks whose time has come. Returns when the next one is due.
fn run_timers() -> Option<Instant> {
    let now = Instant::now();
    let due: Vec<isize> = TIMERS.with(|t| {
        let mut t = t.borrow_mut();
        let due: Vec<isize> = t
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(k, _)| *k)
            .collect();
        for k in &due {
            t.remove(k);
        }
        due
    });
    for key in due {
        call(key);
    }
    TIMERS.with(|t| t.borrow().values().min().copied())
}

/// Waits for the next thing for the app. Window events are sent to their windows on the way;
/// after each, [`Event::Other`] gives the app a turn when nothing else is queued.
///
/// The app handles each event inside an autorelease pool, drained at the next call: with no
/// `NSApplicationMain` around it, what AppKit autoreleases there would never be freed (the
/// pasteboard's copy of a full-screen capture among it).
pub fn next_event() -> Event {
    let pool = POOL.replace(std::ptr::null_mut());
    if !pool.is_null() {
        // SAFETY: pushed on this thread by the previous call; pools inside it are drained.
        unsafe { objc_autoreleasePoolPop(pool) };
    }
    let event = wait();
    // SAFETY: popped by the next call, on this thread.
    POOL.set(unsafe { objc_autoreleasePoolPush() });
    event
}

fn wait() -> Event {
    let mtm = MainThreadMarker::new().expect("Shot runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    super::dev::report();
    // What runs here, outside the app's pool, autoreleases into pools of its own.
    autoreleasepool(|_| super::titled::after_turn());
    loop {
        if let Some(event) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
            return event;
        }
        let next = autoreleasepool(|_| run_timers());
        if QUEUE.with(|q| !q.borrow().is_empty()) {
            continue;
        }
        let handled = autoreleasepool(|_| {
            let until = match next {
                Some(at) => NSDate::dateWithTimeIntervalSinceNow(
                    at.saturating_duration_since(Instant::now()).as_secs_f64(),
                ),
                None => NSDate::distantFuture(),
            };
            let event = unsafe {
                app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&until),
                    NSDefaultRunLoopMode,
                    true,
                )
            };
            let Some(event) = event else {
                return false;
            };
            if event.r#type() == NSEventType::ApplicationDefined && event.subtype().0 == SUBTYPE {
                call(event.data1());
            } else {
                let intercepted = INTERCEPTS
                    .iter()
                    .map(|intercept| intercept(&event))
                    .find(|i| !matches!(i, Intercepted::Pass))
                    .unwrap_or(Intercepted::Pass);
                match intercepted {
                    Intercepted::Pass => app.sendEvent(&event),
                    Intercepted::Swallow => {}
                    Intercepted::Event(e) => push(e),
                }
            }
            true
        });
        if handled && QUEUE.with(|q| q.borrow().is_empty()) {
            return Event::Other;
        }
    }
}

/// Ends the loop: [`next_event`] returns [`Event::Quit`].
pub(crate) fn quit() {
    push(Event::Quit);
}
