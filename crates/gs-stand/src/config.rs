//! `stand/config.toml`. Every constant lifted from the legacy Python/Arduino code lives here
//! with a default that matches it, so the TOML only needs to state what differs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use gs_protocol::StandChannel;
use serde::Deserialize;

use crate::sequence::{parse_action, Action};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub udp: UdpConfig,
    pub serial: SerialConfig,
    pub loadcells: LoadcellsConfig,
    pub valves: ValvesConfig,
    pub safing: SafingConfig,
    pub ground_link: GroundLinkConfig,
    pub mtv: MtvConfig,
    pub logging: LoggingConfig,
    pub sequences: SequencesConfig,
}


#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UdpConfig {
    /// Port gs-stand binds for uplink. 8889 rather than the vehicle's 8888 so a stand and a
    /// vehicle/sim can share one host; the bridge is pointed here with `--stand host:8889`.
    pub port: u16,
}
impl Default for UdpConfig {
    fn default() -> Self {
        Self { port: 8889 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SerialConfig {
    /// Device path, or "auto" to scan `/dev/ttyACM*`, `/dev/stand-*`, `/dev/cu.usbmodem*`.
    /// Roles are always confirmed by behaviour (only the actuation board answers `sync status`),
    /// so a swapped pair is corrected with a warning.
    pub actuation_port: String,
    pub loadcell_port: String,
    pub baud: u32,
    /// The sketch frames commands by a 10 ms silence (`Serial.setTimeout(10)` + `readString()`)
    /// and drains its input after each command, so back-to-back writes merge or vanish.
    pub min_command_gap_ms: u64,
    /// How long to wait for the sketch's `connected` banner after opening (DTR reset).
    pub connect_timeout_s: f64,
    /// How long to wait for `SYNC is N` after the identifying `sync status` probe.
    pub probe_timeout_s: f64,
    /// How long to wait for `<name> loadcell ready` after `<name> setup` (tare included).
    pub loadcell_setup_timeout_s: f64,
    pub reconnect_interval_s: f64,
    /// Liveness probe (`sync status`) period for the actuation board; 0 disables.
    pub probe_interval_s: f64,
    /// Probes are skipped while a sequence runs so they never delay a step.
    pub probe_during_sequence: bool,
    /// Actuation link is declared down after this many missed probe replies.
    pub probe_misses_for_loss: u32,
}
impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            actuation_port: "auto".into(),
            loadcell_port: "auto".into(),
            baud: 115_200,
            min_command_gap_ms: 30,
            connect_timeout_s: 4.0,
            probe_timeout_s: 1.5,
            loadcell_setup_timeout_s: 6.0,
            reconnect_interval_s: 2.0,
            probe_interval_s: 1.0,
            probe_during_sequence: false,
            probe_misses_for_loss: 3,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoadcellsConfig {
    pub cell: Vec<LoadcellConfig>,
    /// Drop a channel from telemetry if no sample arrived for this long.
    pub stale_after_s: f64,
}
impl Default for LoadcellsConfig {
    fn default() -> Self {
        Self {
            cell: vec![
                LoadcellConfig::new("engine", StandChannel::Thrust, true),
                LoadcellConfig::new("nitrous", StandChannel::NitrousMass, true),
                LoadcellConfig::new("rcs", StandChannel::RcsThrust, false),
            ],
            stale_after_s: 2.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadcellConfig {
    /// Arduino device name: `engine`, `nitrous` or `rcs`.
    pub name: String,
    pub channel: StandChannel,
    /// value_out = raw * scale + offset. The sketch's HX711 `set_scale` constants
    /// (1300 / 2120 / 7550) give units nobody documented; calibrate here into N / kg.
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default)]
    pub offset: f32,
    /// Send `<name> setup` (HX711 begin + tare) when the load-cell board connects.
    /// The sketch blocks forever in `setup` if that cell is not wired, so keep this off
    /// for cells that are not on the stand today.
    #[serde(default)]
    pub setup_on_connect: bool,
}
fn one() -> f32 {
    1.0
}
impl LoadcellConfig {
    fn new(name: &str, channel: StandChannel, setup: bool) -> Self {
        Self {
            name: name.into(),
            channel,
            scale: 1.0,
            offset: 0.0,
            setup_on_connect: setup,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValvesConfig {
    /// OISO is a motorized ball valve; hotfire.txt says ~21 s per stroke.
    pub oiso_travel_s: f64,
    /// De-energized state of each valve, for the record. From the sketch: OVENT and PUVENT
    /// are `HIGH == closed` (normally open); every other valve is `HIGH == open`.
    pub fail_state: BTreeMap<String, String>,
}
impl Default for ValvesConfig {
    fn default() -> Self {
        let mut fail_state = BTreeMap::new();
        for v in [
            "omv", "igv", "ofill", "pumv", "pufill", "puiso", "lfvnt",
        ] {
            fail_state.insert(v.to_string(), "closed".to_string());
        }
        fail_state.insert("ovnt".into(), "open".into());
        fail_state.insert("puvnt".into(), "open".into());
        fail_state.insert("oiso".into(), "holds".into());
        Self {
            oiso_travel_s: 21.0,
            fail_state,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SafingConfig {
    /// Send the sketch's `reset all` first: one command that drops every output, igniter and
    /// DAQ sync included, and opens OVENT/PUVENT. The explicit list follows for anything it
    /// does not cover (LFVENT open) and so the log shows each valve.
    pub use_reset_all: bool,
    /// Run the safing list when the actuation board (re)connects. Off by default: the
    /// Arduino's own reset just closed the vents and started closing OISO, and the operator
    /// may not want vents dumped by a software restart. Press Disarm to safe explicitly.
    pub on_connect: bool,
    /// Actions in sequence vocabulary. Order matters: energized things first.
    pub steps: Vec<String>,
}
impl Default for SafingConfig {
    fn default() -> Self {
        Self {
            use_reset_all: true,
            on_connect: false,
            steps: [
                "output igniter off",
                "valve omv close",
                "valve igv close",
                "valve ofill close",
                "valve pumv close",
                "valve puiso close",
                "valve pufill close",
                "mtv 0",
                "valve ovnt open",
                "valve puvnt open",
                "valve lfvnt open",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnLossArmed {
    /// Run the safing list and go Safe.
    Safe,
    /// Log a Warning only.
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnLossSequence {
    /// Log a Critical event and let the sequence finish (a hotfire must not be killed by WiFi).
    Log,
    /// Treat like Abort.
    Abort,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GroundLinkConfig {
    /// Seconds without any uplink packet (bridge heartbeats at 2 Hz) before acting.
    pub timeout_s: f64,
    pub armed_on_loss: OnLossArmed,
    pub sequence_on_loss: OnLossSequence,
}
impl Default for GroundLinkConfig {
    fn default() -> Self {
        Self {
            timeout_s: 3.0,
            armed_on_loss: OnLossArmed::Safe,
            sequence_on_loss: OnLossSequence::Log,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MtvBackendKind {
    /// `python` on Linux, `none` elsewhere.
    Auto,
    /// Spawns `stand/mtv_helper.py`, which reuses the legacy `MTV`/`Servo` classes (Jetson.GPIO).
    Python,
    /// Linux `/sys/class/pwm` directly.
    Sysfs,
    /// Log only.
    None,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MtvConfig {
    pub backend: MtvBackendKind,
    /// Servo degrees at valve closed, before the gear ratio (`MTV(close_angle=44)`).
    pub close_angle_deg: f32,
    /// Servo turns per valve turn (`MTV(gear_ratio=2)`).
    pub gear_ratio: f32,
    /// Degrees the servo spans between `pulse_min_us` and `pulse_max_us` (`servo_range=355`).
    pub servo_range_deg: f32,
    /// `Servo(hz=333, low=500, high=2500)`.
    pub pwm_hz: f32,
    pub pulse_min_us: f32,
    pub pulse_max_us: f32,
    /// Valve degrees at 100 % (`percent_to_angle(angle_limit=90)`).
    pub full_open_deg: f32,
    /// Added to servo 2 (`servo2_offset`, 0 in every legacy script).
    pub servo2_offset_deg: f32,
    /// Startup re-home: legacy commands valve angle -40 (past the closed stop) for 3 s, then
    /// the start angle. We go -40, hold, then 0 %.
    pub home_valve_deg: f32,
    pub home_hold_s: f64,
    pub sysfs: MtvSysfsConfig,
    pub python: MtvPythonConfig,
}
impl Default for MtvConfig {
    fn default() -> Self {
        Self {
            backend: MtvBackendKind::Auto,
            close_angle_deg: 44.0,
            gear_ratio: 2.0,
            servo_range_deg: 355.0,
            pwm_hz: 333.0,
            pulse_min_us: 500.0,
            pulse_max_us: 2500.0,
            full_open_deg: 90.0,
            servo2_offset_deg: 0.0,
            home_valve_deg: -40.0,
            home_hold_s: 3.0,
            sysfs: MtvSysfsConfig::default(),
            python: MtvPythonConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MtvSysfsConfig {
    /// `/sys/class/pwm/pwmchip<chip>/pwm<channel>` for each servo. Board specific; see README.
    pub servo1: PwmChannel,
    pub servo2: PwmChannel,
}
impl Default for MtvSysfsConfig {
    fn default() -> Self {
        Self {
            servo1: PwmChannel {
                chip: 3,
                channel: 0,
            },
            servo2: PwmChannel {
                chip: 1,
                channel: 0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PwmChannel {
    pub chip: u32,
    pub channel: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MtvPythonConfig {
    pub python: String,
    /// Relative paths resolve against the config file's directory's parent (the repo root).
    pub helper: PathBuf,
    /// Directory holding the legacy `mtv_module.py` / `mtv_servo.py`.
    pub legacy_dir: PathBuf,
    /// Jetson BOARD pin numbers (`Procedure(pin1=15, pin2=33)`).
    pub pin1: u32,
    pub pin2: u32,
}
impl Default for MtvPythonConfig {
    fn default() -> Self {
        Self {
            python: "python3".into(),
            helper: "stand/mtv_helper.py".into(),
            legacy_dir: "jetson stuff/jetson/mtv".into(),
            pin1: 15,
            pin2: 33,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Per-run CSV goes to `<dir>/stand-<utc>.csv`; never overwritten.
    pub dir: PathBuf,
}
impl Default for LoggingConfig {
    fn default() -> Self {
        Self { dir: "logs".into() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SequencesConfig {
    pub dir: PathBuf,
}
impl Default for SequencesConfig {
    fn default() -> Self {
        Self {
            dir: "stand/sequences".into(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        self.safing_actions()?;
        if self.mtv.gear_ratio <= 0.0 || self.mtv.servo_range_deg <= 0.0 || self.mtv.pwm_hz <= 0.0 {
            bail!("mtv geometry must be positive");
        }
        if self.mtv.pulse_max_us <= self.mtv.pulse_min_us {
            bail!("mtv.pulse_max_us must exceed pulse_min_us");
        }
        for c in &self.loadcells.cell {
            if !matches!(c.name.as_str(), "engine" | "nitrous" | "rcs") {
                bail!(
                    "loadcell '{}' is not a device the sketch knows (engine|nitrous|rcs)",
                    c.name
                );
            }
        }
        if self.serial.min_command_gap_ms < 15 {
            bail!("serial.min_command_gap_ms below 15 will merge commands on the Arduino");
        }
        Ok(())
    }

    /// The safing list, parsed. Fails on any unknown action so a typo is caught at startup.
    pub fn safing_actions(&self) -> Result<Vec<Action>> {
        self.safing
            .steps
            .iter()
            .map(|s| parse_action(s).with_context(|| format!("safing step '{s}'")))
            .collect()
    }

    /// Resolve a path from the config relative to `root` (the directory containing `stand/`).
    pub fn resolve(root: &Path, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let cfg = Config::default();
        cfg.validate().unwrap();
        assert_eq!(cfg.safing_actions().unwrap().len(), 11);
    }

    #[test]
    fn empty_toml_is_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.udp.port, 8889);
        assert_eq!(cfg.loadcells.cell.len(), 3);
    }

    #[test]
    fn rejects_bad_safing_step() {
        let cfg: Config = toml::from_str("[safing]\nsteps = [\"valve nope open\"]").unwrap();
        assert!(cfg.validate().is_err());
    }
}
