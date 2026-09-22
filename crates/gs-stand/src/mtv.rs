//! MTV throttle: two ganged servos on Jetson PWM. Geometry from `mtv_module.py` /
//! `mtv_servo.py`; three backends so the same binary runs on a laptop, on the Jetson with
//! the proven Jetson.GPIO code, or on the Jetson with raw sysfs PWM.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::config::{MtvBackendKind, MtvConfig, PwmChannel};

/// percent -> valve angle -> servo angle -> pulse width, exactly as the legacy classes do it.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub close_angle_deg: f32,
    pub gear_ratio: f32,
    pub servo_range_deg: f32,
    pub pwm_hz: f32,
    pub pulse_min_us: f32,
    pub pulse_max_us: f32,
    pub full_open_deg: f32,
    pub servo2_offset_deg: f32,
}

impl From<&MtvConfig> for Geometry {
    fn from(c: &MtvConfig) -> Self {
        Self {
            close_angle_deg: c.close_angle_deg,
            gear_ratio: c.gear_ratio,
            servo_range_deg: c.servo_range_deg,
            pwm_hz: c.pwm_hz,
            pulse_min_us: c.pulse_min_us,
            pulse_max_us: c.pulse_max_us,
            full_open_deg: c.full_open_deg,
            servo2_offset_deg: c.servo2_offset_deg,
        }
    }
}

impl Geometry {
    /// `percent_to_angle`: percent / 100 * angle_limit.
    pub fn percent_to_valve_deg(&self, percent: f32) -> f32 {
        percent / 100.0 * self.full_open_deg
    }

    /// `MTV.command`: servo = close_angle * gear + angle * gear (+ offset on servo 2).
    pub fn valve_to_servo_deg(&self, valve_deg: f32, servo2: bool) -> f32 {
        let base = (self.close_angle_deg + valve_deg) * self.gear_ratio;
        if servo2 {
            base + self.servo2_offset_deg
        } else {
            base
        }
    }

    /// `Servo._get_pwm` without the duty-cycle conversion: clip to [0, range], linear map.
    pub fn servo_deg_to_pulse_us(&self, servo_deg: f32) -> f32 {
        let a = servo_deg.clamp(0.0, self.servo_range_deg);
        self.pulse_min_us + (self.pulse_max_us - self.pulse_min_us) * (a / self.servo_range_deg)
    }

    pub fn period_ns(&self) -> u64 {
        (1e9 / self.pwm_hz as f64).round() as u64
    }

    /// Per-servo pulse widths for a valve angle (may be negative for homing).
    pub fn pulses_us(&self, valve_deg: f32) -> [f32; 2] {
        [
            self.servo_deg_to_pulse_us(self.valve_to_servo_deg(valve_deg, false)),
            self.servo_deg_to_pulse_us(self.valve_to_servo_deg(valve_deg, true)),
        ]
    }
}

pub trait MtvBackend: Send {
    fn name(&self) -> &'static str;
    /// Drive the valve to this angle (degrees, 0 = closed; negative allowed for re-homing).
    fn set_valve_deg(&mut self, valve_deg: f32) -> Result<()>;
    /// Called every tick so a backend can notice a dead helper.
    fn health(&mut self) -> Result<()> {
        Ok(())
    }
}

pub struct Mtv {
    geom: Geometry,
    backend: Box<dyn MtvBackend>,
}

impl Mtv {
    pub fn new(geom: Geometry, backend: Box<dyn MtvBackend>) -> Self {
        Self { geom, backend }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn set_percent(&mut self, percent: f32) -> Result<()> {
        self.backend
            .set_valve_deg(self.geom.percent_to_valve_deg(percent))
    }

    pub fn set_valve_deg(&mut self, deg: f32) -> Result<()> {
        self.backend.set_valve_deg(deg)
    }

    pub fn health(&mut self) -> Result<()> {
        self.backend.health()
    }

    /// Legacy startup: drive past the closed stop (-40) for a few seconds, then closed.
    pub fn home(&mut self, home_valve_deg: f32, hold_s: f64) -> Result<()> {
        info!(
            "MTV homing via {}: {home_valve_deg}° for {hold_s} s, then 0 %",
            self.backend.name()
        );
        self.set_valve_deg(home_valve_deg)?;
        std::thread::sleep(std::time::Duration::from_secs_f64(hold_s));
        self.set_percent(0.0)
    }
}

// ---------------------------------------------------------------------------

/// Logs only.
pub struct NoneBackend {
    geom: Geometry,
}

impl MtvBackend for NoneBackend {
    fn name(&self) -> &'static str {
        "none"
    }
    fn set_valve_deg(&mut self, valve_deg: f32) -> Result<()> {
        let p = self.geom.pulses_us(valve_deg);
        info!(
            "MTV (no backend) valve {valve_deg:.1}° -> servo1 {:.0} us, servo2 {:.0} us",
            p[0], p[1]
        );
        Ok(())
    }
}

