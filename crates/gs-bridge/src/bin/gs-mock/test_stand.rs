//! The mock test-stand adapter (`gs-mock --stand`): a fake `gs-stand` with the arming
//! rules from the "Test-stand rules" table in `docs/DESIGN.md`, the ten Arduino valves,
//! igniter, DAQ sync, MTV throttle, three load cells that react to what is open, and
//! canned timed sequences. Independent of the vehicle mock.

use gs_protocol::{
    AckResult, CommandKind, Downlink, EventMsg, SequenceProgress, Severity, Source,
    StandChannel, StandCommand, StandMode, StandOutput, StandStatus, StandTelemetry, ValveId,
    ValveState, ValveStatus,
};

use crate::physics::Noise;

pub const SEQUENCES: [&str; 4] = ["hotfire", "coldflow", "rcs", "igniter_check"];

/// The valves on the actuation Arduino, in its channel order.
pub const ARDUINO_VALVES: [ValveId; 10] = [
    ValveId::Omv,
    ValveId::OVnt,
    ValveId::PuIso,
    ValveId::IgV,
    ValveId::OFill,
    ValveId::LfVnt,
    ValveId::PuFill,
    ValveId::PuVnt,
    ValveId::OIso,
    ValveId::PuMv,
];

/// OISO is motorized; its state is unknown while it strokes.
pub const OISO_STROKE_S: f64 = 21.0;

const MAX_THRUST_N: f32 = 1200.0;
/// Nitrous through an unlit engine still pushes a little.
const COLD_FLOW_FRACTION: f32 = 0.12;
const THRUST_TAU_S: f32 = 0.25;
const RCS_THRUST_N: f32 = 45.0;
const RCS_TAU_S: f32 = 0.08;
const NITROUS_FULL_KG: f32 = 12.0;
const FILL_RATE_KG_S: f32 = 0.25;
/// Mass flow per newton: 1 / (Isp * g0) with Isp ~ 180 s.
const KG_PER_NS: f32 = 1.0 / (180.0 * 9.81);

#[derive(Debug, Clone, Copy)]
enum Action {
    Valve(ValveId, bool),
    Mtv(f32),
    Igniter(bool),
    DaqSync(bool),
    Rcs(bool),
}

struct Step {
    at_s: f32,
    text: &'static str,
    actions: &'static [Action],
}

use Action as A;
use ValveId::*;

const HOTFIRE: &[Step] = &[
    Step { at_s: 0.0, text: "DAQ sync on", actions: &[A::DaqSync(true)] },
    Step { at_s: 2.0, text: "Igniter on", actions: &[A::Igniter(true)] },
    Step { at_s: 3.0, text: "IgV open", actions: &[A::Valve(IgV, true)] },
    Step { at_s: 4.0, text: "OMV open, MTV 40 %", actions: &[A::Valve(Omv, true), A::Mtv(40.0)] },
    Step { at_s: 6.0, text: "MTV 100 %", actions: &[A::Mtv(100.0)] },
    Step { at_s: 14.0, text: "MTV 0 %, OMV closed", actions: &[A::Mtv(0.0), A::Valve(Omv, false)] },
    Step { at_s: 15.0, text: "IgV closed, igniter off", actions: &[A::Valve(IgV, false), A::Igniter(false)] },
    Step { at_s: 16.0, text: "Purge: PuMv open", actions: &[A::Valve(PuMv, true)] },
    Step { at_s: 20.0, text: "Purge closed, DAQ sync off", actions: &[A::Valve(PuMv, false), A::DaqSync(false)] },
];

const COLDFLOW: &[Step] = &[
    Step { at_s: 0.0, text: "DAQ sync on", actions: &[A::DaqSync(true)] },
    Step { at_s: 2.0, text: "OMV open, MTV 30 %", actions: &[A::Valve(Omv, true), A::Mtv(30.0)] },
    Step { at_s: 4.0, text: "MTV 100 %", actions: &[A::Mtv(100.0)] },
    Step { at_s: 10.0, text: "MTV 0 %, OMV closed", actions: &[A::Mtv(0.0), A::Valve(Omv, false)] },
    Step { at_s: 11.0, text: "Purge: PuMv open", actions: &[A::Valve(PuMv, true)] },
    Step { at_s: 15.0, text: "Purge closed", actions: &[A::Valve(PuMv, false)] },
    Step { at_s: 16.0, text: "DAQ sync off", actions: &[A::DaqSync(false)] },
];

