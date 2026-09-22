// Hand-written mirror of crates/gs-protocol/src/lib.rs as it appears on the WebSocket.
//
// JSON is serde's default encoding: unit enum variants are strings ("Hover"),
// data-carrying variants are single-key objects ({"Rejected":"why"}), Option is
// value-or-null, tuples are arrays. SI units; pressures bar, temperatures °C.
// World frame is Z-up with the pad at the origin; body +Z is the nose;
// quaternions are [x, y, z, w], body-to-world.

export type Vec3 = [number, number, number];
export type Quat = [number, number, number, number];

export type Source = "Vehicle" | "Sim" | "Replay";

export const FLIGHT_PHASES = ["Standby", "Armed", "Ascent", "Hover", "Descent", "Landed"] as const;
export type FlightPhase = (typeof FLIGHT_PHASES)[number];

export type ControlMode = "Auto" | "Jog";

export interface SensorSnapshot {
  imu_ok: boolean;
  gps_ok: boolean;
  uwb_ok: boolean;
  accel: Vec3;
  gyro: Vec3;
  chamber_pressure: number | null;
  tank_pressure: number | null;
}

export interface TruthState {
  position: Vec3;
  velocity: Vec3;
  attitude: Quat;
  angular_velocity: Vec3;
}

export interface FlightTelemetry {
  seq: number;
  time_s: number;
  source: Source;
  phase: FlightPhase;
  phase_time_s: number;
  control_mode: ControlMode;
  terminated: boolean;
  position: Vec3;
  velocity: Vec3;
  attitude: Quat;
  angular_velocity: Vec3;
  mass: number;
  gimbal_theta: number;
  gimbal_phi: number;
  thrust: number;
  /** +1 = CW roll thruster, -1 = CCW, 0 = both closed. */
  rcs: number;
  tilt_deg: number;
  trajectory_deviation_m: number | null;
  position_age_s: number;
  link_age_s: number | null;
  sensors: SensorSnapshot;
  truth: TruthState | null;
}

export interface TrajectoryMsg {
  generated_at_s: number;
  time_of_flight_s: number;
  /** Uniformly spaced in time over time_of_flight_s, at most 64 nodes. */
  positions: Vec3[];
  target: Vec3;
}

export const STAND_CHANNELS = ["Opt", "Ipt", "Ept", "M1", "M2", "Pupt", "Lfpt", "T1", "T2", "Thrust"] as const;
export type StandChannel = (typeof STAND_CHANNELS)[number];

export const VALVE_IDS = [
  "Omv", "Mtv", "IgV", "OFill", "OIso", "OVnt", "PuMv", "PuFill",
  "PuIso", "PuVnt", "PuMvnt", "LfVnt", "TVnt", "Rcs1", "Rcs2",
] as const;
export type ValveId = (typeof VALVE_IDS)[number];

export type ValveState = "Closed" | "Open" | "Unknown";

export interface ValveStatus {
  id: ValveId;
  state: ValveState;
  position_deg: number | null;
}

export interface StandTelemetry {
  time_s: number;
  source: Source;
  channels: [StandChannel, number][];
  valves: ValveStatus[];
}

export type Severity = "Info" | "Warning" | "Critical";

export interface EventMsg {
  time_s: number;
  severity: Severity;
  text: string;
}

export type AckResult = "Accepted" | { Rejected: string };

export interface CommandAck {
  seq: number;
  time_s: number;
  result: AckResult;
}

/** Diagonal MPC weights. State order [x y z | qx qy qz qw | vx vy vz | wx wy wz], input order [gimbal_theta gimbal_phi thrust]. */
export interface MpcWeights {
  q: number[]; // 13
  r: number[]; // 3
  qn: number[]; // 13
}

export interface FlightParams {
  hover_altitude_m: number;
  hover_duration_s: number;
  max_tilt_deg: number;
  max_trajectory_deviation_m: number;
}

export interface ParamsMsg {
  flight: FlightParams;
  manual_mpc_weights: MpcWeights | null;
}

export interface JogSetpoint {
  gimbal_theta: number;
  gimbal_phi: number;
  thrust: number;
  rcs: number;
}

export type CommandKind =
  | "Heartbeat"
  | "Arm"
  | "Disarm"
  | "Launch"
  | "Abort"
  | "RequestParams"
  | { SetPhase: FlightPhase }
  | { SetFlightParams: FlightParams }
  | { SetMpcWeights: MpcWeights | null }
  | { SetControlMode: ControlMode }
  | { Jog: JogSetpoint }
  | { SetValve: { id: ValveId; open: boolean } };

