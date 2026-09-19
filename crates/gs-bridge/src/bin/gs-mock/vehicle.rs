//! The mock flight computer: phase state machine, command interlocks (the table in
//! `docs/DESIGN.md`) and telemetry construction.

use gs_protocol::{
    AckResult, CommandKind, ControlMode, Downlink, EventMsg, FlightParams, FlightPhase,
    FlightTelemetry, JogSetpoint, MpcWeights, ParamsMsg, SensorSnapshot, Severity, Source,
    StandTelemetry, TruthState, JOG_TIMEOUT_S,
};

use crate::physics::{
    add, norm, quaternion_from_rotation_vector, sub, Actuation, Body, Noise, Plan, Vec3, G,
    MAX_GIMBAL_RAD, MAX_THRUST_N,
};
use crate::stand::Stand;

/// Offset from the pad (the world origin) so the descent has some lateral travel.
const LAUNCH_SITE: Vec3 = [4.0, -3.0, 0.0];
const PAD: Vec3 = [0.0; 3];
const TRAJECTORY_PERIOD_S: f64 = 1.0;
const AUTO_RESET_DELAY_S: f64 = 5.0;
const GPS_PERIOD_S: f64 = 0.1;

pub struct Vehicle {
    time_s: f64,
    phase: FlightPhase,
    phase_entered_s: f64,
    control_mode: ControlMode,
    terminated: bool,
    params: ParamsMsg,

    body: Body,
    actuation: Actuation,
    /// The reference being flown; `Some` exactly in Ascent, Hover and Descent.
    plan: Option<Plan>,
    last_trajectory_s: f64,
    /// Latest jog setpoint and when it arrived.
    jog: Option<(JogSetpoint, f64)>,
    stand: Stand,

    auto_reset: bool,
    /// When the flight ended (landed, or terminated and on the ground).
    finished_at_s: Option<f64>,

    flight_seq: u32,
    noise: Noise,
    /// Events, params and trajectories waiting to be sent.
    outbox: Vec<Downlink>,
}

impl Vehicle {
    pub fn new(auto_reset: bool) -> Self {
        Self {
            time_s: 0.0,
            phase: FlightPhase::Standby,
            phase_entered_s: 0.0,
            control_mode: ControlMode::Auto,
            terminated: false,
            params: ParamsMsg {
                flight: FlightParams {
                    hover_altitude_m: 30.0,
                    hover_duration_s: 10.0,
                    max_tilt_deg: 15.0,
                    max_trajectory_deviation_m: 5.0,
                },
                manual_mpc_weights: None,
            },
            body: Body::at_rest(LAUNCH_SITE),
            actuation: Actuation::default(),
            plan: None,
            last_trajectory_s: 0.0,
            jog: None,
            stand: Stand::new(),
            auto_reset,
            finished_at_s: None,
            flight_seq: 0,
            noise: Noise::new(),
            outbox: Vec::new(),
        }
    }

    pub fn time_s(&self) -> f64 {
        self.time_s
    }

    pub fn drain_outbox(&mut self) -> impl Iterator<Item = Downlink> + '_ {
        self.outbox.drain(..)
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------

    pub fn handle_command(&mut self, kind: &CommandKind) -> AckResult {
        match self.try_command(kind) {
            Ok(()) => AckResult::Accepted,
            Err(reason) => AckResult::Rejected(reason),
        }
    }

