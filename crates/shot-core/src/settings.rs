//! `settings.toml` (SPEC §3.6): what the settings window changes, plus two counters Shot keeps for
//! itself. A missing or broken file gives the defaults; a broken value gives that value's default,
//! so one typo never resets the rest.

use std::io;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::hotkey::{Hotkey, Os};
use crate::preview::Side;

/// The overlay's key hint is shown for this many captures (`docs/design/overlay-preview.md`).
pub const HINT_CAPTURES: u32 = 5;

/// Where a capture goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    Clipboard,
    File,
    Both,
}

impl Destination {
    pub fn clipboard(self) -> bool {
        matches!(self, Destination::Clipboard | Destination::Both)
    }

    pub fn file(self) -> bool {
        matches!(self, Destination::File | Destination::Both)
    }
}

/// The interface theme: follow the OS or force one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub hotkey: Hotkey,
    pub destination: Destination,
    /// `None`: the platform's default folder, `Pictures/Shot`.
    pub folder: Option<PathBuf>,
    pub preview: bool,
    /// How long a preview card stays, 1–60 s; the settings window offers 3, 5 and 10.
    pub preview_seconds: u32,
    /// The bottom corner the preview cards stack in.
    pub preview_side: Side,
    pub theme: ThemeChoice,
    /// The first-run welcome was shown.
    pub welcomed: bool,
    /// Captures taken, counted up to [`HINT_CAPTURES`].
    pub captures: u32,
}

impl Settings {
    pub fn defaults(os: Os) -> Self {
        Settings {
            hotkey: Hotkey::default_for(os),
            destination: Destination::Both,
            folder: None,
            preview: true,
            preview_seconds: 5,
            preview_side: Side::Left,
            theme: ThemeChoice::System,
            welcomed: false,
            captures: 0,
        }
    }

    /// Whether the overlay shows its key hint this time.
    pub fn show_key_hint(&self) -> bool {
        self.captures < HINT_CAPTURES
    }

    /// Reads the file's text. Returns the settings and what was wrong with it, one line each, for
    /// the log.
    pub fn parse(text: &str, os: Os) -> (Self, Vec<String>) {
        let mut s = Settings::defaults(os);
        let table: Table = match text.parse() {
            Ok(t) => t,
            Err(e) => return (s, vec![format!("not TOML, using defaults: {e}")]),
        };
        let mut problems = Vec::new();
        let mut bad = |key: &str, value: &Value| problems.push(format!("{key} = {value}: ignored"));
        for (key, value) in &table {
            let ok = match key.as_str() {
                "hotkey" => set(&mut s.hotkey, value.as_str().and_then(Hotkey::parse)),
                "after_capture" => set(
                    &mut s.destination,
                    match value.as_str() {
                        Some("clipboard") => Some(Destination::Clipboard),
                        Some("file") => Some(Destination::File),
                        Some("both") => Some(Destination::Both),
                        _ => None,
                    },
                ),
                "folder" => match value.as_str() {
                    Some("") => true,
                    Some(path) => set(&mut s.folder, Some(Some(PathBuf::from(path)))),
                    None => false,
                },
                "preview" => set(&mut s.preview, value.as_bool()),
                "preview_seconds" => set(
                    &mut s.preview_seconds,
                    value
                        .as_integer()
                        .filter(|n| (1..=60).contains(n))
                        .map(|n| n as u32),
                ),
                "preview_side" => set(
                    &mut s.preview_side,
                    match value.as_str() {
                        Some("left") => Some(Side::Left),
                        Some("right") => Some(Side::Right),
                        _ => None,
                    },
                ),
                "theme" => set(
                    &mut s.theme,
                    match value.as_str() {
                        Some("system") => Some(ThemeChoice::System),
                        Some("light") => Some(ThemeChoice::Light),
                        Some("dark") => Some(ThemeChoice::Dark),
                        _ => None,
                    },
                ),
                "welcomed" => set(&mut s.welcomed, value.as_bool()),
                "captures" => set(
                    &mut s.captures,
                    value
                        .as_integer()
                        .map(|n| n.clamp(0, HINT_CAPTURES as i64) as u32),
                ),
                // Keys of a newer version are kept out of the way, not an error.
                _ => true,
            };
            if !ok {
                bad(key, value);
            }
        }
        (s, problems)
    }

