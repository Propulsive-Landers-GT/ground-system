//! JSON shapes of the WebSocket API and the session log. See `docs/DESIGN.md`.

use gs_protocol::{
    CommandAck, CommandKind, Downlink, EventMsg, FlightTelemetry, ParamsMsg, Source,
    StandStatus, StandTelemetry, TrajectoryMsg,
};
use serde::{Deserialize, Serialize};

/// Bridge -> browser. Serializes as `{ "type": "...", "data": ... }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerMessage {
    Flight(FlightTelemetry),
    Trajectory(TrajectoryMsg),
    Stand(StandTelemetry),
    Event(EventMsg),
    Ack(CommandAck),
    Params(ParamsMsg),
    StandStatus(StandStatus),
    Sent(SentCommand),
    Link(LinkStatus),
    Error { message: String },
}

impl ServerMessage {
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    /// Marks telemetry as played back from a log rather than live.
    pub fn mark_as_replay(&mut self) {
        match self {
            Self::Flight(flight) => flight.source = Source::Replay,
            Self::Stand(stand) => stand.source = Source::Replay,
            _ => {}
        }
    }

    pub fn to_json(&self) -> String {
        // Non-finite floats become `null`; nothing in these plain-data types can fail.
        serde_json::to_string(self).expect("ServerMessage serializes to JSON")
    }
}

impl From<Downlink> for ServerMessage {
    fn from(packet: Downlink) -> Self {
        match packet {
            Downlink::Flight(m) => Self::Flight(m),
            Downlink::Trajectory(m) => Self::Trajectory(m),
            Downlink::Stand(m) => Self::Stand(m),
            Downlink::Event(m) => Self::Event(m),
            Downlink::Ack(m) => Self::Ack(m),
            Downlink::Params(m) => Self::Params(m),
            Downlink::StandStatus(m) => Self::StandStatus(m),
        }
    }
}

/// Echo of a command the bridge put on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentCommand {
    pub seq: u32,
    pub kind: CommandKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkStatus {
    pub vehicle_addr: String,
    pub connected: bool,
    pub last_rx_age_s: Option<f64>,
    pub packets_rx: u64,
    pub packets_lost: u64,
    pub rate_hz: f64,
    /// Path of the session log being written, if any.
    pub recording: Option<String>,
    /// The test-stand endpoint; `None` when the bridge was started without `--stand`.
    pub stand: Option<StandLinkStatus>,
}

/// Link health of the test-stand endpoint. Loss and rate are not tracked: only
/// `FlightTelemetry` carries a sequence number, and that comes from the vehicle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandLinkStatus {
    pub addr: String,
    pub connected: bool,
    pub last_rx_age_s: Option<f64>,
}

/// Browser -> bridge: `{ "kind": CommandKind }`. The bridge assigns the sequence number.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientCommand {
    pub kind: CommandKind,
}

/// Browser -> bridge: `{ "control": "...", ... }`. Handled by the bridge itself, never
/// forwarded over UDP.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "control", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlMessage {
    StartRecording {
        #[serde(default)]
        name: Option<String>,
    },
    StopRecording,
}

/// Anything a browser may send. Told apart by the presence of a `control` key.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Command(CommandKind),
    Control(ControlMessage),
}

impl ClientMessage {
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("invalid message: {e}"))?;
        if value.get("control").is_some() {
            serde_json::from_value::<ControlMessage>(value)
                .map(Self::Control)
                .map_err(|e| format!("invalid control message: {e}"))
        } else {
            serde_json::from_value::<ClientCommand>(value)
                .map(|c| Self::Command(c.kind))
                .map_err(|e| format!("invalid command: {e}"))
        }
    }
}

/// One line of a session log: `{ "t": <unix s>, "dir": "down", "type", "data" }` or
/// `{ "t": <unix s>, "dir": "up", "seq", "kind" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "dir", rename_all = "lowercase")]
pub enum LogRecord {
    Down {
        t: f64,
        #[serde(flatten)]
        message: ServerMessage,
    },
    Up {
        t: f64,
        #[serde(flatten)]
        command: SentCommand,
    },
}

