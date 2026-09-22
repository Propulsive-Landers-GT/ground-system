//! Plays a recorded session back through the same WebSocket API, once, with the
//! original timing scaled by `speed`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use gs_protocol::HEARTBEAT_HZ;
use tokio::time::{interval, sleep_until, MissedTickBehavior};
use tracing::{info, warn};

use crate::hub::Hub;
use crate::link_stats::LinkStats;
use crate::messages::{LogRecord, ServerMessage};

pub struct Replay {
    path: PathBuf,
    records: Vec<LogRecord>,
    speed: f64,
}

impl Replay {
    /// Loads the whole file up front so a bad path or format fails at startup. `path`
    /// is a recording directory or its `session.jsonl`.
    pub fn load(path: &Path, speed: f64) -> anyhow::Result<Self> {
        if !(speed.is_finite() && speed > 0.0) {
            bail!("--replay-speed must be a positive number");
        }
        let path = if path.is_dir() {
            path.join("session.jsonl")
        } else {
            path.to_path_buf()
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading replay file {}", path.display()))?;
        let (records, skipped) = parse_records(&text);
        if records.is_empty() {
            bail!("{} contains no replayable records", path.display());
        }
        if skipped > 0 {
            warn!("replay: skipped {skipped} unreadable lines");
        }
        Ok(Self {
            path,
            records,
            speed,
        })
    }

    pub async fn run(self, hub: Arc<Hub>) {
        let label = format!("replay:{}", self.path.display());
        let first_t = self.records[0].time();
        let duration_s = self.records[self.records.len() - 1].time() - first_t;
        info!(
            "replaying {} records ({duration_s:.1} s) at {}x",
            self.records.len(),
            self.speed
        );

        let mut stats = LinkStats::default();
        // Appears in `link` once the recording turns out to contain a test stand.
        let mut stand_stats: Option<LinkStats> = None;
        let mut tick = interval(Duration::from_secs_f64(1.0 / HEARTBEAT_HZ));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let started = tokio::time::Instant::now();
        let mut records = self.records.into_iter().peekable();

        // Keeps publishing `link` after the last record so clients see the link go quiet.
        loop {
            let next_due = records.peek().map(|record| {
                let offset_s = ((record.time() - first_t) / self.speed).max(0.0);
                started + Duration::from_secs_f64(offset_s)
            });
            tokio::select! {
                _ = tick.tick() => {
                    let now = Instant::now();
                    let stand = stand_stats
                        .as_ref()
                        .map(|s| s.stand_status(now, label.clone()));
                    let status = stats.status(now, label.clone(), None, stand);
                    hub.publish(&ServerMessage::Link(status));
                }
                _ = sleep_until(next_due.unwrap_or(started)), if next_due.is_some() => {
                    let Some(record) = records.next() else { continue };
                    let now = Instant::now();
                    let message = record.into_server_message();
                    match &message {
                        ServerMessage::Sent(_) => {}
                        // Only the stand sends these; `source` was rewritten to Replay
                        // already, so the message type is what tells the two apart.
                        ServerMessage::StandStatus(_) => {
                            stand_stats.get_or_insert_default().on_packet(now);
                        }
                        ServerMessage::Flight(flight) => {
                            stats.on_packet(now);
                            stats.on_flight_seq(flight.seq, now);
                        }
                        _ => stats.on_packet(now),
                    }
                    hub.publish(&message);
                    if records.peek().is_none() {
                        info!("replay finished");
                    }
                }
            }
        }
    }
}

/// Returns the parsed records in time order and the number of lines that were skipped.
fn parse_records(text: &str) -> (Vec<LogRecord>, usize) {
    let mut skipped = 0;
    let mut records: Vec<LogRecord> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let parsed = serde_json::from_str::<LogRecord>(line).ok();
            skipped += usize::from(parsed.is_none());
            parsed
        })
        .collect();
    records.sort_by(|a, b| a.time().total_cmp(&b.time()));
    (records, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_lines_and_skips_garbage() {
        let text = concat!(
            r#"{"t":11.0,"dir":"up","seq":1,"kind":"Arm"}"#,
            "\n\n",
            "garbage\n",
            r#"{"t":10.0,"dir":"down","type":"event","data":{"time_s":1.0,"severity":"Info","text":"hi"}}"#,
            "\n",
            r#"{"t":12.0,"dir":"down","type":"ack","data":{"seq":1,"time_s":2.0,"result":{"Rejected":"no"}}}"#,
            "\n",
        );
        let (records, skipped) = parse_records(text);
        assert_eq!(skipped, 1);
        let times: Vec<f64> = records.iter().map(LogRecord::time).collect();
        assert_eq!(times, [10.0, 11.0, 12.0]);
        assert!(matches!(
            records[1].clone().into_server_message(),
            ServerMessage::Sent(_)
        ));
    }
}
