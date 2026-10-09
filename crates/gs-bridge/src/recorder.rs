//! Recording: one directory per operator-started recording, `logs/<utc>-<name>/`, holding
//! `session.jsonl` (everything, for replay) plus `flight.csv`, `stand.csv` and
//! `events.csv` for analysis. Column lists are in `docs/DESIGN.md`.

use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gs_protocol::{
    AckResult, CommandAck, EventMsg, FlightTelemetry, StandChannel, StandMode, StandOutput,
    StandStatus, StandTelemetry, ValveId, ValveState,
};
use tracing::error;

use crate::messages::{LogRecord, SentCommand, ServerMessage};

const MAX_NAME_LEN: usize = 64;

/// Column order of `stand.csv`'s per-channel block.
pub const STAND_CHANNELS: [StandChannel; 12] = [
    StandChannel::Opt,
    StandChannel::Ipt,
    StandChannel::Ept,
    StandChannel::M1,
    StandChannel::M2,
    StandChannel::Pupt,
    StandChannel::Lfpt,
    StandChannel::T1,
    StandChannel::T2,
    StandChannel::Thrust,
    StandChannel::NitrousMass,
    StandChannel::RcsThrust,
];

/// Column order of `stand.csv`'s per-valve blocks.
pub const VALVES: [ValveId; 15] = [
    ValveId::Omv,
    ValveId::Mtv,
    ValveId::IgV,
    ValveId::OFill,
    ValveId::OIso,
    ValveId::OVnt,
    ValveId::PuMv,
    ValveId::PuFill,
    ValveId::PuIso,
    ValveId::PuVnt,
    ValveId::PuMvnt,
    ValveId::LfVnt,
    ValveId::TVnt,
    ValveId::Rcs1,
    ValveId::Rcs2,
];

pub const FLIGHT_HEADER: [&str; 51] = [
    "t_unix",
    "time_s",
    "seq",
    "source",
    "phase",
    "phase_time_s",
    "control_mode",
    "terminated",
    "px",
    "py",
    "pz",
    "vx",
    "vy",
    "vz",
    "qx",
    "qy",
    "qz",
    "qw",
    "wx",
    "wy",
    "wz",
    "mass",
    "gimbal_theta",
    "gimbal_phi",
    "thrust",
    "rcs",
    "tilt_deg",
    "trajectory_deviation_m",
    "position_age_s",
    "link_age_s",
    "imu_ok",
    "gps_ok",
    "uwb_ok",
    "ax",
    "ay",
    "az",
    "gx",
    "gy",
    "gz",
    "chamber_pressure",
    "tank_pressure",
    "truth_px",
    "truth_py",
    "truth_pz",
    "truth_vx",
    "truth_vy",
    "truth_vz",
    "truth_qx",
    "truth_qy",
    "truth_qz",
    "truth_qw",
];

pub const EVENTS_HEADER: [&str; 5] = ["t_unix", "time_s", "kind", "severity", "text"];

