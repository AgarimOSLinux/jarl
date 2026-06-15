use std::collections::HashSet;
use std::sync::Arc;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::config::{Config, Keybindings};
use crate::visualizer::{compute_spectrum, smooth, SampleBuffer};
use crate::icy_meta::TrackTitle;
use crate::favorites;
use crate::history::{self, HistoryEntry};
use crate::meta_poll::MetaPoll;
use crate::player::{Player, PlayerCmd};
use crate::stations::{load_stations, save_stations, Station};
use crate::theme::{all_themes, find_theme, Theme};
use crate::ui;

// ── App mode ──────────────────────────────────────────────────────────────────

/// Redirects stderr to the jarl log file so ALSA/library noise
/// doesn't corrupt the TUI display.
fn redirect_stderr_to_log() {
    use std::fs::OpenOptions;
    let path = crate::logger::log_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        use std::os::fd::IntoRawFd;
        unsafe {
            let fd = f.into_raw_fd();
            libc::dup2(fd, libc::STDERR_FILENO);
            libc::close(fd);
        }
    }
}

pub enum AppMode {
    Normal,
    ThemePicker   { selected: usize },
    ConfirmDelete { index: usize },
    Search,
    History       { selected: usize },
}

// ── App state ─────────────────────────────────────────────────────────────────

pub struct App {
    pub stations:   Vec<Station>,
    pub sel:        usize,
    pub current:    Option<usize>,
    pub player:     Player,
    pub theme:      Theme,
    pub themes:     Vec<Theme>,
    pub config:     Config,
    pub mode:       AppMode,
    pub tick:       u64,
    pub favorites:  HashSet<String>,
    pub fav_filter: bool,
    pub status_msg:   Option<(String, u64)>,
    pub zen_mode:     bool,
    pub show_vis:     bool,
    pub transparent:  bool,
    pub should_quit:  bool,
    pub hide_help:    bool,
    pub spectrum:     Vec<f32>,
    pub sample_buf:   SampleBuffer,
    pub track_title:  TrackTitle,
    /// Ticker scroll offset for long track titles (advances every ~0.5s)
    pub ticker_offset: usize,
    /// Current search query (non-empty while Search mode is active or filter persists).
    pub search_query: String,
    /// Active metadata poller (Some when the current station has a metadata_url).
    meta_poll: Option<MetaPoll>,
    /// Last track title for which a desktop notification was sent.
    last_notified_title: Option<String>,
    /// Playback history (most recent first).
    pub history: Vec<HistoryEntry>,
}

impl App {
    pub fn new(config: Config, first_run: bool) -> Result<Self> {
        let stations  = load_stations().unwrap_or_else(|_| crate::stations::default_stations());
        let themes    = all_themes();
        let theme     = find_theme(&config.theme);
        let player    = Player::new(config.volume);
        let favorites = favorites::load();

        let sample_buf  = Arc::clone(&player.sample_buf);
        let track_title = Arc::clone(&player.track_title);
        let mut app = Self {
            stations, sel: 0, current: None,
            player, theme, themes, config,
            mode: AppMode::Normal, tick: 0,
            favorites, fav_filter: false, status_msg: None,
            zen_mode: false,
            show_vis:  true,
            transparent: false,
            should_quit:  false,
            hide_help:    false,
            spectrum:  vec![0.0; 32],
            sample_buf,
            track_title,
            ticker_offset: 0,
            search_query: String::new(),
            meta_poll: None,
            last_notified_title: None,
            history: history::load(),
        };

        if first_run {
            app.set_status(format!(
                "Welcome! Config created at {}",
                Config::config_dir().display()
            ));
        }
        Ok(app)
    }

    // ── Event loop ────────────────────────────────────────────────────────────

    pub fn run(&mut self) -> Result<()> {
        redirect_stderr_to_log();
        enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let result = self.event_loop(&mut terminal);
        self.player.send(PlayerCmd::Quit);
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        result
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        const TICK_MS: u64 = 120;
        let tick_rate = Duration::from_millis(TICK_MS);
        let mut last_tick = Instant::now();

        loop {
            if self.should_quit { return Ok(()); }
            terminal.draw(|f| ui::draw(f, self))?;

            let timeout = tick_rate.checked_sub(last_tick.elapsed()).unwrap_or_default();
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press { continue; }
                    self.handle_key(key.code, key.modifiers)?;
                }
            }

