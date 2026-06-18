use std::collections::VecDeque;
use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{DeviceSinkBuilder, Player as RodioPlayer, Source};
use crate::visualizer::{CapturingSource, SampleBuffer, new_sample_buffer};
use crate::icy_meta::{IcyReader, TrackTitle, new_track_title};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::CODEC_ID_NULL_AUDIO;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerStatus {
    Idle,
    Connecting,
    Playing,
    Paused,
    Reconnecting(u32),   // attempt number
    Error(String),
}

pub enum PlayerCmd {
    Play { url: String, mime: Option<String> },
    TogglePause,
    Stop,
    Volume(f32),
    Quit,
}

pub struct Player {
    pub tx:          Sender<PlayerCmd>,
    pub status:      Arc<Mutex<PlayerStatus>>,
    pub volume:      Arc<Mutex<f32>>,
    pub sample_buf:  SampleBuffer,
    pub track_title: TrackTitle,
}

impl Player {
    pub fn new(initial_volume: f32) -> Self {
        let (tx, rx)   = mpsc::channel::<PlayerCmd>();
        let status      = Arc::new(Mutex::new(PlayerStatus::Idle));
        let volume      = Arc::new(Mutex::new(initial_volume.clamp(0.0, 1.0)));
        let sample_buf  = new_sample_buffer();
        let track_title = new_track_title();
        let s   = Arc::clone(&status);
        let v   = Arc::clone(&volume);
        let sb  = Arc::clone(&sample_buf);
        let tt  = Arc::clone(&track_title);
        thread::spawn(move || audio_thread(rx, s, v, sb, tt));
        Self { tx, status, volume, sample_buf, track_title }
    }

    pub fn play(&self, url: String) {
        let _ = self.tx.send(PlayerCmd::Play { url, mime: None });
    }
    pub fn send(&self, cmd: PlayerCmd) { let _ = self.tx.send(cmd); }
    pub fn status(&self) -> PlayerStatus { self.status.lock().unwrap().clone() }
    pub fn volume(&self) -> f32         { *self.volume.lock().unwrap() }
}

// ── Symphonia streaming source ────────────────────────────────────────────────

struct RadioStream {
    format:      Box<dyn symphonia::core::formats::FormatReader>,
    decoder:     Box<dyn symphonia::core::codecs::audio::AudioDecoder>,
    track_id:    u32,
    sample_rate: std::num::NonZero<u32>,
    channels:    std::num::NonZero<u16>,
    buf:         VecDeque<f32>,
    done:        bool,
    track_title: TrackTitle,
}

/// Hard cap on decoded samples buffered ahead of playback. At a typical
/// 44.1kHz/stereo stream this is roughly 20 seconds of audio — far more
/// than rodio should ever lag behind, so hitting this means the consumer
/// (audio device) has stalled and we're better off dropping old samples
/// than growing unboundedly and wasting memory for hours-long sessions.
const MAX_BUFFERED_SAMPLES: usize = 44_100 * 2 * 20;

impl RadioStream {
    fn probe(
        reader: Box<dyn Read + Send + Sync + 'static>,
        mime:   Option<&str>,
        track_title: TrackTitle,
    ) -> Result<Self, String> {
        let source = ReadOnlySource::new(reader);
        let mss    = MediaSourceStream::new(Box::new(source), Default::default());

        let mut hint = Hint::new();
        if let Some(ct) = mime {
            let ext = if ct.contains("mpeg") || ct.contains("mp3") { "mp3" }
                      else if ct.contains("aac")                    { "aac" }
                      else if ct.contains("ogg")                    { "ogg" }
                      else if ct.contains("flac")                   { "flac" }
                      else                                          { "" };
            if !ext.is_empty() { hint.with_extension(ext); }
        }

        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| format!("format probe failed: {e}"))?;

        let track = format
            .tracks()
            .iter()
            .find(|t| matches!(&t.codec_params, Some(CodecParameters::Audio(p)) if p.codec != CODEC_ID_NULL_AUDIO))
            .ok_or_else(|| "no playable audio track found".to_string())?;

        let track_id = track.id;
        let audio_params = match &track.codec_params {
            Some(CodecParameters::Audio(p)) => p,
            _ => return Err("track has no audio codec parameters".to_string()),
        };