pub fn stand_header() -> Vec<String> {
    let mut header: Vec<String> = ["t_unix", "time_s", "source", "mtv_percent", "igniter", "daq_sync"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    header.extend(STAND_CHANNELS.iter().map(|c| format!("{c:?}")));
    header.extend(VALVES.iter().map(|v| format!("{v:?}")));
    header.extend(VALVES.iter().map(|v| format!("{v:?}_deg")));
    header
}

// ---------------------------------------------------------------------------
// Row builders (pure, so they can be tested without touching the disk)
// ---------------------------------------------------------------------------

fn cell<T: Display>(v: T) -> String {
    v.to_string()
}

fn opt<T: Display>(v: Option<T>) -> String {
    v.map(cell).unwrap_or_default()
}

fn debug<T: std::fmt::Debug>(v: T) -> String {
    format!("{v:?}")
}

pub fn flight_row(t_unix: f64, f: &FlightTelemetry) -> Vec<String> {
    let mut row = Vec::with_capacity(FLIGHT_HEADER.len());
    row.extend([
        cell(t_unix),
        cell(f.time_s),
        cell(f.seq),
        debug(f.source),
        debug(f.phase),
        cell(f.phase_time_s),
        debug(f.control_mode),
        cell(f.terminated),
    ]);
    row.extend(f.position.iter().map(cell));
    row.extend(f.velocity.iter().map(cell));
    row.extend(f.attitude.iter().map(cell));
    row.extend(f.angular_velocity.iter().map(cell));
    row.extend([
        cell(f.mass),
        cell(f.gimbal_theta),
        cell(f.gimbal_phi),
        cell(f.thrust),
        cell(f.rcs),
        cell(f.tilt_deg),
        opt(f.trajectory_deviation_m),
        cell(f.position_age_s),
        opt(f.link_age_s),
        cell(f.sensors.imu_ok),
        cell(f.sensors.gps_ok),
        cell(f.sensors.uwb_ok),
    ]);
    row.extend(f.sensors.accel.iter().map(cell));
    row.extend(f.sensors.gyro.iter().map(cell));
    row.extend([
        opt(f.sensors.chamber_pressure),
        opt(f.sensors.tank_pressure),
    ]);
    match &f.truth {
        Some(truth) => {
            row.extend(truth.position.iter().map(cell));
            row.extend(truth.velocity.iter().map(cell));
            row.extend(truth.attitude.iter().map(cell));
        }
        None => row.extend(std::iter::repeat_n(String::new(), 10)),
    }
    debug_assert_eq!(row.len(), FLIGHT_HEADER.len());
    row
}

pub fn stand_row(t_unix: f64, s: &StandTelemetry) -> Vec<String> {
    let on = |output| s.outputs_on.contains(&output);
    let mut row = vec![
        cell(t_unix),
        cell(s.time_s),
        debug(s.source),
        opt(s.mtv_percent),
        cell(on(StandOutput::Igniter)),
        cell(on(StandOutput::DaqSync)),
    ];
    for channel in STAND_CHANNELS {
        let value = s.channels.iter().find(|(c, _)| *c == channel).map(|&(_, v)| v);
        row.push(opt(value));
    }
    for valve in VALVES {
        let state = s.valves.iter().find(|v| v.id == valve).map(|v| match v.state {
            ValveState::Open => "open",
            ValveState::Closed => "closed",
            ValveState::Unknown => "unknown",
        });
        row.push(opt(state));
    }
    for valve in VALVES {
        let deg = s
            .valves
            .iter()
            .find(|v| v.id == valve)
            .and_then(|v| v.position_deg);
        row.push(opt(deg));
    }
    row
}

pub fn event_row(t_unix: f64, e: &EventMsg) -> Vec<String> {
    vec![
        cell(t_unix),
        cell(e.time_s),
        "event".into(),
        debug(e.severity),
        e.text.clone(),
    ]
}

pub fn sent_row(t_unix: f64, c: &SentCommand) -> Vec<String> {
    let kind = serde_json::to_string(&c.kind).unwrap_or_default();
    vec![
        cell(t_unix),
        String::new(),
        "sent".into(),
        String::new(),
        format!("{} {kind}", c.seq),
    ]
}

pub fn ack_row(t_unix: f64, a: &CommandAck) -> Vec<String> {
    let result = match &a.result {
        AckResult::Accepted => "Accepted".to_string(),
        AckResult::Rejected(reason) => format!("Rejected: {reason}"),
    };
    vec![
        cell(t_unix),
        cell(a.time_s),
        "ack".into(),
        String::new(),
        format!("{} {result}", a.seq),
    ]
}

/// What `events.csv` remembers about the stand so only transitions are written.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StandSnapshot {
    mode: StandMode,
    sequence: Option<String>,
}