export interface LinkStatus {
  vehicle_addr: string;
  connected: boolean;
  last_rx_age_s: number | null;
  packets_rx: number;
  packets_lost: number;
  rate_hz: number;
  recording: string | null;
}

export interface SentMsg {
  seq: number;
  kind: CommandKind;
}

export type ServerMsg =
  | { type: "flight"; data: FlightTelemetry }
  | { type: "trajectory"; data: TrajectoryMsg }
  | { type: "stand"; data: StandTelemetry }
  | { type: "event"; data: EventMsg }
  | { type: "ack"; data: CommandAck }
  | { type: "params"; data: ParamsMsg }
  | { type: "sent"; data: SentMsg }
  | { type: "link"; data: LinkStatus };

export interface ClientMsg {
  kind: CommandKind;
}

// Limits the vehicle enforces; mirrored here only for display and input clamping.
export const GIMBAL_LIMIT_RAD = (15 * Math.PI) / 180;
export const THRUST_MAX_N = 1200;
export const THRUST_MIN_THROTTLE_N = 300;
export const JOG_TIMEOUT_S = 0.5;
export const MPC_STATE_NAMES = ["x", "y", "z", "qx", "qy", "qz", "qw", "vx", "vy", "vz", "wx", "wy", "wz"] as const;
export const MPC_INPUT_NAMES = ["gimbal θ", "gimbal φ", "thrust"] as const;

export const VALVE_LABEL: Record<ValveId, string> = {
  Omv: "OMV", Mtv: "MTV", IgV: "IG-V", OFill: "O-FILL", OIso: "O-ISO", OVnt: "O-VNT",
  PuMv: "PU-MV", PuFill: "PU-FILL", PuIso: "PU-ISO", PuVnt: "PU-VNT", PuMvnt: "PU-MVNT",
  LfVnt: "LF-VNT", TVnt: "T-VNT", Rcs1: "RCS1", Rcs2: "RCS2",
};

export const VALVE_ROLE: Record<ValveId, string> = {
  Omv: "Oxidizer main valve", Mtv: "Main throttle valve", IgV: "Igniter valve",
  OFill: "Run tank fill", OIso: "N2O supply isolation", OVnt: "N2O supply vent",
  PuMv: "Purge main valve", PuFill: "Pressurant fill", PuIso: "GN2 isolation",
  PuVnt: "GN2 line vent", PuMvnt: "Purge manifold vent", LfVnt: "Igniter line vent",
  TVnt: "Run tank vent", Rcs1: "Roll thruster CW", Rcs2: "Roll thruster CCW",
};

export const CHANNEL_LABEL: Record<StandChannel, string> = {
  Opt: "O-PT", Ipt: "I-PT", Ept: "E-PT", M1: "M1-PT", M2: "M2-PT", Pupt: "PU-PT", Lfpt: "LF-PT",
  T1: "T1", T2: "T2", Thrust: "Thrust",
};

export const CHANNEL_UNIT: Record<StandChannel, string> = {
  Opt: "bar", Ipt: "bar", Ept: "bar", M1: "bar", M2: "bar", Pupt: "bar", Lfpt: "bar",
  T1: "°C", T2: "°C", Thrust: "N",
};

export function describeCommand(kind: CommandKind): string {
  if (typeof kind === "string") return kind === "RequestParams" ? "Request params" : kind;
  if ("SetPhase" in kind) return kind.SetPhase === "Hover" ? "Hold hover" : kind.SetPhase === "Descent" ? "Land now" : `Set phase ${kind.SetPhase}`;
  if ("SetFlightParams" in kind) return "Set flight params";
  if ("SetMpcWeights" in kind) return kind.SetMpcWeights ? "Set MPC weights" : "Restore built-in weights";
  if ("SetControlMode" in kind) return `Control mode ${kind.SetControlMode}`;
  if ("SetValve" in kind) return `${VALVE_LABEL[kind.SetValve.id] ?? kind.SetValve.id} ${kind.SetValve.open ? "open" : "close"}`;
  if ("Jog" in kind) return "Jog";
  return "Command";
}

export function isJog(kind: CommandKind): boolean {
  return typeof kind === "object" && "Jog" in kind;
}
