use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};

pub const DEFAULT_STATIONS_TOML: &str = include_str!("../stations.toml");

// ── Keybindings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Keybindings {
    pub quit:         String,
    pub play:         String,
    pub pause:        String,
    pub stop:         String,
    pub nav_up:       String,
    pub nav_down:     String,
    pub nav_top:      String,
    pub nav_bottom:   String,
    pub volume_up:    String,
    pub volume_down:  String,
    pub favourite:    String,
    pub fav_filter:   String,
    pub delete:       String,
    pub theme:        String,
    pub reload:       String,
    pub reload_themes: String,
    pub zen:          String,
    pub hide_help:    String,
    pub visualizer:   String,
    pub transparent:  String,
    pub history:      String,
    pub toggle_notify: String,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            quit:        "q".into(),
            play:        "enter".into(),
            pause:       "space".into(),
            stop:        "s".into(),
            nav_up:      "k".into(),
            nav_down:    "j".into(),
            nav_top:     "g".into(),
            nav_bottom:  "G".into(),
            volume_up:   "+".into(),
            volume_down: "-".into(),
            favourite:   "f".into(),
            fav_filter:  "F".into(),
            delete:      "d".into(),
            theme:       "t".into(),
            reload:      "r".into(),
            reload_themes: "T".into(),
            zen:         "z".into(),
            hide_help:   "h".into(),
            visualizer:  "v".into(),
            transparent: "p".into(),
            history:       "H".into(),
            toggle_notify: "N".into(),
        }
    }
}

impl Keybindings {
    /// Parse a binding string like "j", "enter", "ctrl+c" into (KeyCode, KeyModifiers).
    pub fn parse(s: &str) -> Option<(KeyCode, KeyModifiers)> {
        let s = s.trim();
        // Modifier prefix
        let (mods, key) = if let Some(rest) = s.strip_prefix("ctrl+") {
            (KeyModifiers::CONTROL, rest)
        } else if let Some(rest) = s.strip_prefix("alt+") {
            (KeyModifiers::ALT, rest)
        } else {
            (KeyModifiers::NONE, s)
        };

        let code = match key.to_lowercase().as_str() {
            "enter"    => KeyCode::Enter,
            "space"    => KeyCode::Char(' '),
            "esc"      => KeyCode::Esc,
            "up"       => KeyCode::Up,
            "down"     => KeyCode::Down,
            "left"     => KeyCode::Left,
            "right"    => KeyCode::Right,
            "pageup"   => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "home"     => KeyCode::Home,
            "end"      => KeyCode::End,
            "tab"      => KeyCode::Tab,
            "backspace"=> KeyCode::Backspace,
            "delete"   => KeyCode::Delete,
            c if c.chars().count() == 1 => {
                // Preserve original case for the char (uppercase matters for shift).
                let ch = key.chars().next().unwrap();
                KeyCode::Char(ch)
            }
            _ => return None,
        };
        Some((code, mods))
    }

    /// Returns true if (code, mods) matches this binding string.
    ///
    /// For single uppercase characters (e.g. "F", "G"), also accepts
    /// KeyModifiers::SHIFT because some terminals send Shift+char for capitals.
    pub fn matches(binding: &str, code: KeyCode, mods: KeyModifiers) -> bool {
        match Self::parse(binding) {
            Some((c, m)) => {
                if c == code && m == mods { return true; }
                // Uppercase single char: also accept with SHIFT modifier.
                if let KeyCode::Char(ch) = c {
                    if ch.is_uppercase() && c == code && mods == KeyModifiers::SHIFT {
                        return true;
                    }
                }
                false
            }
            None => false,
        }
    }

    /// Display string for a binding (used in the help bar).
    pub fn display(binding: &str) -> String {
        match binding.to_lowercase().as_str() {
            "enter" => "⏎".into(),
            "space" => "Spc".into(),
            "up"    => "↑".into(),
            "down"  => "↓".into(),
            _       => binding.to_string(),
        }
    }
}

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme:       String,
    pub volume:      f32,
    pub notify:      bool,
    pub keybindings: Keybindings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme:       "Dracula".into(),
            volume:      0.80,
            notify:      true,
            keybindings: Keybindings::default(),
        }
    }
}

// ── Paths ─────────────────────────────────────────────────────────────────────

