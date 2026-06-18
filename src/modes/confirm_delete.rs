use anyhow::Result;
use crossterm::event::KeyCode;

use crate::app::{App, AppMode};
use crate::favorites;
use crate::player::PlayerCmd;
use crate::stations::save_stations;

/// Key handling for the "delete this station?" confirmation prompt.
pub fn handle(app: &mut App, code: KeyCode) -> Result<()> {
    let idx = if let AppMode::ConfirmDelete { index } = app.mode { index }
              else { return Ok(()); };
    if matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')) {
        let name = app.stations[idx].name.clone();
        if app.current == Some(idx) {
            app.player.send(PlayerCmd::Stop);
            app.current = None;
        } else if let Some(c) = app.current {
            if c > idx { app.current = Some(c - 1); }
        }
        app.favorites.remove(&name);
        let _ = favorites::save(&app.favorites);
        app.stations.remove(idx);
        app.sel = app.sel.min(app.stations.len().saturating_sub(1));
        match save_stations(&app.stations) {
            Ok(_)  => app.set_status(format!("Deleted: {name}")),
            Err(e) => app.set_status(format!("Deleted from memory (save failed: {e})")),
        }
    }
    app.mode = AppMode::Normal;
    Ok(())
}