        let sample_rate = std::num::NonZero::new(audio_params.sample_rate.unwrap_or(44_100))
            .unwrap_or(std::num::NonZero::new(44_100).unwrap());
        let channels = std::num::NonZero::new(
            audio_params.channels.as_ref().map(|c| c.count() as u16).unwrap_or(2)
        ).unwrap_or(std::num::NonZero::new(2).unwrap());

        log::info!(
            "probed: codec={:?} rate={} ch={}",
            audio_params.codec, sample_rate, channels
        );

        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&audio_params, &Default::default())
            .map_err(|e| format!("decoder init failed: {e}"))?;

        let mut stream = Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            buf: VecDeque::new(),
            done: false,
            track_title,
        };

        stream.drain_symphonia_metadata();
        Ok(stream)
    }

    fn drain_symphonia_metadata(&mut self) {
        let mut meta = self.format.metadata();
        if let Some(rev) = meta.current() {
            Self::apply_tags_to(&rev.media.tags, &self.track_title);
        }
        while let Some(rev) = meta.pop() {
            Self::apply_tags_to(&rev.media.tags, &self.track_title);
        }
    }

    fn apply_tags_to(tags: &[symphonia::core::meta::Tag], track_title: &TrackTitle) {
        if tags.is_empty() { return; }

        let mut title:  Option<String> = None;
        let mut artist: Option<String> = None;

        for tag in tags {
            if let Some(std) = &tag.std {
                use symphonia::core::meta::StandardTag;
                match std {
                    StandardTag::TrackTitle(t)  => { title  = Some(t.as_ref().clone()); }
                    StandardTag::Artist(a)       => { if artist.is_none() { artist = Some(a.as_ref().clone()); } }
                    StandardTag::AlbumArtist(a)  => { if artist.is_none() { artist = Some(a.as_ref().clone()); } }
                    _ => {}
                }
            } else {
                let key = tag.raw.key.to_ascii_uppercase();
                let val = tag.raw.value.to_string();
                log::debug!("symphonia raw tag {key}: {val}");
                match key.as_str() {
                    "TITLE" | "STREAMTITLE" => { title  = Some(val); }
                    "ARTIST" | "ALBUMARTIST" => { if artist.is_none() { artist = Some(val); } }
                    _ => {}
                }
            }
        }

        if let Some(t) = title {
            let display = match artist {
                Some(a) if !a.is_empty() => format!("{a} – {t}"),
                _                        => t,
            };
            log::debug!("track title -> {display}");
            if let Ok(mut lock) = track_title.lock() {
                *lock = if display.is_empty() { None } else { Some(display) };
            }
        }
    }

    fn fill_buf(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p))                                   => p,
                Ok(None)                                      => { self.done = true; return false; }
                Err(SymphoniaError::IoError(_))               => { self.done = true; return false; }
                Err(SymphoniaError::ResetRequired)            => {
                    self.drain_symphonia_metadata();
                    if let Some(track) = self.format.tracks().iter()
                        .find(|t| matches!(&t.codec_params, Some(CodecParameters::Audio(p)) if p.codec != CODEC_ID_NULL_AUDIO))
                    {
                        self.track_id = track.id;
                        if let Some(CodecParameters::Audio(p)) = &track.codec_params {
                            if let Ok(dec) = symphonia::default::get_codecs()
                                .make_audio_decoder(p, &Default::default())
                            {
                                self.decoder = dec;
                            }
                        }
                    }
                    continue;
                }
                Err(_) => { self.done = true; return false; }
            };

            self.drain_symphonia_metadata();

            if packet.track_id != self.track_id { continue; }

            let decoded = match self.decoder.decode(&packet) {
                Ok(d)  => d,
                Err(SymphoniaError::DecodeError(e)) => {
                    log::warn!("decode error (skipping packet): {e}");
                    continue;
                }
                Err(_) => { self.done = true; return false; }
            };

            let spec = decoded.spec();
            if let Some(r) = std::num::NonZero::new(spec.rate()) { self.sample_rate = r; }
            if let Some(c) = std::num::NonZero::new(spec.channels().count() as u16) { self.channels = c; }

            let n_samples = decoded.frames() * spec.channels().count();
            let mut tmp: Vec<f32> = vec![0.0f32; n_samples];
            decoded.copy_to_slice_interleaved::<f32, _>(tmp.as_mut_slice());
            for s in tmp { self.buf.push_back(s); }

            // Backpressure: if the consumer (rodio/audio device) has fallen
            // behind, drop the oldest samples rather than growing forever.
            // This trades a brief audible glitch for bounded memory use on
            // long-running sessions.
            if self.buf.len() > MAX_BUFFERED_SAMPLES {
                let excess = self.buf.len() - MAX_BUFFERED_SAMPLES;
                log::warn!("decode buffer exceeded cap, dropping {excess} oldest samples");
                self.buf.drain(..excess);
            }
            return true;
        }
    }
}

