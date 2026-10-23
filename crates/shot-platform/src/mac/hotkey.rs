//! The global hotkey through Carbon's `RegisterEventHotKey`: no Accessibility permission, no event
//! tap. Since macOS 15 a hotkey of only Option and Shift does not register; `shot-core` asks for
//! Cmd or Ctrl in it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Once;

use shot_core::hotkey::{Hotkey, Key};

use super::Error;
use super::pump::{Event, push_and_wake};

type OsStatus = i32;
type EventRef = *mut c_void;
type EventTargetRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type Handler = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OsStatus;

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: Handler,
        count: u32,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut EventHandlerRef,
    ) -> OsStatus;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: EventTargetRef,
        options: u32,
        out: *mut EventHotKeyRef,
    ) -> OsStatus;
    fn UnregisterEventHotKey(hotkey: EventHotKeyRef) -> OsStatus;
    fn GetEventParameter(
        event: EventRef,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> OsStatus;
    fn GetEventTime(event: EventRef) -> f64;
    fn GetCurrentEventTime() -> f64;
}

const fn four(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SIGNATURE: u32 = four(b"Shot");
const K_EVENT_CLASS_KEYBOARD: u32 = four(b"keyb");
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
const K_EVENT_PARAM_DIRECT_OBJECT: u32 = four(b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = four(b"hkid");
const CMD_KEY: u32 = 1 << 8;
const SHIFT_KEY: u32 = 1 << 9;
const OPTION_KEY: u32 = 1 << 11;
const CONTROL_KEY: u32 = 1 << 12;
const EVENT_HOT_KEY_EXISTS_ERR: OsStatus = -9878;

thread_local! {
    static REGISTERED: RefCell<HashMap<i32, usize>> = RefCell::new(HashMap::new());
}

extern "C" fn on_hotkey(_: EventHandlerCallRef, event: EventRef, _: *mut c_void) -> OsStatus {
    let mut id = EventHotKeyId::default();
    let status = unsafe {
        GetEventParameter(
            event,
            K_EVENT_PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            std::ptr::null_mut(),
            size_of::<EventHotKeyId>(),
            std::ptr::null_mut(),
            (&raw mut id).cast(),
        )
    };
    if status == 0 && id.signature == SIGNATURE {
        // Both clocks count seconds since boot.
        let waited = unsafe { GetCurrentEventTime() - GetEventTime(event) };
        push_and_wake(Event::Hotkey {
            queued_ms: (waited * 1000.0).max(0.0) as u32,
        });
    }
    0
}

/// Registers `hotkey` under `id`; an error when the key cannot be named on a Mac keyboard or the
/// OS refuses it.
pub fn register_hotkey(id: i32, hotkey: &Hotkey) -> Result<(), Error> {
    static HANDLER: Once = Once::new();
    HANDLER.call_once(|| {
        let spec = EventTypeSpec {
            class: K_EVENT_CLASS_KEYBOARD,
            kind: K_EVENT_HOT_KEY_PRESSED,
        };
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                on_hotkey,
                1,
                &spec,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != 0 {
            eprintln!("hotkey handler: OSStatus {status}");
        }
    });
    let code = code_of(hotkey.key).ok_or_else(|| Error::new("no such key on a Mac keyboard"))?;
    let mut modifiers = 0;
    for (on, bit) in [
        (hotkey.ctrl, CONTROL_KEY),
        (hotkey.alt, OPTION_KEY),
        (hotkey.shift, SHIFT_KEY),
        (hotkey.meta, CMD_KEY),
    ] {
        if on {
            modifiers |= bit;
        }
    }
    let mut out: EventHotKeyRef = std::ptr::null_mut();
    let status = unsafe {
        RegisterEventHotKey(
            code,
            modifiers,
            EventHotKeyId {
                signature: SIGNATURE,
                id: id as u32,
            },
            GetApplicationEventTarget(),
            0,
            &mut out,
        )
    };
    match status {
        0 => {
            REGISTERED.with(|r| r.borrow_mut().insert(id, out as usize));
            Ok(())
        }
        EVENT_HOT_KEY_EXISTS_ERR => Err(Error::new("another program has it")),
        _ => Err(Error::new(format!("OSStatus {status}"))),
    }
}

pub fn unregister_hotkey(id: i32) {
    if let Some(hotkey) = REGISTERED.with(|r| r.borrow_mut().remove(&id)) {
        unsafe { UnregisterEventHotKey(hotkey as EventHotKeyRef) };
    }
}

/// Virtual key codes of the ANSI keyboard (`kVK_ANSI_*`): the key in that place, whatever the
/// layout prints on it.
const CHARS: [(char, u32); 47] = [
    ('a', 0x00),
    ('s', 0x01),
    ('d', 0x02),
    ('f', 0x03),
    ('h', 0x04),
    ('g', 0x05),
    ('z', 0x06),
    ('x', 0x07),
    ('c', 0x08),
    ('v', 0x09),
    ('b', 0x0B),
    ('q', 0x0C),
    ('w', 0x0D),
    ('e', 0x0E),
    ('r', 0x0F),
    ('y', 0x10),
    ('t', 0x11),
    ('1', 0x12),
    ('2', 0x13),
    ('3', 0x14),
    ('4', 0x15),
    ('6', 0x16),
    ('5', 0x17),
    ('=', 0x18),
    ('9', 0x19),
    ('7', 0x1A),
    ('-', 0x1B),
    ('8', 0x1C),
    ('0', 0x1D),
    (']', 0x1E),
    ('o', 0x1F),
    ('u', 0x20),
    ('[', 0x21),
    ('i', 0x22),
    ('p', 0x23),
    ('l', 0x25),
    ('j', 0x26),
    ('\'', 0x27),
    ('k', 0x28),
    (';', 0x29),
    ('\\', 0x2A),
    (',', 0x2B),
    ('/', 0x2C),
    ('n', 0x2D),
    ('m', 0x2E),
    ('.', 0x2F),
    ('`', 0x32),
];

/// `F1`–`F20` by number.
const FUNCTION: [u32; 20] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A,
    0x40, 0x4F, 0x50, 0x5A,
];

/// A PC keyboard on a Mac sends F13 for Print Screen and F15 for Pause; those come back as F13 and
/// F15.
const NAMED: [(Key, u32); 16] = [
    (Key::Space, 0x31),
    (Key::Enter, 0x24),
    (Key::Tab, 0x30),
    (Key::Backspace, 0x33),
    (Key::Insert, 0x72),
    (Key::Delete, 0x75),
    (Key::Home, 0x73),
    (Key::End, 0x77),
    (Key::PageUp, 0x74),
    (Key::PageDown, 0x79),
    (Key::Left, 0x7B),
    (Key::Right, 0x7C),
    (Key::Up, 0x7E),
    (Key::Down, 0x7D),
    (Key::PrintScreen, 0x69),
    (Key::Pause, 0x71),
];

/// The virtual key code of `key`.
pub fn code_of(key: Key) -> Option<u32> {
    match key {
        Key::Char(c) => {
            let c = c.to_ascii_lowercase();
            CHARS.iter().find(|(ch, _)| *ch == c).map(|(_, code)| *code)
        }
        Key::F(n) => FUNCTION.get(usize::from(n).checked_sub(1)?).copied(),
        named => NAMED
            .iter()
            .find(|(k, _)| *k == named)
            .map(|(_, code)| *code),
    }
}

/// The key of a virtual key code; letters in upper case, as `shot-core` names them.
pub fn key_of(code: u32) -> Option<Key> {
    if let Some((c, _)) = CHARS.iter().find(|(_, k)| *k == code) {
        return Some(Key::Char(c.to_ascii_uppercase()));
    }
    if let Some(i) = FUNCTION.iter().position(|k| *k == code) {
        return Some(Key::F(i as u8 + 1));
    }
    NAMED
        .iter()
        .find(|(_, k)| *k == code && !matches!(*k, 0x69 | 0x71))
        .map(|(key, _)| *key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for c in ('A'..='Z').chain('0'..='9') {
            assert_eq!(key_of(code_of(Key::Char(c)).unwrap()), Some(Key::Char(c)));
        }
        for n in 1..=20 {
            assert_eq!(key_of(code_of(Key::F(n)).unwrap()), Some(Key::F(n)));
        }
        assert_eq!(code_of(Key::Char('2')), Some(0x13));
        assert_eq!(key_of(0x31), Some(Key::Space));
    }
}