/// Hands the commanded percent to a callback — used by `--fake-arduino` so the fake load
/// cells react to the throttle.
pub struct CallbackBackend {
    pub geom: Geometry,
    pub on_percent: Box<dyn FnMut(f32) + Send>,
}

impl MtvBackend for CallbackBackend {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn set_valve_deg(&mut self, valve_deg: f32) -> Result<()> {
        let pct = (valve_deg / self.geom.full_open_deg * 100.0).clamp(0.0, 100.0);
        (self.on_percent)(pct);
        Ok(())
    }
}

// ---------------------------------------------------------------------------

/// Linux `/sys/class/pwm/pwmchipN/pwmM`.
pub struct SysfsBackend {
    geom: Geometry,
    chans: [PathBuf; 2],
}

impl SysfsBackend {
    pub fn new(geom: Geometry, servo1: PwmChannel, servo2: PwmChannel) -> Result<Self> {
        let mut chans = Vec::new();
        for ch in [servo1, servo2] {
            let chip = PathBuf::from(format!("/sys/class/pwm/pwmchip{}", ch.chip));
            if !chip.exists() {
                bail!("{} does not exist (see README for finding the PWM chip)", chip.display());
            }
            let pwm = chip.join(format!("pwm{}", ch.channel));
            if !pwm.exists() {
                std::fs::write(chip.join("export"), ch.channel.to_string())
                    .with_context(|| format!("exporting pwm{} on {}", ch.channel, chip.display()))?;
                // udev may take a moment to make the new node writable.
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let period = geom.period_ns();
            // Setting a period smaller than the current duty fails, so zero duty first.
            let _ = std::fs::write(pwm.join("duty_cycle"), "0");
            std::fs::write(pwm.join("period"), period.to_string())
                .with_context(|| format!("setting period on {}", pwm.display()))?;
            std::fs::write(pwm.join("enable"), "1")
                .with_context(|| format!("enabling {}", pwm.display()))?;
            chans.push(pwm);
        }
        Ok(Self {
            geom,
            chans: [chans.remove(0), chans.remove(0)],
        })
    }
}

impl MtvBackend for SysfsBackend {
    fn name(&self) -> &'static str {
        "sysfs"
    }
    fn set_valve_deg(&mut self, valve_deg: f32) -> Result<()> {
        let pulses = self.geom.pulses_us(valve_deg);
        for (path, us) in self.chans.iter().zip(pulses) {
            let ns = (us as f64 * 1000.0).round() as u64;
            std::fs::write(path.join("duty_cycle"), ns.to_string())
                .with_context(|| format!("writing duty_cycle on {}", path.display()))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------

/// Spawns `stand/mtv_helper.py`, which imports the legacy `MTV` class and reads
/// `"<percent>\n"` lines. Percent may be negative (homing), like `MTV.command(-40)`.
pub struct PythonBackend {
    geom: Geometry,
    child: Child,
    stdin: std::process::ChildStdin,
}

impl PythonBackend {
    pub fn new(
        geom: Geometry,
        python: &str,
        helper: &Path,
        legacy_dir: &Path,
        pin1: u32,
        pin2: u32,
    ) -> Result<Self> {
        if !helper.exists() {
            bail!("MTV helper {} not found", helper.display());
        }
        let mut child = Command::new(python)
            .arg(helper)
            .arg("--legacy-dir")
            .arg(legacy_dir)
            .arg("--pin1")
            .arg(pin1.to_string())
            .arg("--pin2")
            .arg(pin2.to_string())
            .arg("--close-angle")
            .arg(geom.close_angle_deg.to_string())
            .arg("--gear-ratio")
            .arg(geom.gear_ratio.to_string())
            .arg("--servo2-offset")
            .arg(geom.servo2_offset_deg.to_string())
            .arg("--servo-range")
            .arg(geom.servo_range_deg.to_string())
            .arg("--full-open")
            .arg(geom.full_open_deg.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("spawning {python} {}", helper.display()))?;
        let stdin = child.stdin.take().context("helper stdin")?;
        // Give Jetson.GPIO time to set the pins up before the first command.
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let mut b = Self { geom, child, stdin };
        b.health()?;
        Ok(b)
    }
}

impl MtvBackend for PythonBackend {
    fn name(&self) -> &'static str {
        "python"
    }
    fn set_valve_deg(&mut self, valve_deg: f32) -> Result<()> {
        let pct = valve_deg / self.geom.full_open_deg * 100.0;
        writeln!(self.stdin, "{pct:.3}").context("writing to MTV helper")?;
        self.stdin.flush().context("flushing MTV helper")
    }
    fn health(&mut self) -> Result<()> {
        match self.child.try_wait() {
            Ok(Some(status)) => bail!("MTV helper exited: {status}"),
            Ok(None) => Ok(()),
            Err(e) => bail!("MTV helper: {e}"),
        }
    }
}

impl Drop for PythonBackend {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.stdin.flush();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------

/// Build the configured backend; `root` resolves relative paths.
pub fn build(cfg: &MtvConfig, root: &Path) -> Result<Mtv> {
    let geom = Geometry::from(cfg);
    let kind = match cfg.backend {
        MtvBackendKind::Auto => {
            if cfg!(target_os = "linux") {
                MtvBackendKind::Python
            } else {
                MtvBackendKind::None
            }
        }
        k => k,
    };
    let backend: Box<dyn MtvBackend> = match kind {
        MtvBackendKind::None | MtvBackendKind::Auto => Box::new(NoneBackend { geom }),
        MtvBackendKind::Sysfs => Box::new(SysfsBackend::new(geom, cfg.sysfs.servo1, cfg.sysfs.servo2)?),
        MtvBackendKind::Python => {
            let helper = crate::config::Config::resolve(root, &cfg.python.helper);
            let legacy = crate::config::Config::resolve(root, &cfg.python.legacy_dir);
            if !legacy.exists() {
                warn!("legacy MTV dir {} not found; helper import will fail", legacy.display());
            }
            Box::new(PythonBackend::new(
                geom,
                &cfg.python.python,
                &helper,
                &legacy,
                cfg.python.pin1,
                cfg.python.pin2,
            )?)
        }
    };
    Ok(Mtv::new(geom, backend))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom() -> Geometry {
        Geometry::from(&MtvConfig::default())
    }

    #[test]
    fn legacy_numbers_reproduce() {
        let g = geom();
        // percent_to_angle(20) = 18°
        assert_eq!(g.percent_to_valve_deg(20.0), 18.0);
        // MTV.command(18): (44 + 18) * 2 = 124° servo
        assert_eq!(g.valve_to_servo_deg(18.0, false), 124.0);
        // Servo._get_pwm(124) with range 355: 500 + 2000 * 124/355 = 1198.59 us
        let us = g.servo_deg_to_pulse_us(124.0);
        assert!((us - 1198.59).abs() < 0.01, "{us}");
        // Duty % as legacy computes it: us / (10000/333) = 39.91 %
        let duty_pct = us / (10000.0 / 333.0);
        assert!((duty_pct - 39.91).abs() < 0.01, "{duty_pct}");
        // Closed: 88° servo -> 995.77 us. Full open: 268° -> 2009.86 us.
        assert!((g.pulses_us(0.0)[0] - 995.77).abs() < 0.01);
        assert!((g.pulses_us(90.0)[0] - 2009.86).abs() < 0.01);
        // Homing at -40: servo 8° -> 545.07 us; clipped at 0 below -44.
        assert!((g.pulses_us(-40.0)[0] - 545.07).abs() < 0.01);
        assert_eq!(g.pulses_us(-60.0)[0], 500.0);
        // 333 Hz period.
        assert_eq!(g.period_ns(), 3_003_003);
    }

    #[test]
    fn servo2_offset_applies_to_second_only() {
        let mut g = geom();
        g.servo2_offset_deg = 5.0;
        assert_eq!(g.valve_to_servo_deg(0.0, false), 88.0);
        assert_eq!(g.valve_to_servo_deg(0.0, true), 93.0);
    }

    #[test]
    fn callback_backend_clamps_percent() {
        use std::sync::{Arc, Mutex};
        let target = Arc::new(Mutex::new(50.0f32));
        let t2 = target.clone();
        let mut m = Mtv::new(
            geom(),
            Box::new(CallbackBackend {
                geom: geom(),
                on_percent: Box::new(move |p| *t2.lock().unwrap() = p),
            }),
        );
        m.set_percent(20.0).unwrap();
        assert!((*target.lock().unwrap() - 20.0).abs() < 1e-4);
        m.set_valve_deg(-40.0).unwrap();
        assert_eq!(*target.lock().unwrap(), 0.0);
    }
}
