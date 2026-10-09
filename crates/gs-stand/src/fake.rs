//! In-process stand-ins for both Arduinos (`--fake-arduino`), speaking the sketch's protocol
//! closely enough to test everything above the serial layer on a laptop.
//!
//! Quirks reproduced on purpose: the `connected` banner on open, the 10 ms framing rule
//! (a second command inside 10 ms is merged with the first and lost), the loadcell board
//! ignoring actuation commands and vice versa, `<name> loadcell ready` after setup, and a
//! ~10 Hz stream once `begin` is sent. Load-cell values respond to OMV/MTV/PUMV so plots move.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::arduino::{Board, PortFactory, Role};

/// Pin-level state of the fake stand, shared by the two boards and the fake MTV backend.
#[derive(Debug, Default, Clone)]
pub struct FakeWorld {
    // Actuation outputs, as the sketch's digitalWrite levels would leave them, expressed as
    // "open" for valves so tests can read them directly.
    pub omv_open: bool,
    pub ovent_open: bool,
    pub puiso_open: bool,
    pub igv_open: bool,
    pub ofill_open: bool,
    pub lfvent_open: bool,
    pub pufill_open: bool,
    /// OISO_OPEN / OISO_CLOSE pins.
    pub oiso_open_pin: bool,
    pub oiso_close_pin: bool,
    pub pumv_open: bool,
    pub puvent_open: bool,
    pub kaboom: bool,
    pub sync: bool,
    /// Set by the fake MTV backend.
    pub mtv_percent: f32,
    /// Everything either board has been told, in order (for tests).
    pub commands: Vec<(Role, String)>,
    pub nitrous_kg: f32,
    pub streaming: [bool; 3],
    pub setup_done: [bool; 3],
    /// Count of commands the boards dropped because of the 10 ms framing rule.
    pub merged_commands: u32,
}

impl FakeWorld {
    /// The sketch's `setup_starting_states(COLDFLOW)`: everything LOW except OVENT/PUVENT
    /// (HIGH == closed for those) and OISO_CLOSE HIGH.
    pub fn arduino_boot_state(&mut self) {
        self.omv_open = false;
        self.puiso_open = false;
        self.igv_open = false;
        self.ofill_open = false;
        self.lfvent_open = false;
        self.pufill_open = false;
        self.oiso_close_pin = true;
        self.oiso_open_pin = false;
        self.pumv_open = false;
        self.kaboom = false;
        self.ovent_open = false;
        self.puvent_open = false;
        self.sync = false;
    }

    pub fn new_shared() -> Arc<Mutex<FakeWorld>> {
        let mut w = FakeWorld {
            nitrous_kg: 12.0,
            ..Default::default()
        };
        w.arduino_boot_state();
        Arc::new(Mutex::new(w))
    }
}

fn cell_index(name: &str) -> Option<usize> {
    match name {
        "engine" => Some(0),
        "nitrous" => Some(1),
        "rcs" => Some(2),
        _ => None,
    }
}
const CELL_NAMES: [&str; 3] = ["engine", "nitrous", "rcs"];

pub struct FakeBoard {
    path: String,
    role: Role,
    world: Arc<Mutex<FakeWorld>>,
    opened: Instant,
    banner_sent: bool,
    last_cmd: Option<Instant>,
    /// (due, line)
    pending: VecDeque<(Instant, String)>,
    last_stream: Instant,
    stream_period: Duration,
    rng: u32,
}

impl FakeBoard {
    pub fn new(role: Role, world: Arc<Mutex<FakeWorld>>, path: &str) -> Self {
        {
            // Opening the port resets the Arduino: it re-applies its boot state.
            let mut w = world.lock().unwrap();
            if role == Role::Actuation {
                w.arduino_boot_state();
            } else {
                w.streaming = [false; 3];
                w.setup_done = [false; 3];
            }
        }
        Self {
            path: path.to_string(),
            role,
            world,
            opened: Instant::now(),
            banner_sent: false,
            last_cmd: None,
            pending: VecDeque::new(),
            last_stream: Instant::now(),
            stream_period: Duration::from_millis(100),
            rng: 0x1234_5678,
        }
    }

    fn noise(&mut self) -> f32 {
        // xorshift, deterministic.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x % 1000) as f32 / 1000.0 - 0.5
    }

    fn reply(&mut self, delay: Duration, line: impl Into<String>) {
        self.pending.push_back((Instant::now() + delay, line.into()));
    }