impl LogRecord {
    pub fn time(&self) -> f64 {
        match self {
            Self::Down { t, .. } | Self::Up { t, .. } => *t,
        }
    }

    /// What a browser should see when this record is played back.
    pub fn into_server_message(self) -> ServerMessage {
        match self {
            Self::Down { mut message, .. } => {
                message.mark_as_replay();
                message
            }
            Self::Up { command, .. } => ServerMessage::Sent(command),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use gs_protocol::{
        AckResult, ControlMode, FlightPhase, SensorSnapshot, TruthState,
    };
    use serde_json::{json, Value};

    pub(crate) fn sample_flight(seq: u32) -> FlightTelemetry {
        FlightTelemetry {
            seq,
            time_s: 12.5,
            source: Source::Sim,
            phase: FlightPhase::Hover,
            phase_time_s: 3.0,
            control_mode: ControlMode::Auto,
            terminated: false,
            position: [0.5, -0.25, 50.0],
            velocity: [0.0; 3],
            attitude: [0.0, 0.0, 0.0, 1.0],
            angular_velocity: [0.0; 3],
            mass: 74.0,
            gimbal_theta: 0.0,
            gimbal_phi: 0.0,
            thrust: 730.0,
            rcs: -1,
            tilt_deg: 1.5,
            trajectory_deviation_m: None,
            position_age_s: 0.0,
            link_age_s: Some(0.25),
            sensors: SensorSnapshot {
                imu_ok: true,
                gps_ok: true,
                uwb_ok: false,
                accel: [0.0, 0.0, 9.75],
                gyro: [0.0; 3],
                chamber_pressure: Some(15.0),
                tank_pressure: None,
            },
            truth: Some(TruthState {
                position: [0.0, 0.0, 50.0],
                velocity: [0.0; 3],
                attitude: [0.0, 0.0, 0.0, 1.0],
                angular_velocity: [0.0; 3],
            }),
        }
    }

    fn to_value(message: &ServerMessage) -> Value {
        serde_json::from_str(&message.to_json()).unwrap()
    }

    #[test]
    fn flight_json_shape() {
        let v = to_value(&Downlink::Flight(sample_flight(7)).into());
        assert_eq!(v["type"], "flight");
        let data = &v["data"];
        assert_eq!(data["seq"], 7);
        assert_eq!(data["source"], "Sim");
        assert_eq!(data["phase"], "Hover");
        assert_eq!(data["control_mode"], "Auto");
        assert_eq!(data["position"], json!([0.5, -0.25, 50.0]));
        assert_eq!(data["rcs"], -1);
        assert_eq!(data["trajectory_deviation_m"], Value::Null);
        assert_eq!(data["link_age_s"], 0.25);
        assert_eq!(data["sensors"]["tank_pressure"], Value::Null);
        assert_eq!(data["truth"]["position"], json!([0.0, 0.0, 50.0]));
    }

    #[test]
    fn ack_json_shape() {
        let rejected = ServerMessage::Ack(CommandAck {
            seq: 3,
            time_s: 1.0,
            result: AckResult::Rejected("not armed".into()),
        });
        assert_eq!(
            to_value(&rejected),
            json!({"type": "ack", "data": {"seq": 3, "time_s": 1.0, "result": {"Rejected": "not armed"}}})
        );

        let accepted = ServerMessage::Ack(CommandAck {
            seq: 4,
            time_s: 1.0,
            result: AckResult::Accepted,
        });
        assert_eq!(to_value(&accepted)["data"]["result"], "Accepted");
    }

    #[test]
    fn sent_link_and_error_json_shape() {
        let sent = ServerMessage::Sent(SentCommand {
            seq: 9,
            kind: CommandKind::Arm,
        });
        assert_eq!(
            to_value(&sent),
            json!({"type": "sent", "data": {"seq": 9, "kind": "Arm"}})
        );

        assert_eq!(
            to_value(&ServerMessage::error("bad")),
            json!({"type": "error", "data": {"message": "bad"}})
        );
    }

    #[test]
    fn link_json_shape_without_stand() {
        let link = ServerMessage::Link(LinkStatus {
            vehicle_addr: "127.0.0.1:8888".into(),
            connected: false,
            last_rx_age_s: None,
            packets_rx: 0,
            packets_lost: 0,
            rate_hz: 0.0,
            recording: None,
            stand: None,
        });
        assert_eq!(
            to_value(&link),
            json!({"type": "link", "data": {
                "vehicle_addr": "127.0.0.1:8888", "connected": false, "last_rx_age_s": null,
                "packets_rx": 0, "packets_lost": 0, "rate_hz": 0.0, "recording": null,
                "stand": null
            }})
        );
    }

    #[test]
    fn link_json_shape_with_stand() {
        let link = ServerMessage::Link(LinkStatus {
            vehicle_addr: "127.0.0.1:8888".into(),
            connected: true,
            last_rx_age_s: Some(0.02),
            packets_rx: 10,
            packets_lost: 1,
            rate_hz: 50.0,
            recording: Some("logs/session.jsonl".into()),
            stand: Some(StandLinkStatus {
                addr: "127.0.0.1:8889".into(),
                connected: true,
                last_rx_age_s: Some(0.05),
            }),
        });
        assert_eq!(
            to_value(&link),
            json!({"type": "link", "data": {
                "vehicle_addr": "127.0.0.1:8888", "connected": true, "last_rx_age_s": 0.02,
                "packets_rx": 10, "packets_lost": 1, "rate_hz": 50.0,
                "recording": "logs/session.jsonl",
                "stand": { "addr": "127.0.0.1:8889", "connected": true, "last_rx_age_s": 0.05 }
            }})
        );
    }

    #[test]
    fn stand_status_json_shape() {
        let status = ServerMessage::StandStatus(StandStatus {
            time_s: 3.0,
            mode: gs_protocol::StandMode::Sequence,
            actuation_link_ok: true,
            loadcell_link_ok: false,
            sequences: vec!["hotfire".into()],
            sequence: Some(gs_protocol::SequenceProgress {
                name: "hotfire".into(),
                t_s: 1.5,
                duration_s: 20.0,
                next_step: Some((2, "OMV open".into())),
                steps_total: 9,
            }),
        });
        assert_eq!(
            to_value(&status),
            json!({"type": "stand_status", "data": {
                "time_s": 3.0, "mode": "Sequence",
                "actuation_link_ok": true, "loadcell_link_ok": false,
                "sequences": ["hotfire"],
                "sequence": {
                    "name": "hotfire", "t_s": 1.5, "duration_s": 20.0,
                    "next_step": [2, "OMV open"], "steps_total": 9
                }
            }})
        );
    }

    #[test]
    fn replay_rewrites_stand_source_too() {
        let down = LogRecord::Down {
            t: 1.0,
            message: ServerMessage::Stand(StandTelemetry {
                time_s: 1.0,
                source: Source::Stand,
                channels: vec![(gs_protocol::StandChannel::Thrust, 5.0)],
                valves: Vec::new(),
                outputs_on: vec![gs_protocol::StandOutput::DaqSync],
                mtv_percent: Some(40.0),
            }),
        };
        let line = serde_json::to_string(&down).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "stand");
        assert_eq!(v["data"]["source"], "Stand");
        assert_eq!(v["data"]["outputs_on"], json!(["DaqSync"]));

        let parsed: LogRecord = serde_json::from_str(&line).unwrap();
        match parsed.into_server_message() {
            ServerMessage::Stand(s) => assert_eq!(s.source, Source::Replay),
            other => panic!("unexpected {other:?}"),
        }

        // StandStatus has no source field; it must survive the log unchanged.
        let status = ServerMessage::StandStatus(StandStatus {
            time_s: 2.0,
            mode: gs_protocol::StandMode::Safe,
            actuation_link_ok: true,
            loadcell_link_ok: true,
            sequences: Vec::new(),
            sequence: None,
        });
        let line = serde_json::to_string(&LogRecord::Down {
            t: 2.0,
            message: status.clone(),
        })
        .unwrap();
        let parsed: LogRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed.into_server_message(), status);
    }

