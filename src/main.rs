mod app;
mod config;
mod favorites;
mod history;
mod logger;
mod meta_poll;
mod modes;
mod player;
mod playlist;
mod stations;
mod theme;
mod icy;
mod icy_meta;
mod ui;
mod visualizer;

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("jarl {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    // Bootstrap FIRST so ~/.config/jarl/ exists before the logger tries to
    // open jarl.log inside it.
    let (cfg, first_run) = config::Config::load()?;

    // Logger can now safely write to ~/.config/jarl/jarl.log.
    let _ = logger::init();
    log::info!("jarl {} starting", env!("CARGO_PKG_VERSION"));

    if first_run {
        log::info!("first run — config dir: {}", config::Config::config_dir().display());
    }

    if args.iter().any(|a| a == "--reset-stations") {
        let path = config::Config::stations_path();
        std::fs::write(&path, config::DEFAULT_STATIONS_TOML)?;
        println!("Stations reset to defaults.");
        println!("File: {}", path.display());
        return Ok(());
    }

    if args.iter().any(|a| a == "--reset-log") {
        let path = logger::log_path();
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
        std::fs::write(&path, "")?;
        println!("Log cleared: {}", path.display());
        return Ok(());
    }

    if args.iter().any(|a| a == "--reset-themes") {
        let path = config::Config::themes_path();
        std::fs::write(&path, crate::theme::DEFAULT_THEMES_TOML)?;
        println!("Themes reset to defaults.");
        println!("File: {}", path.display());
        return Ok(());
    }

    if args.iter().any(|a| a == "--reset-favorites") {
        let path = config::Config::config_dir().join("favorites.toml");
        std::fs::write(&path, "")?;
        println!("Favourites cleared.");
        println!("File: {}", path.display());
        return Ok(());
    }

    if let Some(pos) = args.iter().position(|a| a == "--add-station") {
        let name    = args.get(pos + 1);
        let genre   = args.get(pos + 2);
        let url     = args.get(pos + 3);
        let bitrate = args.get(pos + 4);

        match (name, genre, url, bitrate) {
            (Some(name), Some(genre), Some(url), Some(bitrate)) => {
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    eprintln!("Error: URL must start with http:// or https://");
                    std::process::exit(1);
                }
                let bitrate = if bitrate.eq_ignore_ascii_case("flac") {
                    stations::Bitrate::Flac
                } else {
                    match bitrate.parse::<u32>() {
                        Ok(n)  => stations::Bitrate::Kbps(n),
                        Err(_) => {
                            eprintln!("Error: bitrate must be a number (e.g. 128) or \"FLAC\"");
                            std::process::exit(1);
                        }
                    }
                };
                let new_station = stations::Station {
                    name:         name.clone(),
                    genre:        genre.clone(),
                    url:          url.clone(),
                    bitrate,
                    metadata_url: None,
                };
                let mut all = stations::load_stations().unwrap_or_default();
                if all.iter().any(|s| s.url == new_station.url) {
                    eprintln!("Warning: a station with that URL already exists.");
                }
                all.push(new_station);
                stations::save_stations(&all)?;
                println!("Station added: {name}");
                println!("File: {}", config::Config::stations_path().display());
            }
            _ => {
                eprintln!("Usage: jarl --add-station <name> <genre> <url> <bitrate>");
                eprintln!("  bitrate: a number in kbps (e.g. 320) or FLAC");
                eprintln!("Example:");
                eprintln!("  jarl --add-station \"SomaFM Drone Zone\" \"Ambient\" \"https://ice6.somafm.com/dronezone-256-mp3\" 256");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let mut application = app::App::new(cfg, first_run)?;
    application.run()
}

fn print_help() {
    let v = env!("CARGO_PKG_VERSION");
    println!("jarl {v}  – terminal radio player");
    println!();
    println!("USAGE:  jarl [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("  -h, --help               Print this help");
    println!("  -v, --version            Print version");
    println!("  --reset-stations         Overwrite stations.toml with built-in defaults");
    println!("  --reset-themes           Overwrite themes.toml with built-in defaults");
    println!("  --reset-log              Clear jarl.log");
    println!("  --reset-favorites        Clear the favourites list");
    println!("  --add-station <name> <genre> <url> <bitrate>");
    println!("                           Append a station to stations.toml");
    println!("                           bitrate: kbps number or FLAC");
    println!();
    println!("SUPPORTED FORMATS:  MP3 · AAC · OGG/Vorbis · FLAC");
    println!();
    println!("CONFIG FILES  (~/.config/jarl/):");
    println!("  config.toml       Theme, volume, notifications, keybindings  (auto-created)");
    println!("  stations.toml     Your station list");
    println!("  favorites.toml    Favourited stations");
    println!("  history.toml      Playback history (last 50 stations)");
    println!("  themes.toml       Themes (enable/disable with enabled = true/false)");
    println!("  jarl.log          Debug log");
    println!();
    println!("KEYS (defaults):");
    println!("  ↑↓ / j k    Navigate         Enter    Play");
    println!("  Space        Pause/resume     s        Stop");
    println!("  [ / ]        Prev/next station");
    println!("  + / -        Volume           f        Toggle favourite");
    println!("  F            Favourites only  /        Search stations");
    println!("  d            Delete station   H        History");
    println!("  t            Theme picker     h        Hide help bar");
    println!("  z            Zen mode         v        Visualizer");
    println!("  N            Notifications    r        Reload stations");
    println!("  q / Ctrl-C   Quit");
}