    fn try_command(&mut self, kind: &CommandKind) -> Result<(), String> {
        use FlightPhase::*;

        match kind {
            CommandKind::Heartbeat => {}
            CommandKind::Abort => self.terminate("Operator abort"),
            // Parameters can always be changed or read, even after termination.
            CommandKind::SetFlightParams(flight) => {
                check_flight_params(flight)?;
                self.params.flight = flight.clone();
                self.params_changed("Flight parameters updated");
            }
            CommandKind::SetMpcWeights(weights) => {
                if let Some(weights) = weights {
                    check_mpc_weights(weights)?;
                }
                self.params.manual_mpc_weights = weights.clone();
                self.params_changed(match weights {
                    Some(_) => "Manual MPC weights applied",
                    None => "Built-in MPC weights restored",
                });
            }
            CommandKind::RequestParams => {
                self.outbox.push(Downlink::Params(self.params.clone()));
            }
            _ if self.terminated => return Err("flight terminated".into()),

            CommandKind::Arm => {
                self.require_phase(&[Standby], "Arm")?;
                if self.control_mode != ControlMode::Auto {
                    return Err("control mode is Jog; switch to Auto before arming".into());
                }
                self.enter_phase(Armed);
            }
            CommandKind::Disarm => {
                self.require_phase(&[Armed], "Disarm")?;
                self.enter_phase(Standby);
            }
            CommandKind::Launch => {
                self.require_phase(&[Armed], "Launch")?;
                self.enter_phase(Ascent);
            }
            CommandKind::SetPhase(target) => {
                if !matches!(target, Hover | Descent) {
                    return Err(format!("SetPhase only accepts Hover or Descent, not {target:?}"));
                }
                self.require_phase(&[Ascent, Hover, Descent], "SetPhase")?;
                self.event(Severity::Warning, format!("Operator phase override to {target:?}"));
                self.enter_phase(*target);
            }
            CommandKind::SetControlMode(mode) => {
                if *mode == ControlMode::Jog {
                    self.require_phase(&[Standby], "Jog mode")?;
                }
                if *mode != self.control_mode {
                    self.control_mode = *mode;
                    self.jog = None;
                    self.event(Severity::Info, format!("Control mode: {mode:?}"));
                }
            }
            CommandKind::Jog(setpoint) => {
                if self.control_mode != ControlMode::Jog {
                    return Err("control mode is not Jog".into());
                }
                self.jog = Some((clamp_jog(setpoint), self.time_s));
            }
            CommandKind::SetValve { id, open } => {
                self.require_phase(&[Standby], "SetValve")?;
                self.stand.set_valve(*id, *open);
                let action = if *open { "opened" } else { "closed" };
                self.event(Severity::Info, format!("Valve {id:?} {action}"));
            }
        }
        Ok(())
    }

    fn require_phase(&self, allowed: &[FlightPhase], what: &str) -> Result<(), String> {
        if allowed.contains(&self.phase) {
            Ok(())
        } else {
            Err(format!(
                "{what} requires {}; phase is {:?}",
                allowed
                    .iter()
                    .map(|p| format!("{p:?}"))
                    .collect::<Vec<_>>()
                    .join("/"),
                self.phase
            ))
        }
    }

    fn params_changed(&mut self, text: &str) {
        self.event(Severity::Info, text.to_string());
        self.outbox.push(Downlink::Params(self.params.clone()));
    }

    // -----------------------------------------------------------------------
    // State machine
    // -----------------------------------------------------------------------

    pub fn step(&mut self, dt: f64) {
        use FlightPhase::*;

        self.time_s += dt;
        let phase_time_s = self.time_s - self.phase_entered_s;

        if self.terminated {
            self.actuation = Actuation::default();
            self.body.fall(dt);
        } else if let Some(plan) = &self.plan {
            self.actuation = self.body.fly(&plan.reference(self.time_s), self.time_s, dt);
            let plan_finished = plan.is_finished(self.time_s);
            match self.phase {
                Ascent if plan_finished => self.enter_phase(Hover),
                Hover if phase_time_s >= f64::from(self.params.flight.hover_duration_s) => {
                    self.enter_phase(Descent)
                }
                Descent if plan_finished && self.body.position[2] < 0.15 => {
                    self.enter_phase(Landed)
                }
                _ => {}
            }
        } else {
            self.actuation = self.jog_actuation();
            self.body.burn(self.actuation.thrust, dt);
        }
        self.stand.step(self.actuation.thrust, dt);

        if let Some(plan) = &self.plan {
            if self.time_s - self.last_trajectory_s >= TRAJECTORY_PERIOD_S {
                self.last_trajectory_s = self.time_s;
                self.outbox.push(Downlink::Trajectory(plan.to_msg(self.time_s)));
            }
        }

        self.auto_reset_when_finished();
    }

