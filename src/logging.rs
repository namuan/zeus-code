//! File logging with size-based rolling.
//!
//! Writes to `~/.zeus-code/logs/zeus.log` and rotates when the file
//! exceeds `MAX_LOG_SIZE`. Keeps `MAX_LOG_FILES` backup files
//! (zeus.1.log, zeus.2.log, …).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Maximum size of a single log file before rotation (10 MB).
const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;

/// Number of backup files to keep.
const MAX_LOG_FILES: usize = 5;

/// Inner state shared across clones.
struct Inner {
    path: PathBuf,
    file: File,
    size: u64,
}

/// A writer that rotates log files by size. Cloneable and `Send + Sync`.
#[derive(Clone)]
pub struct RollingWriter {
    inner: Arc<Mutex<Inner>>,
}

impl RollingWriter {
    /// Open (or create) the log file and return a writer.
    pub fn new() -> io::Result<Self> {
        let log_dir = log_dir();
        fs::create_dir_all(&log_dir)?;

        let path = log_dir.join("zeus.log");
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner { path, file, size })),
        })
    }

    /// Rotate the log file if needed, then write.
    fn write_impl(&self, buf: &[u8]) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.size + buf.len() as u64 > MAX_LOG_SIZE {
            // Drop lock during rotation (re-acquire after)
            drop(inner);
            self.rotate()?;
            inner = self.inner.lock().unwrap();
            inner.size = 0;
        }
        inner.file.write_all(buf)?;
        inner.file.flush()?;
        inner.size += buf.len() as u64;
        Ok(())
    }

    fn rotate(&self) -> io::Result<()> {
        let inner = self.inner.lock().unwrap();
        let base = &inner.path;

        // Shift existing backups: zeus.4.log → zeus.5.log, …, zeus.log → zeus.1.log
        for i in (1..=MAX_LOG_FILES).rev() {
            let old = file_with_suffix(base, i);
            let new = file_with_suffix(base, i + 1);
            if old.exists() {
                if i == MAX_LOG_FILES {
                    let _ = fs::remove_file(&old);
                } else {
                    let _ = fs::rename(&old, &new);
                }
            }
        }
        // Rename current log to zeus.1.log
        if base.exists() {
            let backup = file_with_suffix(base, 1);
            let _ = fs::rename(base, &backup);
        }
        Ok(())
    }

    /// Re-open the log file after rotation.
    /// Called by the non-blocking writer when it needs a fresh handle.
    pub fn reopen(&self) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&inner.path)?;
        inner.file = file;
        inner.size = inner.file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(())
    }
}

impl Write for RollingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_impl(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().unwrap().file.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RollingWriter {
    type Writer = RollingWriter;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

/// Build the log directory path: `~/.zeus-code/logs/`
fn log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zeus-code")
        .join("logs")
}

/// Create a path with a numeric suffix: `zeus.log` → `zeus.1.log`
fn file_with_suffix(base: &std::path::Path, n: usize) -> PathBuf {
    let stem = base.file_stem().unwrap_or_default().to_string_lossy();
    let ext = base.extension().unwrap_or_default().to_string_lossy();
    let parent = base.parent().unwrap_or_else(|| std::path::Path::new("."));
    parent.join(format!("{stem}.{n}.{ext}"))
}
