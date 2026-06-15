//! Background metadata poller for stations that publish a JSON "now playing"
//! endpoint instead of embedding track info in the audio stream.
//!
//! Supported JSON shapes (tried in order):
//!
//!   AzuraCast   {"now_playing":{"song":{"artist":"…","title":"…"}}}
//!   Icecast     {"icestats":{"source":{"title":"Artist - Title"}}}
//!               source may also be an array; the first element is used.
//!   Simple/RP   {"artist":"…","title":"…","time":42}
//!               time (optional) = seconds until next track; used to schedule
//!               the next poll instead of polling on a fixed interval.
//!
//! The poller runs in its own thread and updates the shared TrackTitle.
//! It stops automatically when the `stop` channel is signalled (on Drop).

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crate::icy_meta::TrackTitle;

pub struct MetaPoll {
    stop_tx: Sender<()>,
}

impl MetaPoll {
    /// Spawn a poller for `url`. Updates `track_title` periodically.
    pub fn spawn(url: String, track_title: TrackTitle) -> Self {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        thread::spawn(move || run(url, track_title, stop_rx));
        Self { stop_tx }
    }
}

impl Drop for MetaPoll {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

fn run(url: String, track_title: TrackTitle, stop: Receiver<()>) {
    log::debug!("meta_poll: started for {url}");
    let agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_per_call(Some(Duration::from_secs(10)))
        .build()
        .new_agent();

    let mut interval = Duration::from_secs(15); // default poll interval

    loop {
        // Check for stop signal (non-blocking).
        if stop.try_recv().is_ok() {
            log::debug!("meta_poll: stopped");
            return;
        }

        match agent.get(&url).call() {
            Ok(mut resp) => {
                if let Ok(body) = resp.body_mut().read_to_string() {
                    if let Some((artist, title, secs)) = parse_json(&body) {
                        let display = if artist.is_empty() {
                            title.clone()
                        } else {
                            format!("{artist} – {title}")
                        };
                        log::debug!("meta_poll: {display}");
                        if let Ok(mut t) = track_title.lock() {
                            *t = if display.is_empty() { None } else { Some(display) };
                        }
                        // If the endpoint tells us when the song ends, use that
                        // (plus 2 s buffer) as the next poll interval.
                        if let Some(s) = secs {
                            if s > 0 {
                                interval = Duration::from_secs((s as u64).saturating_add(2));
                            } else {
                                // Negative or zero → song already ended, poll soon.
                                interval = Duration::from_secs(5);
                            }
                        } else {
                            interval = Duration::from_secs(15);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("meta_poll: fetch error: {e}");
                interval = Duration::from_secs(30); // back off on error
            }
        }

        // Sleep in 200 ms increments so we catch the stop signal promptly.
        let steps = (interval.as_millis() / 200).max(1) as u64;
        for _ in 0..steps {
            thread::sleep(Duration::from_millis(200));
            if stop.try_recv().is_ok() {
                log::debug!("meta_poll: stopped");
                return;
            }
        }
    }
}

/// Try to extract (artist, title, optional_time_secs) from a JSON body.
/// Returns None if the body is not parseable or contains no useful fields.
fn parse_json(body: &str) -> Option<(String, String, Option<i64>)> {
    // Minimal hand-rolled parser — avoids pulling in serde_json.

    // Helper: first string value for `"key":"value"`.
    let str_field = |src: &str, key: &str| -> Option<String> {
        let needle = format!("\"{}\"", key);
        let pos    = src.find(&needle)?;
        let after  = src[pos + needle.len()..].trim_start();
        let after  = after.strip_prefix(':')?.trim_start();
        if after.starts_with('"') {
            let inner = &after[1..];
            let end   = inner.find('"')?;
            Some(unescape(&inner[..end]))
        } else {
            None
        }
    };

    // Helper: first integer value for `"key":number`.
    let int_field = |src: &str, key: &str| -> Option<i64> {
        let needle = format!("\"{}\"", key);
        let pos    = src.find(&needle)?;
        let after  = src[pos + needle.len()..].trim_start();
        let after  = after.strip_prefix(':')?.trim_start();
        let end    = after.find(|c: char| !c.is_ascii_digit() && c != '-')?;
        after[..end].parse().ok()
    };

    // ── AzuraCast ─────────────────────────────────────────────────────────────
    // {"now_playing":{"song":{"artist":"…","title":"…"}}}
    if let Some(np_pos) = body.find("\"now_playing\"") {
        let np_slice = &body[np_pos..];
        if let Some(song_pos) = np_slice.find("\"song\"") {
            let song_slice = &np_slice[song_pos..];
            let artist = str_field(song_slice, "artist").unwrap_or_default();
            let title  = str_field(song_slice, "title").unwrap_or_default();
            if !title.is_empty() {
                return Some((artist, title, None));
            }
        }
    }

    // ── Icecast status-json.xsl ───────────────────────────────────────────────
    // {"icestats":{"source":{"title":"Artist - Title"}}}          (single mountpoint)
    // {"icestats":{"source":[{"title":"Artist - Title"}, ...]}}   (multiple mountpoints)
    if let Some(ic_pos) = body.find("\"icestats\"") {
        let ic_slice = &body[ic_pos..];
        if let Some(src_pos) = ic_slice.find("\"source\"") {
            let after_src = ic_slice[src_pos + "\"source\"".len()..].trim_start();
            let after_src = after_src.trim_start_matches(':').trim_start();
            // Skip the opening bracket if source is an array.
            let src_slice = if after_src.starts_with('[') {
                after_src.trim_start_matches('[').trim_start()
            } else {
                after_src
            };
            if let Some(raw) = str_field(src_slice, "title") {
                if !raw.is_empty() {
                    return if let Some(dash) = raw.find(" - ") {
                        Some((raw[..dash].trim().to_string(), raw[dash + 3..].trim().to_string(), None))
                    } else {
                        Some((String::new(), raw, None))
                    };
                }
            }
        }
    }

    // ── Simple / Radio Paradise ───────────────────────────────────────────────
    // {"artist":"…","title":"…","time":42}
    let artist = str_field(body, "artist").unwrap_or_default();
    let title  = str_field(body, "title").unwrap_or_default();
    let time   = int_field(body, "time");

    if !title.is_empty() {
        return Some((artist, title, time));
    }

    None
}

/// Minimal JSON string unescaping (handles \", \\, \n, \r, \t).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"')  => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n')  => out.push('\n'),
                Some('r')  => out.push('\r'),
                Some('t')  => out.push('\t'),
                Some(x)    => { out.push('\\'); out.push(x); }
                None       => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── unescape ──────────────────────────────────────────────────────────────

    #[test]
    fn unescape_plain() {
        assert_eq!(unescape("hello"), "hello");
    }

    #[test]
    fn unescape_quote() {
        assert_eq!(unescape(r#"say \"hi\""#), r#"say "hi""#);
    }

    #[test]
    fn unescape_sequences() {
        assert_eq!(unescape(r"a\nb\tc\\d"), "a\nb\tc\\d");
    }

    #[test]
    fn unescape_unknown_sequence_preserved() {
        assert_eq!(unescape(r"\z"), r"\z");
    }

    // ── parse_json: empty / garbage ───────────────────────────────────────────

    #[test]
    fn parse_json_empty() {
        assert!(parse_json("").is_none());
    }

    #[test]
    fn parse_json_garbage() {
        assert!(parse_json("not json at all").is_none());
    }

    #[test]
    fn parse_json_no_title() {
        assert!(parse_json(r#"{"artist":"Someone"}"#).is_none());
    }

    // ── parse_json: Simple / Radio Paradise ───────────────────────────────────

    #[test]
    fn parse_json_simple_artist_title() {
        let body = r#"{"artist":"Portishead","title":"Glory Box"}"#;
        let (artist, title, time) = parse_json(body).unwrap();
        assert_eq!(artist, "Portishead");
        assert_eq!(title, "Glory Box");
        assert!(time.is_none());
    }

    #[test]
    fn parse_json_simple_title_only() {
        let body = r#"{"title":"Massive Attack - Teardrop"}"#;
        let (artist, title, time) = parse_json(body).unwrap();
        assert_eq!(artist, "");
        assert_eq!(title, "Massive Attack - Teardrop");
        assert!(time.is_none());
    }

    #[test]
    fn parse_json_radio_paradise_with_time() {
        let body = r#"{"artist":"Björk","title":"Jóga","time":187}"#;
        let (artist, title, time) = parse_json(body).unwrap();
        assert_eq!(artist, "Björk");
        assert_eq!(title, "Jóga");
        assert_eq!(time, Some(187));
    }

    #[test]
    fn parse_json_radio_paradise_negative_time() {
        let body = r#"{"artist":"Amon Tobin","title":"Verbal","time":-3}"#;
        let (artist, title, time) = parse_json(body).unwrap();
        assert_eq!(time, Some(-3));
    }

    // ── parse_json: AzuraCast ─────────────────────────────────────────────────

    #[test]
    fn parse_json_azuracast() {
        let body = r#"{"now_playing":{"song":{"artist":"Burial","title":"Archangel"}}}"#;
        let (artist, title, time) = parse_json(body).unwrap();
        assert_eq!(artist, "Burial");
        assert_eq!(title, "Archangel");
        assert!(time.is_none());
    }

    #[test]
    fn parse_json_azuracast_no_artist() {
        let body = r#"{"now_playing":{"song":{"artist":"","title":"Untitled"}}}"#;
        let (artist, title, _) = parse_json(body).unwrap();
        assert_eq!(artist, "");
        assert_eq!(title, "Untitled");
    }

    #[test]
    fn parse_json_azuracast_empty_title_falls_through() {
        // Empty title in AzuraCast block → parser falls through to Simple.
        let body = r#"{"now_playing":{"song":{"artist":"X","title":""}},"artist":"Y","title":"Fallback"}"#;
        let (_, title, _) = parse_json(body).unwrap();
        assert_eq!(title, "Fallback");
    }

    // ── parse_json: Icecast ───────────────────────────────────────────────────

    #[test]
    fn parse_json_icecast_single_source() {
        let body = r#"{"icestats":{"source":{"title":"Deadmau5 - Strobe"}}}"#;
        let (artist, title, _) = parse_json(body).unwrap();
        assert_eq!(artist, "Deadmau5");
        assert_eq!(title, "Strobe");
    }

    #[test]
    fn parse_json_icecast_title_no_dash() {
        let body = r#"{"icestats":{"source":{"title":"Stream Title Without Dash"}}}"#;
        let (artist, title, _) = parse_json(body).unwrap();
        assert_eq!(artist, "");
        assert_eq!(title, "Stream Title Without Dash");
    }

    #[test]
    fn parse_json_icecast_array_source() {
        let body = r#"{"icestats":{"source":[{"title":"Four Tet - Lush"},{"title":"Other"}]}}"#;
        let (artist, title, _) = parse_json(body).unwrap();
        assert_eq!(artist, "Four Tet");
        assert_eq!(title, "Lush");
    }

    #[test]
    fn parse_json_icecast_empty_title_falls_through() {
        // Empty Icecast title → parser falls through to Simple.
        let body = r#"{"icestats":{"source":{"title":""}},"artist":"A","title":"B"}"#;
        let (_, title, _) = parse_json(body).unwrap();
        assert_eq!(title, "B");
    }
}
