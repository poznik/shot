//! A hotkey as the user sees it and the settings store it: modifiers plus one key, named the same
//! way on both OSes (`docs/design/settings.md`, "Key cap"). The platform maps [`Key`] to its own
//! key codes.

use std::fmt;

/// The OS a text is written for: hotkeys read differently on Windows and macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
}

impl Os {
    /// The OS this build runs on.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Windows
        }
    }
}

/// The non-modifier key of a hotkey.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// `A`–`Z`, `0`–`9` and the punctuation keys `` ` - = [ ] \ ; ' , . / ``.
    Char(char),
    /// `F1`–`F24`.
    F(u8),
    Space,
    Enter,
    Tab,
    Backspace,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    PrintScreen,
    Pause,
}

const PUNCTUATION: &str = "`-=[]\\;',./";

/// Named keys with their names, the first one is how Shot writes it.
const NAMED: [(Key, &[&str]); 16] = [
    (Key::Space, &["Space"]),
    (Key::Enter, &["Enter", "Return"]),
    (Key::Tab, &["Tab"]),
    (Key::Backspace, &["Backspace"]),
    (Key::Insert, &["Ins", "Insert"]),
    (Key::Delete, &["Del", "Delete"]),
    (Key::Home, &["Home"]),
    (Key::End, &["End"]),
    (Key::PageUp, &["PgUp", "PageUp"]),
    (Key::PageDown, &["PgDn", "PageDown"]),
    (Key::Left, &["Left"]),
    (Key::Right, &["Right"]),
    (Key::Up, &["Up"]),
    (Key::Down, &["Down"]),
    (Key::PrintScreen, &["PrtSc", "PrintScreen", "PrtScn"]),
    (Key::Pause, &["Pause"]),
];

impl Key {
    /// Every key a hotkey can have.
    pub fn all() -> impl Iterator<Item = Key> {
        ('A'..='Z')
            .chain('0'..='9')
            .chain(PUNCTUATION.chars())
            .map(Key::Char)
            .chain((1..=24).map(Key::F))
            .chain(NAMED.iter().map(|(key, _)| *key))
    }

    pub fn parse(text: &str) -> Option<Key> {
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            let c = c.to_ascii_uppercase();
            return (c.is_ascii_alphanumeric() || PUNCTUATION.contains(c)).then_some(Key::Char(c));
        }
        if let Some(n) = text
            .strip_prefix(['F', 'f'])
            .and_then(|n| n.parse::<u8>().ok())
            && (1..=24).contains(&n)
        {
            return Some(Key::F(n));
        }
        NAMED
            .iter()
            .find(|(_, names)| names.iter().any(|n| n.eq_ignore_ascii_case(text)))
            .map(|(key, _)| *key)
    }

    pub fn name(self) -> String {
        match self {
            Key::Char(c) => c.to_string(),
            Key::F(n) => format!("F{n}"),
            named => NAMED
                .iter()
                .find(|(k, _)| *k == named)
                .map_or("?", |(_, names)| names[0])
                .to_string(),
        }
    }
}

/// A global hotkey: any of four modifiers plus one key. `meta` is `Win` on Windows and `Cmd` on
/// macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: Key,
}

impl Hotkey {
    /// `Ctrl+Shift+2` on Windows, `⇧⌘2` on macOS (SPEC §9, decision 2).
    pub const fn default_for(os: Os) -> Self {
        Hotkey {
            ctrl: matches!(os, Os::Windows),
            alt: false,
            shift: true,
            meta: matches!(os, Os::MacOs),
            key: Key::Char('2'),
        }
    }