    fn handle_actuation(&mut self, device: &str, action: &str) {
        let mut w = self.world.lock().unwrap();
        let status = |name: &str, level: bool| format!("{name} is {}", level as u8);
        let mut replies: Vec<String> = Vec::new();
        match device {
            "omv" | "puiso" | "igv" | "ofill" | "lfvent" | "pufill" | "pumv" => {
                let slot: &mut bool = match device {
                    "omv" => &mut w.omv_open,
                    "puiso" => &mut w.puiso_open,
                    "igv" => &mut w.igv_open,
                    "ofill" => &mut w.ofill_open,
                    "lfvent" => &mut w.lfvent_open,
                    "pufill" => &mut w.pufill_open,
                    _ => &mut w.pumv_open,
                };
                match action {
                    "open" => *slot = true,
                    "close" => *slot = false,
                    // HIGH == open for these
                    "status" => replies.push(status(&device.to_uppercase(), *slot)),
                    _ => {}
                }
            }
            "ovent" | "puvent" => {
                let slot: &mut bool = if device == "ovent" {
                    &mut w.ovent_open
                } else {
                    &mut w.puvent_open
                };
                match action {
                    "open" => *slot = true,
                    "close" => *slot = false,
                    // HIGH == closed for the vents
                    "status" => replies.push(status(&device.to_uppercase(), !*slot)),
                    _ => {}
                }
            }
            "oiso" => match action {
                "open" => {
                    w.oiso_open_pin = true;
                    w.oiso_close_pin = false;
                }
                "close" => {
                    w.oiso_open_pin = false;
                    w.oiso_close_pin = true;
                }
                "status" => {
                    replies.push(status("OISO_OPEN", w.oiso_open_pin));
                    replies.push(status("OISO_CLOSE", w.oiso_close_pin));
                }
                _ => {}
            },
            "kaboom" => match action {
                "start" => w.kaboom = true,
                "end" => w.kaboom = false,
                "status" => replies.push(status("KABOOM", w.kaboom)),
                _ => {}
            },
            "sync" => match action {
                "high" => w.sync = true,
                "low" => w.sync = false,
                "status" => replies.push(status("SYNC", w.sync)),
                _ => {}
            },
            "shutdown" => {
                if action == "begin" {
                    w.ofill_open = false;
                    w.igv_open = false;
                }
            }
            "reset" => match action {
                "all" => {
                    w.omv_open = false;
                    w.puiso_open = false;
                    w.igv_open = false;
                    w.ofill_open = false;
                    w.lfvent_open = false;
                    w.pufill_open = false;
                    w.oiso_close_pin = true;
                    w.oiso_open_pin = false;
                    w.pumv_open = false;
                    w.kaboom = false;
                    w.sync = false;
                    // LOW == open for the vents
                    w.ovent_open = true;
                    w.puvent_open = true;
                }
                "default" => w.arduino_boot_state(),
                _ => {}
            },
            _ => {}
        }
        drop(w);
        for r in replies {
            self.reply(Duration::from_millis(2), r);
        }
    }

    fn handle_loadcell(&mut self, device: &str, action: &str) {
        let Some(i) = cell_index(device) else { return };
        match action {
            "setup" => {
                // HX711 begin + tare takes a moment on the real board.
                self.world.lock().unwrap().setup_done[i] = true;
                self.reply(Duration::from_millis(250), format!("{device} loadcell ready"));
            }
            "begin" => self.world.lock().unwrap().streaming[i] = true,
            "end" => self.world.lock().unwrap().streaming[i] = false,
            "read" => {
                let v = self.sample(i);
                self.reply(Duration::from_millis(100), format!("{v:.2}"));
            }
            _ => {}
        }
    }

    fn sample(&mut self, i: usize) -> f32 {
        let (omv, mtv, pumv, puiso, nitrous) = {
            let w = self.world.lock().unwrap();
            (
                w.omv_open,
                w.mtv_percent,
                w.pumv_open,
                w.puiso_open,
                w.nitrous_kg,
            )
        };
        let n = self.noise();
        match i {
            0 => {
                if omv && nitrous > 0.0 {
                    800.0 * (mtv / 100.0) + n * 8.0
                } else {
                    n * 0.6
                }
            }
            1 => nitrous + n * 0.02,
            _ => {
                if pumv && puiso {
                    22.0 + n * 1.5
                } else {
                    n * 0.3
                }
            }
        }
    }

    fn step_world(&mut self, dt: f32) {
        let mut w = self.world.lock().unwrap();
        if w.omv_open && w.nitrous_kg > 0.0 {
            // ~0.35 kg/s at full throttle.
            w.nitrous_kg = (w.nitrous_kg - 0.35 * (w.mtv_percent / 100.0) * dt).max(0.0);
        }
    }
}

impl Board for FakeBoard {
    fn path(&self) -> &str {
        &self.path
    }