impl Config {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                    .join(".config")
            })
            .join("jarl")
    }

    pub fn config_path()   -> PathBuf { Self::config_dir().join("config.toml")   }
    pub fn stations_path() -> PathBuf { Self::config_dir().join("stations.toml") }
    pub fn themes_path()       -> PathBuf { Self::config_dir().join("themes.toml")       }

    // ── Bootstrap ─────────────────────────────────────────────────────────────

    pub fn bootstrap() -> Result<bool> {
        let dir = Self::config_dir();
        let first_run = !dir.exists();
        fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create config dir: {}", dir.display()))?;

        let sp = Self::stations_path();
        if !sp.exists() {
            fs::write(&sp, DEFAULT_STATIONS_TOML)?;
        }

        let tp = Self::themes_path();
        if !tp.exists() {
            fs::write(&tp, crate::theme::DEFAULT_THEMES_TOML)?;
        }

        let cp = Self::config_path();
        if !cp.exists() {
            fs::write(&cp, Config::default().to_commented_toml())?;
        }

        Ok(first_run)
    }

    // ── Load / save ───────────────────────────────────────────────────────────

    pub fn load() -> Result<(Self, bool)> {
        let first_run = Self::bootstrap()?;
        let raw = fs::read_to_string(Self::config_path()).unwrap_or_default();
        let cfg = toml::from_str::<Config>(&raw).unwrap_or_default();
        Ok((cfg, first_run))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(p) = path.parent() { fs::create_dir_all(p)?; }
        // Save only the data fields (not the full commented template) so the
        // file stays machine-writable (volume is updated on quit).
        let raw = toml::to_string_pretty(self).context("serialize config")?;
        fs::write(&path, raw).context("write config.toml")?;
        Ok(())
    }

    /// Generate the fully annotated config.toml written on first run.
    pub fn to_commented_toml(&self) -> String {
        let kb = &self.keybindings;
        format!(r#"# jarl – configuration
# All fields are optional; the values shown here are the defaults.
# Edit this file and restart jarl (or press 'r' to reload stations).

# ── Appearance ────────────────────────────────────────────────────────────────
#
# Active theme name.
# Built-in: Dracula | Catppuccin Mocha | Nord | Gruvbox Dark | Tokyo Night | Everforest Dark
# Custom themes can be added to: {themes_path}
theme = "{theme}"

# ── Audio ─────────────────────────────────────────────────────────────────────
#
# Initial volume level.  Range: 0.0 (silent) – 1.0 (maximum).
# This value is updated automatically when you quit jarl.
volume = {volume:.2}

# Desktop notifications when the track changes (requires notify-send).
# Set to false to disable.
notify = {notify}

# ── Key bindings ──────────────────────────────────────────────────────────────
#
# Change any binding here to remap it.
#
# Format:
#   Single character : "j"  "k"  "q"  "+"  etc.
#   Named keys       : "enter"  "space"  "esc"
#                      "up"  "down"  "left"  "right"
#                      "pageup"  "pagedown"  "home"  "end"
#   With modifiers   : "ctrl+c"  "ctrl+d"  "alt+j"  etc.
#
# Note: "ctrl+c" is always a hard-coded fallback for quit.
# Note: "/" (search stations) is hard-coded and cannot be remapped.

[keybindings]
# Navigation
nav_up      = "{nav_up}"         # move selection up
nav_down    = "{nav_down}"       # move selection down
nav_top     = "{nav_top}"        # jump to first station
nav_bottom  = "{nav_bottom}"     # jump to last station

# Playback
play        = "{play}"           # play selected station
pause       = "{pause}"          # pause / resume
stop        = "{stop}"           # stop playback

# Volume
volume_up   = "{volume_up}"      # increase volume by 5%
volume_down = "{volume_down}"    # decrease volume by 5%

# Station management
favourite   = "{favourite}"      # toggle ★ favourite on selected station
fav_filter  = "{fav_filter}"     # toggle favourites-only filter
delete      = "{delete}"         # delete selected station from stations.toml
reload         = "{reload}"         # reload stations.toml from disk
reload_themes  = "{reload_themes}"  # reload themes.toml from disk

# UI
theme       = "{theme_key}"      # open theme picker
zen         = "{zen}"            # toggle zen mode (hide header & help bar)
hide_help   = "{hide_help}"      # toggle help bar visibility
visualizer  = "{visualizer}"     # toggle spectrum visualizer
transparent = "{transparent}"    # toggle transparent / opaque background
history     = "{history}"        # open playback history
toggle_notify = "{toggle_notify}" # toggle desktop notifications
quit        = "{quit}"           # quit jarl
"#,
            themes_path   = Self::themes_path().display(),
            theme         = self.theme,
            volume        = self.volume,
            notify        = self.notify,
            nav_up        = kb.nav_up,
            nav_down      = kb.nav_down,
            nav_top       = kb.nav_top,
            nav_bottom    = kb.nav_bottom,
            play          = kb.play,
            pause         = kb.pause,
            stop          = kb.stop,
            volume_up     = kb.volume_up,
            volume_down   = kb.volume_down,
            favourite     = kb.favourite,
            fav_filter    = kb.fav_filter,
            delete        = kb.delete,
            reload        = kb.reload,
            reload_themes = kb.reload_themes,
            theme_key     = kb.theme,
            zen           = kb.zen,
            hide_help     = kb.hide_help,
            visualizer    = kb.visualizer,
            transparent   = kb.transparent,
            history       = kb.history,
            toggle_notify = kb.toggle_notify,
            quit          = kb.quit,
        )
    }
}