impl Iterator for RadioStream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if let Some(s) = self.buf.pop_front() { return Some(s); }
            if self.done                           { return None;     }
            if !self.fill_buf()                   { return None;     }
        }
    }
}

impl Source for RadioStream {
    fn current_span_len(&self)  -> Option<usize>              { None }
    fn channels(&self)          -> std::num::NonZero<u16>     { self.channels    }
    fn sample_rate(&self)       -> std::num::NonZero<u32>     { self.sample_rate }
    fn total_duration(&self)    -> Option<Duration>           { None }
}

// ── Connect helper ────────────────────────────────────────────────────────────
//
// Spawns an HTTP-connect thread and returns a Receiver for the result.
// Extracted so both the initial Play command and auto-reconnect can reuse it.

/// Maximum number of playlist redirections to follow before giving up
/// (guards against a `.pls` pointing at another `.pls` forever).
const MAX_PLAYLIST_HOPS: u32 = 5;

fn start_connect(
    url:         String,
    mime:        Option<String>,
    track_title: TrackTitle,
) -> Receiver<Result<RadioStream, String>> {
    let (ptx, prx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok(mut t) = track_title.lock() { *t = None; }
        let result = connect_resolving_playlists(url, mime, &track_title, 0);
        let _ = ptx.send(result);
    });
    prx
}

/// Connects to `url`, transparently following `.pls`/`.m3u`/`.m3u8`
/// playlist responses until a real audio stream is found (or
/// `MAX_PLAYLIST_HOPS` is exceeded).
fn connect_resolving_playlists(
    url:         String,
    mime:        Option<String>,
    track_title: &TrackTitle,
    hop:         u32,
) -> Result<RadioStream, String> {
    if hop > MAX_PLAYLIST_HOPS {
        return Err(format!("too many playlist redirections (>{MAX_PLAYLIST_HOPS})"));
    }
    log::debug!("http connect (hop {hop}): {url}");

    let agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(12)))
        .timeout_per_call(Some(Duration::from_secs(30)))
        .build()
        .new_agent();

    let result = agent
        .get(&url)
        .header("User-Agent", "jarl/0.1 (terminal radio)")
        .header("Icy-MetaData", "1")
        .call()
        .map_err(|e| { let m = format!("HTTP error: {e}"); log::error!("{m}"); m })
        .and_then(|resp| {
            let ct = resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            log::info!("connected; content-type: {ct}");

            // ── Playlist detection (by content-type or URL extension) ──────
            if crate::playlist::looks_like_playlist(&ct, &url) {
                return resolve_playlist_body(resp, &url, track_title, hop);
            }

            let metaint = resp.headers()
                .get("icy-metaint")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<usize>().ok());
            log::info!("icy-metaint: {:?}", metaint);
            let hint = mime.as_deref()
                .or(if ct.is_empty() { None } else { Some(ct.as_str()) });

            if metaint.is_none() {
                // No ICY metadata interval — peek the first bytes in case the
                // server mislabelled a playlist as e.g. "audio/mpeg" (common
                // with some misconfigured Icecast/Shoutcast mounts).
                let mut reader = resp.into_body().into_reader();
                let mut head = [0u8; 64];
                let n = reader.read(&mut head).unwrap_or(0);
                if crate::playlist::body_looks_like_playlist(&head[..n]) {
                    log::info!("content-type was misleading; body looks like a playlist");
                    let mut rest = Vec::new();
                    let _ = reader.read_to_end(&mut rest);
                    let mut body = head[..n].to_vec();
                    body.extend(rest);
                    return follow_playlist(&body, &url, track_title, hop);
                }
                let chained: Box<dyn Read + Send + Sync + 'static> =
                    Box::new(std::io::Cursor::new(head[..n].to_vec()).chain(reader));
                return RadioStream::probe(chained, hint, track_title.clone());
            }

            let reader: Box<dyn Read + Send + Sync + 'static> =
                Box::new(IcyReader::new(resp.into_body().into_reader(), metaint.unwrap(), track_title.clone()));
            RadioStream::probe(reader, hint, track_title.clone())
        });

    result.or_else(|e| {
        if e.contains("did not start with HTTP")
            || e.contains("Bad Status")
            || e.contains("invalid HTTP")
        {
            log::info!("HTTP failed ({e}), retrying with ICY protocol");
            crate::icy::connect(&url)
                .and_then(|reader| RadioStream::probe(reader, mime.as_deref(), track_title.clone()))
        } else {
            Err(e)
        }
    })
}

