use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::fs;

use crate::config::Config;

// ── Theme struct ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Theme {
    pub name:      String,
    pub bg:        Color,
    pub fg:        Color,
    pub accent:    Color,
    pub highlight: Color,
    pub success:   Color,
    pub warning:   Color,
    pub error:     Color,
    pub muted:     Color,
    pub border:    Color,
}

// ── Hex helper ────────────────────────────────────────────────────────────────

fn hex(s: &str) -> Color {
    let s = s.trim_start_matches('#');
    if s.len() < 6 { return Color::Reset; }
    let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(128);
    let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(128);
    let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(128);
    Color::Rgb(r, g, b)
}

// ── TOML schema ───────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize)]
pub struct ThemeEntry {
    pub name:      String,
    #[serde(default = "default_enabled")]
    pub enabled:   bool,
    pub bg:        String,
    pub fg:        String,
    pub accent:    String,
    pub highlight: String,
    pub success:   String,
    pub warning:   String,
    pub error:     String,
    pub muted:     String,
    pub border:    String,
}

fn default_enabled() -> bool { true }

#[derive(Deserialize)]
struct ThemesFile {
    #[serde(default)]
    themes: Vec<ThemeEntry>,
}

impl From<&ThemeEntry> for Theme {
    fn from(e: &ThemeEntry) -> Self {
        Self {
            name:      e.name.clone(),
            bg:        hex(&e.bg),
            fg:        hex(&e.fg),
            accent:    hex(&e.accent),
            highlight: hex(&e.highlight),
            success:   hex(&e.success),
            warning:   hex(&e.warning),
            error:     hex(&e.error),
            muted:     hex(&e.muted),
            border:    hex(&e.border),
        }
    }
}

// ── Default themes TOML (embedded in binary) ──────────────────────────────────
// Only Catppuccin Mocha is enabled by default.
// To activate a theme: either uncomment its block, or set enabled = true.
// To deactivate a theme: either comment out its block, or set enabled = false.
// At least one theme must remain active.

pub const DEFAULT_THEMES_TOML: &str = include_str!("../themes.toml");

// ── Absolute fallback (used only if themes.toml cannot be loaded) ─────────────

fn fallback_theme() -> Theme {
    Theme {
        name:      "Catppuccin Mocha".into(),
        bg:        hex("#1e1e2e"), fg:        hex("#cdd6f4"),
        accent:    hex("#cba6f7"), highlight: hex("#313244"),
        success:   hex("#a6e3a1"), warning:   hex("#fab387"),
        error:     hex("#f38ba8"), muted:     hex("#6c7086"),
        border:    hex("#45475a"),
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Load all enabled themes from the user's themes.toml.
/// Falls back to the embedded default if the file cannot be read.
pub fn all_themes() -> Vec<Theme> {
    let path = Config::themes_path();
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|_| DEFAULT_THEMES_TOML.to_string());

    let parsed: ThemesFile = toml::from_str(&raw).unwrap_or(ThemesFile { themes: vec![] });
    let enabled: Vec<Theme> = parsed.themes.iter()
        .filter(|e| e.enabled)
        .map(Theme::from)
        .collect();

    if enabled.is_empty() {
        vec![fallback_theme()]
    } else {
        enabled
    }
}

/// Find a theme by name (case-insensitive). Falls back to first available.
pub fn find_theme(name: &str) -> Theme {
    let themes = all_themes();
    themes.iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .cloned()
        .unwrap_or_else(|| themes.into_iter().next().unwrap_or_else(fallback_theme))
}
