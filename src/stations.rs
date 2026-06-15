use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;

use crate::config::Config;

/// Bitrate field: either a numeric kbps value or the string "FLAC".
#[derive(Debug, Clone, PartialEq)]
pub enum Bitrate {
    Kbps(u32),
    Flac,
}

impl Bitrate {
    pub fn display(&self) -> String {
        match self {
            Bitrate::Kbps(n) => format!("{:>4} kbps", n),
            Bitrate::Flac    => "   FLAC  ".to_string(),
        }
    }
}

impl Serialize for Bitrate {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Bitrate::Kbps(n) => s.serialize_u32(*n),
            Bitrate::Flac    => s.serialize_str("FLAC"),
        }
    }
}

impl<'de> Deserialize<'de> for Bitrate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct BitrateVisitor;

        impl<'de> serde::de::Visitor<'de> for BitrateVisitor {
            type Value = Bitrate;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a bitrate in kbps (integer) or the string \"FLAC\"")
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Bitrate, E> {
                Ok(Bitrate::Kbps(v as u32))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Bitrate, E> {
                Ok(Bitrate::Kbps(v as u32))
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Bitrate, E> {
                if v.eq_ignore_ascii_case("flac") {
                    Ok(Bitrate::Flac)
                } else {
                    v.parse::<u32>()
                        .map(Bitrate::Kbps)
                        .map_err(|_| E::invalid_value(serde::de::Unexpected::Str(v), &self))
                }
            }
        }

        d.deserialize_any(BitrateVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Station {
    pub name:         String,
    pub genre:        String,
    pub url:          String,
    pub bitrate:      Bitrate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct StationsFile {
    stations: Vec<Station>,
}

/// Load stations from the user's config dir.
pub fn load_stations() -> Result<Vec<Station>> {
    let path = Config::stations_path();
    let raw  = fs::read_to_string(&path)
        .with_context(|| format!("read stations from {}", path.display()))?;
    let parsed: StationsFile = toml::from_str(&raw).context("parse stations.toml")?;
    Ok(parsed.stations)
}

/// Write the current in-memory station list back to disk.
pub fn save_stations(stations: &[Station]) -> Result<()> {
    let path = Config::stations_path();
    let file = StationsFile { stations: stations.to_vec() };
    let raw  = toml::to_string_pretty(&file).context("serialize stations")?;
    // Prepend the comment header so the file stays human-readable.
    let header = "# jarl – station list\n\
                  #\n\
                  # Add a new station by appending a [[stations]] block.\n\
                  # Press 'r' inside jarl to reload without restarting.\n\
                  # Press 'd' to delete a station (updates this file).\n\
                  #\n\
                  # Fields:\n\
                  #   name         – display name\n\
                  #   genre        – short genre description\n\
                  #   url          – direct stream URL\n\
                  #   bitrate      – kbps as a number (e.g. 128) or \"FLAC\" for lossless streams\n\
                  #   metadata_url – (optional) URL of a JSON endpoint that returns\n\
                  #                  {\"artist\":\"...\",\"title\":\"...\"} or {\"now_playing\":{\"song\":{\"artist\":\"...\",\"title\":\"...\"}}}\n\n";
    fs::write(path, format!("{header}{raw}")).context("write stations.toml")?;
    Ok(())
}

/// Fallback: parse the stations bundled into the binary.
pub fn default_stations() -> Vec<Station> {
    let parsed: StationsFile =
        toml::from_str(crate::config::DEFAULT_STATIONS_TOML).expect("bundled stations.toml is valid");
    parsed.stations
}
