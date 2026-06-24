use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, AppMode};
use crate::config::Keybindings;
use crate::favorites;
use crate::player::PlayerCmd;
use crate::stations::load_stations;

/// Key handling for the default browse/play screen.
pub fn handle(app: &mut App, code: KeyCode, mods: KeyModifiers) -> Result<()> {
    let kb = app.config.keybindings.clone();
    let n  = app.visible_stations().len();

    let m = |s: &str| Keybindings::matches(s, code, mods);

    if m(&kb.quit) {
        app.save_config()?;
        app.should_quit = true;
    }

    if m(&kb.nav_down) || code == KeyCode::Down {
        if n > 0 { app.sel = (app.sel + 1).min(n - 1); }
    } else if m(&kb.nav_up) || code == KeyCode::Up {
        app.sel = app.sel.saturating_sub(1);
    } else if m(&kb.nav_top) || code == KeyCode::Home {
        app.sel = 0;
    } else if m(&kb.nav_bottom) || code == KeyCode::End {
        if n > 0 { app.sel = n - 1; }
    } else if code == KeyCode::PageDown
              || (code == KeyCode::Char('d') && mods == KeyModifiers::CONTROL) {
        if n > 0 { app.sel = (app.sel + 10).min(n - 1); }
    } else if code == KeyCode::PageUp
              || (code == KeyCode::Char('u') && mods == KeyModifiers::CONTROL) {
        app.sel = app.sel.saturating_sub(10);

    // Search  ('/') ── vim convention
    } else if code == KeyCode::Char('/') {
        app.search_query = String::new();
        app.sel = 0;
        app.mode = AppMode::Search;

    // Playback
    } else if m(&kb.play) {
        app.play_selected();
    } else if m(&kb.pause) {
        if app.current.is_some() { app.player.send(PlayerCmd::TogglePause); }
    } else if m(&kb.stop) {
        app.player.send(PlayerCmd::Stop);
        app.clear_now_playing();
    } else if m(&kb.next_station) {
        app.play_relative(1);
    } else if m(&kb.prev_station) {
        app.play_relative(-1);

    // Volume
    } else if m(&kb.volume_up) {
        app.adjust_volume(0.05);
    } else if m(&kb.volume_down) {
        app.adjust_volume(-0.05);

    // Favourites
    } else if m(&kb.favourite) {
        if let Some(idx) = app.real_index(app.sel) {
            let name = app.stations[idx].name.clone();
            if app.favorites.contains(&name) {
                app.favorites.remove(&name);
                app.set_status(format!("Removed from favourites: {name}"));
            } else {
                app.favorites.insert(name.clone());
                app.set_status(format!("Added to favourites: {name}"));
            }
            let _ = favorites::save(&app.favorites);
        }
    } else if m(&kb.fav_filter) {
        app.fav_filter = !app.fav_filter;
        app.sel = 0;
        app.set_status(if app.fav_filter {
            "Showing favourites only".into()
        } else {
            "Showing all stations".into()
        });

    // Station management
    } else if m(&kb.delete) {
        if let Some(idx) = app.real_index(app.sel) {
            app.mode = AppMode::ConfirmDelete { index: idx };
        }

    } else if m(&kb.reload_themes) {
        let new_themes = crate::theme::all_themes();
        if new_themes.is_empty() {
            app.set_status("No active themes in themes.toml — edit the file and set enabled = true".into());
        } else {
            let prev_name = app.theme.name.clone();
            app.themes = new_themes;
            if let Some(t) = app.themes.iter().find(|t| t.name == prev_name) {
                app.theme = t.clone();
            } else {
                app.theme = app.themes[0].clone();
                app.config.theme = app.theme.name.clone();
                let _ = app.config.save();
                app.set_status(format!("Theme '{}' no longer active — switched to {}", prev_name, app.theme.name));
            }
            if app.theme.name == prev_name {
                app.set_status(format!("Themes reloaded: {} available", app.themes.len()));
            }
        }
    } else if m(&kb.reload) {
        match load_stations() {
            Ok(stations) => {
                let prev = app.stations.len();
                app.stations = stations;
                app.sel = app.sel.min(app.stations.len().saturating_sub(1));
                if app.current.map(|i| i >= app.stations.len()).unwrap_or(false) {
                    app.player.send(PlayerCmd::Stop);
                    app.current = None;
                }
                let diff = app.stations.len() as isize - prev as isize;
                app.set_status(format!("Reloaded: {} stations ({:+})", app.stations.len(), diff));
            }
            Err(e) => app.set_status(format!("Reload failed: {e}")),
        }
    } else if m(&kb.transparent) {
        app.transparent = !app.transparent;
        app.set_status(if app.transparent { "Transparent mode on".into() } else { "Opaque mode on".into() });
    } else if m(&kb.hide_help) {
        app.hide_help = !app.hide_help;
    } else if m(&kb.zen) {
        app.zen_mode = !app.zen_mode;
        app.set_status(if app.zen_mode { "Zen mode on".into() } else { "Zen mode off".into() });
    } else if m(&kb.visualizer) {
        app.show_vis = !app.show_vis;
        if !app.show_vis { app.spectrum.iter_mut().for_each(|s| *s = 0.0); }
        app.set_status(if app.show_vis { "Visualizer on".into() } else { "Visualizer off".into() });
    } else if m(&kb.theme) {
        let idx = app.themes.iter()
            .position(|th| th.name == app.theme.name)
            .unwrap_or(0);
        app.mode = AppMode::ThemePicker { selected: idx };
    } else if m(&kb.history) {
        app.mode = AppMode::History { selected: 0 };
    } else if m(&kb.toggle_notify) {
        app.config.notify = !app.config.notify;
        let _ = app.config.save();
        app.set_status(if app.config.notify {
            "Notifications on".into()
        } else {
            "Notifications off".into()
        });
    } else if m(&kb.chiquito) {
        app.toggle_chiquito();
    }

    Ok(())
}
