# jarl

Terminal radio player written in Rust. Focused on ambient, trip-hop, downtempo, bossa nova and electronic music.

Supported formats: **MP3 · AAC · OGG/Vorbis · FLAC**

jarl is named in tribute to Gregorio Sánchez, "Chiquito de la Calzada" — the
now-playing box rotates one of his catchphrases every 5 minutes on its
right-hand side.

---

## Dependencies (Void Linux)

```sh
sudo xbps-install base-devel alsa-lib-devel cargo
```

For desktop notifications when the track changes (optional):

```sh
sudo xbps-install libnotify
```

Cargo installs compiled binaries into `$CARGO_HOME/bin`. If `CARGO_HOME` is not set, it defaults to `~/.cargo`.

**Bash** — add to `~/.bashrc` or `~/.bash_profile`:
```sh
export CARGO_HOME="$HOME/.cargo"
export PATH="$CARGO_HOME/bin:$PATH"
```

**Zsh** — add to `~/.zshrc`:
```sh
export CARGO_HOME="$HOME/.cargo"
export PATH="$CARGO_HOME/bin:$PATH"
```

**Fish** — run once (Fish uses a persistent universal variable):
```fish
set -Ux CARGO_HOME $HOME/.cargo
fish_add_path $CARGO_HOME/bin
```

Reload your shell or open a new terminal to apply the changes.

---

## Building

```sh
git clone <this-repo>
cd jarl
cargo build --release
sudo cp target/release/jarl /usr/local/bin/
```

---

## First run

On first launch jarl creates its config directory and all default files:

```
~/.config/jarl/
├── config.toml       ← theme, volume, notifications, keybindings
├── stations.toml     ← your station list
├── favorites.toml    ← starred stations
├── history.toml      ← playback history (last 50 stations)
├── themes.toml       ← custom themes
└── jarl.log          ← debug log
```

---

## Key bindings (defaults)

| Key | Action |
|---|---|
| `↑` `↓` / `j` `k` | Navigate |
| `g` / `G` | Jump to first / last station |
| `Enter` | Play selected station |
| `Space` | Pause / resume |
| `s` | Stop |
| `+` / `-` | Volume up / down |
| `f` | Toggle ★ favourite |
| `F` | Show favourites only |
| `[` / `]` | Previous / next station |
| `/` | Search stations |
| `d` | Delete selected station |
| `t` | Theme picker |
| `H` | Playback history |
| `r` | Reload stations.toml from disk |
| `T` | Reload themes.toml from disk |
| `z` | Zen mode (hide header & help bar) |
| `h` | Hide / show help bar |
| `v` | Toggle spectrum visualizer |
| `p` | Toggle transparent background |
| `N` | Toggle desktop notifications |
| `q` / `Ctrl-C` | Quit |

All bindings can be remapped in `~/.config/jarl/config.toml`. Modifier prefixes
`ctrl+`, `alt+` and `shift+` can be combined, e.g. `shift+tab` or `ctrl+shift+x`.

---

## Command-line options

```
jarl                     Launch the TUI
jarl --reset-stations    Overwrite stations.toml with built-in defaults
jarl --reset-themes      Overwrite themes.toml with built-in defaults
jarl --reset-favorites   Clear the favourites list
jarl --reset-log         Clear jarl.log
jarl --add-station <name> <genre> <url> <bitrate>
                         Append a station to stations.toml
                         bitrate: a number in kbps (e.g. 320) or FLAC
jarl --version
jarl --help
```

---

## How to add a station

jarl needs an audio stream URL. As of this version, jarl automatically detects
and resolves simple `.pls`, `.m3u` and `.m3u8` playlists that point to one
direct audio stream — paste the playlist link directly and jarl will follow it.
This does **not** cover true HLS (segmented `.m3u8` manifests with quality
variants and `.ts` chunks), which still isn't supported. The manual extraction
steps below are only needed for stations that don't expose even a playlist
file (e.g. some station webpages).

### Step 1 — Get the stream or playlist URL

**Playlist URL works directly**

If you already have a `.pls`, `.m3u` or `.m3u8` link, you can use it as-is in
`stations.toml` — jarl will fetch it, detect the playlist format (by
Content-Type or by sniffing the body), and follow it to the underlying audio
stream automatically.

**From SomaFM**

Every SomaFM channel has a `.pls` file at a predictable address — this can be
used directly as the station URL, no manual extraction needed:

```
https://somafm.com/CHANNELNAME256.pls
```

Replace `CHANNELNAME` with the channel slug (e.g. `groovesalad`, `thetrip`, `bossa`).
Full channel list: <https://somafm.com/listen/>

**From a station webpage (radio.net, tunein.com, etc.)**

These sites do not expose a stream or playlist URL directly. Use the browser:

1. Open the station page in Firefox or Chrome
2. Press `F12` to open Developer Tools
3. Go to the **Network** tab and filter by **Media**
4. Press Play on the station
5. An audio (or playlist) request will appear — right-click it and copy the URL

### Step 2 — Verify it plays and check the bitrate

```sh
mpv --no-video "YOUR_STREAM_OR_PLAYLIST_URL"
```

mpv prints the audio details in its output:

```
Audio: mp3, 44100 Hz, stereo, 256 kb/s
```

Press `q` to stop mpv once you have confirmed it works.

### Step 3 — Add it to jarl

Edit `~/.config/jarl/stations.toml` and append a block:

```toml
[[stations]]
name    = "My Station"
genre   = "Ambient · Downtempo"
url     = "https://example.com/stream.mp3"
bitrate = 256
```

