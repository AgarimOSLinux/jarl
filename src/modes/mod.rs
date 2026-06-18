//! Per-`AppMode` input handling, split out of `app.rs` to keep each mode's
//! key-dispatch logic in its own file rather than one growing match arm.
//!
//! Each submodule exposes a single `handle(app: &mut App, ...) -> Result<()>`
//! function that owns the key-dispatch logic for that mode. `App` itself
//! still owns all state; these are free functions operating on `&mut App`
//! rather than inherent methods, which is why some `App` helpers needed by
//! more than one mode (e.g. `play_selected`) are `pub(crate)`.

pub mod normal;
pub mod search;
pub mod history;
pub mod theme_picker;
pub mod confirm_delete;