    fn enter_phase(&mut self, phase: FlightPhase) {
        use FlightPhase::*;

        let hover_altitude = f64::from(self.params.flight.hover_altitude_m);
        let position = self.body.position;
        self.plan = match phase {
            Ascent => Some(Plan::between(
                position,
                add(LAUNCH_SITE, [0.0, 0.0, hover_altitude]),
                self.time_s,
            )),
            // Hold wherever the vehicle is, so an operator override does not climb back up.
            Hover => Some(Plan::hold(
                position,
                self.time_s,
                f64::from(self.params.flight.hover_duration_s),
            )),
            Descent => Some(Plan::between(position, PAD, self.time_s)),
            Standby | Armed | Landed => None,
        };
        if phase == Landed {
            self.body.position[2] = 0.0;
            self.body.velocity = [0.0; 3];
            self.body.angular_velocity = [0.0; 3];
        }
        if let Some(plan) = &self.plan {
            self.last_trajectory_s = self.time_s;
            self.outbox.push(Downlink::Trajectory(plan.to_msg(self.time_s)));
        }

        if phase != self.phase {
            self.event(Severity::Info, format!("Phase: {:?} -> {phase:?}", self.phase));
        }
        self.phase = phase;
        self.phase_entered_s = self.time_s;
        // Any phase change forces Auto.
        self.control_mode = ControlMode::Auto;
        self.jog = None;
    }

    fn terminate(&mut self, reason: &str) {
        if !self.terminated {
            self.terminated = true;
            self.plan = None;
            self.jog = None;
            self.control_mode = ControlMode::Auto;
            self.event(Severity::Critical, format!("Flight terminated: {reason}"));
        }
    }

    /// Actuators follow the jog setpoint until it goes stale (deadman), then return to zero.
    fn jog_actuation(&mut self) -> Actuation {
        let Some((setpoint, received_s)) = self.jog.clone() else {
            return Actuation::default();
        };
        if self.time_s - received_s > JOG_TIMEOUT_S {
            self.jog = None;
            self.event(Severity::Warning, "Jog setpoint expired; actuators zeroed".into());
            return Actuation::default();
        }
        Actuation {
            gimbal_theta: f64::from(setpoint.gimbal_theta),
            gimbal_phi: f64::from(setpoint.gimbal_phi),
            thrust: f64::from(setpoint.thrust),
            rcs: setpoint.rcs,
        }
    }

    fn auto_reset_when_finished(&mut self) {
        let finished =
            self.phase == FlightPhase::Landed || (self.terminated && self.body.on_ground());
        if !finished {
            self.finished_at_s = None;
            return;
        }
        let finished_at_s = *self.finished_at_s.get_or_insert(self.time_s);
        if self.auto_reset && self.time_s - finished_at_s >= AUTO_RESET_DELAY_S {
            self.terminated = false;
            self.finished_at_s = None;
            self.body = Body::at_rest(LAUNCH_SITE);
            self.stand = Stand::new();
            self.event(
                Severity::Info,
                "Mock vehicle reset: refuelled, back on the launch site".into(),
            );
            self.enter_phase(FlightPhase::Standby);
        }
    }

    fn event(&mut self, severity: Severity, text: String) {
        eprintln!("[mock {:8.2}] {severity:?}: {text}", self.time_s);
        self.outbox.push(Downlink::Event(EventMsg {
            time_s: self.time_s,
            severity,
            text,
        }));
    }

    // -----------------------------------------------------------------------
    // Telemetry
    // -----------------------------------------------------------------------

