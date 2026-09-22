//! GTPL ground-station wire protocol.
//!
//! One crate shared by everything that talks on the link: the flight software
//! (`monoprop-flight-software/Lander`), the simulator (`simulations/rust_rocket_sim`),
//! and the ground bridge (`gs-bridge`). The vehicle sends [`Downlink`] packets and
//! receives [`Uplink`] packets, one packet per UDP datagram.
//!
//! Conventions (match the flight software):
//! - SI units: m, m/s, rad, rad/s, N, kg, s. Pressures in bar, temperatures in °C.
//! - World frame is local Z-up with the landing pad at the origin.
//! - Body frame: +Z is the nose / thrust axis.
//! - Quaternions are `[x, y, z, w]` (vector first), body-to-world.
//!
//! Packet layout: `MAGIC (2) | PROTOCOL_VERSION (1) | postcard payload`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[cfg(feature = "std")]
mod link;
#[cfg(feature = "std")]
pub use link::VehicleLink;

pub const MAGIC: [u8; 2] = *b"GT";
pub const PROTOCOL_VERSION: u8 = 1;
const HEADER_LEN: usize = 3;

/// Port the vehicle (or sim) binds for uplink commands.
pub const DEFAULT_VEHICLE_PORT: u16 = 8888;
/// Port the ground bridge binds for downlink telemetry.
pub const DEFAULT_GROUND_PORT: u16 = 9999;

/// Keep every datagram under one Ethernet MTU so it is never IP-fragmented.
pub const MAX_PACKET_LEN: usize = 1400;
/// Trajectories are downsampled to this many nodes before transmission.
pub const MAX_TRAJECTORY_NODES: usize = 64;

/// A jog setpoint is dropped if it is not refreshed within this window (deadman).
pub const JOG_TIMEOUT_S: f64 = 0.5;
/// The ground sends [`CommandKind::Heartbeat`] at this rate; the vehicle reports link age.
pub const HEARTBEAT_HZ: f64 = 2.0;