/// Reads a response body already known (by content-type) to be a playlist,
/// extracts the first stream URL, and recurses into it.
fn resolve_playlist_body(
    resp: ureq::http::Response<ureq::Body>,
    base_url: &str,
    track_title: &TrackTitle,
    hop: u32,
) -> Result<RadioStream, String> {
    let mut body = Vec::new();
    resp.into_body().into_reader().read_to_end(&mut body)
        .map_err(|e| format!("failed reading playlist body: {e}"))?;
    follow_playlist(&body, base_url, track_title, hop)
}

fn follow_playlist(
    body: &[u8],
    base_url: &str,
    track_title: &TrackTitle,
    hop: u32,
) -> Result<RadioStream, String> {
    let next = crate::playlist::extract_first_url(body)
        .ok_or_else(|| "playlist contained no usable stream URL".to_string())?;
    log::info!("playlist resolved -> {next}");
    if next == base_url {
        return Err("playlist points at itself".to_string());
    }
    connect_resolving_playlists(next, None, track_title, hop + 1)
}

// ── Audio thread ──────────────────────────────────────────────────────────────

fn set_status(s: &Arc<Mutex<PlayerStatus>>, v: PlayerStatus) {
    log::debug!("player status -> {:?}", v);
    *s.lock().unwrap() = v;
}

/// Maximum number of consecutive reconnect attempts before giving up.
pub const MAX_RECONNECT_ATTEMPTS: u32 = 8;

/// Backoff delays (seconds) indexed by attempt number (capped at last entry).
const RECONNECT_DELAYS: &[u64] = &[2, 4, 8, 15, 30];

/// Returns a backoff delay with ±20% jitter applied, so that if several
/// streams happen to drop around the same moment they don't all retry in
/// perfect lockstep against the same server. Jitter is derived from the
/// current time's sub-millisecond component — no `rand` dependency needed
/// for something this low-stakes.
fn reconnect_delay_secs(attempt: u32) -> u64 {
    let idx = (attempt as usize).saturating_sub(1).min(RECONNECT_DELAYS.len() - 1);
    let base = RECONNECT_DELAYS[idx];

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // Map the low bits of the current nanosecond count onto [-20%, +20%].
    let jitter_pct = ((nanos % 41) as i64) - 20; // -20..=20
    let jittered = (base as i64) + (base as i64 * jitter_pct / 100);
    jittered.max(1) as u64
}