            if last_tick.elapsed() >= tick_rate {
                self.tick = self.tick.wrapping_add(1);
                last_tick = Instant::now();
                if let Some((_, born)) = self.status_msg {
                    if self.tick.wrapping_sub(born) > 25 {
                        self.status_msg = None;
                    }
                }
                if self.show_vis {
                    let raw = compute_spectrum(&self.sample_buf, self.spectrum.len());
                    smooth(&mut self.spectrum, &raw, 0.6, 0.15);
                }
                if self.tick % 4 == 0 {
                    self.ticker_offset = self.ticker_offset.wrapping_add(1);
                }
                self.maybe_notify_track_change();
            }
        }
    }

    // ── Input dispatch ────────────────────────────────────────────────────────

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Result<()> {
        match &self.mode {
            AppMode::Normal              => self.handle_normal(code, mods),
            AppMode::ThemePicker  { .. } => self.handle_theme_picker(code, mods),
            AppMode::ConfirmDelete { .. } => self.handle_confirm_delete(code),
            AppMode::Search              => self.handle_search(code, mods),
            AppMode::History      { .. } => self.handle_history(code, mods),
        }
    }

    // ── Normal mode ───────────────────────────────────────────────────────────

    fn handle_normal(&mut self, code: KeyCode, mods: KeyModifiers) -> Result<()> {
        let kb = self.config.keybindings.clone();
        let n  = self.visible_stations().len();

        let m = |s: &str| Keybindings::matches(s, code, mods);

        if m(&kb.quit) {
            self.save_config()?;
            self.should_quit = true;
        }

        if m(&kb.nav_down) || code == KeyCode::Down {
            if n > 0 { self.sel = (self.sel + 1).min(n - 1); }
        } else if m(&kb.nav_up) || code == KeyCode::Up {
            self.sel = self.sel.saturating_sub(1);
        } else if m(&kb.nav_top) || code == KeyCode::Home {
            self.sel = 0;
        } else if m(&kb.nav_bottom) || code == KeyCode::End {
            if n > 0 { self.sel = n - 1; }
        } else if code == KeyCode::PageDown
                  || (code == KeyCode::Char('d') && mods == KeyModifiers::CONTROL) {
            if n > 0 { self.sel = (self.sel + 10).min(n - 1); }
        } else if code == KeyCode::PageUp
                  || (code == KeyCode::Char('u') && mods == KeyModifiers::CONTROL) {
            self.sel = self.sel.saturating_sub(10);

        // Search  ('/') ── vim convention
        } else if code == KeyCode::Char('/') {
            self.search_query = String::new();
            self.sel = 0;
            self.mode = AppMode::Search;

        // Playback
        } else if m(&kb.play) {
            self.play_selected();
        } else if m(&kb.pause) {
            if self.current.is_some() { self.player.send(PlayerCmd::TogglePause); }
        } else if m(&kb.stop) {
            self.player.send(PlayerCmd::Stop);
            self.meta_poll = None;
            self.last_notified_title = None;
            self.current = None;

        // Volume
        } else if m(&kb.volume_up) {
            self.adjust_volume(0.05);
        } else if m(&kb.volume_down) {
            self.adjust_volume(-0.05);

        // Favourites
        } else if m(&kb.favourite) {
            if let Some(idx) = self.real_index(self.sel) {
                let name = self.stations[idx].name.clone();
                if self.favorites.contains(&name) {
                    self.favorites.remove(&name);
                    self.set_status(format!("Removed from favourites: {name}"));
                } else {
                    self.favorites.insert(name.clone());
                    self.set_status(format!("Added to favourites: {name}"));
                }
                let _ = favorites::save(&self.favorites);
            }
        } else if m(&kb.fav_filter) {
            self.fav_filter = !self.fav_filter;
            self.sel = 0;
            self.set_status(if self.fav_filter {
                "Showing favourites only".into()
            } else {
                "Showing all stations".into()
            });

        // Station management
        } else if m(&kb.delete) {
            if let Some(idx) = self.real_index(self.sel) {
                self.mode = AppMode::ConfirmDelete { index: idx };
            }

        } else if m(&kb.reload_themes) {
            let new_themes = crate::theme::all_themes();
            if new_themes.is_empty() {
                self.set_status("No active themes in themes.toml — edit the file and set enabled = true".into());
            } else {
                let prev_name = self.theme.name.clone();
                self.themes = new_themes;
                if let Some(t) = self.themes.iter().find(|t| t.name == prev_name) {
                    self.theme = t.clone();
                } else {
                    self.theme = self.themes[0].clone();
                    self.config.theme = self.theme.name.clone();
                    let _ = self.config.save();
                    self.set_status(format!("Theme '{}' no longer active — switched to {}", prev_name, self.theme.name));
                }
                if self.theme.name == prev_name {
                    self.set_status(format!("Themes reloaded: {} available", self.themes.len()));
                }
            }
        } else if m(&kb.reload) {
            match load_stations() {
                Ok(stations) => {
                    let prev = self.stations.len();
                    self.stations = stations;
                    self.sel = self.sel.min(self.stations.len().saturating_sub(1));
                    if self.current.map(|i| i >= self.stations.len()).unwrap_or(false) {
                        self.player.send(PlayerCmd::Stop);
                        self.current = None;
                    }
                    let diff = self.stations.len() as isize - prev as isize;
                    self.set_status(format!("Reloaded: {} stations ({:+})", self.stations.len(), diff));
                }
                Err(e) => self.set_status(format!("Reload failed: {e}")),
            }
        } else if m(&kb.transparent) {
            self.transparent = !self.transparent;
            self.set_status(if self.transparent { "Transparent mode on".into() } else { "Opaque mode on".into() });
        } else if m(&kb.hide_help) {
            self.hide_help = !self.hide_help;
        } else if m(&kb.zen) {
            self.zen_mode = !self.zen_mode;
            self.set_status(if self.zen_mode { "Zen mode on".into() } else { "Zen mode off".into() });
        } else if m(&kb.visualizer) {
            self.show_vis = !self.show_vis;
            if !self.show_vis { self.spectrum.iter_mut().for_each(|s| *s = 0.0); }
            self.set_status(if self.show_vis { "Visualizer on".into() } else { "Visualizer off".into() });
        } else if m(&kb.theme) {
            let idx = self.themes.iter()
                .position(|th| th.name == self.theme.name)
                .unwrap_or(0);
            self.mode = AppMode::ThemePicker { selected: idx };
        } else if m(&kb.history) {
            self.mode = AppMode::History { selected: 0 };
        } else if m(&kb.toggle_notify) {
            self.config.notify = !self.config.notify;
            let _ = self.config.save();
            self.set_status(if self.config.notify {
                "Notifications on".into()
            } else {
                "Notifications off".into()
            });
        }

        Ok(())
    }

    // ── Search mode ───────────────────────────────────────────────────────────

    fn handle_search(&mut self, code: KeyCode, _mods: KeyModifiers) -> Result<()> {
        match code {
            KeyCode::Esc => {
                // Exit search: clear query and go back to Normal.
                self.mode = AppMode::Normal;
                self.search_query = String::new();
                self.sel = 0;
            }
            KeyCode::Enter => {
                // Play the currently highlighted station and exit search.
                self.mode = AppMode::Normal;
                self.play_selected();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                let n = self.visible_stations().len();
                if n > 0 { self.sel = self.sel.min(n - 1); } else { self.sel = 0; }
            }
            KeyCode::Down => {
                let n = self.visible_stations().len();
                if n > 0 { self.sel = (self.sel + 1).min(n - 1); }
            }
            KeyCode::Up => {
                self.sel = self.sel.saturating_sub(1);
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                // Reset selection when the result set changes.
                self.sel = 0;
            }
            _ => {}
        }
        Ok(())
    }

    // ── History mode ──────────────────────────────────────────────────────────

    fn handle_history(&mut self, code: KeyCode, mods: KeyModifiers) -> Result<()> {
        let kb = self.config.keybindings.clone();
        let n  = self.history.len();

        if let AppMode::History { ref mut selected } = self.mode {
            let m = |s: &str| Keybindings::matches(s, code, mods);

            if code == KeyCode::Esc || m(&kb.quit) || m(&kb.history) {
                self.mode = AppMode::Normal;
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
                self.mode = AppMode::Normal;
                if let Some(entry) = self.history.get(idx).cloned() {
                    self.play_from_history(entry);
                }
            }
        }
        Ok(())
    }

    fn handle_theme_picker(&mut self, code: KeyCode, mods: KeyModifiers) -> Result<()> {
        let kb = self.config.keybindings.clone();
        let n  = self.themes.len();

        if let AppMode::ThemePicker { ref mut selected } = self.mode {
            let m = |s: &str| Keybindings::matches(s, code, mods);

            if m(&kb.nav_down) || code == KeyCode::Down {
                if n > 0 { *selected = (*selected + 1).min(n - 1); }
            } else if m(&kb.nav_up) || code == KeyCode::Up {
                *selected = selected.saturating_sub(1);
            } else if m(&kb.play) {
                let idx = *selected;
                self.theme = self.themes[idx].clone();
                self.config.theme = self.theme.name.clone();
                let _ = self.config.save();
                self.set_status(format!("Theme: {}", self.theme.name));
                self.mode = AppMode::Normal;
            } else if code == KeyCode::Esc || m(&kb.theme) || m(&kb.quit) {
                self.mode = AppMode::Normal;
            }
        }
        Ok(())
    }

    // ── Confirm delete ────────────────────────────────────────────────────────

    fn handle_confirm_delete(&mut self, code: KeyCode) -> Result<()> {
        let idx = if let AppMode::ConfirmDelete { index } = self.mode { index }
                  else { return Ok(()); };
        if matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')) {
            let name = self.stations[idx].name.clone();
            if self.current == Some(idx) {
                self.player.send(PlayerCmd::Stop);
                self.current = None;
            } else if let Some(c) = self.current {
                if c > idx { self.current = Some(c - 1); }
            }
            self.favorites.remove(&name);
            let _ = favorites::save(&self.favorites);
            self.stations.remove(idx);
            self.sel = self.sel.min(self.stations.len().saturating_sub(1));
            match save_stations(&self.stations) {
                Ok(_)  => self.set_status(format!("Deleted: {name}")),
                Err(e) => self.set_status(format!("Deleted from memory (save failed: {e})")),
            }
        }
        self.mode = AppMode::Normal;
        Ok(())
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    pub fn visible_stations(&self) -> Vec<(usize, &Station)> {
        let q = self.search_query.to_lowercase();
        let mut result: Vec<(usize, &Station)> = self.stations.iter().enumerate()
            .filter(|(_, s)| !self.fav_filter || self.favorites.contains(&s.name))
            .filter(|(_, s)| {
                q.is_empty()
                    || s.name.to_lowercase().contains(&q)
                    || s.genre.to_lowercase().contains(&q)
            })
            .collect();
        result.sort_by_key(|(_, s)| !self.favorites.contains(&s.name));
        result
    }

    fn real_index(&self, vis_sel: usize) -> Option<usize> {
        self.visible_stations().get(vis_sel).map(|(i, _)| *i)
    }

    /// Plays the currently selected station (shared by Normal and Search modes).
    fn play_selected(&mut self) {
        if let Some(idx) = self.real_index(self.sel) {
            let station  = &self.stations[idx];
            let url      = station.url.clone();
            let murl     = station.metadata_url.clone();
            let entry    = HistoryEntry {
                name:  station.name.clone(),
                genre: station.genre.clone(),
                url:   url.clone(),
            };
            self.meta_poll = murl.map(|m| MetaPoll::spawn(m, Arc::clone(&self.track_title)));
            self.current = Some(idx);
            self.ticker_offset = 0;
            self.last_notified_title = None;
            history::push(entry, &mut self.history);
            let _ = history::save(&self.history);
            self.player.play(url);
        }
    }

    /// Play a station from the history list (URL may no longer be in stations list).
    fn play_from_history(&mut self, entry: HistoryEntry) {
        // Try to find the station in the current list to get metadata_url.
        let station_idx = self.stations.iter().position(|s| s.url == entry.url);
        let murl = station_idx
            .and_then(|i| self.stations[i].metadata_url.clone());
        let url = entry.url.clone();

        self.meta_poll = murl.map(|m| MetaPoll::spawn(m, Arc::clone(&self.track_title)));
        self.current = station_idx;
        self.ticker_offset = 0;
        self.last_notified_title = None;
        history::push(entry, &mut self.history);
        let _ = history::save(&self.history);
        self.player.play(url);
    }

    fn adjust_volume(&mut self, delta: f32) {
        let v = (self.player.volume() + delta).clamp(0.0, 1.0);
        self.config.volume = v;
        self.player.send(PlayerCmd::Volume(v));
    }

    fn save_config(&mut self) -> Result<()> {
        self.config.volume = self.player.volume();
        self.config.theme  = self.theme.name.clone();
        self.config.save()
    }

    pub fn set_status(&mut self, msg: String) {
        self.status_msg = Some((msg, self.tick));
    }

    /// Send a desktop notification via `notify-send` if the track title has
    /// changed since the last notification. Silently does nothing if
    /// `notify-send` is not installed or the notification fails.
    fn maybe_notify_track_change(&mut self) {
        let current_title = self.track_title.lock().ok()
            .and_then(|g| g.clone());

        if current_title == self.last_notified_title { return; }
        self.last_notified_title = current_title.clone();

        let Some(title) = current_title else { return; };
        if !self.config.notify { return; }
        let station_name = self.current
            .map(|i| self.stations[i].name.as_str())
            .unwrap_or("jarl");

        std::thread::spawn({
            let title       = title.clone();
            let station     = station_name.to_string();
            move || {
                let _ = std::process::Command::new("notify-send")
                    .arg("--app-name=jarl")
                    .arg("--icon=audio-x-generic")
                    .arg("--expire-time=4000")
                    .arg(&station)
                    .arg(&title)
                    .status();
            }
        });
    }
}