const RCS: &[Step] = &[
    Step { at_s: 0.0, text: "DAQ sync on", actions: &[A::DaqSync(true)] },
    Step { at_s: 2.0, text: "RCS pulse 1 on", actions: &[A::Rcs(true)] },
    Step { at_s: 4.0, text: "RCS pulse 1 off", actions: &[A::Rcs(false)] },
    Step { at_s: 6.0, text: "RCS pulse 2 on", actions: &[A::Rcs(true)] },
    Step { at_s: 9.0, text: "RCS pulse 2 off", actions: &[A::Rcs(false)] },
    Step { at_s: 12.0, text: "DAQ sync off", actions: &[A::DaqSync(false)] },
];

const IGNITER_CHECK: &[Step] = &[
    Step { at_s: 0.0, text: "DAQ sync on", actions: &[A::DaqSync(true)] },
    Step { at_s: 1.0, text: "Igniter on", actions: &[A::Igniter(true)] },
    Step { at_s: 3.0, text: "Igniter off", actions: &[A::Igniter(false)] },
    Step { at_s: 6.0, text: "DAQ sync off", actions: &[A::DaqSync(false)] },
];

fn steps_for(name: &str) -> Option<&'static [Step]> {
    match name {
        "hotfire" => Some(HOTFIRE),
        "coldflow" => Some(COLDFLOW),
        "rcs" => Some(RCS),
        "igniter_check" => Some(IGNITER_CHECK),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct Valve {
    id: ValveId,
    /// `None` until the first command: the Arduino gives no feedback.
    commanded_open: Option<bool>,
    commanded_at_s: f64,
}

struct Running {
    name: &'static str,
    steps: &'static [Step],
    started_s: f64,
    next: usize,
}

pub struct TestStand {
    time_s: f64,
    mode: StandMode,
    valves: Vec<Valve>,
    igniter: bool,
    daq_sync: bool,
    mtv_percent: f32,
    rcs_firing: bool,
    sequence: Option<Running>,

    thrust_n: f32,
    rcs_thrust_n: f32,
    nitrous_kg: f32,

    pub actuation_link_ok: bool,
    pub loadcell_link_ok: bool,

    noise: Noise,
    outbox: Vec<Downlink>,
}

impl TestStand {
    pub fn new() -> Self {
        Self {
            time_s: 0.0,
            mode: StandMode::Safe,
            valves: ARDUINO_VALVES
                .iter()
                .map(|&id| Valve {
                    id,
                    commanded_open: None,
                    commanded_at_s: 0.0,
                })
                .collect(),
            igniter: false,
            daq_sync: false,
            mtv_percent: 0.0,
            rcs_firing: false,
            sequence: None,
            thrust_n: 0.0,
            rcs_thrust_n: 0.0,
            nitrous_kg: NITROUS_FULL_KG,
            actuation_link_ok: true,
            loadcell_link_ok: true,
            noise: Noise::new(),
            outbox: Vec::new(),
        }
    }

    pub fn time_s(&self) -> f64 {
        self.time_s
    }

    #[cfg(test)]
    pub fn mode(&self) -> StandMode {
        self.mode
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
        match kind {
            CommandKind::Heartbeat => Ok(()),
            CommandKind::Stand(command) => self.try_stand_command(command),
            CommandKind::SetValve { id, open } => {
                self.require_mode(&[StandMode::Armed], "SetValve")?;
                if !ARDUINO_VALVES.contains(id) {
                    return Err(format!("{id:?} is not a test-stand valve"));
                }
                self.set_valve(*id, *open);
                let action = if *open { "opened" } else { "closed" };
                self.event(Severity::Info, format!("Valve {id:?} {action}"));
                Ok(())
            }
            other => Err(format!(
                "{} is a vehicle command; the test stand cannot act on it",
                command_name(other)
            )),
        }
    }

    fn try_stand_command(&mut self, command: &StandCommand) -> Result<(), String> {
        use StandMode::*;
        match command {
            StandCommand::Arm => {
                self.require_mode(&[Safe], "Arm")?;
                if !self.actuation_link_ok {
                    return Err("actuation Arduino link is down".into());
                }
                if !self.loadcell_link_ok {
                    return Err("load-cell Arduino link is down".into());
                }
                self.enter_mode(Armed);
            }
            StandCommand::Disarm => {
                self.require_mode(&[Safe, Armed], "Disarm")?;
                self.run_safing_list("Disarm");
                self.enter_mode(Safe);
            }
            StandCommand::Abort => {
                let ended = self.sequence.take().map(|r| r.name);
                self.igniter = false;
                self.rcs_firing = false;
                self.run_safing_list("Abort");
                let text = match ended {
                    Some(name) => format!("ABORT: sequence {name} ended, outputs safed"),
                    None => "ABORT: outputs safed".to_string(),
                };
                self.event(Severity::Critical, text);
                self.enter_mode(Safe);
            }
            StandCommand::SetMtvPercent(percent) => {
                self.require_mode(&[Armed], "SetMtvPercent")?;
                if !(percent.is_finite() && (0.0..=100.0).contains(percent)) {
                    return Err(format!("MTV {percent} % is outside 0..100"));
                }
                self.mtv_percent = *percent;
                self.event(Severity::Info, format!("MTV set to {percent:.0} %"));
            }
            StandCommand::SetOutput {
                id: StandOutput::Igniter,
                on,
            } => {
                self.require_mode(&[Armed], "Igniter")?;
                self.igniter = *on;
                let state = if *on { "on" } else { "off" };
                self.event(Severity::Warning, format!("Igniter {state}"));
            }
            StandCommand::SetOutput {
                id: StandOutput::DaqSync,
                on,
            } => {
                self.require_mode(&[Safe, Armed], "DaqSync")?;
                self.daq_sync = *on;
                let state = if *on { "on" } else { "off" };
                self.event(Severity::Info, format!("DAQ sync {state}"));
            }
            StandCommand::StartSequence(name) => {
                self.require_mode(&[Armed], "StartSequence")?;
                let (name, steps) = SEQUENCES
                    .iter()
                    .find(|s| **s == name.as_str())
                    .and_then(|s| steps_for(s).map(|steps| (*s, steps)))
                    .ok_or_else(|| {
                        format!(
                            "unknown sequence {name:?}; available: {}",
                            SEQUENCES.join(", ")
                        )
                    })?;
                self.sequence = Some(Running {
                    name,
                    steps,
                    started_s: self.time_s,
                    next: 0,
                });
                self.event(
                    Severity::Info,
                    format!(
                        "Sequence {name} started: {} steps over {:.0} s",
                        steps.len(),
                        duration_s(steps)
                    ),
                );
                self.enter_mode(Sequence);
            }
        }
        Ok(())
    }

    fn require_mode(&self, allowed: &[StandMode], what: &str) -> Result<(), String> {
        if allowed.contains(&self.mode) {
            Ok(())
        } else {
            let hint = if self.mode == StandMode::Sequence {
                "; Abort to end the sequence"
            } else {
                ""
            };
            Err(format!(
                "{what} requires {}; stand is {:?}{hint}",
                allowed
                    .iter()
                    .map(|m| format!("{m:?}"))
                    .collect::<Vec<_>>()
                    .join("/"),
                self.mode
            ))
        }
    }

    fn enter_mode(&mut self, mode: StandMode) {
        if mode != self.mode {
            self.event(Severity::Info, format!("Mode: {:?} -> {mode:?}", self.mode));
            self.mode = mode;
        }
    }

    /// `stand/config.toml` default: fuel-side valves closed, igniter off, MTV shut,
    /// vents opened. OISO is left alone.
    fn run_safing_list(&mut self, why: &str) {
        for id in [Omv, IgV, OFill, PuMv, PuIso, PuFill] {
            self.set_valve(id, false);
        }
        for id in [OVnt, PuVnt, LfVnt] {
            self.set_valve(id, true);
        }
        self.igniter = false;
        self.mtv_percent = 0.0;
        self.event(
            Severity::Info,
            format!("{why}: safing list run (fuel valves closed, vents open, MTV 0 %)"),
        );
    }

    fn set_valve(&mut self, id: ValveId, open: bool) {
        if let Some(valve) = self.valves.iter_mut().find(|v| v.id == id) {
            valve.commanded_open = Some(open);
            valve.commanded_at_s = self.time_s;
        }
    }

    fn is_open(&self, id: ValveId) -> bool {
        self.valves
            .iter()
            .any(|v| v.id == id && v.commanded_open == Some(true))
    }

    fn apply(&mut self, action: Action) {
        match action {
            A::Valve(id, open) => self.set_valve(id, open),
            A::Mtv(percent) => self.mtv_percent = percent,
            A::Igniter(on) => self.igniter = on,
            A::DaqSync(on) => self.daq_sync = on,
            A::Rcs(on) => self.rcs_firing = on,
        }
    }

    // -----------------------------------------------------------------------
    // Time
    // -----------------------------------------------------------------------

    pub fn step(&mut self, dt: f64) {
        self.time_s += dt;
        self.advance_sequence();
        self.simulate(dt as f32);
    }

    fn advance_sequence(&mut self) {
        let Some(running) = &mut self.sequence else { return };
        let t_s = (self.time_s - running.started_s) as f32;
        let total = running.steps.len();
        let mut fired = Vec::new();
        while let Some(step) = running.steps.get(running.next) {
            if t_s < step.at_s {
                break;
            }
            fired.push((running.next, step));
            running.next += 1;
        }
        let name = running.name;
        let finished = running.next >= total;
        for (i, step) in fired {
            for &action in step.actions {
                self.apply(action);
            }
            self.event(
                Severity::Info,
                format!("{name} step {}/{total}: {}", i + 1, step.text),
            );
        }
        if finished {
            self.sequence = None;
            self.event(Severity::Info, format!("Sequence {name} complete"));
            self.enter_mode(StandMode::Armed);
        }
    }

    fn simulate(&mut self, dt: f32) {
        let feeding = self.is_open(Omv) && self.nitrous_kg > 0.0;
        let hot = if self.igniter { 1.0 } else { COLD_FLOW_FRACTION };
        let target = if feeding {
            MAX_THRUST_N * self.mtv_percent / 100.0 * hot
        } else {
            0.0
        };
        self.thrust_n += (target - self.thrust_n) * (dt / THRUST_TAU_S).min(1.0);

        let rcs_target = if self.rcs_firing { RCS_THRUST_N } else { 0.0 };
        self.rcs_thrust_n += (rcs_target - self.rcs_thrust_n) * (dt / RCS_TAU_S).min(1.0);

        self.nitrous_kg -= self.thrust_n * KG_PER_NS * dt;
        if self.is_open(OFill) {
            self.nitrous_kg += FILL_RATE_KG_S * dt;
        }
        self.nitrous_kg = self.nitrous_kg.clamp(0.0, NITROUS_FULL_KG);
    }

    fn event(&mut self, severity: Severity, text: String) {
        eprintln!("[stand {:8.2}] {severity:?}: {text}", self.time_s);
        self.outbox.push(Downlink::Event(EventMsg {
            time_s: self.time_s,
            severity,
            text,
        }));
    }

    // -----------------------------------------------------------------------
    // Telemetry
    // -----------------------------------------------------------------------

    pub fn telemetry(&mut self) -> StandTelemetry {
        let channels = vec![
            (StandChannel::Thrust, self.thrust_n + self.noise.sample(1.5) as f32),
            (StandChannel::NitrousMass, self.nitrous_kg + self.noise.sample(0.02) as f32),
            (StandChannel::RcsThrust, self.rcs_thrust_n + self.noise.sample(0.3) as f32),
        ];
        let valves = self
            .valves
            .iter()
            .map(|v| ValveStatus {
                id: v.id,
                state: self.valve_state(v),
                position_deg: None,
            })
            .collect();
        let mut outputs_on = Vec::new();
        if self.igniter {
            outputs_on.push(StandOutput::Igniter);
        }
        if self.daq_sync {
            outputs_on.push(StandOutput::DaqSync);
        }
        StandTelemetry {
            time_s: self.time_s,
            source: Source::Stand,
            channels,
            valves,
            outputs_on,
            mtv_percent: Some(self.mtv_percent),
        }
    }

    /// Reported state is the commanded state: `Unknown` before the first command, and
    /// for OISO while its motor is still stroking.
    fn valve_state(&self, valve: &Valve) -> ValveState {
        match valve.commanded_open {
            None => ValveState::Unknown,
            Some(_) if valve.id == OIso && self.time_s - valve.commanded_at_s < OISO_STROKE_S => {
                ValveState::Unknown
            }
            Some(true) => ValveState::Open,
            Some(false) => ValveState::Closed,
        }
    }

    pub fn status(&self) -> StandStatus {
        StandStatus {
            time_s: self.time_s,
            mode: self.mode,
            actuation_link_ok: self.actuation_link_ok,
            loadcell_link_ok: self.loadcell_link_ok,
            sequences: SEQUENCES.iter().map(|s| s.to_string()).collect(),
            sequence: self.sequence.as_ref().map(|r| SequenceProgress {
                name: r.name.to_string(),
                t_s: (self.time_s - r.started_s) as f32,
                duration_s: duration_s(r.steps),
                next_step: r
                    .steps
                    .get(r.next)
                    .map(|step| (r.next as u32, step.text.to_string())),
                steps_total: r.steps.len() as u32,
            }),
        }
    }
}

fn duration_s(steps: &[Step]) -> f32 {
    steps.last().map_or(0.0, |s| s.at_s)
}

fn command_name(kind: &CommandKind) -> String {
    let text = format!("{kind:?}");
    text.split(['(', ' ', '{'])
        .next()
        .unwrap_or(&text)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use StandMode::*;

    const DT: f64 = 0.05;

    fn run(stand: &mut TestStand, seconds: f64) {
        for _ in 0..(seconds / DT).round() as usize {
            stand.step(DT);
        }
    }

    fn rejected(result: AckResult) -> bool {
        matches!(result, AckResult::Rejected(_))
    }

    fn stand(cmd: StandCommand) -> CommandKind {
        CommandKind::Stand(cmd)
    }

    fn valve(id: ValveId, open: bool) -> CommandKind {
        CommandKind::SetValve { id, open }
    }

    fn igniter(on: bool) -> CommandKind {
        stand(StandCommand::SetOutput {
            id: StandOutput::Igniter,
            on,
        })
    }

    fn daq(on: bool) -> CommandKind {
        stand(StandCommand::SetOutput {
            id: StandOutput::DaqSync,
            on,
        })
    }

    fn start(name: &str) -> CommandKind {
        stand(StandCommand::StartSequence(name.into()))
    }

    fn armed() -> TestStand {
        let mut s = TestStand::new();
        assert_eq!(s.handle_command(&stand(StandCommand::Arm)), AckResult::Accepted);
        s
    }

    fn state_of(t: &StandTelemetry, id: ValveId) -> ValveState {
        t.valves.iter().find(|v| v.id == id).unwrap().state
    }

    fn channel(t: &StandTelemetry, c: StandChannel) -> f32 {
        t.channels.iter().find(|(ch, _)| *ch == c).unwrap().1
    }

    #[test]
    fn arm_requires_safe_and_both_arduino_links() {
        let mut s = TestStand::new();
        s.actuation_link_ok = false;
        assert!(rejected(s.handle_command(&stand(StandCommand::Arm))));
        s.actuation_link_ok = true;
        s.loadcell_link_ok = false;
        assert!(rejected(s.handle_command(&stand(StandCommand::Arm))));
        s.loadcell_link_ok = true;
        assert_eq!(s.handle_command(&stand(StandCommand::Arm)), AckResult::Accepted);
        assert_eq!(s.mode(), Armed);
        // Not Safe any more.
        assert!(rejected(s.handle_command(&stand(StandCommand::Arm))));
        s.handle_command(&start("hotfire"));
        assert!(rejected(s.handle_command(&stand(StandCommand::Arm))));
    }

    #[test]
    fn disarm_from_safe_or_armed_runs_the_safing_list() {
        let mut s = armed();
        s.handle_command(&valve(Omv, true));
        s.handle_command(&valve(OVnt, false));
        s.handle_command(&igniter(true));
        s.handle_command(&stand(StandCommand::SetMtvPercent(50.0)));
        assert_eq!(s.handle_command(&stand(StandCommand::Disarm)), AckResult::Accepted);
        assert_eq!(s.mode(), Safe);
        let t = s.telemetry();
        assert_eq!(state_of(&t, Omv), ValveState::Closed);
        assert_eq!(state_of(&t, OVnt), ValveState::Open);
        assert_eq!(state_of(&t, PuVnt), ValveState::Open);
        assert_eq!(state_of(&t, LfVnt), ValveState::Open);
        assert_eq!(state_of(&t, OIso), ValveState::Unknown, "OISO is not on the safing list");
        assert!(t.outputs_on.is_empty());
        assert_eq!(t.mtv_percent, Some(0.0));

        // Safe -> Safe is allowed (re-safes).
        assert_eq!(s.handle_command(&stand(StandCommand::Disarm)), AckResult::Accepted);

        // Sequence: rejected; Abort is the way out.
        let mut s = armed();
        s.handle_command(&start("hotfire"));
        assert!(rejected(s.handle_command(&stand(StandCommand::Disarm))));
        assert_eq!(s.mode(), Sequence);
    }

    #[test]
    fn abort_is_always_accepted_and_safes() {
        for setup in [
            |_: &mut TestStand| {},
            |s: &mut TestStand| {
                s.handle_command(&stand(StandCommand::Arm));
            },
            |s: &mut TestStand| {
                s.handle_command(&stand(StandCommand::Arm));
                s.handle_command(&start("hotfire"));
                run(s, 7.0);
            },
        ] {
            let mut s = TestStand::new();
            setup(&mut s);
            assert_eq!(s.handle_command(&stand(StandCommand::Abort)), AckResult::Accepted);
            assert_eq!(s.mode(), Safe);
            assert!(s.status().sequence.is_none());
            let t = s.telemetry();
            assert_eq!(state_of(&t, Omv), ValveState::Closed);
            assert_eq!(state_of(&t, IgV), ValveState::Closed);
            // DAQ sync is not on the safing list: the external DAQ keeps capturing.
            assert!(!t.outputs_on.contains(&StandOutput::Igniter));
            assert_eq!(t.mtv_percent, Some(0.0));
            let events: Vec<Downlink> = s.drain_outbox().collect();
            assert!(events.iter().any(|m| matches!(
                m,
                Downlink::Event(EventMsg { severity: Severity::Critical, text, .. })
                    if text.contains("ABORT") && text.contains("safed")
            )));
        }
    }

    #[test]
    fn valve_mtv_and_igniter_need_armed() {
        let mut s = TestStand::new();
        assert!(rejected(s.handle_command(&valve(Omv, true))));
        assert!(rejected(s.handle_command(&stand(StandCommand::SetMtvPercent(10.0)))));
        assert!(rejected(s.handle_command(&igniter(true))));

        s.handle_command(&stand(StandCommand::Arm));
        assert_eq!(s.handle_command(&valve(Omv, true)), AckResult::Accepted);
        assert_eq!(s.handle_command(&stand(StandCommand::SetMtvPercent(10.0))), AckResult::Accepted);
        assert_eq!(s.handle_command(&igniter(true)), AckResult::Accepted);
        assert!(rejected(s.handle_command(&stand(StandCommand::SetMtvPercent(101.0)))));
        assert!(rejected(s.handle_command(&stand(StandCommand::SetMtvPercent(f32::NAN)))));
        // Valves that are not on the actuation Arduino.
        assert!(rejected(s.handle_command(&valve(Mtv, true))));
        assert!(rejected(s.handle_command(&valve(Rcs1, true))));

        s.handle_command(&igniter(false));
        s.handle_command(&start("coldflow"));
        assert!(rejected(s.handle_command(&valve(Omv, false))));
        assert!(rejected(s.handle_command(&stand(StandCommand::SetMtvPercent(10.0)))));
        assert!(rejected(s.handle_command(&igniter(true))));
    }

    #[test]
    fn daq_sync_needs_safe_or_armed() {
        let mut s = TestStand::new();
        assert_eq!(s.handle_command(&daq(true)), AckResult::Accepted);
        assert_eq!(s.telemetry().outputs_on, [StandOutput::DaqSync]);
        s.handle_command(&stand(StandCommand::Arm));
        assert_eq!(s.handle_command(&daq(false)), AckResult::Accepted);
        s.handle_command(&start("rcs"));
        assert!(rejected(s.handle_command(&daq(true))));
    }

    #[test]
    fn start_sequence_needs_armed_and_a_known_name() {
        let mut s = TestStand::new();
        assert!(rejected(s.handle_command(&start("hotfire"))));
        s.handle_command(&stand(StandCommand::Arm));
        assert!(rejected(s.handle_command(&start("bbq"))));
        assert_eq!(s.handle_command(&start("hotfire")), AckResult::Accepted);
        assert_eq!(s.mode(), Sequence);
        assert!(rejected(s.handle_command(&start("coldflow"))));
        let status = s.status();
        assert_eq!(status.sequences, SEQUENCES);
        let progress = status.sequence.unwrap();
        assert_eq!(progress.name, "hotfire");
        assert_eq!(progress.steps_total, HOTFIRE.len() as u32);
        assert_eq!(progress.duration_s, 20.0);
    }

    #[test]
    fn hotfire_runs_to_completion_and_returns_to_armed() {
        let mut s = armed();
        s.handle_command(&start("hotfire"));
        s.drain_outbox().for_each(drop);

        run(&mut s, 8.0);
        let t = s.telemetry();
        assert_eq!(state_of(&t, Omv), ValveState::Open);
        assert_eq!(t.mtv_percent, Some(100.0));
        assert!(t.outputs_on.contains(&StandOutput::Igniter));
        assert!(channel(&t, StandChannel::Thrust) > 1000.0, "{}", channel(&t, StandChannel::Thrust));
        assert!(channel(&t, StandChannel::NitrousMass) < NITROUS_FULL_KG - 0.5);
        let next = s.status().sequence.unwrap().next_step.unwrap();
        assert_eq!(next.0, 5);
        assert!(next.1.starts_with("MTV 0 %"));

        run(&mut s, 13.0);
        assert_eq!(s.mode(), Armed);
        assert!(s.status().sequence.is_none());
        let t = s.telemetry();
        assert_eq!(state_of(&t, Omv), ValveState::Closed);
        assert!(t.outputs_on.is_empty());
        assert!(channel(&t, StandChannel::Thrust).abs() < 10.0);

        let texts: Vec<String> = s
            .drain_outbox()
            .filter_map(|m| match m {
                Downlink::Event(e) => Some(e.text),
                _ => None,
            })
            .collect();
        assert_eq!(texts.iter().filter(|t| t.contains("hotfire step")).count(), HOTFIRE.len());
        assert!(texts.iter().any(|t| t == "Sequence hotfire complete"));
        assert!(texts.iter().any(|t| t == "Mode: Sequence -> Armed"));
    }

    #[test]
    fn coldflow_thrust_is_low_and_rcs_sequence_moves_the_rcs_cell() {
        let mut s = armed();
        s.handle_command(&start("coldflow"));
        run(&mut s, 7.0);
        let thrust = channel(&s.telemetry(), StandChannel::Thrust);
        assert!((100.0..200.0).contains(&thrust), "{thrust}");

        let mut s = armed();
        s.handle_command(&start("rcs"));
        run(&mut s, 3.0);
        let rcs = channel(&s.telemetry(), StandChannel::RcsThrust);
        assert!((RCS_THRUST_N - rcs).abs() < 3.0, "{rcs}");
        run(&mut s, 2.0);
        assert!(channel(&s.telemetry(), StandChannel::RcsThrust).abs() < 3.0);
    }

    #[test]
    fn valves_are_unknown_before_command_and_oiso_while_stroking() {
        let mut s = armed();
        let t = s.telemetry();
        assert_eq!(t.valves.len(), ARDUINO_VALVES.len());
        assert!(t.valves.iter().all(|v| v.state == ValveState::Unknown));

        s.handle_command(&valve(OIso, true));
        s.handle_command(&valve(PuIso, true));
        assert_eq!(state_of(&s.telemetry(), PuIso), ValveState::Open);
        assert_eq!(state_of(&s.telemetry(), OIso), ValveState::Unknown);
        run(&mut s, OISO_STROKE_S - 0.5);
        assert_eq!(state_of(&s.telemetry(), OIso), ValveState::Unknown);
        run(&mut s, 1.0);
        assert_eq!(state_of(&s.telemetry(), OIso), ValveState::Open);
    }

    #[test]
    fn vehicle_commands_are_rejected() {
        let mut s = armed();
        let result = s.handle_command(&CommandKind::Launch);
        match result {
            AckResult::Rejected(reason) => assert!(reason.contains("Launch"), "{reason}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(s.handle_command(&CommandKind::Heartbeat), AckResult::Accepted);
    }
}