/// Rows for the mode and sequence transitions between the previous status and this one.
fn stand_status_rows(
    t_unix: f64,
    previous: Option<&StandSnapshot>,
    status: &StandStatus,
) -> Vec<Vec<String>> {
    let row = |text: String| {
        vec![
            cell(t_unix),
            cell(status.time_s),
            "stand_status".into(),
            String::new(),
            text,
        ]
    };
    let sequence = status.sequence.as_ref().map(|s| s.name.clone());
    let mut rows = Vec::new();
    match previous {
        None => rows.push(row(format!("mode {:?}", status.mode))),
        Some(prev) if prev.mode != status.mode => {
            rows.push(row(format!("mode {:?} -> {:?}", prev.mode, status.mode)));
        }
        Some(_) => {}
    }
    let prev_sequence = previous.and_then(|p| p.sequence.as_deref());
    if prev_sequence != sequence.as_deref() {
        if let Some(name) = prev_sequence {
            rows.push(row(format!("sequence {name} ended")));
        }
        if let Some(name) = &sequence {
            rows.push(row(format!("sequence {name} started")));
        }
    }
    rows
}

fn recording_row(t_unix: f64, text: &str) -> Vec<String> {
    vec![
        cell(t_unix),
        String::new(),
        "recording".into(),
        String::new(),
        text.to_string(),
    ]
}

/// RFC 4180 quoting: only cells that need it are quoted.
fn csv_line(cells: &[String]) -> String {
    let mut line = String::new();
    for (i, c) in cells.iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        if c.contains([',', '"', '\n', '\r']) {
            line.push('"');
            line.push_str(&c.replace('"', "\"\""));
            line.push('"');
        } else {
            line.push_str(c);
        }
    }
    line.push('\n');
    line
}

/// Keeps `[A-Za-z0-9._-]`, replaces anything else with `_`, and caps the length.
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .take(MAX_NAME_LEN)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The recorder
// ---------------------------------------------------------------------------

struct Files {
    session: BufWriter<File>,
    flight: BufWriter<File>,
    stand: BufWriter<File>,
    events: BufWriter<File>,
}

pub struct Recorder {
    dir: PathBuf,
    /// `None` after a write error: recording stops, the bridge keeps running.
    files: Option<Files>,
    last_stand: Option<StandSnapshot>,
}

