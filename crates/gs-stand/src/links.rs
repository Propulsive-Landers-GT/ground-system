//! Owns the two serial ports: discovery, role identification by behaviour, the handshake
//! (`connected` banner, `sync status` probe, load-cell `setup`), spaced command queues,
//! liveness probing and reconnects. Everything here is non-blocking; call [`BoardManager::tick`]
//! from the main loop.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::arduino::{classify, Board, BoardMsg, PortFactory, Role};
use crate::config::{LoadcellConfig, SerialConfig};

#[derive(Debug, Clone, PartialEq)]
pub enum LinkEvent {
    /// A board finished its handshake (first time, after a reconnect, or after a spontaneous reset).
    Connected(Role, String),
    Lost(Role, String),
    /// Parsed load-cell samples.
    Loadcells(Vec<(String, f32)>),
    /// `SYNC is N` from the actuation board.
    SyncLevel(bool),
    /// Any other line, for the log.
    Line(Role, String),
    Warn(String),
    Info(String),
    /// A command left for the board (for the CSV log).
    Sent(Role, String),
}

enum SlotState {
    WaitBanner,
    Probing { sent: Instant, saw_banner: bool },
}

struct Slot {
    board: Box<dyn Board>,
    opened: Instant,
    state: SlotState,
}

struct Link {
    board: Box<dyn Board>,
    queue: VecDeque<QueuedCmd>,
    last_write: Option<Instant>,
    /// Waiting for this reply before sending anything else (load-cell `setup` blocks the sketch).
    blocked: Option<(String, Instant)>,
    last_probe_sent: Option<Instant>,
    probe_outstanding: bool,
    probe_misses: u32,
    setup_pending: VecDeque<String>,
}

struct QueuedCmd {
    text: String,
    /// Liveness probe rather than a real command: not logged as Sent.
    probe: bool,
    /// After writing, hold the queue until this load cell reports ready.
    block_on: Option<String>,
}

impl QueuedCmd {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            probe: false,
            block_on: None,
        }
    }
    fn probe() -> Self {
        Self {
            text: "sync status".into(),
            probe: true,
            block_on: None,
        }
    }
}

pub struct BoardManager {
    factory: Box<dyn PortFactory>,
    cfg: SerialConfig,
    loadcells: Vec<LoadcellConfig>,
    slots: Vec<Slot>,
    actuation: Option<Link>,
    loadcell: Option<Link>,
    last_scan: Option<Instant>,
    explicit_actuation: Option<String>,
    explicit_loadcell: Option<String>,
    pub probes_allowed: bool,
}

impl BoardManager {
    pub fn new(factory: Box<dyn PortFactory>, cfg: SerialConfig, loadcells: Vec<LoadcellConfig>) -> Self {
        let explicit = |s: &str| (s != "auto").then(|| s.to_string());
        Self {
            explicit_actuation: explicit(&cfg.actuation_port),
            explicit_loadcell: explicit(&cfg.loadcell_port),
            factory,
            cfg,
            loadcells,
            slots: Vec::new(),
            actuation: None,
            loadcell: None,
            last_scan: None,
            probes_allowed: true,
        }
    }

    pub fn actuation_ok(&self) -> bool {
        self.actuation
            .as_ref()
            .is_some_and(|l| l.probe_misses < self.cfg.probe_misses_for_loss)
    }

    pub fn loadcell_ok(&self) -> bool {
        self.loadcell.as_ref().is_some_and(|l| l.blocked.is_none() && l.setup_pending.is_empty())
    }

    pub fn actuation_path(&self) -> Option<&str> {
        self.actuation.as_ref().map(|l| l.board.path())
    }

    pub fn loadcell_path(&self) -> Option<&str> {
        self.loadcell.as_ref().map(|l| l.board.path())
    }

    /// Queue a command for a board. Dropped (with a warning event) if that board is not up.
    pub fn send(&mut self, role: Role, cmd: String, events: &mut Vec<LinkEvent>) {
        let link = match role {
            Role::Actuation => self.actuation.as_mut(),
            Role::Loadcell => self.loadcell.as_mut(),
        };
        match link {
            Some(l) => l.queue.push_back(QueuedCmd::plain(cmd)),
            None => events.push(LinkEvent::Warn(format!(
                "{role} Arduino not connected; dropped '{cmd}'"
            ))),
        }
    }

    pub fn tick(&mut self, now: Instant) -> Vec<LinkEvent> {
        let mut events = Vec::new();
        self.tick_links(now, &mut events);
        self.tick_slots(now, &mut events);
        self.tick_scan(now, &mut events);
        events
    }

    // ----------------------------------------------------------------- identified links

