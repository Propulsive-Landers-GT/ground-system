//! gs-stand: the test-stand adapter. See `README.md` and `docs/DESIGN.md`.
//!
//! Layering, bottom up:
//! - [`arduino`]: the sketch's wire protocol, line parsing, real serial ports.
//! - [`links`]: port discovery, role identification, handshakes, spaced command queues.
//! - [`mtv`]: throttle geometry and PWM backends.
//! - [`sequence`]: sequence files, action vocabulary, MTV profile DSL.
//! - [`stand`]: the pure state machine (modes, interlocks, safing, sequence engine).
//! - [`App`]: glues them to the UDP link at 50 Hz.

pub mod arduino;
pub mod config;
pub mod fake;
pub mod links;
pub mod mtv;
pub mod runlog;
pub mod sequence;
pub mod stand;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use gs_protocol::{CommandKind, Downlink, EventMsg, Severity, StandChannel, VehicleLink};
use tracing::{debug, error, info, warn};

use crate::arduino::{PortFactory, Role, SerialFactory};
use crate::config::Config;
use crate::fake::{FakeFactory, FakeWorld};
use crate::links::{BoardManager, LinkEvent};
use crate::mtv::{CallbackBackend, Geometry, Mtv};
use crate::runlog::{RowCtx, RunLog};
use crate::stand::{Effect, Links, Rules, Stand};

pub const LOOP_HZ: f64 = 50.0;
pub const TELEMETRY_HZ: f64 = 20.0;
pub const STATUS_HZ: f64 = 5.0;

pub struct AppOptions {
    pub config: Config,
    /// Directory that relative config paths resolve against (the repo root).
    pub root: PathBuf,
    pub udp_port: u16,
    pub time_scale: f64,
    /// Use in-process fake Arduinos and a fake MTV backend.
    pub fake: Option<Arc<Mutex<FakeWorld>>>,
    /// Write the per-run CSV.
    pub csv: bool,
}

struct Cell {
    name: String,
    channel: StandChannel,
    scale: f32,
    offset: f32,
    last: Option<(f32, Instant)>,
}

pub struct App {
    cfg: Config,
    link: VehicleLink,
    boards: BoardManager,
    stand: Stand,
    mtv: Mtv,
    log: Option<RunLog>,
    start: Instant,
    cells: Vec<Cell>,
    next_tele: Instant,
    next_status: Instant,
    ground_lost: bool,
    ground_seen: bool,
    mtv_failed: bool,
    actuation_was_ok: bool,
}

impl App {
    pub fn new(opts: AppOptions) -> Result<Self> {
        let cfg = opts.config;
        cfg.validate()?;
        let seq_dir = Config::resolve(&opts.root, &cfg.sequences.dir);
        let sequences = sequence::load_dir(&seq_dir)?;
        for (name, s) in &sequences {
            info!(
                "sequence '{name}': {} steps, {:.1} s{}",
                s.steps.len(),
                s.duration_s,
                s.mtv_profile
                    .as_ref()
                    .map(|p| format!(", MTV profile at T+{:.1}", p.start_t))
                    .unwrap_or_default()
            );
        }

        let link = VehicleLink::bind(opts.udp_port)
            .with_context(|| format!("binding UDP 0.0.0.0:{}", opts.udp_port))?;
        info!("listening for the bridge on UDP {}", opts.udp_port);

        let (factory, mtv): (Box<dyn PortFactory>, Mtv) = match &opts.fake {
            Some(world) => {
                let w = world.clone();
                let geom = Geometry::from(&cfg.mtv);
                let backend = CallbackBackend {
                    geom,
                    on_percent: Box::new(move |p| w.lock().unwrap().mtv_percent = p),
                };
                (
                    Box::new(FakeFactory { world: world.clone() }),
                    Mtv::new(geom, Box::new(backend)),
                )
            }
            None => {
                let mut explicit = Vec::new();
                for p in [&cfg.serial.actuation_port, &cfg.serial.loadcell_port] {
                    if p != "auto" {
                        explicit.push(p.clone());
                    }
                }
                (
                    Box::new(SerialFactory { explicit }),
                    mtv::build(&cfg.mtv, &opts.root)?,
                )
            }
        };
        let mut mtv = mtv;
        info!("MTV backend: {}", mtv.backend_name());
        mtv.home(cfg.mtv.home_valve_deg, if opts.fake.is_some() { 0.0 } else { cfg.mtv.home_hold_s })?;

        let boards = BoardManager::new(factory, cfg.serial.clone(), cfg.loadcells.cell.clone());
        let stand = Stand::new(Rules {
            safing: cfg.safing_actions()?,
            use_reset_all: cfg.safing.use_reset_all,
            oiso_travel_s: cfg.valves.oiso_travel_s,
            time_scale: opts.time_scale,
            sequences,
        });
        let cells: Vec<Cell> = cfg
            .loadcells
            .cell
            .iter()
            .map(|c| Cell {
                name: c.name.clone(),
                channel: c.channel,
                scale: c.scale,
                offset: c.offset,
                last: None,
            })
            .collect();
        let log = if opts.csv {
            let dir = Config::resolve(&opts.root, &cfg.logging.dir);
            let names: Vec<String> = cells.iter().map(|c| c.name.clone()).collect();
            let l = RunLog::open(&dir, &names)?;
            info!("run log: {}", l.path().display());
            Some(l)
        } else {
            None
        };
        let now = Instant::now();
        Ok(Self {
            cfg,
            link,
            boards,
            stand,
            mtv,
            log,
            start: now,
            cells,
            next_tele: now,
            next_status: now,
            ground_lost: false,
            ground_seen: false,
            mtv_failed: false,
            actuation_was_ok: false,
        })
    }

