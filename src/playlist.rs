//! Detection and parsing of playlist files (.pls, .m3u, .m3u8).
//!
//! Many radio directories hand out a "stream URL" that is actually a tiny
//! playlist file pointing at the real audio stream (common with Shoutcast/
//! Icecast directories). Without unwrapping these, jarl would try to probe
//! a text file as audio and fail with a confusing decoder error.

/// Returns true if the given content-type or URL suggests a playlist file
/// rather than a direct audio stream.
pub fn looks_like_playlist(content_type: &str, url: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("audio/x-mpegurl")
        || ct.contains("audio/mpegurl")
        || ct.contains("application/vnd.apple.mpegurl")
        || ct.contains("audio/x-scpls")
        || ct.contains("application/pls+xml")
    {
        return true;
    }
    let u = url.to_ascii_lowercase();
    u.ends_with(".pls") || u.ends_with(".m3u") || u.ends_with(".m3u8")
}

/// Returns true if the body content itself looks like a playlist, regardless
/// of what the server claimed in Content-Type (some servers mislabel these
/// as text/plain or even audio/mpeg). Operates on raw bytes since this is
/// called on a small peeked prefix of the response body that may not be
/// valid UTF-8 in the binary case (real audio).
pub fn body_looks_like_playlist(head: &[u8]) -> bool {
    // Only consider this a playlist if the peeked bytes are valid UTF-8 text;
    // real audio frames are essentially never valid UTF-8 for more than a
    // few bytes, so this cheaply rules out the common (non-playlist) case.
    let Ok(text) = std::str::from_utf8(head) else { return false; };
    let trimmed = text.trim_start();
    if trimmed.starts_with("#EXTM3U") || trimmed.to_ascii_lowercase().starts_with("[playlist]") {
        return true;
    }
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

/// Extract the first usable stream URL from playlist bytes. Supports both
/// PLS (`File1=...`) and M3U/M3U8 (plain URL lines, `#EXT*` ignored) formats.
pub fn extract_first_url(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim_start();

    if trimmed.to_ascii_lowercase().starts_with("[playlist]") || text.contains("File1=") {
        // PLS format: look for File1=, File2=, ... in order, take the lowest.
        let mut entries: Vec<(u32, String)> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("File") {
                if let Some(eq) = rest.find('=') {
                    let (num_part, url_part) = (&rest[..eq], &rest[eq + 1..]);
                    if let Ok(n) = num_part.parse::<u32>() {
                        let url = url_part.trim().to_string();
                        if !url.is_empty() {
                            entries.push((n, url));
                        }
                    }
                }
            }
        }
        entries.sort_by_key(|(n, _)| *n);
        return entries.into_iter().map(|(_, u)| u).next();
    }

    // M3U/M3U8 format: first non-comment, non-empty line.
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pls() {
        let pls = b"[playlist]\nNumberOfEntries=2\nFile1=http://example.com/stream1\nTitle1=Stream One\nFile2=http://example.com/stream2\nVersion=2\n";
        assert_eq!(extract_first_url(pls), Some("http://example.com/stream1".to_string()));
    }

    #[test]
    fn parses_m3u() {
        let m3u = b"#EXTM3U\n#EXTINF:-1,Some Station\nhttp://example.com/stream.mp3\n";
        assert_eq!(extract_first_url(m3u), Some("http://example.com/stream.mp3".to_string()));
    }

    #[test]
    fn parses_plain_m3u_no_header() {
        let m3u = b"http://example.com/stream.mp3\n";
        assert_eq!(extract_first_url(m3u), Some("http://example.com/stream.mp3".to_string()));
    }

    #[test]
    fn detects_by_content_type() {
        assert!(looks_like_playlist("audio/x-mpegurl", "http://x.com/a"));
        assert!(looks_like_playlist("application/vnd.apple.mpegurl; charset=utf-8", "http://x.com/a"));
        assert!(looks_like_playlist("audio/x-scpls", "http://x.com/a"));
        assert!(!looks_like_playlist("audio/mpeg", "http://x.com/a.mp3"));
    }

    #[test]
    fn detects_by_extension() {
        assert!(looks_like_playlist("application/octet-stream", "http://x.com/station.pls"));
        assert!(looks_like_playlist("text/plain", "http://x.com/station.m3u8"));
        assert!(!looks_like_playlist("audio/mpeg", "http://x.com/stream"));
    }

    #[test]
    fn detects_body_heuristically() {
        assert!(body_looks_like_playlist(b"#EXTM3U\nhttp://x.com/a.mp3\n"));
        assert!(body_looks_like_playlist(b"[playlist]\nFile1=http://x.com/a\n"));
        assert!(!body_looks_like_playlist(&[0xFF, 0xFB, 0x90, 0x00, 0x00, 0x00]));
    }
}
