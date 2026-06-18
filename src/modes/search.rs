use anyhow::Result;
use crossterm::event::KeyCode;

use crate::app::{App, AppMode};

/// Key handling for the live station-search overlay.
pub fn handle(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => {
            // Exit search: clear query and go back to Normal.
            app.mode = AppMode::Normal;
            app.search_query = String::new();
            app.sel = 0;
        }
        KeyCode::Enter => {
            // Play the currently highlighted station and exit search.
            app.mode = AppMode::Normal;
            app.play_selected();
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            let n = app.visible_stations().len();
            if n > 0 { app.sel = app.sel.min(n - 1); } else { app.sel = 0; }
        }
        KeyCode::Down => {
            let n = app.visible_stations().len();
            if n > 0 { app.sel = (app.sel + 1).min(n - 1); }
        }
        KeyCode::Up => {
            app.sel = app.sel.saturating_sub(1);
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            // Reset selection when the result set changes.
            app.sel = 0;
        }
        _ => {}
    }
    Ok(())
}