For FLAC streams where the bitrate is not fixed:

```toml
[[stations]]
name    = "My FLAC Station"
genre   = "Jazz"
url     = "https://example.com/stream.flac"
bitrate = "FLAC"
```

If the station publishes a JSON "now playing" endpoint, add it to get track titles:

```toml
[[stations]]
name         = "My Station"
genre        = "Ambient"
url          = "https://example.com/stream.mp3"
bitrate      = 256
metadata_url = "https://example.com/api/nowplaying/1"
```

Supported endpoint formats: AzuraCast, Icecast `status-json.xsl`, and any endpoint
returning `{"artist":"…","title":"…"}`.

Or add a station directly from the command line:

```sh
jarl --add-station "My Station" "Ambient · Downtempo" "https://example.com/stream.mp3" 256
```

Then press `r` inside jarl to reload without restarting.

---

## Playback history

Press `H` to open the history panel, which shows the last 50 stations played (most
recent first). Navigate with `j`/`k` and press `Enter` to play any entry. The history
is persisted in `~/.config/jarl/history.toml`.

---

## Desktop notifications

jarl sends a desktop notification via `notify-send` whenever the track title changes.
Requires `libnotify` to be installed. Toggle notifications at any time with `N`; the
setting is saved to `config.toml` and persists across sessions.

---

## Automatic reconnection

If a stream drops unexpectedly, jarl will attempt to reconnect automatically up to 8
times using exponential backoff (with a small randomised jitter on each delay, so
multiple stations dropping at once don't all retry in lockstep). The status badge
shows the current attempt number (e.g. `◌ reconnecting… (2/8)`). If all attempts
fail, playback stops.

jarl also detects "dead air": some stations keep the connection open but serve
silence after a backend failure, which a normal connection-level reconnect can't
catch. If the decoded audio measures as silent for several consecutive seconds
while a station is supposedly playing, jarl forces a reconnect on its own.

---

## Themes

Press `t` to open the theme picker.

Built-in themes: **Dracula · Catppuccin Mocha · Nord · Gruvbox Dark · Tokyo Night · Everforest Dark**

### Adding a custom theme

Edit `~/.config/jarl/themes.toml`:

```toml
[[themes]]
name      = "My Theme"
bg        = "#1a1b26"   # main background
fg        = "#c0caf5"   # primary text
accent    = "#7aa2f7"   # titles, highlighted keys
highlight = "#292e42"   # selected row background
success   = "#9ece6a"   # live indicator, playing station name
warning   = "#e0af68"   # connecting / paused
error     = "#f7768e"   # error messages
muted     = "#565f89"   # genre labels, secondary text
border    = "#3b4261"   # box borders
```

---

## Troubleshooting

**No sound at all**
Run `alsamixer` and check the Master volume is not muted.
If using PipeWire, ensure the ALSA plugin routes correctly.

**Build fails: `alsa/asoundlib.h: No such file or directory`**
```sh
sudo xbps-install alsa-lib-devel
```

**A station shows "error" immediately**
The stream URL may be wrong or the server may be down.
Check `~/.config/jarl/jarl.log` for the exact HTTP error.
Re-verify the URL with `mpv --no-video "URL"`.

**A station connects but produces no audio**
If the stream is technically connected but silent for more than a few seconds,
jarl will detect this and reconnect automatically. If that doesn't resolve it,
the stream may be using a format jarl can't decode. jarl resolves simple
`.pls`/`.m3u`/`.m3u8` playlists that point directly at one audio stream, but it
does not support true HLS (segmented `.m3u8` manifests with `#EXT-X-STREAM-INF`
variants and `.ts` chunks) or DASH.

**No desktop notifications**
Ensure `libnotify` is installed (`sudo xbps-install libnotify`) and that
notifications are enabled in jarl (press `N` to toggle, check `config.toml`
for `notify = true`).

---

## Running the tests

```sh
cargo test
```

Tests cover the JSON metadata parser (`meta_poll.rs`), the ICY stream title
parser (`icy_meta.rs`), and playlist detection/parsing (`playlist.rs`).

---

## Project layout

```
jarl/
├── Cargo.toml
├── stations.toml       ← default station list (copied to config dir on first run)
├── README.md
└── src/
    ├── main.rs         ← entry point, CLI flags
    ├── config.rs       ← config dir, load/save, keybindings
    ├── stations.rs     ← Station struct, TOML loader/saver
    ├── favorites.rs    ← load/save favourites
    ├── history.rs      ← playback history (load/push/save)
    ├── theme.rs        ← built-in themes, custom theme loader
    ├── player.rs       ← symphonia decoder + rodio audio engine, auto-reconnect
    ├── playlist.rs     ← .pls/.m3u/.m3u8 detection and parsing
    ├── meta_poll.rs    ← background metadata poller for now-playing endpoints
    ├── icy.rs          ← ICY stream reader (MP3/AAC metadata)
    ├── icy_meta.rs     ← shared TrackTitle type
    ├── logger.rs       ← file logger (~/.config/jarl/jarl.log)
    ├── visualizer.rs   ← FFT spectrum analyser, RMS-based silence detection
    ├── app.rs          ← application state, event loop, shared playback helpers
    ├── modes/          ← per-AppMode key dispatch
    │   ├── normal.rs
    │   ├── search.rs
    │   ├── history.rs
    │   ├── theme_picker.rs
    │   └── confirm_delete.rs
    └── ui.rs           ← ratatui rendering
```

---

## License

GNU General Public License v3.0 (GPL-3.0)