    fn write_cmd(&mut self, cmd: &str) -> io::Result<()> {
        let now = Instant::now();
        if let Some(last) = self.last_cmd {
            if now.duration_since(last) < Duration::from_millis(10) {
                // Sketch would have read both as one string and matched neither.
                self.world.lock().unwrap().merged_commands += 1;
                return Ok(());
            }
        }
        self.last_cmd = Some(now);
        self.world
            .lock()
            .unwrap()
            .commands
            .push((self.role, cmd.to_string()));
        let (device, action) = cmd.split_once(' ').unwrap_or((cmd, ""));
        match self.role {
            Role::Actuation => self.handle_actuation(device, action),
            Role::Loadcell => self.handle_loadcell(device, action),
        }
        Ok(())
    }

    fn poll_lines(&mut self) -> io::Result<Vec<String>> {
        let now = Instant::now();
        let mut out = Vec::new();
        if !self.banner_sent && now.duration_since(self.opened) >= Duration::from_millis(60) {
            self.banner_sent = true;
            out.push("connected".to_string());
        }
        while let Some((due, _)) = self.pending.front() {
            if *due <= now {
                out.push(self.pending.pop_front().unwrap().1);
            } else {
                break;
            }
        }
        if self.role == Role::Loadcell && now.duration_since(self.last_stream) >= self.stream_period
        {
            let dt = now.duration_since(self.last_stream).as_secs_f32();
            self.last_stream = now;
            self.step_world(dt);
            let streaming = self.world.lock().unwrap().streaming;
            let mut parts = Vec::new();
            for (i, name) in CELL_NAMES.iter().enumerate() {
                if streaming[i] {
                    let v = self.sample(i);
                    parts.push(format!("{name}:{v:.2}"));
                }
            }
            if !parts.is_empty() {
                out.push(parts.join(" "));
            }
        }
        Ok(out)
    }
}

/// Hands out `fake:actuation` and `fake:loadcell`. Which is which is *not* told to the
/// caller: it has to identify them by behaviour like it would with real ports.
pub struct FakeFactory {
    pub world: Arc<Mutex<FakeWorld>>,
}

impl PortFactory for FakeFactory {
    fn candidates(&self) -> Vec<String> {
        // Listed "loadcell first" so a naive "first port is actuation" assumption breaks.
        vec!["fake:B".to_string(), "fake:A".to_string()]
    }

    fn open(&self, path: &str, _baud: u32) -> io::Result<Box<dyn Board>> {
        let role = match path {
            "fake:A" => Role::Actuation,
            "fake:B" => Role::Loadcell,
            _ => return Err(io::Error::new(io::ErrorKind::NotFound, path.to_string())),
        };
        Ok(Box::new(FakeBoard::new(role, self.world.clone(), path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn actuation_fake_answers_status_and_drives_pins() {
        let world = FakeWorld::new_shared();
        let mut b = FakeBoard::new(Role::Actuation, world.clone(), "fake:A");
        sleep(Duration::from_millis(70));
        assert_eq!(b.poll_lines().unwrap(), vec!["connected".to_string()]);
        b.write_cmd("sync status").unwrap();
        sleep(Duration::from_millis(5));
        assert_eq!(b.poll_lines().unwrap(), vec!["SYNC is 0".to_string()]);
        sleep(Duration::from_millis(30));
        b.write_cmd("omv open").unwrap();
        assert!(world.lock().unwrap().omv_open);
        // Too soon: merged and lost.
        b.write_cmd("omv close").unwrap();
        assert!(world.lock().unwrap().omv_open);
        assert_eq!(world.lock().unwrap().merged_commands, 1);
        sleep(Duration::from_millis(30));
        b.write_cmd("ovent open").unwrap();
        sleep(Duration::from_millis(30));
        b.write_cmd("ovent status").unwrap();
        sleep(Duration::from_millis(5));
        // Vent open == pin LOW == 0
        assert_eq!(b.poll_lines().unwrap(), vec!["OVENT is 0".to_string()]);
    }

    #[test]
    fn loadcell_fake_ignores_valves_and_streams() {
        let world = FakeWorld::new_shared();
        let mut b = FakeBoard::new(Role::Loadcell, world.clone(), "fake:B");
        b.write_cmd("sync status").unwrap();
        sleep(Duration::from_millis(70));
        assert_eq!(b.poll_lines().unwrap(), vec!["connected".to_string()]);
        b.write_cmd("engine setup").unwrap();
        sleep(Duration::from_millis(300));
        let lines = b.poll_lines().unwrap();
        assert!(lines.contains(&"engine loadcell ready".to_string()), "{lines:?}");
        b.write_cmd("engine begin").unwrap();
        sleep(Duration::from_millis(30));
        b.write_cmd("nitrous begin").unwrap();
        sleep(Duration::from_millis(120));
        let lines = b.poll_lines().unwrap();
        let stream: Vec<_> = lines.iter().filter(|l| l.starts_with("engine:")).collect();
        assert!(!stream.is_empty(), "{lines:?}");
        assert!(stream[0].contains(" nitrous:"), "{stream:?}");
    }
}
