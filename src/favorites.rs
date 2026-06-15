use std::collections::HashSet;
use std::fs;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Debug, Default, Serialize, Deserialize)]
struct FavoritesFile {
    #[serde(default)]
    favorites: Vec<String>,
}

fn path() -> std::path::PathBuf {
    Config::config_dir().join("favorites.toml")
}

pub fn load() -> HashSet<String> {
    let p = path();
    if !p.exists() {
        return HashSet::new();
    }
    let raw = fs::read_to_string(&p).unwrap_or_default();
    let f: FavoritesFile = toml::from_str(&raw).unwrap_or_default();
    f.favorites.into_iter().collect()
}

pub fn save(favs: &HashSet<String>) -> Result<()> {
    let mut list: Vec<String> = favs.iter().cloned().collect();
    list.sort();
    let raw = toml::to_string_pretty(&FavoritesFile { favorites: list })?;
    fs::write(path(), raw)?;
    Ok(())
}