impl Recorder {
    /// Creates `<log_dir>/<utc>-<name>/` (just `<utc>/` without a name) with headers
    /// written and a start marker in `events.csv`.
    pub fn start(log_dir: &Path, name: Option<&str>) -> io::Result<Self> {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let name = name.map(sanitize_name).filter(|n| !n.is_empty());
        let dir_name = match name {
            Some(name) => format!("{stamp}-{name}"),
            None => stamp,
        };
        let dir = log_dir.join(dir_name);
        fs::create_dir_all(&dir)?;

        let open = |file: &str| -> io::Result<BufWriter<File>> {
            Ok(BufWriter::new(File::create(dir.join(file))?))
        };
        let mut files = Files {
            session: open("session.jsonl")?,
            flight: open("flight.csv")?,
            stand: open("stand.csv")?,
            events: open("events.csv")?,
        };
        let header = |cells: &[&str]| cells.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        files.flight.write_all(csv_line(&header(&FLIGHT_HEADER)).as_bytes())?;
        files.stand.write_all(csv_line(&stand_header()).as_bytes())?;
        files.events.write_all(csv_line(&header(&EVENTS_HEADER)).as_bytes())?;
        files
            .events
            .write_all(csv_line(&recording_row(unix_time_s(), "start")).as_bytes())?;
        files.events.flush()?;

        Ok(Self {
            dir,
            files: Some(files),
            last_stand: None,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path reported in the `link` message; `None` once recording has failed.
    pub fn active_path(&self) -> Option<String> {
        self.files.as_ref().map(|_| self.dir.display().to_string())
    }

    pub fn failed(&self) -> bool {
        self.files.is_none()
    }

    pub fn record(&mut self, record: &LogRecord) {
        let Some(files) = &mut self.files else { return };
        let result = Self::write_record(files, &mut self.last_stand, record);
        self.stop_on_error(result);
    }

    fn write_record(
        files: &mut Files,
        last_stand: &mut Option<StandSnapshot>,
        record: &LogRecord,
    ) -> io::Result<()> {
        serde_json::to_writer(&mut files.session, record)?;
        files.session.write_all(b"\n")?;

        let t = record.time();
        match record {
            LogRecord::Down { message, .. } => match message {
                ServerMessage::Flight(f) => {
                    files.flight.write_all(csv_line(&flight_row(t, f)).as_bytes())?;
                }
                ServerMessage::Stand(s) => {
                    files.stand.write_all(csv_line(&stand_row(t, s)).as_bytes())?;
                }
                ServerMessage::Event(e) => {
                    files.events.write_all(csv_line(&event_row(t, e)).as_bytes())?;
                }
                ServerMessage::Ack(a) => {
                    files.events.write_all(csv_line(&ack_row(t, a)).as_bytes())?;
                }
                ServerMessage::StandStatus(status) => {
                    for row in stand_status_rows(t, last_stand.as_ref(), status) {
                        files.events.write_all(csv_line(&row).as_bytes())?;
                    }
                    *last_stand = Some(StandSnapshot {
                        mode: status.mode,
                        sequence: status.sequence.as_ref().map(|s| s.name.clone()),
                    });
                }
                ServerMessage::Trajectory(_)
                | ServerMessage::Params(_)
                | ServerMessage::Sent(_)
                | ServerMessage::Link(_)
                | ServerMessage::Error { .. } => {}
            },
            LogRecord::Up { command, .. } => {
                files.events.write_all(csv_line(&sent_row(t, command)).as_bytes())?;
            }
        }
        Ok(())
    }

    /// Called periodically so a crash loses at most a fraction of a second.
    pub fn flush(&mut self) {
        if let Some(files) = &mut self.files {
            let result = files
                .session
                .flush()
                .and_then(|()| files.flight.flush())
                .and_then(|()| files.stand.flush())
                .and_then(|()| files.events.flush());
            self.stop_on_error(result);
        }
    }

    /// Writes the stop marker and closes the files.
    pub fn stop(mut self) {
        if let Some(files) = &mut self.files {
            let result = files
                .events
                .write_all(csv_line(&recording_row(unix_time_s(), "stop")).as_bytes());
            self.stop_on_error(result);
        }
        self.flush();
        self.files = None;
    }

    fn stop_on_error(&mut self, result: io::Result<()>) {
        if let Err(e) = result {
            error!("recording to {} stopped: {e}", self.dir.display());
            self.files = None;
        }
    }
}

pub fn unix_time_s() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tests::sample_flight;
    use gs_protocol::{CommandKind, SequenceProgress, Severity, Source, ValveStatus};

    fn col(header: &[&str], name: &str) -> usize {
        header.iter().position(|h| *h == name).unwrap()
    }

    #[test]
    fn flight_row_matches_header_and_blanks_nones() {
        let mut f = sample_flight(7);
        f.trajectory_deviation_m = None;
        f.link_age_s = None;
        f.sensors.tank_pressure = None;
        f.truth = None;
        let row = flight_row(1.5, &f);
        assert_eq!(row.len(), FLIGHT_HEADER.len());
        assert_eq!(row[col(&FLIGHT_HEADER, "t_unix")], "1.5");
        assert_eq!(row[col(&FLIGHT_HEADER, "seq")], "7");
        assert_eq!(row[col(&FLIGHT_HEADER, "source")], "Sim");
        assert_eq!(row[col(&FLIGHT_HEADER, "phase")], "Hover");
        assert_eq!(row[col(&FLIGHT_HEADER, "terminated")], "false");
        assert_eq!(row[col(&FLIGHT_HEADER, "px")], "0.5");
        assert_eq!(row[col(&FLIGHT_HEADER, "qw")], "1");
        assert_eq!(row[col(&FLIGHT_HEADER, "rcs")], "-1");
        assert_eq!(row[col(&FLIGHT_HEADER, "trajectory_deviation_m")], "");
        assert_eq!(row[col(&FLIGHT_HEADER, "link_age_s")], "");
        assert_eq!(row[col(&FLIGHT_HEADER, "chamber_pressure")], "15");
        assert_eq!(row[col(&FLIGHT_HEADER, "tank_pressure")], "");
        for name in ["truth_px", "truth_vz", "truth_qw"] {
            assert_eq!(row[col(&FLIGHT_HEADER, name)], "", "{name}");
        }

        let with_truth = flight_row(2.0, &sample_flight(8));
        assert_eq!(with_truth[col(&FLIGHT_HEADER, "truth_pz")], "50");
        assert_eq!(with_truth[col(&FLIGHT_HEADER, "link_age_s")], "0.25");
    }

    #[test]
    fn stand_row_matches_header_and_blanks_missing_channels_and_valves() {
        let header = stand_header();
        let col = |name: &str| header.iter().position(|h| h == name).unwrap();
        let s = StandTelemetry {
            time_s: 3.0,
            source: Source::Stand,
            channels: vec![
                (StandChannel::Thrust, 812.5),
                (StandChannel::NitrousMass, 11.2),
            ],
            valves: vec![
                ValveStatus {
                    id: ValveId::Omv,
                    state: ValveState::Open,
                    position_deg: None,
                },
                ValveStatus {
                    id: ValveId::OIso,
                    state: ValveState::Unknown,
                    position_deg: None,
                },
                ValveStatus {
                    id: ValveId::Mtv,
                    state: ValveState::Open,
                    position_deg: Some(45.0),
                },
            ],
            outputs_on: vec![StandOutput::DaqSync],
            mtv_percent: Some(50.0),
        };
        let row = stand_row(9.0, &s);
        assert_eq!(row.len(), header.len());
        assert_eq!(row[col("t_unix")], "9");
        assert_eq!(row[col("source")], "Stand");
        assert_eq!(row[col("mtv_percent")], "50");
        assert_eq!(row[col("igniter")], "false");
        assert_eq!(row[col("daq_sync")], "true");
        assert_eq!(row[col("Thrust")], "812.5");
        assert_eq!(row[col("NitrousMass")], "11.2");
        assert_eq!(row[col("Opt")], "");
        assert_eq!(row[col("RcsThrust")], "");
        assert_eq!(row[col("Omv")], "open");
        assert_eq!(row[col("OIso")], "unknown");
        assert_eq!(row[col("Mtv")], "open");
        assert_eq!(row[col("Rcs1")], "");
        assert_eq!(row[col("Mtv_deg")], "45");
        assert_eq!(row[col("Omv_deg")], "");

        let empty = StandTelemetry {
            channels: Vec::new(),
            valves: Vec::new(),
            outputs_on: Vec::new(),
            mtv_percent: None,
            ..s
        };
        let row = stand_row(9.0, &empty);
        assert_eq!(row.len(), header.len());
        assert!(row[3..].iter().all(|c| c.is_empty() || c == "false"));
    }

    #[test]
    fn event_rows() {
        let e = event_row(
            1.0,
            &EventMsg {
                time_s: 2.0,
                severity: Severity::Warning,
                text: "hello, \"world\"".into(),
            },
        );
        assert_eq!(e, ["1", "2", "event", "Warning", "hello, \"world\""]);
        assert_eq!(
            csv_line(&e),
            "1,2,event,Warning,\"hello, \"\"world\"\"\"\n"
        );

        let sent = sent_row(
            1.0,
            &SentCommand {
                seq: 5,
                kind: CommandKind::SetValve {
                    id: ValveId::Omv,
                    open: true,
                },
            },
        );
        assert_eq!(sent[2], "sent");
        assert_eq!(sent[4], r#"5 {"SetValve":{"id":"Omv","open":true}}"#);

        let ack = ack_row(
            1.0,
            &CommandAck {
                seq: 5,
                time_s: 2.5,
                result: AckResult::Rejected("not armed".into()),
            },
        );
        assert_eq!(ack, ["1", "2.5", "ack", "", "5 Rejected: not armed"]);
    }

    #[test]
    fn stand_status_only_records_transitions() {
        let status = |mode, sequence: Option<&str>| StandStatus {
            time_s: 0.0,
            mode,
            actuation_link_ok: true,
            loadcell_link_ok: true,
            sequences: Vec::new(),
            sequence: sequence.map(|name| SequenceProgress {
                name: name.into(),
                t_s: 0.0,
                duration_s: 1.0,
                next_step: None,
                steps_total: 1,
            }),
        };
        let texts = |rows: Vec<Vec<String>>| -> Vec<String> {
            rows.into_iter().map(|r| r[4].clone()).collect()
        };
        let safe = StandSnapshot {
            mode: StandMode::Safe,
            sequence: None,
        };
        assert_eq!(
            texts(stand_status_rows(0.0, None, &status(StandMode::Safe, None))),
            ["mode Safe"]
        );
        assert!(stand_status_rows(0.0, Some(&safe), &status(StandMode::Safe, None)).is_empty());
        assert_eq!(
            texts(stand_status_rows(
                0.0,
                Some(&safe),
                &status(StandMode::Sequence, Some("hotfire"))
            )),
            ["mode Safe -> Sequence", "sequence hotfire started"]
        );
        let running = StandSnapshot {
            mode: StandMode::Sequence,
            sequence: Some("hotfire".into()),
        };
        assert_eq!(
            texts(stand_status_rows(
                0.0,
                Some(&running),
                &status(StandMode::Armed, None)
            )),
            ["mode Sequence -> Armed", "sequence hotfire ended"]
        );
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("hotfire-3"), "hotfire-3");
        assert_eq!(sanitize_name("cold flow/2 (v1)"), "cold_flow_2__v1_");
        assert_eq!(sanitize_name("../../etc"), ".._.._etc");
        assert_eq!(sanitize_name(&"x".repeat(100)).len(), MAX_NAME_LEN);
    }

    #[test]
    fn writes_all_four_files() {
        let tmp = std::env::temp_dir().join(format!("gs-bridge-rec-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut rec = Recorder::start(&tmp, Some("unit test")).unwrap();
        assert!(rec.dir().file_name().unwrap().to_str().unwrap().ends_with("-unit_test"));
        rec.record(&LogRecord::Down {
            t: 1.0,
            message: ServerMessage::Flight(sample_flight(1)),
        });
        rec.record(&LogRecord::Up {
            t: 1.1,
            command: SentCommand {
                seq: 1,
                kind: CommandKind::Arm,
            },
        });
        let dir = rec.dir().to_path_buf();
        rec.stop();

        let flight = fs::read_to_string(dir.join("flight.csv")).unwrap();
        assert_eq!(flight.lines().count(), 2);
        assert!(flight.starts_with("t_unix,time_s,seq,"));
        let events = fs::read_to_string(dir.join("events.csv")).unwrap();
        let lines: Vec<&str> = events.lines().collect();
        assert_eq!(lines[0], "t_unix,time_s,kind,severity,text");
        assert!(lines[1].contains(",recording,,start"));
        assert!(lines[2].ends_with(",sent,,1 \"\"Arm\"\"\"") || lines[2].contains("sent"));
        assert!(lines[3].contains(",recording,,stop"));
        let stand = fs::read_to_string(dir.join("stand.csv")).unwrap();
        assert_eq!(stand.lines().count(), 1);
        let session = fs::read_to_string(dir.join("session.jsonl")).unwrap();
        assert_eq!(session.lines().count(), 2);
        fs::remove_dir_all(&tmp).unwrap();
    }
}
