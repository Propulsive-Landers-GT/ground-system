//! The stand's state machine: mode, interlocks, commanded valve states, safing and the
//! sequence engine. Pure: it takes commands and time, returns [`Effect`]s for the I/O layer
//! to carry out, so every rule in `docs/DESIGN.md` is unit-testable without hardware.

use std::collections::{BTreeMap, HashMap};

use gs_protocol::{
    AckResult, CommandKind, SequenceProgress, Severity, StandChannel, StandCommand, StandMode,
    StandOutput, StandStatus, StandTelemetry, Source, ValveId, ValveState, ValveStatus,
};

use crate::arduino::Role;
use crate::sequence::{arduino_device, valve_tag, Action, Sequence, STAND_VALVES};

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Write this command to that board.
    Serial(Role, String),
    /// Command the MTV to this opening.
    Mtv(f32),
    Event(Severity, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Links {
    pub actuation: bool,
    pub loadcell: bool,
}

impl Links {
    pub fn both() -> Self {
        Self {
            actuation: true,
            loadcell: true,
        }
    }
}

pub struct Rules {
    pub safing: Vec<Action>,
    pub use_reset_all: bool,
    pub oiso_travel_s: f64,
    /// Sequence clock multiplier (dev only; 1.0 on the stand).
    pub time_scale: f64,
    pub sequences: BTreeMap<String, Sequence>,
}

struct Run {
    name: String,
    /// Monotonic time of T-0.
    t0: f64,
    next_step: usize,
    profile_active: bool,
    last_profile_pct: Option<f32>,
}

pub struct Stand {
    rules: Rules,
    mode: StandMode,
    valves: HashMap<ValveId, ValveState>,
    oiso_cmd_at: Option<f64>,
    igniter_on: bool,
    daq_sync_on: bool,
    mtv_percent: f32,
    run: Option<Run>,
}

const REJECT_NOT_STAND: &str = "not a test-stand command";

impl Stand {
    pub fn new(rules: Rules) -> Self {
        let mut s = Self {
            rules,
            mode: StandMode::Safe,
            valves: HashMap::new(),
            oiso_cmd_at: None,
            igniter_on: false,
            daq_sync_on: false,
            mtv_percent: 0.0,
            run: None,
        };
        s.forget_valve_states();
        s
    }

    pub fn mode(&self) -> StandMode {
        self.mode
    }

    pub fn mtv_percent(&self) -> f32 {
        self.mtv_percent
    }

    pub fn sequence_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn sequence_time(&self, now: f64) -> Option<f64> {
        self.run.as_ref().map(|r| (now - r.t0) * self.rules.time_scale)
    }

    fn forget_valve_states(&mut self) {
        for (id, _) in STAND_VALVES {
            self.valves.insert(*id, ValveState::Unknown);
        }
        self.oiso_cmd_at = None;
    }

    fn set_mode(&mut self, mode: StandMode, fx: &mut Vec<Effect>) {
        if self.mode != mode {
            fx.push(Effect::Event(
                Severity::Info,
                format!("stand mode {:?} -> {:?}", self.mode, mode),
            ));
            self.mode = mode;
        }
    }

    // ----------------------------------------------------------------- commands

    /// Apply a ground command. Interlocks per `docs/DESIGN.md`.
    pub fn handle(&mut self, kind: &CommandKind, links: Links, now: f64) -> (AckResult, Vec<Effect>) {
        let mut fx = Vec::new();
        let ack = match self.dispatch(kind, links, now, &mut fx) {
            Ok(()) => AckResult::Accepted,
            Err(reason) => AckResult::Rejected(reason),
        };
        (ack, fx)
    }

    fn dispatch(
        &mut self,
        kind: &CommandKind,
        links: Links,
        now: f64,
        fx: &mut Vec<Effect>,
    ) -> Result<(), String> {
        match kind {
            CommandKind::Heartbeat => Ok(()),
            CommandKind::Stand(StandCommand::Arm) => self.arm(links, fx),
            CommandKind::Stand(StandCommand::Disarm) => match self.mode {
                StandMode::Safe | StandMode::Armed => {
                    self.safing(now, fx);
                    self.set_mode(StandMode::Safe, fx);
                    Ok(())
                }
                StandMode::Sequence => Err("sequence running; use Abort".to_string()),
            },
            CommandKind::Stand(StandCommand::Abort) => {
                self.abort(now, "operator abort", fx);
                Ok(())
            }
            CommandKind::SetValve { id, open } => {
                self.require_armed()?;
                match id {
                    ValveId::Mtv => {
                        self.apply(&Action::Mtv(if *open { 100.0 } else { 0.0 }), now, fx);
                        Ok(())
                    }
                    id if arduino_device(*id).is_some() => {
                        self.apply(&Action::Valve { id: *id, open: *open }, now, fx);
                        Ok(())
                    }
                    id => Err(format!("valve {} is not on this stand", valve_tag(*id))),
                }
            }
            CommandKind::Stand(StandCommand::SetMtvPercent(p)) => {
                self.require_armed()?;
                if !p.is_finite() || !(0.0..=100.0).contains(p) {
                    return Err(format!("MTV percent {p} outside 0..=100"));
                }
                self.apply(&Action::Mtv(*p), now, fx);
                Ok(())
            }
            CommandKind::Stand(StandCommand::SetOutput { id, on }) => {
                match id {
                    StandOutput::Igniter => self.require_armed()?,
                    StandOutput::DaqSync => {
                        if self.mode == StandMode::Sequence {
                            return Err(
                                "sequence running; DAQ sync is under sequence control".into()
                            );
                        }
                    }
                }
                self.apply(&Action::Output { id: *id, on: *on }, now, fx);
                Ok(())
            }
            CommandKind::Stand(StandCommand::StartSequence(name)) => {
                self.require_armed()?;
                self.start_sequence(name, now, fx)
            }
            _ => Err(REJECT_NOT_STAND.to_string()),
        }
    }

    fn require_armed(&self) -> Result<(), String> {
        match self.mode {
            StandMode::Armed => Ok(()),
            StandMode::Safe => Err("stand is Safe; Arm first".into()),
            StandMode::Sequence => Err("sequence running; only Abort is accepted".into()),
        }
    }

    fn arm(&mut self, links: Links, fx: &mut Vec<Effect>) -> Result<(), String> {
        match self.mode {
            StandMode::Safe => {}
            StandMode::Armed => return Err("already Armed".into()),
            StandMode::Sequence => return Err("sequence running".into()),
        }
        let mut down = Vec::new();
        if !links.actuation {
            down.push("actuation");
        }
        if !links.loadcell {
            down.push("load-cell");
        }
        if !down.is_empty() {
            return Err(format!("{} Arduino link down", down.join(" and ")));
        }
        self.set_mode(StandMode::Armed, fx);
        Ok(())
    }

    fn start_sequence(&mut self, name: &str, now: f64, fx: &mut Vec<Effect>) -> Result<(), String> {
        let Some(seq) = self.rules.sequences.get(name) else {
            let known: Vec<&str> = self.rules.sequences.keys().map(String::as_str).collect();
            return Err(format!(
                "unknown sequence '{name}' (have: {})",
                known.join(", ")
            ));
        };
        let profile_pct = seq
            .mtv_profile
            .as_ref()
            .filter(|p| p.preposition)
            .map(|p| p.initial_percent());
        let duration = seq.duration_s;
        self.run = Some(Run {
            name: name.to_string(),
            t0: now,
            next_step: 0,
            profile_active: false,
            last_profile_pct: None,
        });
        self.set_mode(StandMode::Sequence, fx);
        fx.push(Effect::Event(
            Severity::Info,
            format!("sequence '{name}' T-0 ({duration:.1} s)"),
        ));
        // T-0 is the DAQ sync line going high (legacy `sync high` right before the profile).
        self.apply(&Action::Output { id: StandOutput::DaqSync, on: true }, now, fx);
        if let Some(pct) = profile_pct {
            self.apply(&Action::Mtv(pct), now, fx);
            fx.push(Effect::Event(
                Severity::Info,
                format!("T+0.0 MTV pre-positioned to {pct:.1} % (profile start)"),
            ));
        }
        Ok(())
    }

    /// Stop everything now. Accepted in every mode.
    pub fn abort(&mut self, now: f64, why: &str, fx: &mut Vec<Effect>) {
        if let Some(run) = self.run.take() {
            let t = (now - run.t0) * self.rules.time_scale;
            fx.push(Effect::Event(
                Severity::Critical,
                format!("ABORT ({why}): sequence '{}' stopped at T+{t:.1}", run.name),
            ));
        } else {
            fx.push(Effect::Event(Severity::Critical, format!("ABORT ({why})")));
        }
        // Igniter first, then everything else.
        self.apply(&Action::Output { id: StandOutput::Igniter, on: false }, now, fx);
        self.safing(now, fx);
        self.apply(&Action::Mtv(0.0), now, fx);
        self.apply(&Action::Output { id: StandOutput::DaqSync, on: false }, now, fx);
        self.set_mode(StandMode::Safe, fx);
    }

    /// The safing list from config, optionally preceded by the sketch's `reset all`.
    pub fn safing(&mut self, now: f64, fx: &mut Vec<Effect>) {
        fx.push(Effect::Event(Severity::Info, "safing list".into()));
        if self.rules.use_reset_all {
            fx.push(Effect::Serial(Role::Actuation, "reset all".into()));
            // What `reset all` does on the sketch: every valve LOW (closed), vents LOW (open),
            // OISO driven closed, KABOOM and SYNC LOW.
            for (id, _) in STAND_VALVES {
                let state = match id {
                    ValveId::OVnt | ValveId::PuVnt => ValveState::Open,
                    _ => ValveState::Closed,
                };
                self.valves.insert(*id, state);
            }
            self.oiso_cmd_at = Some(now);
            self.igniter_on = false;
            self.daq_sync_on = false;
        }
        let steps = self.rules.safing.clone();
        for a in &steps {
            self.apply(a, now, fx);
        }
    }

    /// Carry out one action: update commanded state and emit the wire command.
    fn apply(&mut self, action: &Action, now: f64, fx: &mut Vec<Effect>) {
        match action {
            Action::Valve { id, open } => {
                let Some(dev) = arduino_device(*id) else { return };
                self.valves.insert(
                    *id,
                    if *open { ValveState::Open } else { ValveState::Closed },
                );
                if *id == ValveId::OIso {
                    self.oiso_cmd_at = Some(now);
                }
                fx.push(Effect::Serial(
                    Role::Actuation,
                    format!("{dev} {}", if *open { "open" } else { "close" }),
                ));
            }
            Action::Output { id: StandOutput::Igniter, on } => {
                self.igniter_on = *on;
                fx.push(Effect::Serial(
                    Role::Actuation,
                    format!("kaboom {}", if *on { "start" } else { "end" }),
                ));
            }
            Action::Output { id: StandOutput::DaqSync, on } => {
                self.daq_sync_on = *on;
                fx.push(Effect::Serial(
                    Role::Actuation,
                    format!("sync {}", if *on { "high" } else { "low" }),
                ));
            }
            Action::Loadcell { name, begin } => {
                fx.push(Effect::Serial(
                    Role::Loadcell,
                    format!("{name} {}", if *begin { "begin" } else { "end" }),
                ));
            }
            Action::Mtv(p) => {
                self.mtv_percent = *p;
                fx.push(Effect::Mtv(*p));
            }
        }
    }

    // ----------------------------------------------------------------- time

    /// Advance the sequence engine.
    pub fn tick(&mut self, now: f64) -> Vec<Effect> {
        let mut fx = Vec::new();
        let Some(run) = self.run.as_ref() else { return fx };
        let name = run.name.clone();
        let t = ((now - run.t0) * self.rules.time_scale) as f32;
        let seq = self.rules.sequences[&name].clone();

        // Steps due.
        loop {
            let idx = self.run.as_ref().unwrap().next_step;
            let Some(step) = seq.steps.get(idx) else { break };
            if step.t > t {
                break;
            }
            self.run.as_mut().unwrap().next_step += 1;
            self.apply(&step.action, now, &mut fx);
            fx.push(Effect::Event(Severity::Info, step.describe()));
        }

        // Profile.
        if let Some(p) = &seq.mtv_profile {
            let run = self.run.as_mut().unwrap();
            match p.percent_at(t) {
                Some(pct) => {
                    if !run.profile_active {
                        run.profile_active = true;
                        fx.push(Effect::Event(
                            Severity::Info,
                            format!("T+{:.1} MTV profile start ({:.1} s)", p.start_t, p.duration()),
                        ));
                    }
                    if run.last_profile_pct.is_none_or(|last| (last - pct).abs() > 1e-3) {
                        run.last_profile_pct = Some(pct);
                        self.apply(&Action::Mtv(pct), now, &mut fx);
                    }
                }
                None if t >= p.end_t() && self.run.as_ref().unwrap().profile_active => {
                    let run = self.run.as_mut().unwrap();
                    run.profile_active = false;
                    let end = p.final_percent();
                    fx.push(Effect::Event(
                        Severity::Info,
                        format!("T+{:.1} MTV profile end, holding {end:.1} %", p.end_t()),
                    ));
                    if run.last_profile_pct != Some(end) {
                        run.last_profile_pct = Some(end);
                        self.apply(&Action::Mtv(end), now, &mut fx);
                    }
                }
                None => {}
            }
        }

        // Done?
        let run = self.run.as_ref().unwrap();
        if run.next_step >= seq.steps.len() && t >= seq.duration_s {
            self.run = None;
            fx.push(Effect::Event(
                Severity::Info,
                format!("sequence '{name}' complete at T+{t:.1}"),
            ));
            self.apply(&Action::Output { id: StandOutput::DaqSync, on: false }, now, &mut fx);
            self.set_mode(StandMode::Armed, &mut fx);
        }
        fx
    }

    // ----------------------------------------------------------------- link events

    /// The actuation sketch just (re)started: its outputs are at boot state and it forgot
    /// everything. Mode goes Safe; valve states are Unknown until commanded again.
    pub fn on_actuation_connected(&mut self, now: f64, run_safing: bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(run) = self.run.take() {
            fx.push(Effect::Event(
                Severity::Critical,
                format!("sequence '{}' lost: actuation Arduino reset", run.name),
            ));
        }
        self.forget_valve_states();
        self.igniter_on = false;
        self.daq_sync_on = false;
        fx.push(Effect::Event(
            Severity::Warning,
            "actuation Arduino connected: boot state has OVENT/PUVENT CLOSED and OISO driving \
             closed; valves Unknown until commanded (Disarm runs the safing list)"
                .into(),
        ));
        self.set_mode(StandMode::Safe, &mut fx);
        if run_safing {
            self.safing(now, &mut fx);
        }
        fx
    }

    pub fn on_actuation_lost(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(run) = self.run.take() {
            fx.push(Effect::Event(
                Severity::Critical,
                format!(
                    "sequence '{}' stopped: actuation Arduino link lost, outputs frozen at last state",
                    run.name
                ),
            ));
        } else {
            fx.push(Effect::Event(
                Severity::Critical,
                "actuation Arduino link lost; outputs frozen at last state".into(),
            ));
        }
        self.forget_valve_states();
        self.set_mode(StandMode::Safe, &mut fx);
        fx
    }

    pub fn on_loadcell_lost(&self) -> Vec<Effect> {
        vec![Effect::Event(
            Severity::Warning,
            "load-cell Arduino link lost".into(),
        )]
    }

    /// No uplink for longer than the configured timeout.
    pub fn on_ground_lost(
        &mut self,
        now: f64,
        armed: crate::config::OnLossArmed,
        sequence: crate::config::OnLossSequence,
    ) -> Vec<Effect> {
        use crate::config::{OnLossArmed, OnLossSequence};
        let mut fx = Vec::new();
        match self.mode {
            StandMode::Safe => fx.push(Effect::Event(
                Severity::Warning,
                "ground link lost (Safe, nothing to do)".into(),
            )),
            StandMode::Armed => match armed {
                OnLossArmed::Safe => {
                    fx.push(Effect::Event(
                        Severity::Warning,
                        "ground link lost while Armed: safing and going Safe".into(),
                    ));
                    self.safing(now, &mut fx);
                    self.set_mode(StandMode::Safe, &mut fx);
                }
                OnLossArmed::Log => fx.push(Effect::Event(
                    Severity::Warning,
                    "ground link lost while Armed (policy: log only)".into(),
                )),
            },
            StandMode::Sequence => match sequence {
                OnLossSequence::Log => fx.push(Effect::Event(
                    Severity::Critical,
                    "ground link lost during sequence: continuing (policy: log only)".into(),
                )),
                OnLossSequence::Abort => self.abort(now, "ground link lost", &mut fx),
            },
        }
        fx
    }

    // ----------------------------------------------------------------- reporting

    pub fn valve_state(&self, id: ValveId, now: f64) -> ValveState {
        if id == ValveId::OIso {
            if let Some(t) = self.oiso_cmd_at {
                if now - t < self.rules.oiso_travel_s {
                    return ValveState::Unknown;
                }
            }
        }
        self.valves.get(&id).copied().unwrap_or(ValveState::Unknown)
    }

    pub fn telemetry(&self, now: f64, time_s: f64, channels: Vec<(StandChannel, f32)>) -> StandTelemetry {
        let mut valves: Vec<ValveStatus> = STAND_VALVES
            .iter()
            .map(|(id, _)| ValveStatus {
                id: *id,
                state: self.valve_state(*id, now),
                position_deg: None,
            })
            .collect();
        valves.push(ValveStatus {
            id: ValveId::Mtv,
            state: if self.mtv_percent > 0.0 {
                ValveState::Open
            } else {
                ValveState::Closed
            },
            position_deg: Some(self.mtv_percent / 100.0 * 90.0),
        });
        let mut outputs_on = Vec::new();
        if self.igniter_on {
            outputs_on.push(StandOutput::Igniter);
        }
        if self.daq_sync_on {
            outputs_on.push(StandOutput::DaqSync);
        }
        StandTelemetry {
            time_s,
            source: Source::Stand,
            channels,
            valves,
            outputs_on,
            mtv_percent: Some(self.mtv_percent),
        }
    }

    pub fn status(&self, now: f64, time_s: f64, links: Links) -> StandStatus {
        let sequence = self.run.as_ref().map(|run| {
            let seq = &self.rules.sequences[&run.name];
            SequenceProgress {
                name: run.name.clone(),
                t_s: ((now - run.t0) * self.rules.time_scale) as f32,
                duration_s: seq.duration_s,
                next_step: seq
                    .steps
                    .get(run.next_step)
                    .map(|s| (run.next_step as u32, s.describe())),
                steps_total: seq.steps.len() as u32,
            }
        });
        StandStatus {
            time_s,
            mode: self.mode,
            actuation_link_ok: links.actuation,
            loadcell_link_ok: links.loadcell,
            sequences: self.rules.sequences.keys().cloned().collect(),
            sequence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, OnLossArmed, OnLossSequence};
    use crate::sequence::Sequence;

    fn test_sequence() -> Sequence {
        Sequence::parse(
            "t",
            r#"
[mtv_profile]
start_t = 1.0
profile = "hold-20-1 ramp-20-100-1"
[[step]]
t = 0.0
action = "valve omv open"
[[step]]
t = 0.5
action = "output igniter on"
[[step]]
t = 1.5
action = "output igniter off"
[[step]]
t = 2.0
action = "loadcell engine end"
"#,
        )
        .unwrap()
    }

    fn stand() -> Stand {
        let cfg = Config::default();
        let mut sequences = BTreeMap::new();
        sequences.insert("t".to_string(), test_sequence());
        Stand::new(Rules {
            safing: cfg.safing_actions().unwrap(),
            use_reset_all: true,
            oiso_travel_s: 21.0,
            time_scale: 1.0,
            sequences,
        })
    }

    fn armed() -> Stand {
        let mut s = stand();
        let (ack, _) = s.handle(&CommandKind::Stand(StandCommand::Arm), Links::both(), 0.0);
        assert_eq!(ack, AckResult::Accepted);
        s
    }

    fn in_sequence() -> Stand {
        let mut s = armed();
        let (ack, _) = s.handle(
            &CommandKind::Stand(StandCommand::StartSequence("t".into())),
            Links::both(),
            10.0,
        );
        assert_eq!(ack, AckResult::Accepted);
        assert_eq!(s.mode(), StandMode::Sequence);
        s
    }

    fn accepted(s: &mut Stand, k: CommandKind) -> Vec<Effect> {
        let (ack, fx) = s.handle(&k, Links::both(), 100.0);
        assert_eq!(ack, AckResult::Accepted, "{k:?}");
        fx
    }

    fn rejected(s: &mut Stand, k: CommandKind) -> String {
        let (ack, _) = s.handle(&k, Links::both(), 100.0);
        match ack {
            AckResult::Rejected(r) => r,
            AckResult::Accepted => panic!("{k:?} should be rejected"),
        }
    }

    fn serial_cmds(fx: &[Effect]) -> Vec<String> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::Serial(_, c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    }

    fn set_valve(id: ValveId, open: bool) -> CommandKind {
        CommandKind::SetValve { id, open }
    }
    fn mtv(p: f32) -> CommandKind {
        CommandKind::Stand(StandCommand::SetMtvPercent(p))
    }
    fn output(id: StandOutput, on: bool) -> CommandKind {
        CommandKind::Stand(StandCommand::SetOutput { id, on })
    }
    fn start(name: &str) -> CommandKind {
        CommandKind::Stand(StandCommand::StartSequence(name.into()))
    }
    const ARM: CommandKind = CommandKind::Stand(StandCommand::Arm);
    const DISARM: CommandKind = CommandKind::Stand(StandCommand::Disarm);
    const ABORT: CommandKind = CommandKind::Stand(StandCommand::Abort);

    // --- Arm: Safe and both links up

    #[test]
    fn arm_requires_safe_and_links() {
        let mut s = stand();
        let (ack, _) = s.handle(&ARM, Links { actuation: false, loadcell: true }, 0.0);
        assert!(matches!(ack, AckResult::Rejected(r) if r.contains("actuation")));
        let (ack, _) = s.handle(&ARM, Links { actuation: true, loadcell: false }, 0.0);
        assert!(matches!(ack, AckResult::Rejected(r) if r.contains("load-cell")));
        let (ack, _) = s.handle(&ARM, Links::default(), 0.0);
        assert!(matches!(ack, AckResult::Rejected(r) if r.contains("actuation and load-cell")));
        assert_eq!(s.mode(), StandMode::Safe);
        accepted(&mut s, ARM);
        assert_eq!(s.mode(), StandMode::Armed);
        rejected(&mut s, ARM);
        let mut s = in_sequence();
        rejected(&mut s, ARM);
    }

    // --- Disarm: Safe or Armed, runs safing

    #[test]
    fn disarm_runs_safing_in_safe_and_armed_not_in_sequence() {
        let mut s = stand();
        let fx = accepted(&mut s, DISARM);
        let cmds = serial_cmds(&fx);
        assert_eq!(cmds[0], "reset all");
        assert!(cmds.contains(&"omv close".to_string()));
        assert!(cmds.contains(&"ovent open".to_string()));
        assert!(cmds.contains(&"lfvent open".to_string()));
        assert!(fx.contains(&Effect::Mtv(0.0)));
        assert_eq!(s.mode(), StandMode::Safe);

        let mut s = armed();
        accepted(&mut s, DISARM);
        assert_eq!(s.mode(), StandMode::Safe);

        let mut s = in_sequence();
        let r = rejected(&mut s, DISARM);
        assert!(r.contains("Abort"), "{r}");
        assert_eq!(s.mode(), StandMode::Sequence);
    }

    // --- Abort: always

    #[test]
    fn abort_always_accepted_and_safes_everything() {
        for mut s in [stand(), armed(), in_sequence()] {
            let was_running = s.sequence_running();
            let fx = accepted(&mut s, ABORT);
            let cmds = serial_cmds(&fx);
            assert_eq!(cmds[0], "kaboom end", "igniter first");
            assert!(cmds.contains(&"reset all".to_string()));
            assert!(cmds.contains(&"omv close".to_string()));
            assert!(cmds.contains(&"igv close".to_string()));
            assert!(cmds.contains(&"ofill close".to_string()));
            assert!(cmds.contains(&"pumv close".to_string()));
            assert!(cmds.contains(&"puiso close".to_string()));
            assert!(cmds.contains(&"pufill close".to_string()));
            assert!(cmds.contains(&"ovent open".to_string()));
            assert!(cmds.contains(&"puvent open".to_string()));
            assert_eq!(cmds.last().unwrap(), "sync low");
            assert!(fx.contains(&Effect::Mtv(0.0)));
            assert!(fx.iter().any(|e| matches!(e, Effect::Event(Severity::Critical, t) if t.contains("ABORT"))));
            if was_running {
                assert!(fx.iter().any(|e| matches!(e, Effect::Event(_, t) if t.contains("stopped at T+"))));
            }
            assert_eq!(s.mode(), StandMode::Safe);
            assert!(!s.sequence_running());
            assert_eq!(s.mtv_percent(), 0.0);
        }
    }

    // --- SetValve / SetMtvPercent / SetOutput(Igniter): Armed only

    #[test]
    fn manual_outputs_need_armed() {
        for k in [
            set_valve(ValveId::Omv, true),
            mtv(50.0),
            output(StandOutput::Igniter, true),
        ] {
            let mut s = stand();
            assert!(rejected(&mut s, k.clone()).contains("Safe"));
            let mut s = in_sequence();
            assert!(rejected(&mut s, k.clone()).contains("Abort"));
            let mut s = armed();
            accepted(&mut s, k);
        }
    }

    #[test]
    fn set_valve_maps_to_arduino_and_tracks_state() {
        let mut s = armed();
        assert_eq!(s.valve_state(ValveId::Omv, 100.0), ValveState::Unknown);
        let fx = accepted(&mut s, set_valve(ValveId::Omv, true));
        assert_eq!(fx, vec![Effect::Serial(Role::Actuation, "omv open".into())]);
        assert_eq!(s.valve_state(ValveId::Omv, 100.0), ValveState::Open);
        let fx = accepted(&mut s, set_valve(ValveId::OVnt, false));
        assert_eq!(fx, vec![Effect::Serial(Role::Actuation, "ovent close".into())]);
        let fx = accepted(&mut s, set_valve(ValveId::Mtv, true));
        assert_eq!(fx, vec![Effect::Mtv(100.0)]);
        // Valves in the protocol that this stand cannot drive.
        for id in [ValveId::Rcs1, ValveId::Rcs2, ValveId::TVnt, ValveId::PuMvnt] {
            assert!(rejected(&mut s, set_valve(id, true)).contains("not on this stand"));
        }
    }

    #[test]
    fn mtv_percent_range_checked() {
        let mut s = armed();
        assert!(rejected(&mut s, mtv(100.1)).contains("outside"));
        assert!(rejected(&mut s, mtv(-0.1)).contains("outside"));
        assert!(rejected(&mut s, mtv(f32::NAN)).contains("outside"));
        let fx = accepted(&mut s, mtv(20.0));
        assert_eq!(fx, vec![Effect::Mtv(20.0)]);
        assert_eq!(s.mtv_percent(), 20.0);
    }

    #[test]
    fn igniter_maps_to_kaboom() {
        let mut s = armed();
        let fx = accepted(&mut s, output(StandOutput::Igniter, true));
        assert_eq!(fx, vec![Effect::Serial(Role::Actuation, "kaboom start".into())]);
        let tele = s.telemetry(100.0, 0.0, vec![]);
        assert_eq!(tele.outputs_on, vec![StandOutput::Igniter]);
        let fx = accepted(&mut s, output(StandOutput::Igniter, false));
        assert_eq!(fx, vec![Effect::Serial(Role::Actuation, "kaboom end".into())]);
    }

    // --- SetOutput(DaqSync): Safe or Armed

    #[test]
    fn daq_sync_safe_or_armed() {
        let mut s = stand();
        let fx = accepted(&mut s, output(StandOutput::DaqSync, true));
        assert_eq!(fx, vec![Effect::Serial(Role::Actuation, "sync high".into())]);
        let mut s = armed();
        accepted(&mut s, output(StandOutput::DaqSync, false));
        let mut s = in_sequence();
        rejected(&mut s, output(StandOutput::DaqSync, false));
    }

    // --- StartSequence: Armed; Sequence until last step or Abort

    #[test]
    fn start_sequence_needs_armed_and_known_name() {
        let mut s = stand();
        rejected(&mut s, start("t"));
        let mut s = armed();
        let r = rejected(&mut s, start("nope"));
        assert!(r.contains("nope") && r.contains("t"), "{r}");
        assert_eq!(s.mode(), StandMode::Armed);
        let mut s = in_sequence();
        rejected(&mut s, start("t"));
    }

    #[test]
    fn start_sequence_raises_sync_and_prepositions_mtv() {
        let mut s = armed();
        let fx = accepted(&mut s, start("t"));
        let cmds = serial_cmds(&fx);
        assert_eq!(cmds, vec!["sync high".to_string()]);
        assert!(fx.contains(&Effect::Mtv(20.0)));
        assert_eq!(s.mode(), StandMode::Sequence);
    }

    #[test]
    fn sequence_fires_steps_in_order_then_returns_to_armed() {
        let mut s = in_sequence(); // t0 = 10.0
        // T+0 step fires on the first tick.
        let fx = s.tick(10.0);
        assert!(serial_cmds(&fx).contains(&"omv open".to_string()));
        assert!(fx.iter().any(|e| matches!(e, Effect::Event(_, t) if t == "T+0.0 OMV open")));
        assert!(s.tick(10.3).is_empty());
        let fx = s.tick(10.5);
        assert_eq!(serial_cmds(&fx), vec!["kaboom start".to_string()]);
        // Profile: hold 20 from 1.0 to 2.0, then ramp to 100 by 3.0.
        let fx = s.tick(11.0);
        assert!(fx.contains(&Effect::Mtv(20.0)));
        let fx = s.tick(11.5);
        assert_eq!(serial_cmds(&fx), vec!["kaboom end".to_string()]);
        let fx = s.tick(12.0);
        assert_eq!(serial_cmds(&fx), vec!["engine end".to_string()]);
        let fx = s.tick(12.5);
        let m = fx.iter().find_map(|e| match e {
            Effect::Mtv(p) => Some(*p),
            _ => None,
        });
        assert!((m.unwrap() - 60.0).abs() < 0.01, "{m:?}");
        assert_eq!(s.mode(), StandMode::Sequence);
        let st = s.status(12.5, 0.0, Links::both());
        let prog = st.sequence.unwrap();
        assert_eq!(prog.next_step, None);
        assert_eq!(prog.steps_total, 4);
        assert!((prog.duration_s - 3.0).abs() < 1e-5);
        // Profile end at 3.0: hold 100, sequence complete, sync low, Armed.
        let fx = s.tick(13.0);
        assert!(fx.contains(&Effect::Mtv(100.0)));
        assert_eq!(serial_cmds(&fx), vec!["sync low".to_string()]);
        assert_eq!(s.mode(), StandMode::Armed);
        assert!(!s.sequence_running());
        assert_eq!(s.mtv_percent(), 100.0, "profile end value is held, not reset");
    }

    #[test]
    fn sequence_next_step_description() {
        let mut s = in_sequence();
        let st = s.status(10.2, 0.0, Links::both());
        let prog = st.sequence.unwrap();
        assert_eq!(prog.next_step, Some((0, "T+0.0 OMV open".into())));
        s.tick(10.2);
        let st = s.status(10.2, 0.0, Links::both());
        assert_eq!(st.sequence.unwrap().next_step, Some((1, "T+0.5 igniter on".into())));
    }

    #[test]
    fn time_scale_speeds_sequence_clock() {
        let mut s = stand();
        s.rules.time_scale = 10.0;
        accepted(&mut s, ARM);
        accepted(&mut s, start("t")); // t0 = 100
        let fx = s.tick(100.05); // T+0.5 scaled
        assert!(serial_cmds(&fx).contains(&"kaboom start".to_string()));
    }

    // --- safing list

    #[test]
    fn safing_list_drives_fail_states() {
        let mut s = armed();
        accepted(&mut s, set_valve(ValveId::Omv, true));
        accepted(&mut s, set_valve(ValveId::OVnt, false));
        accepted(&mut s, mtv(40.0));
        accepted(&mut s, output(StandOutput::Igniter, true));
        let mut fx = Vec::new();
        s.safing(200.0, &mut fx);
        for id in [ValveId::Omv, ValveId::IgV, ValveId::OFill, ValveId::PuMv, ValveId::PuIso, ValveId::PuFill] {
            assert_eq!(s.valve_state(id, 200.0), ValveState::Closed, "{id:?}");
        }
        for id in [ValveId::OVnt, ValveId::PuVnt, ValveId::LfVnt] {
            assert_eq!(s.valve_state(id, 200.0), ValveState::Open, "{id:?}");
        }
        assert_eq!(s.mtv_percent(), 0.0);
        assert!(s.telemetry(200.0, 0.0, vec![]).outputs_on.is_empty());
        // Order: igniter off is the first explicit step after reset all.
        let cmds = serial_cmds(&fx);
        assert_eq!(&cmds[..2], &["reset all".to_string(), "kaboom end".to_string()]);
    }

    #[test]
    fn safing_without_reset_all_uses_only_the_list() {
        let mut s = stand();
        s.rules.use_reset_all = false;
        let mut fx = Vec::new();
        s.safing(0.0, &mut fx);
        let cmds = serial_cmds(&fx);
        assert!(!cmds.contains(&"reset all".to_string()));
        assert_eq!(cmds.len(), 10, "{cmds:?}"); // 11 steps, one is mtv
    }

    // --- OISO Unknown window

    #[test]
    fn oiso_unknown_during_travel() {
        let mut s = armed();
        assert_eq!(s.valve_state(ValveId::OIso, 100.0), ValveState::Unknown);
        let (ack, _) = s.handle(&set_valve(ValveId::OIso, true), Links::both(), 100.0);
        assert_eq!(ack, AckResult::Accepted);
        assert_eq!(s.valve_state(ValveId::OIso, 100.0), ValveState::Unknown);
        assert_eq!(s.valve_state(ValveId::OIso, 120.9), ValveState::Unknown);
        assert_eq!(s.valve_state(ValveId::OIso, 121.0), ValveState::Open);
        // A new command restarts the window.
        let (_, _) = s.handle(&set_valve(ValveId::OIso, false), Links::both(), 130.0);
        assert_eq!(s.valve_state(ValveId::OIso, 150.0), ValveState::Unknown);
        assert_eq!(s.valve_state(ValveId::OIso, 151.5), ValveState::Closed);
        // Other valves are immediate.
        s.handle(&set_valve(ValveId::Omv, true), Links::both(), 160.0);
        assert_eq!(s.valve_state(ValveId::Omv, 160.0), ValveState::Open);
    }

    // --- link / ground events

    #[test]
    fn arduino_reconnect_goes_safe_and_forgets_valves() {
        let mut s = in_sequence();
        s.tick(10.0);
        assert_eq!(s.valve_state(ValveId::Omv, 10.0), ValveState::Open);
        let fx = s.on_actuation_connected(11.0, false);
        assert_eq!(s.mode(), StandMode::Safe);
        assert!(!s.sequence_running());
        assert_eq!(s.valve_state(ValveId::Omv, 11.0), ValveState::Unknown);
        assert!(serial_cmds(&fx).is_empty());
        assert!(fx.iter().any(|e| matches!(e, Effect::Event(Severity::Critical, _))));
        let fx = s.on_actuation_connected(12.0, true);
        assert!(serial_cmds(&fx).contains(&"reset all".to_string()));
    }

    #[test]
    fn actuation_loss_ends_sequence_and_goes_safe() {
        let mut s = in_sequence();
        let fx = s.on_actuation_lost();
        assert_eq!(s.mode(), StandMode::Safe);
        assert!(fx.iter().any(|e| matches!(e, Effect::Event(Severity::Critical, _))));
    }

    #[test]
    fn ground_loss_policies() {
        // Armed + Safe policy -> safing, Safe.
        let mut s = armed();
        let fx = s.on_ground_lost(50.0, OnLossArmed::Safe, OnLossSequence::Log);
        assert_eq!(s.mode(), StandMode::Safe);
        assert!(serial_cmds(&fx).contains(&"reset all".to_string()));
        // Armed + Log policy -> stays Armed.
        let mut s = armed();
        let fx = s.on_ground_lost(50.0, OnLossArmed::Log, OnLossSequence::Log);
        assert_eq!(s.mode(), StandMode::Armed);
        assert!(serial_cmds(&fx).is_empty());
        // Sequence + Log -> keeps running, Critical event.
        let mut s = in_sequence();
        let fx = s.on_ground_lost(11.0, OnLossArmed::Safe, OnLossSequence::Log);
        assert_eq!(s.mode(), StandMode::Sequence);
        assert!(s.sequence_running());
        assert!(fx.iter().any(|e| matches!(e, Effect::Event(Severity::Critical, _))));
        assert!(serial_cmds(&fx).is_empty());
        // Sequence + Abort -> aborted.
        let mut s = in_sequence();
        let fx = s.on_ground_lost(11.0, OnLossArmed::Safe, OnLossSequence::Abort);
        assert_eq!(s.mode(), StandMode::Safe);
        assert_eq!(serial_cmds(&fx)[0], "kaboom end");
        // Safe -> only a warning.
        let mut s = stand();
        let fx = s.on_ground_lost(1.0, OnLossArmed::Safe, OnLossSequence::Abort);
        assert!(serial_cmds(&fx).is_empty());
    }

    #[test]
    fn vehicle_commands_are_rejected() {
        let mut s = armed();
        for k in [
            CommandKind::Arm,
            CommandKind::Disarm,
            CommandKind::Launch,
            CommandKind::Abort,
            CommandKind::RequestParams,
        ] {
            assert_eq!(rejected(&mut s, k), REJECT_NOT_STAND);
        }
        assert_eq!(s.mode(), StandMode::Armed);
    }

    #[test]
    fn telemetry_shape() {
        let s = stand();
        let t = s.telemetry(0.0, 1.0, vec![(StandChannel::Thrust, 5.0)]);
        assert_eq!(t.source, Source::Stand);
        assert_eq!(t.valves.len(), STAND_VALVES.len() + 1);
        assert!(t.valves.iter().all(|v| v.id == ValveId::Mtv || v.state == ValveState::Unknown));
        assert_eq!(t.mtv_percent, Some(0.0));
        let st = s.status(0.0, 1.0, Links::both());
        assert_eq!(st.sequences, vec!["t".to_string()]);
        assert!(st.sequence.is_none());
    }
}
