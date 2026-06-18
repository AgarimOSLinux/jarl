use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, AppMode};
use crate::config::Keybindings;

/// Key handling for the playback-history overlay.
pub fn handle(app: &mut App, code: KeyCode, mods: KeyModifiers) -> Result<()> {
    let kb = app.config.keybindings.clone();
    let n  = app.history.len();

    if let AppMode::History { ref mut selected } = app.mode {
        let m = |s: &str| Keybindings::matches(s, code, mods);

        if code == KeyCode::Esc || m(&kb.quit) || m(&kb.history) {
            app.mode = AppMode::Normal;
        } else if m(&kb.nav_down) || code == KeyCode::Down {
            if n > 0 { *selected = (*selected + 1).min(n - 1); }
        } else if m(&kb.nav_up) || code == KeyCode::Up {
            *selected = selected.saturating_sub(1);
        } else if m(&kb.nav_top) || code == KeyCode::Home {
            *selected = 0;
        } else if m(&kb.nav_bottom) || code == KeyCode::End {
            if n > 0 { *selected = n - 1; }
        } else if m(&kb.play) {
            let idx = *selected;
            app.mode = AppMode::Normal;
            if let Some(entry) = app.history.get(idx).cloned() {
                app.play_from_history(entry);
            }
        }
    }
    Ok(())
}
