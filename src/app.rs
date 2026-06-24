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

use crate::config::Config;
use crate::visualizer::{compute_spectrum, is_silent, smooth, SampleBuffer};
use crate::icy_meta::TrackTitle;
use crate::favorites;
use crate::history::{self, HistoryEntry};
use crate::meta_poll::MetaPoll;
use crate::player::{Player, PlayerCmd, PlayerStatus};
use crate::stations::{load_stations, Station};
use crate::theme::{all_themes, find_theme, Theme};
use crate::ui;

// ── Startup helpers ───────────────────────────────────────────────────────────

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

// ── App mode ──────────────────────────────────────────────────────────────────

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
    /// Consecutive ticks where the audio buffer has measured as silent
    /// while a station is supposedly playing. Used to detect "dead air"
    /// (stream connected but producing no real audio) and force a
    /// reconnect. Resets to 0 whenever sound resumes or playback stops.
    silent_ticks: u32,
    /// When true, the help bar shows Chiquito de la Calzada quotes instead
    /// of the keyboard shortcuts.
    pub chiquito_mode: bool,
    /// Index into `quotes::QUOTES` for the quote currently shown in the help bar.
    pub chiquito_bar_idx: usize,
    /// Tick at which the last Chiquito quote rotation happened.
    chiquito_last_rotate: u64,
}

impl App {
    pub fn new(config: Config, first_run: bool) -> Result<Self> {
        let (stations, stations_load_error) = match load_stations() {
            Ok(s)  => (s, false),
            Err(e) => {
                log::warn!("stations.toml load/parse error, using bundled defaults: {e}");
                (crate::stations::default_stations(), true)
            }
        };
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
            silent_ticks: 0,
            chiquito_mode: false,
            chiquito_bar_idx: 0,
            chiquito_last_rotate: 0,
        };

