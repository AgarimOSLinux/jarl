use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, AppMode};
use crate::config::Keybindings;

/// Key handling for the theme-picker overlay.
pub fn handle(app: &mut App, code: KeyCode, mods: KeyModifiers) -> Result<()> {
    let kb = app.config.keybindings.clone();
    let n  = app.themes.len();

    if let AppMode::ThemePicker { ref mut selected } = app.mode {
        let m = |s: &str| Keybindings::matches(s, code, mods);

        if m(&kb.nav_down) || code == KeyCode::Down {
            if n > 0 { *selected = (*selected + 1).min(n - 1); }
        } else if m(&kb.nav_up) || code == KeyCode::Up {
            *selected = selected.saturating_sub(1);
        } else if m(&kb.play) {
            let idx = *selected;
            app.theme = app.themes[idx].clone();
            app.config.theme = app.theme.name.clone();
            let _ = app.config.save();
            app.set_status(format!("Theme: {}", app.theme.name));
            app.mode = AppMode::Normal;
        } else if code == KeyCode::Esc || m(&kb.theme) || m(&kb.quit) {
            app.mode = AppMode::Normal;
        }
    }
    Ok(())
}
