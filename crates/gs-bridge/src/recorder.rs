//! Session log writer: one JSON object per line, see `messages::LogRecord`.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::error;

use crate::messages::LogRecord;

pub struct Recorder {
    path: PathBuf,
    /// `None` after a write error: recording stops, the bridge keeps running.
    writer: Option<BufWriter<File>>,
}

impl Recorder {
    /// Creates `<log_dir>/session-<utc>.jsonl`.
    pub fn create(log_dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(log_dir)?;
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let path = log_dir.join(format!("session-{stamp}.jsonl"));
        let writer = BufWriter::new(File::create(&path)?);
        Ok(Self {
            path,
            writer: Some(writer),
        })
    }

    /// Path reported in the `link` message; `None` once recording has failed.
    pub fn active_path(&self) -> Option<String> {
        self.writer
            .as_ref()
            .map(|_| self.path.display().to_string())
    }

    pub fn record(&mut self, record: &LogRecord) {
        let result = match &mut self.writer {
            Some(writer) => serde_json::to_writer(&mut *writer, record)
                .map_err(io::Error::from)
                .and_then(|()| writer.write_all(b"\n")),
            None => return,
        };
        self.stop_on_error(result);
    }

    /// Called periodically so a crash loses at most a fraction of a second.
    pub fn flush(&mut self) {
        if let Some(writer) = &mut self.writer {
            let result = writer.flush();
            self.stop_on_error(result);
        }
    }

    fn stop_on_error(&mut self, result: io::Result<()>) {
        if let Err(e) = result {
            error!("recording to {} stopped: {e}", self.path.display());
            self.writer = None;
        }
    }
}

pub fn unix_time_s() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