    pub fn time_s(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn links(&self) -> Links {
        Links {
            actuation: self.boards.actuation_ok(),
            loadcell: self.boards.loadcell_ok(),
        }
    }

    /// Run the 50 Hz loop until `stop` is set.
    pub fn run(&mut self, stop: &AtomicBool) {
        let period = Duration::from_secs_f64(1.0 / LOOP_HZ);
        let mut next = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            self.tick();
            next += period;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                // Fell behind (blocking I/O, suspend): resync rather than burst.
                next = now;
            }
        }
        info!("stopping");
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let t = self.time_s();

        // 1. Ground commands.
        while let Some(cmd) = self.link.poll() {
            if !self.ground_seen {
                self.ground_seen = true;
                self.event(
                    Severity::Info,
                    format!(
                        "ground link up from {}",
                        self.link.ground_addr().map(|a| a.to_string()).unwrap_or_default()
                    ),
                );
            }
            if matches!(cmd.kind, CommandKind::Heartbeat) {
                continue;
            }
            let links = self.links();
            let (ack, fx) = self.stand.handle(&cmd.kind, links, t);
            self.link.ack(&cmd, t, ack.clone());
            let detail = format!("#{} {:?} -> {:?}", cmd.seq, cmd.kind, ack);
            self.log_row("cmd", &detail);
            if let gs_protocol::AckResult::Rejected(r) = &ack {
                warn!("rejected {:?}: {r}", cmd.kind);
                self.event(Severity::Warning, format!("rejected {}: {r}", describe(&cmd.kind)));
            } else {
                info!("accepted {:?}", cmd.kind);
            }
            self.apply(fx);
        }

        // 2. Loss of ground.
        if let Some(age) = self.link.link_age_s() {
            let lost = age as f64 > self.cfg.ground_link.timeout_s;
            if lost && !self.ground_lost {
                self.ground_lost = true;
                let fx = self.stand.on_ground_lost(
                    t,
                    self.cfg.ground_link.armed_on_loss,
                    self.cfg.ground_link.sequence_on_loss,
                );
                self.apply(fx);
            } else if !lost && self.ground_lost {
                self.ground_lost = false;
                self.event(Severity::Info, "ground link restored".into());
            }
        }

        // 3. Arduinos.
        self.boards.probes_allowed =
            self.cfg.serial.probe_during_sequence || !self.stand.sequence_running();
        let events = self.boards.tick(now);
        for ev in events {
            self.on_link_event(ev, t, now);
        }
        let act_ok = self.boards.actuation_ok();
        if self.actuation_was_ok && !act_ok {
            let fx = self.stand.on_actuation_lost();
            self.apply(fx);
        }
        self.actuation_was_ok = act_ok;

        // 4. Sequence engine.
        let fx = self.stand.tick(t);
        self.apply(fx);

        // 5. MTV backend health.
        if let Err(e) = self.mtv.health() {
            if !self.mtv_failed {
                self.mtv_failed = true;
                self.event(Severity::Critical, format!("MTV backend failed: {e}"));
            }
        }