// ---------------------------------------------------------------------------
// Downlink: vehicle -> ground
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Downlink {
    /// Vehicle state, actuation and health. Nominally 50 Hz.
    Flight(FlightTelemetry),
    /// The guidance reference trajectory. Sent whenever it is regenerated (~1 Hz).
    Trajectory(TrajectoryMsg),
    /// Propulsion / test-stand sensors and valve states. Nominally 20 Hz.
    Stand(StandTelemetry),
    /// Diagnostics: phase transitions, warnings, termination reasons.
    Event(EventMsg),
    /// Reply to an [`Uplink`], matched by `seq`.
    Ack(CommandAck),
    /// Current tunable parameters. Sent on request and after any change.
    Params(ParamsMsg),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Vehicle,
    Sim,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlightPhase {
    Standby,
    Armed,
    Ascent,
    Hover,
    Descent,
    Landed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlMode {
    /// Actuators follow the autopilot.
    Auto,
    /// Actuators follow ground jog setpoints. Standby only.
    Jog,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightTelemetry {
    pub seq: u32,
    /// Vehicle mission clock.
    pub time_s: f64,
    pub source: Source,

    pub phase: FlightPhase,
    /// Time since the current phase was entered.
    pub phase_time_s: f32,
    pub control_mode: ControlMode,
    pub terminated: bool,

    // Navigation estimate
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub attitude: [f32; 4],
    pub angular_velocity: [f32; 3],
    pub mass: f32,

    // Actuation (what is being commanded to the hardware right now)
    pub gimbal_theta: f32,
    pub gimbal_phi: f32,
    pub thrust: f32,
    /// +1 = CW roll thruster, -1 = CCW, 0 = both closed.
    pub rcs: i8,

    // Safety margins, mirrored from the flight-termination checks
    pub tilt_deg: f32,
    pub trajectory_deviation_m: Option<f32>,
    /// Seconds since the last GPS/UWB position fix.
    pub position_age_s: f32,
    /// Seconds since the last valid uplink packet; `None` until the first one arrives.
    pub link_age_s: Option<f32>,

    pub sensors: SensorSnapshot,
    /// Ground truth, only populated by the simulator.
    pub truth: Option<TruthState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub imu_ok: bool,
    pub gps_ok: bool,
    pub uwb_ok: bool,
    pub accel: [f32; 3],
    pub gyro: [f32; 3],
    pub chamber_pressure: Option<f32>,
    pub tank_pressure: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TruthState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub attitude: [f32; 4],
    pub angular_velocity: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryMsg {
    /// Mission time at which guidance produced this trajectory.
    pub generated_at_s: f64,
    pub time_of_flight_s: f32,
    /// Uniformly spaced in time over `time_of_flight_s`, at most [`MAX_TRAJECTORY_NODES`].
    pub positions: Vec<[f32; 3]>,
    pub target: [f32; 3],
}

impl TrajectoryMsg {
    /// Build a message from full-rate guidance output, downsampling uniformly so the
    /// first and last nodes are always kept.
    pub fn from_positions(
        generated_at_s: f64,
        time_of_flight_s: f64,
        positions: &[[f64; 3]],
        target: [f64; 3],
    ) -> Self {
        let n = positions.len();
        let keep = n.min(MAX_TRAJECTORY_NODES);
        let mut out = Vec::with_capacity(keep);
        for i in 0..keep {
            let idx = if keep > 1 { i * (n - 1) / (keep - 1) } else { 0 };
            let p = positions[idx];
            out.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        }
        Self {
            generated_at_s,
            time_of_flight_s: time_of_flight_s as f32,
            positions: out,
            target: [target[0] as f32, target[1] as f32, target[2] as f32],
        }
    }
}

/// Analog channels on the propulsion P&ID (`Propulsion/VISIO P&ID Versions`), using
/// the tag names from the test-stand DAQ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StandChannel {
    /// O-PT: oxidizer run tank pressure (bar)
    Opt,
    /// I-PT: injector pressure (bar)
    Ipt,
    /// E-PT: engine / chamber pressure (bar)
    Ept,
    /// M1-PT, M2-PT: main line pressures (bar)
    M1,
    M2,
    /// PU-PT: purge / pressurant pressure (bar)
    Pupt,
    /// LF-PT: low-flow line pressure (bar)
    Lfpt,
    /// Thermocouples (°C)
    T1,
    T2,
    /// Load cell (N)
    Thrust,
}

/// Commandable valves on the P&ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValveId {
    Omv,
    Mtv,
    IgV,
    OFill,
    OIso,
    OVnt,
    PuMv,
    PuFill,
    PuIso,
    PuVnt,
    PuMvnt,
    LfVnt,
    TVnt,
    Rcs1,
    Rcs2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValveState {
    Closed,
    Open,
    /// Commanded but no feedback, or in transit.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValveStatus {
    pub id: ValveId,
    pub state: ValveState,
    /// Throttling valves (MTV) report an angle; 0 = closed.
    pub position_deg: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandTelemetry {
    pub time_s: f64,
    pub source: Source,
    pub channels: Vec<(StandChannel, f32)>,
    pub valves: Vec<ValveStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMsg {
    pub time_s: f64,
    pub severity: Severity,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandAck {
    pub seq: u32,
    pub time_s: f64,
    pub result: AckResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AckResult {
    Accepted,
    /// The vehicle refused the command; the string says why (e.g. wrong phase).
    Rejected(String),
}

/// Diagonal MPC weights. State order: `[x y z | qx qy qz qw | vx vy vz | wx wy wz]`,
/// input order: `[gimbal_theta gimbal_phi thrust]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MpcWeights {
    pub q: [f32; 13],
    pub r: [f32; 3],
    pub qn: [f32; 13],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightParams {
    pub hover_altitude_m: f32,
    pub hover_duration_s: f32,
    pub max_tilt_deg: f32,
    pub max_trajectory_deviation_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamsMsg {
    pub flight: FlightParams,
    /// `None` while the MPC is using its built-in per-phase weights.
    pub manual_mpc_weights: Option<MpcWeights>,
}

// ---------------------------------------------------------------------------
// Uplink: ground -> vehicle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Uplink {
    /// Monotonic per ground session; echoed in the [`CommandAck`].
    pub seq: u32,
    pub kind: CommandKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommandKind {
    /// Keeps `link_age_s` fresh and tells the vehicle where to send telemetry. Not acked.
    Heartbeat,

    Arm,
    Disarm,
    Launch,
    /// Terminate the flight: controls are zeroed and the control loop stops.
    /// Accepted in every phase.
    Abort,

    /// Operator phase override. Only `Hover` and `Descent` are accepted, and only in flight.
    SetPhase(FlightPhase),

    SetFlightParams(FlightParams),
    /// `Some` switches the MPC to these weights; `None` restores the built-in ones.
    SetMpcWeights(Option<MpcWeights>),
    RequestParams,

    /// Switching to `Jog` is only accepted in Standby. Any phase change forces `Auto`.
    SetControlMode(ControlMode),
    /// Jog setpoint; must be refreshed within [`JOG_TIMEOUT_S`] or actuators return to zero.
    /// Not acked.
    Jog(JogSetpoint),

    /// Test-stand / ground checkout valve command. Only accepted in Standby.
    SetValve { id: ValveId, open: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JogSetpoint {
    pub gimbal_theta: f32,
    pub gimbal_phi: f32,
    pub thrust: f32,
    pub rcs: i8,
}

impl CommandKind {
    /// High-rate commands are fire-and-forget; everything else gets a [`CommandAck`].
    pub fn wants_ack(&self) -> bool {
        !matches!(self, CommandKind::Heartbeat | CommandKind::Jog(_))
    }
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    TooShort,
    BadMagic,
    /// The peer speaks a different protocol version (carried here).
    VersionMismatch(u8),
    Malformed,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::TooShort => write!(f, "packet shorter than header"),
            DecodeError::BadMagic => write!(f, "bad magic bytes"),
            DecodeError::VersionMismatch(v) => {
                write!(f, "protocol version {} (expected {})", v, PROTOCOL_VERSION)
            }
            DecodeError::Malformed => write!(f, "malformed payload"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DecodeError {}

fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(&MAGIC);
    buf.push(PROTOCOL_VERSION);
    // Serializing these plain-data types into a growable buffer cannot fail.
    postcard::to_extend(msg, buf).expect("postcard encode")
}

fn decode<'a, T: Deserialize<'a>>(packet: &'a [u8]) -> Result<T, DecodeError> {
    if packet.len() < HEADER_LEN {
        return Err(DecodeError::TooShort);
    }
    if packet[..2] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    if packet[2] != PROTOCOL_VERSION {
        return Err(DecodeError::VersionMismatch(packet[2]));
    }
    postcard::from_bytes(&packet[HEADER_LEN..]).map_err(|_| DecodeError::Malformed)
}

impl Downlink {
    pub fn encode(&self) -> Vec<u8> {
        encode(self)
    }
    pub fn decode(packet: &[u8]) -> Result<Self, DecodeError> {
        decode(packet)
    }
}

impl Uplink {
    pub fn encode(&self) -> Vec<u8> {
        encode(self)
    }
    pub fn decode(packet: &[u8]) -> Result<Self, DecodeError> {
        decode(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_flight() -> FlightTelemetry {
        FlightTelemetry {
            seq: 7,
            time_s: 12.5,
            source: Source::Sim,
            phase: FlightPhase::Hover,
            phase_time_s: 3.0,
            control_mode: ControlMode::Auto,
            terminated: false,
            position: [0.1, -0.2, 50.0],
            velocity: [0.0; 3],
            attitude: [0.0, 0.0, 0.0, 1.0],
            angular_velocity: [0.0; 3],
            mass: 74.0,
            gimbal_theta: 0.01,
            gimbal_phi: -0.02,
            thrust: 730.0,
            rcs: -1,
            tilt_deg: 1.5,
            trajectory_deviation_m: Some(0.4),
            position_age_s: 0.01,
            link_age_s: Some(0.2),
            sensors: SensorSnapshot {
                imu_ok: true,
                gps_ok: true,
                uwb_ok: false,
                accel: [0.0, 0.0, 9.81],
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

    #[test]
    fn downlink_round_trip() {
        let msg = Downlink::Flight(sample_flight());
        let bytes = msg.encode();
        assert!(bytes.len() < MAX_PACKET_LEN);
        assert_eq!(Downlink::decode(&bytes).unwrap(), msg);
    }

    #[test]
    fn uplink_round_trip() {
        let msg = Uplink {
            seq: 42,
            kind: CommandKind::SetMpcWeights(Some(MpcWeights {
                q: [1.0; 13],
                r: [2.0; 3],
                qn: [3.0; 13],
            })),
        };
        assert_eq!(Uplink::decode(&msg.encode()).unwrap(), msg);
    }

    #[test]
    fn full_trajectory_fits_in_one_packet() {
        let positions: Vec<[f64; 3]> = (0..500).map(|i| [i as f64, 0.0, 50.0]).collect();
        let traj = TrajectoryMsg::from_positions(1.0, 20.0, &positions, [0.0; 3]);
        assert_eq!(traj.positions.len(), MAX_TRAJECTORY_NODES);
        assert_eq!(traj.positions[0], [0.0, 0.0, 50.0]);
        assert_eq!(traj.positions[MAX_TRAJECTORY_NODES - 1], [499.0, 0.0, 50.0]);
        assert!(Downlink::Trajectory(traj).encode().len() < MAX_PACKET_LEN);
    }

    #[test]
    fn rejects_foreign_packets() {
        assert_eq!(Downlink::decode(b"G"), Err(DecodeError::TooShort));
        assert_eq!(Downlink::decode(b"XX\x01abc"), Err(DecodeError::BadMagic));
        assert_eq!(
            Downlink::decode(&[b'G', b'T', 99, 0]),
            Err(DecodeError::VersionMismatch(99))
        );
    }
}