fn audio_thread(
    rx:          std::sync::mpsc::Receiver<PlayerCmd>,
    status:      Arc<Mutex<PlayerStatus>>,
    volume:      Arc<Mutex<f32>>,
    sample_buf:  SampleBuffer,
    track_title: TrackTitle,
) {
    log::info!("audio thread started");

    let device_sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(s)  => { log::info!("OutputStream OK"); s }
        Err(e) => {
            set_status(&status, PlayerStatus::Error(format!("audio init: {e}")));
            return;
        }
    };

    let mut sink:    Option<RodioPlayer> = None;
    let mut pending: Option<Receiver<Result<RadioStream, String>>> = None;

    // ── Reconnect state ───────────────────────────────────────────────────────
    // Tracks the URL of the last intentional Play so we can reconnect
    // automatically if the stream drops without an explicit Stop.
    let mut reconnect_url:      Option<String> = None;
    let mut reconnect_attempts: u32            = 0;
    let mut reconnect_at:       Option<Instant> = None;

    loop {
        // ── Commands ──────────────────────────────────────────────────────────
        match rx.try_recv() {
            Ok(PlayerCmd::Play { url, mime }) => {
                log::info!("play: {url}");
                if let Some(s) = sink.take() { s.stop(); }
                drop(pending.take());
                reconnect_at       = None;
                reconnect_attempts = 0;
                reconnect_url      = Some(url.clone());
                set_status(&status, PlayerStatus::Connecting);
                pending = Some(start_connect(url, mime, Arc::clone(&track_title)));
            }

            Ok(PlayerCmd::TogglePause) => {
                if let Some(ref s) = sink {
                    if s.is_paused() {
                        s.play();
                        set_status(&status, PlayerStatus::Playing);
                    } else {
                        s.pause();
                        set_status(&status, PlayerStatus::Paused);
                    }
                }
            }

            Ok(PlayerCmd::Stop) => {
                log::info!("stop");
                if let Some(s) = sink.take() { s.stop(); }
                drop(pending.take());
                reconnect_url      = None;
                reconnect_attempts = 0;
                reconnect_at       = None;
                if let Ok(mut buf) = sample_buf.try_lock() { buf.clear(); }
                if let Ok(mut t) = track_title.try_lock() { *t = None; }
                set_status(&status, PlayerStatus::Idle);
            }

            Ok(PlayerCmd::Volume(v)) => {
                let v = v.clamp(0.0, 1.0);
                *volume.lock().unwrap() = v;
                if let Some(ref s) = sink { s.set_volume(v); }
            }

            Ok(PlayerCmd::Quit) | Err(TryRecvError::Disconnected) => {
                log::info!("audio thread quit");
                return;
            }
            Err(TryRecvError::Empty) => {}
        }

        // ── Pick up a completed HTTP connect ──────────────────────────────────
        let done = if let Some(ref prx) = pending {
            match prx.try_recv() {
                Ok(Ok(stream)) => {
                    let s = RodioPlayer::connect_new(device_sink.mixer());
                    s.set_volume(*volume.lock().unwrap());
                    s.append(CapturingSource::new(stream, Arc::clone(&sample_buf)));
                    log::info!("playback started");
                    reconnect_attempts = 0;   // successful connect resets the counter
                    set_status(&status, PlayerStatus::Playing);
                    sink = Some(s);
                    true
                }
                Ok(Err(e)) => {
                    // Connect failed — schedule a reconnect if we have a URL.
                    if let Some(ref url) = reconnect_url {
                        reconnect_attempts += 1;
                        if reconnect_attempts <= MAX_RECONNECT_ATTEMPTS {
                            let secs = reconnect_delay_secs(reconnect_attempts);
                            log::warn!("connect failed ({e}); reconnect #{reconnect_attempts} in {secs}s");
                            reconnect_at = Some(Instant::now() + Duration::from_secs(secs));
                            set_status(&status, PlayerStatus::Reconnecting(reconnect_attempts));
                        } else {
                            log::error!("giving up after {MAX_RECONNECT_ATTEMPTS} attempts: {url}");
                            reconnect_url = None;
                            set_status(&status, PlayerStatus::Error(e));
                        }
                    } else {
                        set_status(&status, PlayerStatus::Error(e));
                    }
                    true
                }
                Err(TryRecvError::Empty)        => false,
                Err(TryRecvError::Disconnected) => true,
            }
        } else { false };
        if done { pending = None; }

        // ── Detect stream drop ────────────────────────────────────────────────
        if matches!(sink.as_ref(), Some(s) if s.len() == 0) {
            sink = None;
            if let Ok(mut buf) = sample_buf.try_lock() { buf.clear(); }

            // Only trigger reconnect if we're not already trying to connect.
            if pending.is_none() {
                if let Some(ref url) = reconnect_url {
                    reconnect_attempts += 1;
                    if reconnect_attempts <= MAX_RECONNECT_ATTEMPTS {
                        let secs = reconnect_delay_secs(reconnect_attempts);
                        log::warn!("stream ended; reconnect #{reconnect_attempts} in {secs}s: {url}");
                        reconnect_at = Some(Instant::now() + Duration::from_secs(secs));
                        set_status(&status, PlayerStatus::Reconnecting(reconnect_attempts));
                    } else {
                        log::warn!("stream ended; giving up after {MAX_RECONNECT_ATTEMPTS} attempts");
                        reconnect_url = None;
                        set_status(&status, PlayerStatus::Idle);
                    }
                } else {
                    log::warn!("stream ended");
                    set_status(&status, PlayerStatus::Idle);
                }
            }
        }

        // ── Reconnect timer ───────────────────────────────────────────────────
        if let Some(when) = reconnect_at {
            if Instant::now() >= when && pending.is_none() {
                reconnect_at = None;
                if let Some(ref url) = reconnect_url {
                    log::info!("reconnecting (attempt #{reconnect_attempts}): {url}");
                    set_status(&status, PlayerStatus::Connecting);
                    pending = Some(start_connect(url.clone(), None, Arc::clone(&track_title)));
                }
            }
        }

        thread::sleep(Duration::from_millis(40));
    }
}