        if first_run {
            app.set_status(format!(
                "Welcome! Config created at {}",
                Config::config_dir().display()
            ));
        } else if stations_load_error {
            app.set_status("stations.toml invalid — loaded built-in defaults (see jarl.log)".to_string());
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
                self.check_dead_air();
                self.maybe_rotate_chiquito();
            }
        }
    }

    // ── Input dispatch ────────────────────────────────────────────────────────

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Result<()> {
        match &self.mode {
            AppMode::Normal               => crate::modes::normal::handle(self, code, mods),
            AppMode::ThemePicker   { .. }  => crate::modes::theme_picker::handle(self, code, mods),
            AppMode::ConfirmDelete { .. }  => crate::modes::confirm_delete::handle(self, code),
            AppMode::Search                => crate::modes::search::handle(self, code),
            AppMode::History       { .. }  => crate::modes::history::handle(self, code, mods),
        }
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

    pub(crate) fn real_index(&self, vis_sel: usize) -> Option<usize> {
        self.visible_stations().get(vis_sel).map(|(i, _)| *i)
    }

    /// Plays the next/previous station relative to the one currently playing,
    /// within the visible (filtered/searched) list. `delta` is +1 or -1.
    /// If nothing is playing, falls back to the current cursor position.
    pub(crate) fn play_relative(&mut self, delta: isize) {
        let visible = self.visible_stations();
        if visible.is_empty() { return; }

        let cur_vis_pos = self.current
            .and_then(|real| visible.iter().position(|(i, _)| *i == real))
            .unwrap_or(self.sel);

        let len = visible.len() as isize;
        let new_pos = ((cur_vis_pos as isize + delta).rem_euclid(len)) as usize;

        self.sel = new_pos;
        self.play_selected();
    }

    /// Plays the currently selected station (shared by Normal and Search modes).
    pub(crate) fn play_selected(&mut self) {
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
            self.silent_ticks = 0;
            history::push(entry, &mut self.history);
            let _ = history::save(&self.history);
            self.player.play(url);
        }
    }

    /// Play a station from the history list (URL may no longer be in stations list).
    pub(crate) fn play_from_history(&mut self, entry: HistoryEntry) {
        // Try to find the station in the current list to get metadata_url.
        let station_idx = self.stations.iter().position(|s| s.url == entry.url);
        let murl = station_idx
            .and_then(|i| self.stations[i].metadata_url.clone());
        let url = entry.url.clone();

        self.meta_poll = murl.map(|m| MetaPoll::spawn(m, Arc::clone(&self.track_title)));
        self.current = station_idx;
        self.ticker_offset = 0;
        self.last_notified_title = None;
        self.silent_ticks = 0;
        history::push(entry, &mut self.history);
        let _ = history::save(&self.history);
        self.player.play(url);
    }

    /// Clears now-playing state (metadata poller, last-notified title,
    /// current station index) without touching the player itself — used
    /// when stopping playback explicitly. Pairs with `PlayerCmd::Stop`,
    /// which the caller is expected to have already sent.
    pub(crate) fn clear_now_playing(&mut self) {
        self.meta_poll = None;
        self.last_notified_title = None;
        self.current = None;
        self.chiquito_last_rotate = self.tick;
    }

    pub(crate) fn adjust_volume(&mut self, delta: f32) {
        let v = (self.player.volume() + delta).clamp(0.0, 1.0);
        self.config.volume = v;
        self.player.send(PlayerCmd::Volume(v));
    }

    pub(crate) fn save_config(&mut self) -> Result<()> {
        self.config.volume = self.player.volume();
        self.config.theme  = self.theme.name.clone();
        self.config.save()
    }

    pub fn set_status(&mut self, msg: String) {
        self.status_msg = Some((msg, self.tick));
    }

    /// Detects "dead air": the player reports `Playing` (stream still
    /// connected, no I/O error) but the decoded audio has been silent for
    /// several consecutive ticks. This happens with some stations whose
    /// CDN edge keeps the TCP connection alive while serving flat silence
    /// after a backend failure — something the normal reconnect logic in
    /// `player.rs` can't catch, since from its point of view nothing failed.
    ///
    /// At the default 120ms tick rate, `DEAD_AIR_TICKS` ticks is roughly
    /// the threshold below before triggering a forced reconnect. This is
    /// deliberately generous to avoid false positives during quiet musical
    /// passages or intentional dead-air idents.
    fn check_dead_air(&mut self) {
        const DEAD_AIR_TICKS: u32 = 100; // ~12s at 120ms/tick

        let Some(idx) = self.current else { self.silent_ticks = 0; return; };
        if self.player.status() != PlayerStatus::Playing {
            self.silent_ticks = 0;
            return;
        }

        if is_silent(&self.sample_buf) {
            self.silent_ticks = self.silent_ticks.saturating_add(1);
        } else {
            self.silent_ticks = 0;
            return;
        }

        if self.silent_ticks >= DEAD_AIR_TICKS {
            self.silent_ticks = 0;
            let url = self.stations[idx].url.clone();
            log::warn!("dead air detected on '{}', forcing reconnect", self.stations[idx].name);
            self.set_status(format!("No audio detected — reconnecting to {}…", self.stations[idx].name));
            self.player.play(url);
        }
    }

    /// Toggle the Chiquito de la Calzada mode on the help bar.
    /// Picks a random quote on activation and records the tick for rotation.
    pub fn toggle_chiquito(&mut self) {
        self.chiquito_mode = !self.chiquito_mode;
        if self.chiquito_mode {
            self.chiquito_bar_idx = self.random_quote_idx();
            self.chiquito_last_rotate = self.tick;
        }
    }

    /// Rotate to a new random quote every 30 seconds while chiquito_mode is on
    /// and a station is playing.
    fn maybe_rotate_chiquito(&mut self) {
        if !self.chiquito_mode { return; }
        if self.player.status() != PlayerStatus::Playing { return; }
        // TICK_MS = 120ms → 250 ticks ≈ 30 seconds
        const ROTATE_TICKS: u64 = 250;
        if self.tick.wrapping_sub(self.chiquito_last_rotate) >= ROTATE_TICKS {
            self.chiquito_bar_idx = self.random_quote_idx();
            self.chiquito_last_rotate = self.tick;
        }
    }

    /// Returns a pseudo-random index into QUOTES, avoiding repeating the
    /// current one if there is more than one quote available.
    fn random_quote_idx(&self) -> usize {
        let len = crate::quotes::QUOTES.len();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(self.tick as usize);
        let mut idx = nanos % len;
        if len > 1 && idx == self.chiquito_bar_idx {
            idx = (idx + 1) % len;
        }
        idx
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
