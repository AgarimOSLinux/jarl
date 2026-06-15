use std::io::{self, Read};
use std::sync::{Arc, Mutex};

/// Wraps any `Read` stream that includes ICY metadata blocks.
///
/// ICY servers interleave metadata every `metaint` bytes of audio.
/// Each metadata block is:
///   - 1 byte  : block_count  (actual length = block_count × 16)
///   - N bytes : null-padded UTF-8, contains `StreamTitle='...';`
///
/// `IcyReader` strips the metadata bytes out so the caller only sees
/// clean audio bytes, and updates `title` whenever a new title arrives.
pub struct IcyReader<R: Read + Send + Sync> {
    inner:            R,
    metaint:          usize,
    bytes_until_meta: usize,
    pub title:        Arc<Mutex<Option<String>>>,
}

impl<R: Read + Send + Sync> IcyReader<R> {
    pub fn new(inner: R, metaint: usize, title: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            inner,
            metaint,
            bytes_until_meta: metaint,
            title,
        }
    }

    fn read_metadata_block(&mut self) -> io::Result<()> {
        // Read the 1-byte length indicator.
        let mut len_buf = [0u8; 1];
        self.inner.read_exact(&mut len_buf)?;
        let block_len = len_buf[0] as usize * 16;
        if block_len == 0 { return Ok(()); }

        // Read the metadata block.
        let mut meta_buf = vec![0u8; block_len];
        self.inner.read_exact(&mut meta_buf)?;

        // Parse `StreamTitle='...';`
        let meta_str = String::from_utf8_lossy(&meta_buf);
        if let Some(title) = parse_stream_title(&meta_str) {
            log::debug!("ICY title: {title}");
            if let Ok(mut t) = self.title.lock() {
                *t = if title.is_empty() { None } else { Some(title) };
            }
        }
        Ok(())
    }
}

impl<R: Read + Send + Sync> Read for IcyReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() { return Ok(0); }

        // How many audio bytes can we read before the next metadata block?
        let max_read = buf.len().min(self.bytes_until_meta);
        let n = self.inner.read(&mut buf[..max_read])?;
        if n == 0 { return Ok(0); }

        self.bytes_until_meta -= n;

        if self.bytes_until_meta == 0 {
            // Read and discard the metadata block (updates self.title).
            let _ = self.read_metadata_block();
            self.bytes_until_meta = self.metaint;
        }

        Ok(n)
    }
}

fn parse_stream_title(meta: &str) -> Option<String> {
    // Find StreamTitle='...'
    let start = meta.find("StreamTitle='")?;
    let after = &meta[start + "StreamTitle='".len()..];
    // Title ends at the next unescaped `'`
    let end = after.find('\'')?;
    let title = after[..end].trim().to_string();
    Some(title)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_title_basic() {
        let meta = "StreamTitle='Portishead - Glory Box';StreamUrl='';";
        assert_eq!(parse_stream_title(meta).unwrap(), "Portishead - Glory Box");
    }

    #[test]
    fn parse_title_empty() {
        let meta = "StreamTitle='';StreamUrl='';";
        assert_eq!(parse_stream_title(meta).unwrap(), "");
    }

    #[test]
    fn parse_title_whitespace_trimmed() {
        let meta = "StreamTitle='  Massive Attack - Teardrop  ';";
        assert_eq!(parse_stream_title(meta).unwrap(), "Massive Attack - Teardrop");
    }

    #[test]
    fn parse_title_missing_key() {
        assert!(parse_stream_title("StreamUrl='http://example.com';").is_none());
    }

    #[test]
    fn parse_title_unicode() {
        let meta = "StreamTitle='Björk - Jóga';";
        assert_eq!(parse_stream_title(meta).unwrap(), "Björk - Jóga");
    }

    #[test]
    fn parse_title_null_padded_block() {
        // ICY blocks are null-padded to a multiple of 16 bytes.
        let meta = "StreamTitle='Four Tet - Lush';\0\0\0\0\0\0\0";
        assert_eq!(parse_stream_title(meta).unwrap(), "Four Tet - Lush");
    }
}

// ── Shared title handle ───────────────────────────────────────────────────────

pub type TrackTitle = Arc<Mutex<Option<String>>>;

pub fn new_track_title() -> TrackTitle {
    Arc::new(Mutex::new(None))
}
