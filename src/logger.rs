//! File logger that writes to ~/.config/jarl/jarl.log
//! This directory is guaranteed to exist because Config::bootstrap() creates it.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};

use crate::config::Config;

pub fn log_path() -> PathBuf {
    Config::config_dir().join("jarl.log")
}

struct FileLogger {
    file: Mutex<std::fs::File>,
}

impl Log for FileLogger {
    fn enabled(&self, meta: &Metadata) -> bool {
        meta.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!(
            "[{ts}] [{:5}] {}: {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

/// Initialise the file logger.
///
/// MUST be called after Config::bootstrap() so that the config directory
/// already exists. Writes to ~/.config/jarl/jarl.log.
pub fn init() -> Result<(), SetLoggerError> {
    let path = log_path();

    // Ensure the directory exists (should already, but be safe).
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f)  => f,
        Err(e) => {
            // Cannot open the log file — continue without logging rather
            // than crashing the application.
            eprintln!("jarl: could not open log file {}: {e}", path.display());
            return Ok(());
        }
    };

    let logger = Box::new(FileLogger { file: Mutex::new(file) });
    log::set_boxed_logger(logger)?;
    log::set_max_level(LevelFilter::Debug);
    Ok(())
}
