//! Fake propulsion / test-stand sensors and valve states, driven by what the engine is doing.

use gs_protocol::{
    Source, StandChannel, StandTelemetry, ValveId, ValveState, ValveStatus,
};

use crate::physics::{Actuation, Noise, MAX_THRUST_N};

const ALL_VALVES: [ValveId; 15] = [
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

const AMBIENT_C: f32 = 25.0;
const TANK_FULL_BAR: f32 = 55.0;
const TANK_EMPTY_BAR: f32 = 30.0;
const CHAMBER_BAR_PER_N: f32 = 40.0 / 1000.0;
const MTV_FULL_OPEN_DEG: f32 = 90.0;

pub struct Stand {
    /// Valve states as commanded by `SetValve`, starting from the normal positions.
    commanded_open: Vec<(ValveId, bool)>,
    chamber_temp_c: f32,
    nozzle_temp_c: f32,
}

impl Stand {
    pub fn new() -> Self {
        Self {
            commanded_open: ALL_VALVES
                .iter()
                .map(|&id| (id, is_normally_open(id)))
                .collect(),
            chamber_temp_c: AMBIENT_C,
            nozzle_temp_c: AMBIENT_C,
        }
    }

    pub fn set_valve(&mut self, id: ValveId, open: bool) {
        if let Some(entry) = self.commanded_open.iter_mut().find(|(v, _)| *v == id) {
            entry.1 = open;
        }
    }

    /// Thermocouples lag the engine: they warm while firing and cool back to ambient.
    pub fn step(&mut self, thrust: f64, dt: f64) {
        let throttle = (thrust / MAX_THRUST_N) as f32;
        let dt = dt as f32;
        let approach = |temp: &mut f32, hot_c: f32, tau_heat_s: f32, tau_cool_s: f32| {
            let (target, tau) = if throttle > 0.0 {
                (AMBIENT_C + (hot_c - AMBIENT_C) * throttle, tau_heat_s)
            } else {
                (AMBIENT_C, tau_cool_s)
            };
            *temp += (target - *temp) * dt / tau;
        };
        approach(&mut self.chamber_temp_c, 750.0, 8.0, 30.0);
        approach(&mut self.nozzle_temp_c, 450.0, 15.0, 45.0);
    }

    pub fn tank_pressure_bar(&self, propellant_used_fraction: f64) -> f32 {
        TANK_FULL_BAR - (TANK_FULL_BAR - TANK_EMPTY_BAR) * propellant_used_fraction as f32
    }

    pub fn chamber_pressure_bar(&self, thrust: f64) -> f32 {
        thrust as f32 * CHAMBER_BAR_PER_N
    }

    pub fn telemetry(
        &self,
        time_s: f64,
        actuation: &Actuation,
        propellant_used_fraction: f64,
        noise: &mut Noise,
    ) -> StandTelemetry {
        let firing = actuation.thrust > 0.0;
        let tank = self.tank_pressure_bar(propellant_used_fraction);
        let chamber = self.chamber_pressure_bar(actuation.thrust);
        let main_valve_open = firing || self.is_commanded_open(ValveId::Omv);

        let mut channels = vec![
            (StandChannel::Opt, tank),
            (StandChannel::Ipt, chamber * 1.2),
            (StandChannel::Ept, chamber),
            // M1 sits between the main valve and the throttle valve, M2 after the throttle.
            (StandChannel::M1, if main_valve_open { tank * 0.97 } else { 0.0 }),
            (StandChannel::M2, chamber * 1.35),
            (StandChannel::Pupt, 180.0 - 20.0 * propellant_used_fraction as f32),
            (StandChannel::Lfpt, 6.0),
            (StandChannel::T1, self.chamber_temp_c),
            (StandChannel::T2, self.nozzle_temp_c),
            (StandChannel::Thrust, actuation.thrust as f32),
        ];
        for (channel, value) in &mut channels {
            let amplitude = match channel {
                StandChannel::T1 | StandChannel::T2 => 0.5,
                StandChannel::Thrust => 2.0,
                _ => 0.05,
            };
            *value += noise.sample(amplitude) as f32;
        }

        let valves = self
            .commanded_open
            .iter()
            .map(|&(id, commanded)| {
                let open = commanded
                    || match id {
                        ValveId::Omv | ValveId::Mtv => firing,
                        ValveId::Rcs1 => actuation.rcs > 0,
                        ValveId::Rcs2 => actuation.rcs < 0,
                        _ => false,
                    };
                let state = if open { ValveState::Open } else { ValveState::Closed };
                let position_deg =
                    (id == ValveId::Mtv).then(|| mtv_position_deg(actuation, commanded));
                ValveStatus { id, state, position_deg }
            })
            .collect();

        StandTelemetry {
            outputs_on: Vec::new(),
            mtv_percent: None,
            time_s,
            source: Source::Sim,
            channels,
            valves,
        }
    }

    fn is_commanded_open(&self, id: ValveId) -> bool {
        self.commanded_open
            .iter()
            .any(|&(v, open)| v == id && open)
    }
}

/// The throttle valve follows thrust while firing; a manual open drives it fully open.
fn mtv_position_deg(actuation: &Actuation, commanded_open: bool) -> f32 {
    if actuation.thrust > 0.0 {
        MTV_FULL_OPEN_DEG * (actuation.thrust / MAX_THRUST_N) as f32
    } else if commanded_open {
        MTV_FULL_OPEN_DEG
    } else {
        0.0
    }
}

fn is_normally_open(id: ValveId) -> bool {
    matches!(
        id,
        ValveId::OVnt | ValveId::PuVnt | ValveId::PuMvnt | ValveId::LfVnt | ValveId::TVnt
    )
}