    pub fn flight_telemetry(&mut self, link_age_s: Option<f32>) -> FlightTelemetry {
        let body = &self.body;
        let t = self.time_s;

        // The "estimate" is truth plus a slowly drifting bias and a little noise.
        let position_bias = [
            0.06 * (0.21 * t).sin(),
            0.05 * (0.17 * t).cos(),
            0.04 * (0.13 * t).sin(),
        ];
        let velocity_bias = [
            0.02 * (0.9 * t).sin(),
            0.02 * (1.1 * t).cos(),
            0.015 * (0.7 * t).sin(),
        ];
        let moving = !body.on_ground();
        let jitter = if moving { 1.0 } else { 0.1 };
        let est_position = add(add(body.position, position_bias), self.noise.vec3(0.01 * jitter));
        let est_velocity = add(add(body.velocity, velocity_bias), self.noise.vec3(0.01 * jitter));
        let est_rotation = add(body.rotation, self.noise.vec3(0.002 * jitter));
        let est_angular_velocity = add(body.angular_velocity, self.noise.vec3(0.002));

        let deviation = self
            .plan
            .as_ref()
            .map(|plan| norm(sub(est_position, plan.reference(t).position)) as f32);

        let thrust = self.actuation.thrust;
        // Specific force along the body axis: the ground pushes back 1 g while sitting on it.
        let accel_z = if moving { thrust / body.mass } else { G };
        let accel = add([0.0, 0.0, accel_z], self.noise.vec3(0.05));

        self.flight_seq = self.flight_seq.wrapping_add(1);
        FlightTelemetry {
            seq: self.flight_seq,
            time_s: t,
            source: Source::Sim,
            phase: self.phase,
            phase_time_s: (t - self.phase_entered_s) as f32,
            control_mode: self.control_mode,
            terminated: self.terminated,
            position: to_f32(est_position),
            velocity: to_f32(est_velocity),
            attitude: quaternion_from_rotation_vector(est_rotation).map(|v| v as f32),
            angular_velocity: to_f32(est_angular_velocity),
            mass: body.mass as f32,
            gimbal_theta: self.actuation.gimbal_theta as f32,
            gimbal_phi: self.actuation.gimbal_phi as f32,
            thrust: thrust as f32,
            rcs: self.actuation.rcs,
            tilt_deg: body.tilt_deg() as f32,
            trajectory_deviation_m: deviation,
            position_age_s: (t % GPS_PERIOD_S) as f32,
            link_age_s,
            sensors: SensorSnapshot {
                imu_ok: true,
                gps_ok: true,
                uwb_ok: true,
                accel: to_f32(accel),
                gyro: to_f32(est_angular_velocity),
                chamber_pressure: Some(self.stand.chamber_pressure_bar(thrust)),
                tank_pressure: Some(
                    self.stand
                        .tank_pressure_bar(body.propellant_used_fraction()),
                ),
            },
            truth: Some(TruthState {
                position: to_f32(body.position),
                velocity: to_f32(body.velocity),
                attitude: body.attitude().map(|v| v as f32),
                angular_velocity: to_f32(body.angular_velocity),
            }),
        }
    }

    pub fn stand_telemetry(&mut self) -> StandTelemetry {
        self.stand.telemetry(
            self.time_s,
            &self.actuation,
            self.body.propellant_used_fraction(),
            &mut self.noise,
        )
    }
}

fn clamp_jog(setpoint: &JogSetpoint) -> JogSetpoint {
    let max_gimbal = MAX_GIMBAL_RAD as f32;
    // `clamp` passes NaN through, so treat anything non-finite as zero first.
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    JogSetpoint {
        gimbal_theta: finite(setpoint.gimbal_theta).clamp(-max_gimbal, max_gimbal),
        gimbal_phi: finite(setpoint.gimbal_phi).clamp(-max_gimbal, max_gimbal),
        thrust: finite(setpoint.thrust).clamp(0.0, MAX_THRUST_N as f32),
        rcs: setpoint.rcs.clamp(-1, 1),
    }
}

fn check_flight_params(params: &FlightParams) -> Result<(), String> {
    let check = |name: &str, value: f32, min: f32, max: f32| {
        if (min..=max).contains(&value) {
            Ok(())
        } else {
            Err(format!("{name} = {value} is outside {min}..{max}"))
        }
    };
    check("hover_altitude_m", params.hover_altitude_m, 1.0, 200.0)?;
    check("hover_duration_s", params.hover_duration_s, 0.0, 120.0)?;
    check("max_tilt_deg", params.max_tilt_deg, 1.0, 45.0)?;
    check(
        "max_trajectory_deviation_m",
        params.max_trajectory_deviation_m,
        0.1,
        50.0,
    )
}

fn check_mpc_weights(weights: &MpcWeights) -> Result<(), String> {
    let valid = |values: &[f32]| values.iter().all(|w| w.is_finite() && *w >= 0.0);
    if valid(&weights.q) && valid(&weights.r) && valid(&weights.qn) {
        Ok(())
    } else {
        Err("MPC weights must be finite and non-negative".into())
    }
}

