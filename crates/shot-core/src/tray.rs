//! The tray menu (`docs/design/settings.md`, "Tray menu"): which items it has and how they read on
//! each OS. The platform shows them as a native menu and reports the chosen [`Command`].

use crate::hotkey::{Hotkey, Os};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// The hotkey did not register: open the settings to change it.
    FixHotkey,
    /// macOS has no Screen Recording permission: open the window that asks for it.
    FixPermission,
    Capture,
    Settings,
    OpenFolder,
    Autostart,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Separator,
    Command {
        command: Command,
        label: &'static str,
        /// Shown on the right: the capture hotkey; on macOS also the menu's own shortcuts.
        shortcut: Option<String>,
        checked: bool,
        enabled: bool,
        /// The item a left click on the icon runs; Windows shows it bold.
        default: bool,
    },
}

/// What the menu depends on.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub hotkey: Hotkey,
    /// The OS took the hotkey.
    pub hotkey_works: bool,
    /// macOS lets Shot record the screen; always true on Windows.
    pub permission: bool,
    pub autostart: bool,
}

impl State {
    /// The icon's alert variant is shown.
    pub fn alert(&self) -> bool {
        !self.hotkey_works || !self.permission
    }
}

pub fn menu(state: &State, os: Os) -> Vec<Item> {
    let item = |command, label| Item::Command {
        command,
        label,
        shortcut: None,
        checked: false,
        enabled: true,
        default: false,
    };
    let mut items = Vec::new();
    if !state.permission {
        items.extend([
            item(Command::FixPermission, "Нет разрешения на запись экрана…"),
            Item::Separator,
        ]);
    } else if !state.hotkey_works {
        items.extend([
            item(Command::FixHotkey, "Хоткей не работает — изменить…"),
            Item::Separator,
        ]);
    }
    let mac = os == Os::MacOs;
    items.extend([
        Item::Command {
            command: Command::Capture,
            label: "Снять",
            shortcut: state.hotkey_works.then(|| state.hotkey.display(os)),
            checked: false,
            enabled: state.permission,
            default: !mac,
        },
        Item::Separator,
        Item::Command {
            command: Command::Settings,
            label: "Настройки…",
            shortcut: mac.then(|| "\u{2318},".to_string()),
            checked: false,
            enabled: true,
            default: false,
        },
        item(Command::OpenFolder, "Открыть папку"),
        Item::Command {
            command: Command::Autostart,
            label: "Автозапуск",
            shortcut: None,
            checked: state.autostart,
            enabled: true,
            default: false,
        },
        Item::Separator,
        Item::Command {
            command: Command::Quit,
            label: if mac {
                "Завершить Shot"
            } else {
                "Выход"
            },
            shortcut: mac.then(|| "\u{2318}Q".to_string()),
            checked: false,
            enabled: true,
            default: false,
        },
    ]);
    items
}

/// The icon's tooltip, Windows only: `Shot — Ctrl+Shift+2`.
pub fn tooltip(state: &State, os: Os) -> String {
    if state.hotkey_works {
        format!("Shot — {}", state.hotkey.display(os))
    } else {
        "Shot — хоткей не работает".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State {
            hotkey: Hotkey::default_for(Os::Windows),
            hotkey_works: true,
            permission: true,
            autostart: true,
        }
    }

    fn commands(items: &[Item]) -> Vec<Command> {
        items
            .iter()
            .filter_map(|i| match i {
                Item::Command { command, .. } => Some(*command),
                Item::Separator => None,
            })
            .collect()
    }

    #[test]
    fn windows_menu_as_designed() {
        let items = menu(&state(), Os::Windows);
        assert_eq!(
            commands(&items),
            [
                Command::Capture,
                Command::Settings,
                Command::OpenFolder,
                Command::Autostart,
                Command::Quit
            ]
        );
        assert!(matches!(
            &items[0],
            Item::Command { shortcut: Some(s), default: true, .. } if s == "Ctrl+Shift+2"
        ));
        assert!(matches!(&items[4], Item::Command { checked: true, .. }));
        assert_eq!(tooltip(&state(), Os::Windows), "Shot — Ctrl+Shift+2");
    }

    #[test]
    fn broken_hotkey_adds_the_fix_and_drops_the_shortcut() {
        let s = State {
            hotkey_works: false,
            ..state()
        };
        assert!(s.alert());
        let items = menu(&s, Os::Windows);
        assert_eq!(commands(&items)[0], Command::FixHotkey);
        assert_eq!(items[1], Item::Separator);
        assert!(matches!(&items[2], Item::Command { shortcut: None, .. }));
        assert_eq!(tooltip(&s, Os::Windows), "Shot — хоткей не работает");
    }

    #[test]
    fn missing_permission_disables_capture() {
        let s = State {
            permission: false,
            hotkey: Hotkey::default_for(Os::MacOs),
            ..state()
        };
        let items = menu(&s, Os::MacOs);
        assert_eq!(commands(&items)[0], Command::FixPermission);
        assert!(matches!(
            &items[2],
            Item::Command {
                enabled: false,
                default: false,
                ..
            }
        ));
        assert!(matches!(
            items.last(),
            Some(Item::Command {
                label: "Завершить Shot",
                ..
            })
        ));
    }
}
