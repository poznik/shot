//! Capture overlay (DES-002, `docs/design/overlay-preview.md`): the frozen frame, the dim
//! outside the target, the target's frame, the key hint, the size label and the crosshair.
//!
//! At the hotkey the platform builds a [`Selection`] from the monitors, the window list and the
//! cursor, wraps it in an [`Overlay`] and draws every monitor once with [`Overlay::render`].
//! Then it feeds input [`Event`]s to [`Overlay::handle`] and redraws only the rects each
//! [`Update`] names, until the update carries an [`Outcome`].

mod layout;
mod render;
mod selection;

pub use render::{Damage, Overlay, Update};
pub use selection::{Event, Monitor, Outcome, Selection, Target, TargetKind};
