# jarl

Terminal radio player written in Rust. Focused on ambient, trip-hop, downtempo, bossa nova and electronic music.

Supported formats: **MP3 · AAC · OGG/Vorbis · FLAC**

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

All bindings can be remapped in `~/.config/jarl/config.toml`.

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

jarl needs a **direct audio stream URL** — not a webpage, not a playlist file.
The URL must point to an MP3, AAC, OGG or FLAC stream.

### Step 1 — Get the direct stream URL

**From a .pls or .m3u playlist file**

Playlist files are plain text and contain the real stream URL on a `File1=` line:

```sh
curl -s "https://example.com/stream.pls" | grep "^File1"
curl -s "https://example.com/stream.m3u" | grep "^http"
```

The value after `File1=` is the URL you need.

**From a station webpage (radio.net, tunein.com, etc.)**

These sites do not expose the stream URL directly. Use the browser:

1. Open the station page in Firefox or Chrome
2. Press `F12` to open Developer Tools
3. Go to the **Network** tab and filter by **Media**
4. Press Play on the station
5. An audio request will appear — right-click it and copy the URL

**From SomaFM**

Every SomaFM channel has a `.pls` file at a predictable address:

```sh
curl -s "https://somafm.com/CHANNELNAME256.pls" | grep "^File1"
```

Replace `CHANNELNAME` with the channel slug (e.g. `groovesalad`, `thetrip`, `bossa`).
Full channel list: <https://somafm.com/listen/>

Example:
```sh
curl -s "https://somafm.com/bossa256.pls" | grep "^File1"
# File1=http://ice2.somafm.com/bossa-256-mp3
```

### Step 2 — Verify it plays and check the bitrate

```sh
mpv --no-video "YOUR_STREAM_URL"
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
times using exponential backoff. The status badge shows the current attempt number
(e.g. `◌ reconnecting… (2/8)`). If all attempts fail, playback stops.

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
The stream may be serving a format that could not be probed
(e.g. an HLS stream disguised as MP3). Only direct audio streams are supported —
HLS (`.m3u8`) and DASH are not.

**No desktop notifications**
Ensure `libnotify` is installed (`sudo xbps-install libnotify`) and that
notifications are enabled in jarl (press `N` to toggle, check `config.toml`
for `notify = true`).

---

## Running the tests

```sh
cargo test
```

Tests cover the JSON metadata parser (`meta_poll.rs`) and the ICY stream title
parser (`icy_meta.rs`).

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
    ├── meta_poll.rs    ← background metadata poller for now-playing endpoints
    ├── icy.rs          ← ICY stream reader (MP3/AAC metadata)
    ├── icy_meta.rs     ← shared TrackTitle type
    ├── logger.rs       ← file logger (~/.config/jarl/jarl.log)
    ├── visualizer.rs   ← FFT spectrum analyser
    ├── app.rs          ← application state, event loop, keybindings
    └── ui.rs           ← ratatui rendering
```

---

## License

GNU General Public License v3.0 (GPL-3.0)
