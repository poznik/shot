//! The menu-bar icon and its menu. A click opens the menu, which the app fills each time: the
//! click comes to the app as [`TrayEvent::Menu`], and [`Tray::menu`] shows the items it gets.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBitmapImageRep,
    NSControlStateValueOff, NSControlStateValueOn, NSEventMask, NSEventModifierFlags, NSImage,
    NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString};
use shot_core::geom::Point;
use shot_core::tray::{Command, Item};

use super::pump::{Event, push};
use super::{Error, capture};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// `Снять` chosen in the menu a moment ago.
    Capture,
    /// A click on the icon: show the menu with [`Tray::menu`].
    Menu(Point),
    /// Shot was launched again.
    Wake,
}

thread_local! {
    /// The tag of the item chosen in the open menu.
    static CHOSEN: Cell<Option<isize>> = const { Cell::new(None) };
    /// With `SHOT_DEV`: the command the next menu chooses, by its place, instead of showing.
    static DEV_PICK: Cell<Option<usize>> = const { Cell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(clicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            push(Event::Tray(TrayEvent::Menu(Point::new(0, 0))));
        }

        #[unsafe(method(chose:))]
        fn chose(&self, sender: &NSMenuItem) {
            CHOSEN.set(Some(sender.tag()));
        }
    }
);

pub struct Tray {
    item: Retained<NSStatusItem>,
    target: Retained<Target>,
    normal: Retained<NSImage>,
    alert_image: Retained<NSImage>,
    alert: bool,
}

/// A template image from the 1x and 2x PNGs: AppKit tints it for the menu bar.
fn template(png: &[u8], png2x: &[u8]) -> Retained<NSImage> {
    let size = NSSize::new(18.0, 18.0);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    for data in [png, png2x] {
        if let Some(rep) = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(data)) {
            rep.setSize(size);
            image.addRepresentation(&rep);
        }
    }
    image.setTemplate(true);
    image
}

impl Tray {
    /// `tooltip` is for Windows; the menu bar has none.
    pub fn new(_tooltip: &str, alert: bool) -> Result<Self, Error> {
        let mtm = MainThreadMarker::new().ok_or_else(|| Error::new("not the main thread"))?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let target: Retained<Target> = unsafe { msg_send![Target::alloc(mtm), init] };
        let normal = template(
            include_bytes!("../../../../assets/tray/macos/trayTemplate.png"),
            include_bytes!("../../../../assets/tray/macos/trayTemplate@2x.png"),
        );
        let alert_image = template(
            include_bytes!("../../../../assets/tray/macos/trayAlertTemplate.png"),
            include_bytes!("../../../../assets/tray/macos/trayAlertTemplate@2x.png"),
        );
        let button = item
            .button(mtm)
            .ok_or_else(|| Error::new("no status item button"))?;
        button.setImage(Some(if alert { &alert_image } else { &normal }));
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(clicked:)));
        }
        // The menu opens on the press, as every menu-bar menu does.
        button.sendActionOn(NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown);
        Ok(Tray {
            item,
            target,
            normal,
            alert_image,
            alert,
        })
    }

    pub fn set(&mut self, _tooltip: &str, alert: bool) {
        if self.alert == alert {
            return;
        }
        self.alert = alert;
        if capture::dev() {
            eprintln!("dev: the menu-bar icon alerts: {alert}");
        }
        if let Some(mtm) = MainThreadMarker::new()
            && let Some(button) = self.item.button(mtm)
        {
            button.setImage(Some(if alert {
                &self.alert_image
            } else {
                &self.normal
            }));
        }
    }

    /// Shows `items` under the icon until one is chosen or the menu is dismissed. `Снять` also
    /// comes as [`TrayEvent::Capture`] once the menu is gone.
    pub fn menu(&self, items: &[Item], _at: Point) -> Option<Command> {
        if let Some(n) = DEV_PICK.take() {
            return dev_menu(items, n);
        }
        let mtm = MainThreadMarker::new()?;
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::new());
        menu.setAutoenablesItems(false);
        let mut commands = Vec::new();
        for item in items {
            match item {
                Item::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                Item::Command {
                    command,
                    label,
                    shortcut,
                    checked,
                    enabled,
                    ..
                } => {
                    let (key, mods) = shortcut
                        .as_deref()
                        .map(key_equivalent)
                        .unwrap_or((String::new(), NSEventModifierFlags(0)));
                    let entry = unsafe {
                        NSMenuItem::initWithTitle_action_keyEquivalent(
                            NSMenuItem::alloc(mtm),
                            &NSString::from_str(label),
                            Some(sel!(chose:)),
                            &NSString::from_str(&key),
                        )
                    };
                    entry.setKeyEquivalentModifierMask(mods);
                    unsafe { entry.setTarget(Some(&self.target)) };
                    entry.setTag(commands.len() as isize);
                    entry.setEnabled(*enabled);
                    entry.setState(if *checked {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                    menu.addItem(&entry);
                    commands.push(*command);
                }
            }
        }
        CHOSEN.set(None);
        // With a menu set, a click on the button opens it as AppKit's own; it returns when the
        // menu closes, after the chosen item's action.
        self.item.setMenu(Some(&menu));
        if let Some(button) = self.item.button(mtm) {
            unsafe { button.performClick(None) };
        }
        self.item.setMenu(None);
        let chosen = CHOSEN
            .take()
            .and_then(|i| commands.get(i as usize).copied());
        if chosen == Some(Command::Capture) {
            push(Event::Tray(TrayEvent::Capture));
        }
        chosen
    }
}

/// With `SHOT_DEV`: the next menu chooses its `n`-th command.
pub(crate) fn dev_pick(n: usize) {
    DEV_PICK.set(Some(n));
}

/// Prints the menu to stderr instead of showing it, and chooses its `n`-th command if that one is
/// enabled.
fn dev_menu(items: &[Item], n: usize) -> Option<Command> {
    let commands: Vec<(Command, &str, bool)> = items
        .iter()
        .filter_map(|item| match item {
            Item::Command {
                command,
                label,
                enabled,
                ..
            } => Some((*command, *label, *enabled)),
            Item::Separator => None,
        })
        .collect();
    for (i, (_, label, enabled)) in commands.iter().enumerate() {
        let off = if *enabled { "" } else { " (disabled)" };
        eprintln!("dev: menu {i}: {label}{off}");
    }
    let (command, _, enabled) = *commands.get(n)?;
    if !enabled {
        return None;
    }
    if command == Command::Capture {
        push(Event::Tray(TrayEvent::Capture));
    }
    Some(command)
}

/// `⇧⌘2` as AppKit's key equivalent: the key, lower case, and its modifiers.
fn key_equivalent(shortcut: &str) -> (String, NSEventModifierFlags) {
    let mut mods = NSEventModifierFlags(0);
    let mut key = String::new();
    for c in shortcut.chars() {
        match c {
            '\u{2303}' => mods |= NSEventModifierFlags::Control,
            '\u{2325}' => mods |= NSEventModifierFlags::Option,
            '\u{21E7}' => mods |= NSEventModifierFlags::Shift,
            '\u{2318}' => mods |= NSEventModifierFlags::Command,
            c => key.push(c.to_ascii_lowercase()),
        }
    }
    (key, mods)
}

/// `None` follows the system; otherwise Shot's menus and window frames are light or dark.
pub fn set_menu_theme(dark: Option<bool>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let appearance = dark.and_then(|dark| {
        let name = unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        NSAppearance::appearanceNamed(name)
    });
    NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
}