    /// Reads `Ctrl+Shift+2` and the like: modifiers in any order and case, the key last. Accepts
    /// both OSes' names for a modifier (`Win`, `Cmd`; `Alt`, `Opt`).
    pub fn parse(text: &str) -> Option<Hotkey> {
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        // `Ctrl++` would split into empty parts; `+` is not a supported key.
        let key = Key::parse(parts.pop()?)?;
        let mut hotkey = Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            key,
        };
        for part in parts {
            let flag = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => &mut hotkey.ctrl,
                "alt" | "opt" | "option" => &mut hotkey.alt,
                "shift" => &mut hotkey.shift,
                "win" | "cmd" | "command" | "meta" | "super" => &mut hotkey.meta,
                _ => return None,
            };
            *flag = true;
        }
        Some(hotkey)
    }

    /// The key caps, in the OS's order: `Ctrl` `Alt` `Shift` `Win` key on Windows, `⌃` `⌥` `⇧` `⌘`
    /// key on macOS.
    pub fn caps(&self, os: Os) -> Vec<String> {
        let mut caps = modifier_caps([self.ctrl, self.alt, self.shift, self.meta], os);
        caps.push(self.key.name());
        caps
    }

    /// How the interface writes it: `Ctrl+Shift+2` on Windows, `⇧⌘2` on macOS.
    pub fn display(&self, os: Os) -> String {
        match os {
            Os::Windows => self.caps(os).join("+"),
            Os::MacOs => self.caps(os).concat(),
        }
    }

    /// How the settings file writes it: modifier words joined with `+` on both OSes, so the file
    /// stays readable and typable.
    pub fn to_text(&self, os: Os) -> String {
        let names: [&str; 4] = match os {
            Os::Windows => ["Ctrl", "Alt", "Shift", "Win"],
            Os::MacOs => ["Ctrl", "Opt", "Shift", "Cmd"],
        };
        [self.ctrl, self.alt, self.shift, self.meta]
            .into_iter()
            .zip(names)
            .filter(|(on, _)| *on)
            .map(|(_, name)| name.to_string())
            .chain([self.key.name()])
            .collect::<Vec<_>>()
            .join("+")
    }

    /// Whether the combination can be a global hotkey: on Windows it needs `Ctrl` or `Alt`
    /// (`Shift+2` types text, `Win` combinations belong to the shell); on macOS `⌘` or `⌃`
    /// (macOS 15 refuses Option/Shift-only hotkeys).
    pub fn has_required_modifier(&self, os: Os) -> bool {
        match os {
            Os::Windows => self.ctrl || self.alt,
            Os::MacOs => self.meta || self.ctrl,
        }
    }

    /// macOS keeps `⇧⌘3`, `⇧⌘4` and `⇧⌘5` for its own screenshots.
    pub fn is_reserved(&self, os: Os) -> bool {
        os == Os::MacOs
            && self.shift
            && self.meta
            && !self.ctrl
            && !self.alt
            && matches!(self.key, Key::Char('3' | '4' | '5'))
    }
}

/// The caps of the modifiers that are on, given as `[ctrl, alt, shift, meta]`, in the OS's order:
/// what a hotkey field shows while they are held.
pub fn modifier_caps(on: [bool; 4], os: Os) -> Vec<String> {
    let names: [&str; 4] = match os {
        Os::Windows => ["Ctrl", "Alt", "Shift", "Win"],
        Os::MacOs => ["\u{2303}", "\u{2325}", "\u{21E7}", "\u{2318}"],
    };
    on.into_iter()
        .zip(names)
        .filter(|(on, _)| *on)
        .map(|(_, name)| name.to_string())
        .collect()
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(Os::current()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reads_as_the_spec_says() {
        assert_eq!(
            Hotkey::default_for(Os::Windows).display(Os::Windows),
            "Ctrl+Shift+2"
        );
        assert_eq!(
            Hotkey::default_for(Os::MacOs).display(Os::MacOs),
            "\u{21E7}\u{2318}2"
        );
        assert_eq!(
            Hotkey::default_for(Os::MacOs).to_text(Os::MacOs),
            "Shift+Cmd+2"
        );
    }

    #[test]
    fn parse_takes_any_order_and_case() {
        let h = Hotkey::parse("shift + CTRL + s").unwrap();
        assert!(h.ctrl && h.shift && !h.alt && !h.meta);
        assert_eq!(h.key, Key::Char('S'));
        assert_eq!(h.to_text(Os::Windows), "Ctrl+Shift+S");
        assert_eq!(
            Hotkey::parse("Cmd+Opt+F12").unwrap().display(Os::MacOs),
            "\u{2325}\u{2318}F12"
        );
        assert_eq!(
            Hotkey::parse("Alt+PrintScreen").unwrap().key,
            Key::PrintScreen
        );
    }

    #[test]
    fn parse_refuses_junk() {
        for text in [
            "",
            "Ctrl+",
            "Ctrl+Hyper+2",
            "Ctrl+F25",
            "Ctrl+Shift+Ж",
            "Ctrl+AB",
        ] {
            assert_eq!(Hotkey::parse(text), None, "{text}");
        }
    }

    #[test]
    fn every_key_name_parses_back() {
        let keys = NAMED
            .iter()
            .map(|(k, _)| *k)
            .chain((1..=24).map(Key::F))
            .chain(
                ('A'..='Z')
                    .chain('0'..='9')
                    .chain(PUNCTUATION.chars())
                    .map(Key::Char),
            );
        for key in keys {
            assert_eq!(Key::parse(&key.name()), Some(key), "{}", key.name());
        }
    }

    #[test]
    fn required_modifiers() {
        let shift_2 = Hotkey::parse("Shift+2").unwrap();
        assert!(!shift_2.has_required_modifier(Os::Windows));
        assert!(
            Hotkey::parse("Alt+2")
                .unwrap()
                .has_required_modifier(Os::Windows)
        );
        assert!(
            !Hotkey::parse("Alt+2")
                .unwrap()
                .has_required_modifier(Os::MacOs)
        );
        assert!(
            Hotkey::parse("Cmd+2")
                .unwrap()
                .has_required_modifier(Os::MacOs)
        );
    }
}