    #[test]
    fn parses_browser_commands() {
        let parse = |s: &str| serde_json::from_str::<ClientCommand>(s).map(|c| c.kind);

        assert_eq!(parse(r#"{"kind":"Arm"}"#).unwrap(), CommandKind::Arm);
        assert_eq!(
            parse(r#"{"kind":{"SetPhase":"Descent"}}"#).unwrap(),
            CommandKind::SetPhase(FlightPhase::Descent)
        );
        assert_eq!(
            parse(r#"{"kind":{"Jog":{"gimbal_theta":0.05,"gimbal_phi":0,"thrust":0,"rcs":0}}}"#)
                .unwrap(),
            CommandKind::Jog(gs_protocol::JogSetpoint {
                gimbal_theta: 0.05,
                gimbal_phi: 0.0,
                thrust: 0.0,
                rcs: 0,
            })
        );
        assert_eq!(
            parse(r#"{"kind":{"SetValve":{"id":"Omv","open":true}}}"#).unwrap(),
            CommandKind::SetValve {
                id: gs_protocol::ValveId::Omv,
                open: true
            }
        );
        assert_eq!(
            parse(r#"{"kind":{"SetMpcWeights":null}}"#).unwrap(),
            CommandKind::SetMpcWeights(None)
        );

        assert!(parse(r#"{"kind":"Explode"}"#).is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn parses_control_messages_apart_from_commands() {
        assert_eq!(
            ClientMessage::parse(r#"{"control":"start_recording","name":"hotfire-3"}"#).unwrap(),
            ClientMessage::Control(ControlMessage::StartRecording {
                name: Some("hotfire-3".into())
            })
        );
        assert_eq!(
            ClientMessage::parse(r#"{"control":"start_recording"}"#).unwrap(),
            ClientMessage::Control(ControlMessage::StartRecording { name: None })
        );
        assert_eq!(
            ClientMessage::parse(r#"{"control":"stop_recording"}"#).unwrap(),
            ClientMessage::Control(ControlMessage::StopRecording)
        );
        assert_eq!(
            ClientMessage::parse(r#"{"kind":{"Stand":"Arm"}}"#).unwrap(),
            ClientMessage::Command(CommandKind::Stand(gs_protocol::StandCommand::Arm))
        );

        let err = ClientMessage::parse(r#"{"control":"self_destruct"}"#).unwrap_err();
        assert!(err.starts_with("invalid control message"), "{err}");
        let err = ClientMessage::parse(r#"{"control":"start_recording","name":7}"#).unwrap_err();
        assert!(err.starts_with("invalid control message"), "{err}");
        let err = ClientMessage::parse(r#"{"nope":1}"#).unwrap_err();
        assert!(err.starts_with("invalid command"), "{err}");
    }

    #[test]
    fn log_records_round_trip_and_replay_rewrites_source() {
        let down = LogRecord::Down {
            t: 1_700_000_000.5,
            message: ServerMessage::Flight(sample_flight(1)),
        };
        let line = serde_json::to_string(&down).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["dir"], "down");
        assert_eq!(v["type"], "flight");
        assert_eq!(v["data"]["seq"], 1);

        let parsed: LogRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed, down);
        match parsed.into_server_message() {
            ServerMessage::Flight(f) => assert_eq!(f.source, Source::Replay),
            other => panic!("unexpected {other:?}"),
        }

        let up = LogRecord::Up {
            t: 2.0,
            command: SentCommand {
                seq: 5,
                kind: CommandKind::SetPhase(FlightPhase::Hover),
            },
        };
        let line = serde_json::to_string(&up).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"dir": "up", "t": 2.0, "seq": 5, "kind": {"SetPhase": "Hover"}})
        );
        assert_eq!(serde_json::from_str::<LogRecord>(&line).unwrap(), up);
    }
}