fn to_f32(v: Vec3) -> [f32; 3] {
    v.map(|x| x as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use FlightPhase::*;

    const DT: f64 = 0.02;

    fn run(vehicle: &mut Vehicle, seconds: f64) {
        for _ in 0..(seconds / DT) as usize {
            vehicle.step(DT);
        }
    }

    fn rejected(result: AckResult) -> bool {
        matches!(result, AckResult::Rejected(_))
    }

    #[test]
    fn interlocks_follow_the_design_table() {
        let mut v = Vehicle::new(false);
        assert!(rejected(v.handle_command(&CommandKind::Launch)));
        assert!(rejected(v.handle_command(&CommandKind::Disarm)));
        assert!(rejected(v.handle_command(&CommandKind::SetPhase(Descent))));

        assert_eq!(
            v.handle_command(&CommandKind::SetControlMode(ControlMode::Jog)),
            AckResult::Accepted
        );
        assert!(rejected(v.handle_command(&CommandKind::Arm)));
        v.handle_command(&CommandKind::SetControlMode(ControlMode::Auto));

        assert_eq!(v.handle_command(&CommandKind::Arm), AckResult::Accepted);
        assert!(rejected(v.handle_command(&CommandKind::SetControlMode(ControlMode::Jog))));
        assert!(rejected(v.handle_command(&CommandKind::SetValve {
            id: gs_protocol::ValveId::Omv,
            open: true
        })));
        assert_eq!(v.handle_command(&CommandKind::Launch), AckResult::Accepted);
        assert_eq!(v.phase, Ascent);
        assert!(rejected(v.handle_command(&CommandKind::SetPhase(Landed))));
        assert_eq!(v.handle_command(&CommandKind::SetPhase(Descent)), AckResult::Accepted);

        assert_eq!(v.handle_command(&CommandKind::Abort), AckResult::Accepted);
        assert!(v.terminated);
        assert!(rejected(v.handle_command(&CommandKind::Arm)));
    }

    #[test]
    fn flies_a_full_mission_and_resets() {
        let mut v = Vehicle::new(true);
        v.handle_command(&CommandKind::SetFlightParams(FlightParams {
            hover_altitude_m: 10.0,
            hover_duration_s: 2.0,
            max_tilt_deg: 15.0,
            max_trajectory_deviation_m: 5.0,
        }));
        v.handle_command(&CommandKind::Arm);
        v.handle_command(&CommandKind::Launch);

        let mut seen = vec![v.phase];
        let mut max_altitude: f64 = 0.0;
        for _ in 0..(60.0 / DT) as usize {
            v.step(DT);
            max_altitude = max_altitude.max(v.body.position[2]);
            if seen.last() != Some(&v.phase) {
                seen.push(v.phase);
            }
        }
        assert_eq!(seen, [Ascent, Hover, Descent, Landed, Standby]);
        assert!((max_altitude - 10.0).abs() < 0.5, "{max_altitude}");
        assert_eq!(v.body.position, LAUNCH_SITE);

        let sent: Vec<Downlink> = v.drain_outbox().collect();
        assert!(sent.iter().any(|m| matches!(m, Downlink::Trajectory(_))));
        assert!(sent.iter().any(|m| matches!(m, Downlink::Params(_))));
    }

    #[test]
    fn jog_is_clamped_and_expires() {
        let mut v = Vehicle::new(false);
        v.handle_command(&CommandKind::SetControlMode(ControlMode::Jog));
        v.handle_command(&CommandKind::Jog(JogSetpoint {
            gimbal_theta: 1.0,
            gimbal_phi: -1.0,
            thrust: 5000.0,
            rcs: 1,
        }));
        v.step(DT);
        assert!((v.actuation.gimbal_theta - MAX_GIMBAL_RAD).abs() < 1e-6);
        assert!((v.actuation.gimbal_phi + MAX_GIMBAL_RAD).abs() < 1e-6);
        assert_eq!(v.actuation.thrust, MAX_THRUST_N);

        run(&mut v, JOG_TIMEOUT_S + 0.1);
        assert_eq!(v.actuation.thrust, 0.0);
        assert_eq!(v.actuation.rcs, 0);
    }

    #[test]
    fn abort_in_flight_falls_to_the_ground() {
        let mut v = Vehicle::new(false);
        v.handle_command(&CommandKind::Arm);
        v.handle_command(&CommandKind::Launch);
        run(&mut v, 6.0);
        assert!(v.body.position[2] > 5.0);

        v.handle_command(&CommandKind::Abort);
        run(&mut v, 10.0);
        assert!(v.body.on_ground());
        assert_eq!(v.flight_telemetry(None).thrust, 0.0);
    }
}
