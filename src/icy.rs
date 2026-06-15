//! Raw TCP connector for Shoutcast/ICY streams.
//!
//! Some radio servers (Shoutcast) respond with "ICY 200 OK" instead of
//! "HTTP/1.1 200 OK". Standard HTTP clients reject this. We handle it
//! by opening a raw TCP socket, sending a minimal GET request, reading
//! past the ICY response headers, and returning the raw audio byte stream.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Connect to an ICY (Shoutcast) stream URL and return a reader positioned
/// at the start of the audio data.
///
/// Only supports `http://` URLs (ICY servers never use TLS).
pub fn connect(url: &str) -> Result<Box<dyn Read + Send + Sync + 'static>, String> {
    // Parse the URL manually — it's always plain HTTP for ICY.
    let without_scheme = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("ICY fallback only supports http://, got: {url}"))?;

    let (host_port, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None    => (without_scheme, "/"),
    };

    // Default port 80 unless specified.
    let (host, port) = match host_port.rfind(':') {
        Some(i) => (&host_port[..i], host_port[i+1..].parse::<u16>().unwrap_or(80)),
        None    => (host_port, 80),
    };

    log::debug!("icy: connecting to {host}:{port}{path}");

    let mut stream = TcpStream::connect((host, port))
        .map_err(|e| format!("ICY connect error: {e}"))?;

    stream.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| format!("ICY set_read_timeout: {e}"))?;

    // Send a minimal HTTP/1.0 GET request.
    // HTTP/1.0 prevents chunked encoding and works with ICY servers.
    let request = format!(
        "GET {path} HTTP/1.0\r\n\
         Host: {host}:{port}\r\n\
         User-Agent: jarl/0.1 (terminal radio)\r\n\
         Accept: */*\r\n\
         Icy-MetaData: 0\r\n\
         \r\n"
    );
    stream.write_all(request.as_bytes())
        .map_err(|e| format!("ICY write error: {e}"))?;

    // Read and discard headers until the blank line.
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)
        .map_err(|e| format!("ICY read status: {e}"))?;

    log::info!("icy: status line: {}", status_line.trim());

    // Accept both "ICY 200 OK" and "HTTP/... 200 ..."
    if !status_line.contains("200") {
        return Err(format!("ICY server returned non-200: {}", status_line.trim()));
    }

    // Drain remaining headers.
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)
            .map_err(|e| format!("ICY drain headers: {e}"))?;
        let trimmed = line.trim();
        log::debug!("icy header: {trimmed}");
        if trimmed.is_empty() { break; }
    }

    log::info!("icy: headers consumed, audio data follows");
    Ok(Box::new(reader))
}
