//! Persistent playback history.
//!
//! Stores the last `MAX_HISTORY` stations played in
//! `~/.config/jarl/history.toml`, most recent first.
//! Consecutive duplicates are collapsed into one entry.
//!
//! The history is loaded on startup and saved every time a station is played.

use std::fs;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;

pub const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub name:  String,
    pub genre: String,
    pub url:   String,
}

#[derive(Default, Serialize, Deserialize)]
struct HistoryFile {
    #[serde(default)]
    history: Vec<HistoryEntry>,
}

fn path() -> std::path::PathBuf {
    Config::config_dir().join("history.toml")
}

pub fn load() -> Vec<HistoryEntry> {
    let p = path();
    if !p.exists() { return vec![]; }
    let raw = fs::read_to_string(&p).unwrap_or_default();
    let f: HistoryFile = toml::from_str(&raw).unwrap_or_default();
    f.history
}

pub fn push(entry: HistoryEntry, history: &mut Vec<HistoryEntry>) {
    // Collapse consecutive duplicates.
    if history.first().map(|e| e.url == entry.url).unwrap_or(false) {
        return;
    }
    history.insert(0, entry);
    history.truncate(MAX_HISTORY);
}

pub fn save(history: &[HistoryEntry]) -> Result<()> {
    let file = HistoryFile { history: history.to_vec() };
    let raw  = toml::to_string_pretty(&file)?;
    fs::write(path(), raw)?;
    Ok(())
}
