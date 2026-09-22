//! End-to-end against the in-process fake Arduinos, over real UDP: the same path the bridge uses.

use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gs_protocol::{
    AckResult, CommandKind, Downlink, Severity, StandCommand, StandMode, Uplink, ValveId,
    ValveState,
};
use gs_stand::config::Config;
use gs_stand::fake::FakeWorld;
use gs_stand::{App, AppOptions};

struct Harness {
    sock: UdpSocket,
    stand_addr: std::net::SocketAddr,
    world: Arc<Mutex<FakeWorld>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    seq: u32,
    last_heartbeat: Instant,
    pub inbox: Vec<(Instant, Downlink)>,
}

impl Harness {
    fn start(time_scale: f64) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut config = Config::default();
        // Fast handshakes so the test is quick; the real defaults are conservative.
        config.serial.connect_timeout_s = 0.5;
        config.serial.probe_timeout_s = 0.2;
        config.serial.reconnect_interval_s = 0.1;
        config.serial.probe_interval_s = 0.2;
        config.ground_link.timeout_s = 1.0;

        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let world = FakeWorld::new_shared();
        let mut app = App::new(AppOptions {
            config,
            root,
            udp_port: port,
            time_scale,
            fake: Some(world.clone()),
            csv: false,
        })
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let thread = thread::spawn(move || app.run(&s2));

        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
        Self {
            sock,
            stand_addr: format!("127.0.0.1:{port}").parse().unwrap(),
            world,
            stop,
            thread: Some(thread),
            seq: 0,
            last_heartbeat: Instant::now() - Duration::from_secs(1),
            inbox: Vec::new(),
        }
    }

    fn send(&mut self, kind: CommandKind) -> u32 {
        self.seq += 1;
        let up = Uplink {
            seq: self.seq,
            kind,
        };
        self.sock.send_to(&up.encode(), self.stand_addr).unwrap();
        self.seq
    }

    /// Pump heartbeats and receive for `d`.
    fn pump(&mut self, d: Duration) {
        let end = Instant::now() + d;
        let mut buf = [0u8; 2000];
        loop {
            if self.last_heartbeat.elapsed() >= Duration::from_millis(200) {
                self.last_heartbeat = Instant::now();
                self.send(CommandKind::Heartbeat);
            }
            if let Ok((n, _)) = self.sock.recv_from(&mut buf) {
                if let Ok(msg) = Downlink::decode(&buf[..n]) {
                    self.inbox.push((Instant::now(), msg));
                }
            }
            if Instant::now() >= end {
                break;
            }
        }
    }

    /// Pump until `pred` matches a received message (returns its arrival time) or time out.
    fn wait_for(
        &mut self,
        timeout: Duration,
        mut pred: impl FnMut(&Downlink) -> bool,
    ) -> Option<Instant> {
        let end = Instant::now() + timeout;
        let mut scanned = 0;
        loop {
            self.pump(Duration::from_millis(10));
            for (t, m) in &self.inbox[scanned..] {
                if pred(m) {
                    return Some(*t);
                }
            }
            scanned = self.inbox.len();
            if Instant::now() >= end {
                return None;
            }
        }
    }

    fn wait_ack(&mut self, seq: u32) -> AckResult {
        let mut result = None;
        let got = self.wait_for(Duration::from_secs(2), |m| match m {
            Downlink::Ack(a) if a.seq == seq => {
                result = Some(a.result.clone());
                true
            }
            _ => false,
        });
        assert!(got.is_some(), "no ack for seq {seq}");
        result.unwrap()
    }

    fn command(&mut self, kind: CommandKind) -> AckResult {
        let seq = self.send(kind);
        self.wait_ack(seq)
    }

    fn wait_links_up(&mut self) {
        let ok = self.wait_for(Duration::from_secs(5), |m| {
            matches!(m, Downlink::StandStatus(s) if s.actuation_link_ok && s.loadcell_link_ok)
        });
        assert!(ok.is_some(), "Arduino links never came up: {:?}", self.events());
    }

    fn events(&self) -> Vec<String> {
        self.inbox
            .iter()
            .filter_map(|(_, m)| match m {
                Downlink::Event(e) => Some(e.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn event_time(&self, text: &str) -> Option<Instant> {
        self.inbox.iter().find_map(|(t, m)| match m {
            Downlink::Event(e) if e.text == text => Some(*t),
            _ => None,
        })
    }

    fn last_status(&self) -> Option<gs_protocol::StandStatus> {
        self.inbox.iter().rev().find_map(|(_, m)| match m {
            Downlink::StandStatus(s) => Some(s.clone()),
            _ => None,
        })
    }

    fn last_telemetry(&self) -> Option<gs_protocol::StandTelemetry> {
        self.inbox.iter().rev().find_map(|(_, m)| match m {
            Downlink::Stand(t) => Some(t.clone()),
            _ => None,
        })
    }

    fn world(&self) -> FakeWorld {
        self.world.lock().unwrap().clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn arm(h: &mut Harness) {
    h.wait_links_up();
    assert_eq!(h.command(CommandKind::Stand(StandCommand::Arm)), AckResult::Accepted);
}

#[test]
fn igniter_check_runs_with_correct_timing() {
    let mut h = Harness::start(1.0);
    arm(&mut h);

    let seq = h.send(CommandKind::Stand(StandCommand::StartSequence("igniter_check".into())));
    assert_eq!(h.wait_ack(seq), AckResult::Accepted);
    let t0 = h
        .wait_for(Duration::from_secs(1), |m| {
            matches!(m, Downlink::Event(e) if e.text.starts_with("sequence 'igniter_check' T-0"))
        })
        .expect("T-0 event");

    let done = h.wait_for(Duration::from_secs(12), |m| {
        matches!(m, Downlink::Event(e) if e.text.starts_with("sequence 'igniter_check' complete"))
    });
    assert!(done.is_some(), "sequence did not complete: {:?}", h.events());
    h.pump(Duration::from_millis(300));

    // Every step, in order, at the right time (±30 ms relative to T-0).
    let expected = [
        (0.0, "T+0.0 OMV open"),
        (0.1, "T+0.1 OMV open"),
        (1.0, "T+1.0 IGV open"),
        (1.2, "T+1.2 igniter on"),
        (1.7, "T+1.7 MTV profile start (2.8 s)"),
        (4.5, "T+4.5 IGV close"),
        (4.5, "T+4.5 MTV profile end, holding 100.0 %"),
        (4.6, "T+4.6 igniter off"),
        (5.5, "T+5.5 OMV close"),
        (5.6, "T+5.6 PUISO open"),
        (5.7, "T+5.7 PUMV open"),
        (7.7, "T+7.7 PUISO close"),
        (8.7, "T+8.7 PUMV close"),
        (8.8, "T+8.8 PUMV close"),
    ];
    let events = h.events();
    let mut last_idx = 0;
    for (t_expected, text) in expected {
        let idx = events
            .iter()
            .position(|e| e == text)
            .unwrap_or_else(|| panic!("missing event {text:?} in {events:?}"));
        assert!(idx >= last_idx, "{text:?} out of order in {events:?}");
        last_idx = idx;
        let at = h.event_time(text).unwrap();
        let dt = at.duration_since(t0).as_secs_f64();
        assert!(
            (dt - t_expected).abs() <= 0.030,
            "{text}: expected T+{t_expected:.3}, observed T+{dt:.3}"
        );
    }

    // Hardware end state: burn over, purge closed, igniter off, MTV held at profile end.
    let w = h.world();
    assert!(!w.omv_open && !w.igv_open && !w.puiso_open && !w.pumv_open);
    assert!(!w.kaboom, "igniter must be off after igniter_check");
    assert!(!w.sync, "DAQ sync dropped at sequence end");
    assert!((w.mtv_percent - 100.0).abs() < 0.01, "{}", w.mtv_percent);
    assert_eq!(w.merged_commands, 0, "commands were sent too close together");
    let act: Vec<&str> = w
        .commands
        .iter()
        .filter(|(r, _)| *r == gs_stand::arduino::Role::Actuation)
        .map(|(_, c)| c.as_str())
        .filter(|c| *c != "sync status")
        .collect();
    assert_eq!(
        act,
        vec![
            "sync high", "omv open", "omv open", "igv open", "kaboom start", "igv close",
            "kaboom end", "omv close", "puiso open", "pumv open", "puiso close", "pumv close",
            "pumv close", "sync low",
        ],
        "{act:?}"
    );

    // Back to Armed with no sequence.
    let st = h.last_status().unwrap();
    assert_eq!(st.mode, StandMode::Armed);
    assert!(st.sequence.is_none());
    assert!(st.sequences.contains(&"hotfire".to_string()));
    let tele = h.last_telemetry().unwrap();
    assert_eq!(tele.source, gs_protocol::Source::Stand);
    let omv = tele.valves.iter().find(|v| v.id == ValveId::Omv).unwrap();
    assert_eq!(omv.state, ValveState::Closed);
    assert_eq!(tele.mtv_percent, Some(100.0));
}

#[test]
fn abort_mid_sequence_safes_the_stand() {
    let mut h = Harness::start(1.0);
    arm(&mut h);
    // Open a vent-closing state first so we can see safing reopen the vents.
    assert_eq!(
        h.command(CommandKind::SetValve { id: ValveId::OVnt, open: false }),
        AckResult::Accepted
    );
    let seq = h.send(CommandKind::Stand(StandCommand::StartSequence("igniter_check".into())));
    assert_eq!(h.wait_ack(seq), AckResult::Accepted);
    h.wait_for(Duration::from_secs(3), |m| {
        matches!(m, Downlink::Event(e) if e.text == "T+1.2 igniter on")
    })
    .expect("igniter on event");
    h.pump(Duration::from_millis(100));
    {
        let w = h.world();
        assert!(w.omv_open && w.igv_open && w.kaboom && w.sync);
        assert!(!w.ovent_open);
    }

    // Manual commands are refused mid-sequence; Abort is not.
    assert!(matches!(
        h.command(CommandKind::SetValve { id: ValveId::Omv, open: false }),
        AckResult::Rejected(r) if r.contains("Abort")
    ));
    let t_abort = Instant::now();
    assert_eq!(h.command(CommandKind::Stand(StandCommand::Abort)), AckResult::Accepted);
    // reset all + 11 list commands + sync low at 30 ms spacing ≈ 0.4 s.
    h.pump(Duration::from_millis(800));

    let w = h.world();
    assert!(!w.kaboom, "igniter off");
    assert!(!w.omv_open && !w.igv_open && !w.ofill_open && !w.pumv_open && !w.puiso_open && !w.pufill_open);
    assert!(w.ovent_open && w.puvent_open && w.lfvent_open, "vents opened");
    assert!(w.oiso_close_pin && !w.oiso_open_pin, "OISO driven closed");
    assert!(!w.sync, "DAQ sync low");
    assert_eq!(w.mtv_percent, 0.0, "MTV closed");
    assert_eq!(w.merged_commands, 0);
    // Igniter was the first thing to go off after the abort.
    let after_abort: Vec<&str> = w
        .commands
        .iter()
        .filter(|(r, c)| *r == gs_stand::arduino::Role::Actuation && c != "sync status")
        .map(|(_, c)| c.as_str())
        .skip_while(|c| *c != "kaboom end")
        .collect();
    assert_eq!(after_abort[0], "kaboom end");
    assert_eq!(after_abort[1], "reset all");
    assert_eq!(*after_abort.last().unwrap(), "sync low");
    assert!(t_abort.elapsed() < Duration::from_secs(2));

    let st = h.last_status().unwrap();
    assert_eq!(st.mode, StandMode::Safe);
    assert!(st.sequence.is_none());
    assert!(h.inbox.iter().any(|(_, m)| matches!(
        m,
        Downlink::Event(e) if e.severity == Severity::Critical && e.text.contains("ABORT")
    )));
    // No further sequence steps fired after the abort.
    assert!(!h.events().iter().any(|e| e == "T+4.5 IGV close"));
}

#[test]
fn interlocks_and_manual_control_over_the_wire() {
    let mut h = Harness::start(1.0);
    h.wait_links_up();
    // Safe: manual outputs rejected, Disarm accepted (safing), DAQ sync accepted.
    assert!(matches!(
        h.command(CommandKind::SetValve { id: ValveId::Omv, open: true }),
        AckResult::Rejected(r) if r.contains("Safe")
    ));
    assert!(matches!(
        h.command(CommandKind::Stand(StandCommand::SetMtvPercent(20.0))),
        AckResult::Rejected(_)
    ));
    assert_eq!(h.command(CommandKind::Stand(StandCommand::Disarm)), AckResult::Accepted);
    assert_eq!(
        h.command(CommandKind::Stand(StandCommand::SetOutput {
            id: gs_protocol::StandOutput::DaqSync,
            on: true
        })),
        AckResult::Accepted
    );
    // Vehicle-only command is refused.
    assert!(matches!(h.command(CommandKind::Launch), AckResult::Rejected(_)));

    assert_eq!(h.command(CommandKind::Stand(StandCommand::Arm)), AckResult::Accepted);
    assert_eq!(
        h.command(CommandKind::SetValve { id: ValveId::Omv, open: true }),
        AckResult::Accepted
    );
    assert_eq!(
        h.command(CommandKind::Stand(StandCommand::SetMtvPercent(20.0))),
        AckResult::Accepted
    );
    assert!(matches!(
        h.command(CommandKind::Stand(StandCommand::SetMtvPercent(120.0))),
        AckResult::Rejected(_)
    ));
    assert!(matches!(
        h.command(CommandKind::Stand(StandCommand::StartSequence("nope".into()))),
        AckResult::Rejected(r) if r.contains("nope")
    ));
    // Start load cells by hand and check telemetry channels appear and react.
    assert_eq!(
        h.command(CommandKind::Stand(StandCommand::StartSequence("rcs".into()))),
        AckResult::Accepted
    );
    h.pump(Duration::from_millis(600));
    let w = h.world();
    assert!(w.omv_open);
    assert!((w.mtv_percent - 20.0).abs() < 0.01);
    assert!(w.sync, "sequence start raised sync");
    assert!(w.streaming[2], "rcs streaming");
    let tele = h.last_telemetry().unwrap();
    assert!(
        tele.channels.iter().any(|(c, _)| *c == gs_protocol::StandChannel::RcsThrust),
        "{:?}",
        tele.channels
    );
    assert_eq!(tele.mtv_percent, Some(20.0));
    assert!(tele.valves.iter().any(|v| v.id == ValveId::Omv && v.state == ValveState::Open));
    // OISO never commanded: Unknown.
    assert!(tele.valves.iter().any(|v| v.id == ValveId::OIso && v.state == ValveState::Unknown));
    let st = h.last_status().unwrap();
    assert_eq!(st.mode, StandMode::Sequence);
    let prog = st.sequence.unwrap();
    assert_eq!(prog.name, "rcs");
    assert!(prog.next_step.is_some());
    assert!((prog.duration_s - 5.7).abs() < 1e-3);
}

#[test]
fn ground_loss_while_armed_safes() {
    let mut h = Harness::start(1.0);
    arm(&mut h);
    assert_eq!(
        h.command(CommandKind::SetValve { id: ValveId::Omv, open: true }),
        AckResult::Accepted
    );
    h.pump(Duration::from_millis(100));
    assert!(h.world().omv_open);
    // Stop heartbeating for longer than the (test) 1 s timeout.
    thread::sleep(Duration::from_millis(1600));
    h.pump(Duration::from_millis(600));
    let st = h.last_status().unwrap();
    assert_eq!(st.mode, StandMode::Safe);
    assert!(!h.world().omv_open, "safing closed OMV");
    assert!(h.events().iter().any(|e| e.contains("ground link lost while Armed")));
    assert!(h.events().iter().any(|e| e == "ground link restored"));
}