    /// The file's text: every setting, with a comment where the allowed values are not obvious.
    pub fn to_toml(&self, os: Os) -> String {
        let string = |s: &str| Value::String(s.to_string()).to_string();
        let folder = self
            .folder
            .as_deref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let default_folder = match os {
            Os::Windows => "Pictures\\Shot",
            Os::MacOs => "~/Pictures/Shot",
        };
        let destination = match self.destination {
            Destination::Clipboard => "clipboard",
            Destination::File => "file",
            Destination::Both => "both",
        };
        let side = match self.preview_side {
            Side::Left => "left",
            Side::Right => "right",
        };
        let theme = match self.theme {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        };
        format!(
            "# Shot settings. The settings window writes this file too; a value Shot cannot read\n\
             # falls back to its default.\n\
             \n\
             hotkey = {hotkey}\n\
             # clipboard, file or both\n\
             after_capture = \"{destination}\"\n\
             # Empty: {default_folder}\n\
             folder = {folder}\n\
             preview = {preview}\n\
             preview_seconds = {seconds}\n\
             # left or right: the bottom corner the preview appears in\n\
             preview_side = \"{side}\"\n\
             # system, light or dark\n\
             theme = \"{theme}\"\n\
             \n\
             # Kept by Shot: the welcome was shown; captures counted for the overlay's key hint.\n\
             welcomed = {welcomed}\n\
             captures = {captures}\n",
            hotkey = string(&self.hotkey.to_text(os)),
            folder = string(&folder),
            preview = self.preview,
            seconds = self.preview_seconds,
            welcomed = self.welcomed,
            captures = self.captures,
        )
    }

    /// Reads the settings file. A missing file is the first run: defaults, no problem reported.
    pub fn load(path: &Path, os: Os) -> (Self, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Settings::parse(&text, os),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (Settings::defaults(os), Vec::new()),
            Err(e) => (
                Settings::defaults(os),
                vec![format!("{}: {e}, using defaults", path.display())],
            ),
        }
    }

    /// Writes the file whole through a temporary file, so a crash never leaves half of it.
    pub fn save(&self, path: &Path, os: Os) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_toml(os))?;
        std::fs::rename(&tmp, path)
    }
}

/// Puts `value` into `slot` if there is one; tells whether there was.
fn set<T>(slot: &mut T, value: Option<T>) -> bool {
    match value {
        Some(v) => {
            *slot = v;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::Key;

    #[test]
    fn round_trip() {
        for os in [Os::Windows, Os::MacOs] {
            let s = Settings {
                hotkey: Hotkey::parse("Ctrl+Alt+F9").unwrap(),
                destination: Destination::File,
                folder: Some(PathBuf::from(r#"D:\Скриншоты "мои"\Shot"#)),
                preview: false,
                preview_seconds: 10,
                preview_side: Side::Right,
                theme: ThemeChoice::Dark,
                welcomed: true,
                captures: 3,
            };
            let (back, problems) = Settings::parse(&s.to_toml(os), os);
            assert_eq!(back, s);
            assert!(problems.is_empty(), "{problems:?}");
        }
    }

    #[test]
    fn defaults_round_trip_with_an_empty_folder() {
        let s = Settings::defaults(Os::Windows);
        let text = s.to_toml(Os::Windows);
        assert!(text.contains("folder = \"\""), "{text}");
        assert!(text.contains("hotkey = \"Ctrl+Shift+2\""), "{text}");
        assert_eq!(Settings::parse(&text, Os::Windows).0, s);
    }

    #[test]
    fn broken_file_gives_defaults() {
        let (s, problems) = Settings::parse("hotkey = \"Ctrl+Shift+2\n[[[", Os::MacOs);
        assert_eq!(s, Settings::defaults(Os::MacOs));
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn bad_value_falls_back_alone() {
        let text = "hotkey = \"Ctrl+Hyper+X\"\nafter_capture = \"clipboard\"\npreview_seconds = 0\n\
                    preview_side = \"top\"\ntheme = 7\nfuture_key = 1\ncaptures = 99\n";
        let (s, problems) = Settings::parse(text, Os::Windows);
        assert_eq!(s.hotkey, Hotkey::default_for(Os::Windows));
        assert_eq!(s.destination, Destination::Clipboard);
        assert_eq!(s.preview_seconds, 5);
        assert_eq!(s.preview_side, Side::Left);
        assert_eq!(s.theme, ThemeChoice::System);
        assert_eq!(s.captures, HINT_CAPTURES);
        assert!(!s.show_key_hint());
        assert_eq!(problems.len(), 4, "{problems:?}");
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("shot-settings-{}", std::process::id()));
        let path = dir.join("Shot").join("settings.toml");
        let (missing, problems) = Settings::load(&path, Os::Windows);
        assert_eq!(missing, Settings::defaults(Os::Windows));
        assert!(problems.is_empty());

        let mut s = missing;
        s.hotkey.key = Key::F(5);
        s.save(&path, Os::Windows).unwrap();
        s.captures = 1;
        s.save(&path, Os::Windows).unwrap();
        assert_eq!(Settings::load(&path, Os::Windows).0, s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