    fn tick_links(&mut self, now: Instant, events: &mut Vec<LinkEvent>) {
        let gap = Duration::from_millis(self.cfg.min_command_gap_ms);
        for role in [Role::Actuation, Role::Loadcell] {
            let slot = match role {
                Role::Actuation => &mut self.actuation,
                Role::Loadcell => &mut self.loadcell,
            };
            let Some(link) = slot.as_mut() else { continue };

            // Incoming.
            let lines = match link.board.poll_lines() {
                Ok(l) => l,
                Err(e) => {
                    let path = link.board.path().to_string();
                    *slot = None;
                    events.push(LinkEvent::Lost(role, format!("{path}: {e}")));
                    continue;
                }
            };
            for line in lines {
                match classify(&line) {
                    BoardMsg::Connected => {
                        // Spontaneous reset (brownout, USB glitch). Outputs are at boot state.
                        events.push(LinkEvent::Warn(format!(
                            "{role} Arduino ({}) reset itself",
                            link.board.path()
                        )));
                        link.queue.clear();
                        link.blocked = None;
                        link.probe_outstanding = false;
                        link.probe_misses = 0;
                        if role == Role::Loadcell {
                            link.setup_pending = setup_list(&self.loadcells);
                        }
                        // Re-sync like a fresh connect: a throwaway command absorbs any bytes
                        // the sketch drops right after reset.
                        link.queue.push_back(QueuedCmd::probe());
                        events.push(LinkEvent::Connected(role, link.board.path().to_string()));
                    }
                    BoardMsg::Status { device, value } if device == "SYNC" => {
                        link.probe_outstanding = false;
                        link.probe_misses = 0;
                        events.push(LinkEvent::SyncLevel(value != 0));
                    }
                    BoardMsg::LoadcellReady(name) => {
                        if let Some((expected, _)) = &link.blocked {
                            if *expected == name {
                                link.blocked = None;
                            }
                        }
                        events.push(LinkEvent::Info(format!("{name} load cell ready (tared)")));
                    }
                    BoardMsg::Loadcells(cells) => events.push(LinkEvent::Loadcells(cells)),
                    BoardMsg::Status { .. } | BoardMsg::Other(_) => {
                        events.push(LinkEvent::Line(role, line))
                    }
                }
            }

            // Blocked on a setup reply?
            if let Some((name, deadline)) = &link.blocked {
                if now >= *deadline {
                    events.push(LinkEvent::Warn(format!(
                        "{name} load cell did not report ready within {:.0} s (not wired? the sketch \
                         blocks in setup forever if so)",
                        self.cfg.loadcell_setup_timeout_s
                    )));
                    link.blocked = None;
                } else {
                    continue;
                }
            }

            // Pending load-cell setups go ahead of the queue.
            if link.queue.is_empty() {
                if let Some(name) = link.setup_pending.pop_front() {
                    link.queue.push_front(QueuedCmd {
                        text: format!("{name} setup"),
                        probe: false,
                        block_on: Some(name),
                    });
                }
            }

            // Liveness probe for the actuation board when idle.
            if role == Role::Actuation
                && self.probes_allowed
                && self.cfg.probe_interval_s > 0.0
                && link.queue.is_empty()
            {
                let interval = Duration::from_secs_f64(self.cfg.probe_interval_s);
                let due = link
                    .last_probe_sent
                    .is_none_or(|t| now.duration_since(t) >= interval);
                if due {
                    if link.probe_outstanding {
                        link.probe_misses += 1;
                        if link.probe_misses >= self.cfg.probe_misses_for_loss {
                            // Close it; the rescan reopens (and DTR-resets) the board.
                            let path = link.board.path().to_string();
                            *slot = None;
                            events.push(LinkEvent::Lost(
                                role,
                                format!("{path}: {} probes unanswered", self.cfg.probe_misses_for_loss),
                            ));
                            continue;
                        }
                    }
                    link.queue.push_back(QueuedCmd::probe());
                    link.last_probe_sent = Some(now);
                    link.probe_outstanding = true;
                }
            }

            // One write per tick, spaced so the sketch sees each command alone.
            let can_write = link
                .last_write
                .is_none_or(|t| now.duration_since(t) >= gap);
            if can_write {
                if let Some(cmd) = link.queue.pop_front() {
                    if let Err(e) = link.board.write_cmd(&cmd.text) {
                        let path = link.board.path().to_string();
                        *slot = None;
                        events.push(LinkEvent::Lost(role, format!("{path}: write failed: {e}")));
                        continue;
                    }
                    link.last_write = Some(now);
                    if let Some(name) = cmd.block_on {
                        link.blocked = Some((
                            name,
                            now + Duration::from_secs_f64(self.cfg.loadcell_setup_timeout_s),
                        ));
                    }
                    if !cmd.probe {
                        events.push(LinkEvent::Sent(role, cmd.text));
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------- identification

    fn tick_slots(&mut self, now: Instant, events: &mut Vec<LinkEvent>) {
        let connect_timeout = Duration::from_secs_f64(self.cfg.connect_timeout_s);
        let probe_timeout = Duration::from_secs_f64(self.cfg.probe_timeout_s);
        let mut i = 0;
        while i < self.slots.len() {
            let slot = &mut self.slots[i];
            let lines = match slot.board.poll_lines() {
                Ok(l) => l,
                Err(e) => {
                    events.push(LinkEvent::Warn(format!("{}: {e}", slot.board.path())));
                    self.slots.remove(i);
                    continue;
                }
            };
            let mut saw_banner = false;
            let mut saw_sync = false;
            for line in lines {
                match classify(&line) {
                    BoardMsg::Connected => saw_banner = true,
                    BoardMsg::Status { device, .. } if device == "SYNC" => saw_sync = true,
                    _ => {}
                }
            }
            let mut decided: Option<Option<Role>> = None; // Some(None) = give up on this port
            match &mut slot.state {
                SlotState::WaitBanner => {
                    let timed_out = now.duration_since(slot.opened) >= connect_timeout;
                    if saw_banner || timed_out {
                        if timed_out {
                            events.push(LinkEvent::Warn(format!(
                                "{}: no 'connected' banner in {:.0} s; probing anyway",
                                slot.board.path(),
                                self.cfg.connect_timeout_s
                            )));
                        }
                        // Sketch drops bytes right after reset; a short settle helps, and the
                        // probe doubles as the throwaway "sync status" the legacy code needed.
                        if let Err(e) = slot.board.write_cmd("sync status") {
                            events.push(LinkEvent::Warn(format!("{}: {e}", slot.board.path())));
                            decided = Some(None);
                        } else {
                            slot.state = SlotState::Probing {
                                sent: now,
                                saw_banner,
                            };
                        }
                    }
                }
                SlotState::Probing {
                    sent,
                    saw_banner: banner,
                } => {
                    if saw_sync {
                        decided = Some(Some(Role::Actuation));
                    } else if now.duration_since(*sent) >= probe_timeout {
                        if *banner {
                            decided = Some(Some(Role::Loadcell));
                        } else {
                            events.push(LinkEvent::Warn(format!(
                                "{}: silent (no banner, no probe reply); ignoring it",
                                slot.board.path()
                            )));
                            decided = Some(None);
                        }
                    }
                }
            }
            match decided {
                None => i += 1,
                Some(None) => {
                    self.slots.remove(i);
                }
                Some(Some(role)) => {
                    let slot = self.slots.remove(i);
                    self.adopt(slot.board, role, now, events);
                }
            }
        }
    }

    fn adopt(&mut self, board: Box<dyn Board>, role: Role, now: Instant, events: &mut Vec<LinkEvent>) {
        let path = board.path().to_string();
        let target = match role {
            Role::Actuation => &mut self.actuation,
            Role::Loadcell => &mut self.loadcell,
        };
        if target.is_some() {
            events.push(LinkEvent::Warn(format!(
                "{path} also behaves like the {role} board; already have one, ignoring it"
            )));
            return;
        }
        let expected_other = match role {
            Role::Actuation => &self.explicit_loadcell,
            Role::Loadcell => &self.explicit_actuation,
        };
        if expected_other.as_deref() == Some(path.as_str()) {
            events.push(LinkEvent::Warn(format!(
                "{path} is configured as the other board but behaves like {role}; using behaviour \
                 (the two Arduinos swapped ports)"
            )));
        }
        info!("{role} Arduino identified on {path}");
        let setup_pending = if role == Role::Loadcell {
            setup_list(&self.loadcells)
        } else {
            VecDeque::new()
        };
        *target = Some(Link {
            board,
            queue: VecDeque::new(),
            last_write: Some(now),
            blocked: None,
            last_probe_sent: Some(now),
            probe_outstanding: false,
            probe_misses: 0,
            setup_pending,
        });
        events.push(LinkEvent::Connected(role, path));
    }

    // ----------------------------------------------------------------- discovery

    fn tick_scan(&mut self, now: Instant, events: &mut Vec<LinkEvent>) {
        let need = (self.actuation.is_none() as usize) + (self.loadcell.is_none() as usize);
        if need == 0 || !self.slots.is_empty() {
            return;
        }
        let interval = Duration::from_secs_f64(self.cfg.reconnect_interval_s);
        if self
            .last_scan
            .is_some_and(|t| now.duration_since(t) < interval)
        {
            return;
        }
        self.last_scan = Some(now);

        let mut open: HashSet<String> = HashSet::new();
        if let Some(l) = &self.actuation {
            open.insert(l.board.path().to_string());
        }
        if let Some(l) = &self.loadcell {
            open.insert(l.board.path().to_string());
        }

        let candidates: Vec<String> = match (&self.explicit_actuation, &self.explicit_loadcell) {
            (Some(a), Some(b)) => vec![a.clone(), b.clone()],
            (Some(a), None) => {
                let mut v = vec![a.clone()];
                v.extend(self.factory.candidates());
                v
            }
            (None, Some(b)) => {
                let mut v = vec![b.clone()];
                v.extend(self.factory.candidates());
                v
            }
            (None, None) => self.factory.candidates(),
        };
        let mut tried = HashSet::new();
        let mut opened = 0;
        for path in candidates {
            if opened >= need || open.contains(&path) || !tried.insert(path.clone()) {
                continue;
            }
            match self.factory.open(&path, self.cfg.baud) {
                Ok(board) => {
                    info!("opened {path}, waiting for the Arduino banner");
                    self.slots.push(Slot {
                        board,
                        opened: now,
                        state: SlotState::WaitBanner,
                    });
                    opened += 1;
                }
                Err(e) => {
                    warn!("could not open {path}: {e}");
                    events.push(LinkEvent::Warn(format!("could not open {path}: {e}")));
                }
            }
        }
    }
}

fn setup_list(cells: &[LoadcellConfig]) -> VecDeque<String> {
    cells
        .iter()
        .filter(|c| c.setup_on_connect)
        .map(|c| c.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LoadcellsConfig;
    use crate::fake::{FakeFactory, FakeWorld};
    use std::thread::sleep;

    fn manager() -> (BoardManager, std::sync::Arc<std::sync::Mutex<FakeWorld>>) {
        let world = FakeWorld::new_shared();
        let cfg = SerialConfig {
            connect_timeout_s: 0.5,
            probe_timeout_s: 0.2,
            loadcell_setup_timeout_s: 1.0,
            reconnect_interval_s: 0.1,
            probe_interval_s: 0.2,
            ..SerialConfig::default()
        };
        let m = BoardManager::new(
            Box::new(FakeFactory { world: world.clone() }),
            cfg,
            LoadcellsConfig::default().cell,
        );
        (m, world)
    }

    fn run(m: &mut BoardManager, secs: f64) -> Vec<LinkEvent> {
        let mut all = Vec::new();
        let end = Instant::now() + Duration::from_secs_f64(secs);
        while Instant::now() < end {
            all.extend(m.tick(Instant::now()));
            sleep(Duration::from_millis(10));
        }
        all
    }

    #[test]
    fn identifies_both_fakes_by_behaviour_and_sets_up_loadcells() {
        let (mut m, world) = manager();
        let ev = run(&mut m, 1.5);
        assert!(ev.contains(&LinkEvent::Connected(Role::Actuation, "fake:A".into())), "{ev:?}");
        assert!(ev.contains(&LinkEvent::Connected(Role::Loadcell, "fake:B".into())), "{ev:?}");
        assert!(m.actuation_ok());
        assert!(m.loadcell_ok(), "setups should have completed");
        let cmds = world.lock().unwrap().commands.clone();
        let lc: Vec<&str> = cmds
            .iter()
            .filter(|(r, _)| *r == Role::Loadcell)
            .map(|(_, c)| c.as_str())
            .collect();
        assert_eq!(lc, vec!["sync status", "engine setup", "nitrous setup"], "{lc:?}");
        assert_eq!(world.lock().unwrap().merged_commands, 0);
    }

    #[test]
    fn queue_spacing_and_routing() {
        let (mut m, world) = manager();
        run(&mut m, 1.5);
        let mut ev = Vec::new();
        for c in ["omv open", "igv open", "kaboom start"] {
            m.send(Role::Actuation, c.into(), &mut ev);
        }
        m.send(Role::Loadcell, "engine begin".into(), &mut ev);
        let ev = run(&mut m, 0.5);
        let sent: Vec<String> = ev
            .iter()
            .filter_map(|e| match e {
                LinkEvent::Sent(Role::Actuation, c) => Some(c.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(sent, vec!["omv open", "igv open", "kaboom start"]);
        let w = world.lock().unwrap();
        assert!(w.omv_open && w.igv_open && w.kaboom);
        assert!(w.streaming[0]);
        assert_eq!(w.merged_commands, 0, "commands must never merge");
        drop(w);
        assert!(ev.iter().any(|e| matches!(e, LinkEvent::Loadcells(_))));
    }

    #[test]
    fn dropped_when_not_connected() {
        let (mut m, _) = manager();
        let mut ev = Vec::new();
        m.send(Role::Actuation, "omv open".into(), &mut ev);
        assert!(matches!(&ev[0], LinkEvent::Warn(w) if w.contains("dropped")));
    }
}