        // 6. Downlink. Deadlines advance by the period (not "now") so tick jitter does not
        // lower the average rate; a big stall resyncs instead of bursting.
        if now >= self.next_tele {
            self.next_tele = advance(self.next_tele, now, Duration::from_secs_f64(1.0 / TELEMETRY_HZ));
            let channels = self.channels(now);
            let tele = self.stand.telemetry(t, t, channels);
            self.link.send(&Downlink::Stand(tele));
        }
        if now >= self.next_status {
            self.next_status = advance(self.next_status, now, Duration::from_secs_f64(1.0 / STATUS_HZ));
            let st = self.stand.status(t, t, self.links());
            self.link.send(&Downlink::StandStatus(st));
        }
    }

    fn on_link_event(&mut self, ev: LinkEvent, t: f64, now: Instant) {
        match ev {
            LinkEvent::Connected(Role::Actuation, path) => {
                self.event(Severity::Info, format!("actuation Arduino on {path}"));
                let fx = self.stand.on_actuation_connected(t, self.cfg.safing.on_connect);
                self.apply(fx);
                self.actuation_was_ok = true;
            }
            LinkEvent::Connected(Role::Loadcell, path) => {
                self.event(Severity::Info, format!("load-cell Arduino on {path}"));
            }
            LinkEvent::Lost(Role::Actuation, why) => {
                self.event(Severity::Critical, format!("actuation link lost: {why}"));
                let fx = self.stand.on_actuation_lost();
                self.apply(fx);
                self.actuation_was_ok = false;
            }
            LinkEvent::Lost(Role::Loadcell, why) => {
                self.event(Severity::Warning, format!("load-cell link lost: {why}"));
                let fx = self.stand.on_loadcell_lost();
                self.apply(fx);
                for c in &mut self.cells {
                    c.last = None;
                }
            }
            LinkEvent::Loadcells(samples) => {
                let mut known = false;
                for (name, v) in samples {
                    if let Some(c) = self.cells.iter_mut().find(|c| c.name == name) {
                        c.last = Some((v * c.scale + c.offset, now));
                        known = true;
                    } else {
                        debug!("unconfigured load cell '{name}' in stream");
                    }
                }
                if known {
                    self.log_row("sample", "");
                }
            }
            LinkEvent::SyncLevel(_) => {}
            LinkEvent::Line(role, line) => {
                debug!("{role} rx: {line}");
                self.log_row("serial_rx", &format!("{role}: {line}"));
            }
            LinkEvent::Sent(role, cmd) => {
                debug!("{role} tx: {cmd}");
                self.log_row("serial_tx", &format!("{role}: {cmd}"));
            }
            LinkEvent::Warn(w) => self.event(Severity::Warning, w),
            LinkEvent::Info(i) => self.event(Severity::Info, i),
        }
    }

    fn apply(&mut self, fx: Vec<Effect>) {
        for e in fx {
            match e {
                Effect::Serial(role, cmd) => {
                    let mut warnings = Vec::new();
                    self.boards.send(role, cmd, &mut warnings);
                    for w in warnings {
                        if let LinkEvent::Warn(w) = w {
                            self.event(Severity::Warning, w);
                        }
                    }
                }
                Effect::Mtv(p) => match self.mtv.set_percent(p) {
                    Ok(()) => {
                        self.mtv_failed = false;
                        self.log_row("mtv", &format!("{p:.2}"));
                    }
                    Err(err) => {
                        error!("MTV set {p} %: {err}");
                        if !self.mtv_failed {
                            self.mtv_failed = true;
                            self.event(Severity::Critical, format!("MTV command failed: {err}"));
                        }
                    }
                },
                Effect::Event(sev, text) => self.event(sev, text),
            }
        }
    }

    fn event(&mut self, severity: Severity, text: String) {
        match severity {
            Severity::Info => info!("{text}"),
            Severity::Warning => warn!("{text}"),
            Severity::Critical => error!("{text}"),
        }
        let t = self.time_s();
        self.link.send(&Downlink::Event(EventMsg {
            time_s: t,
            severity,
            text: text.clone(),
        }));
        self.log_row(
            match severity {
                Severity::Info => "event",
                Severity::Warning => "warning",
                Severity::Critical => "critical",
            },
            &text,
        );
    }

    fn channels(&self, now: Instant) -> Vec<(StandChannel, f32)> {
        let stale = Duration::from_secs_f64(self.cfg.loadcells.stale_after_s);
        self.cells
            .iter()
            .filter_map(|c| {
                let (v, at) = c.last?;
                (now.duration_since(at) < stale).then_some((c.channel, v))
            })
            .collect()
    }

    fn log_row(&mut self, kind: &str, detail: &str) {
        let Some(log) = self.log.as_mut() else { return };
        let cells: Vec<(String, Option<f32>)> = self
            .cells
            .iter()
            .map(|c| (c.name.clone(), c.last.map(|(v, _)| v)))
            .collect();
        let unix_s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let ctx = RowCtx {
            unix_s,
            seq_t: self.stand.sequence_time(self.start.elapsed().as_secs_f64()),
            mode: self.stand.mode(),
            cells: &cells,
            mtv_percent: self.stand.mtv_percent(),
        };
        log.row(&ctx, kind, detail);
    }
}

fn advance(deadline: Instant, now: Instant, period: Duration) -> Instant {
    let next = deadline + period;
    if next < now {
        now + period
    } else {
        next
    }
}

fn describe(kind: &CommandKind) -> String {
    match kind {
        CommandKind::SetValve { id, open } => format!(
            "SetValve {} {}",
            sequence::valve_tag(*id),
            if *open { "open" } else { "close" }
        ),
        CommandKind::Stand(c) => format!("{c:?}"),
        other => format!("{other:?}"),
    }
}
