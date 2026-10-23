//! OS-independent core of Shot.
//!
//! Everything the user sees is drawn here, so Windows and macOS look the same: the capture model,
//! annotations and editor tools, rendering with tiny-skia, UI widgets, settings, file naming and
//! PNG encoding. This crate must not depend on any OS API (SPEC §5.3).

mod canvas;
pub mod draw;
pub mod editor;
pub mod geom;
pub mod hotkey;
pub mod icons;
pub mod image;
pub mod naming;
pub mod overlay;
pub mod preview;
pub mod settings;
pub mod text;
pub mod theme;
pub mod tray;
pub mod ui;
