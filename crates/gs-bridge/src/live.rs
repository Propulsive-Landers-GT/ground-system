//! The live UDP link: receives downlink packets, heartbeats the vehicle and forwards
//! browser commands. One task owns the socket and all link state, so nothing is locked.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gs_protocol::{CommandKind, DecodeError, Downlink, Uplink, HEARTBEAT_HZ, MAX_PACKET_LEN};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{info, warn};

use crate::hub::Hub;
use crate::link_stats::LinkStats;
use crate::messages::{LogRecord, SentCommand, ServerMessage};
use crate::recorder::{unix_time_s, Recorder};

const VERSION_WARNING_INTERVAL: Duration = Duration::from_secs(5);
/// An outage longer than this may have been a vehicle reboot, so params are re-requested.
const REBOOT_GAP: Duration = Duration::from_secs(5);

pub struct LiveLink {
    pub socket: UdpSocket,
    pub vehicle_addr: SocketAddr,
    pub hub: Arc<Hub>,
    pub recorder: Option<Recorder>,
    /// Commands from browsers, already validated.
    pub commands: mpsc::Receiver<CommandKind>,
}

impl LiveLink {
    pub async fn run(self) {
        let Self {
            socket,
            vehicle_addr,
            hub,
            recorder,
            mut commands,
        } = self;
        let mut state = LinkState {
            socket,
            vehicle_addr,
            hub,
            recorder,
            stats: LinkStats::default(),
            next_seq: 1,
            connected: false,
            disconnected_at: None,
            last_version_warning: None,
        };

        // Heartbeat and link status share one 2 Hz tick.
        let mut tick = interval(Duration::from_secs_f64(1.0 / HEARTBEAT_HZ));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // Slack above the protocol maximum so an oversized datagram fails to decode
        // instead of being silently truncated into something that might.
        let mut buf = [0u8; MAX_PACKET_LEN + 1];

        loop {
            tokio::select! {
                received = state.socket.recv_from(&mut buf) => match received {
                    Ok((len, from)) => state.on_datagram(&buf[..len], from).await,
                    Err(e) => {
                        // E.g. an ICMP "port unreachable" surfacing while the vehicle is down.
                        warn!("UDP receive error: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                Some(kind) = commands.recv() => state.send_command(kind).await,
                _ = tick.tick() => state.on_tick().await,
            }
        }
    }
}

struct LinkState {
    socket: UdpSocket,
    vehicle_addr: SocketAddr,
    hub: Arc<Hub>,
    recorder: Option<Recorder>,
    stats: LinkStats,
    next_seq: u32,
    connected: bool,
    disconnected_at: Option<Instant>,
    last_version_warning: Option<Instant>,
}

impl LinkState {
    async fn on_datagram(&mut self, datagram: &[u8], from: SocketAddr) {
        let packet = match Downlink::decode(datagram) {
            Ok(packet) => packet,
            Err(DecodeError::VersionMismatch(version)) => {
                self.warn_version_mismatch(version, from);
                return;
            }
            // Stray traffic on the port is not worth a log line per packet.
            Err(_) => return,
        };

        let now = Instant::now();
        self.stats.on_packet(now);
        if let Downlink::Flight(flight) = &packet {
            self.stats.on_flight_seq(flight.seq, now);
        }

        let message = ServerMessage::from(packet);
        self.hub.publish(&message);
        if let Some(recorder) = &mut self.recorder {
            recorder.record(&LogRecord::Down {
                t: unix_time_s(),
                message,
            });
        }

        if !self.connected {
            self.connected = true;
            info!("vehicle connected: receiving telemetry from {from}");
            // The vehicle only sends params on request or change, so ask when it first
            // appears. A short dropout (a stalled flight loop, a radio fade) is the same
            // vehicle with the same params; only a long one could have been a reboot.
            let rebooted = self
                .disconnected_at
                .is_none_or(|t| now.duration_since(t) > REBOOT_GAP);
            if rebooted {
                self.send_command(CommandKind::RequestParams).await;
            }
        }
    }

    async fn on_tick(&mut self) {
        let now = Instant::now();
        if self.connected && !self.stats.connected(now) {
            self.connected = false;
            self.disconnected_at = Some(now);
            warn!("vehicle disconnected: no telemetry for over 1 s");
        }

        self.send_uplink(CommandKind::Heartbeat).await;

        let recording = self.recorder.as_mut().and_then(|recorder| {
            recorder.flush();
            recorder.active_path()
        });
        let status = self
            .stats
            .status(now, self.vehicle_addr.to_string(), recording);
        self.hub.publish(&ServerMessage::Link(status));
    }

    /// Sends an operator command, echoes it to every client and logs it.
    async fn send_command(&mut self, kind: CommandKind) {
        let Some(seq) = self.send_uplink(kind.clone()).await else {
            self.hub
                .publish(&ServerMessage::error("command could not be sent to the vehicle"));
            return;
        };
        let command = SentCommand { seq, kind };
        if let Some(recorder) = &mut self.recorder {
            recorder.record(&LogRecord::Up {
                t: unix_time_s(),
                command: command.clone(),
            });
        }
        self.hub.publish(&ServerMessage::Sent(command));
    }

    /// Heartbeats and commands share one counter so `seq` is monotonic on the wire.
    async fn send_uplink(&mut self, kind: CommandKind) -> Option<u32> {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let is_heartbeat = kind == CommandKind::Heartbeat;
        let packet = Uplink { seq, kind }.encode();
        match self.socket.send_to(&packet, self.vehicle_addr).await {
            Ok(_) => Some(seq),
            Err(e) => {
                // A down interface would otherwise log twice a second.
                if !is_heartbeat {
                    warn!("UDP send to {} failed: {e}", self.vehicle_addr);
                }
                None
            }
        }
    }

    fn warn_version_mismatch(&mut self, version: u8, from: SocketAddr) {
        let now = Instant::now();
        let due = self
            .last_version_warning
            .is_none_or(|t| now.duration_since(t) >= VERSION_WARNING_INTERVAL);
        if due {
            self.last_version_warning = Some(now);
            warn!(
                "dropping packets from {from}: protocol version {version}, this bridge speaks {}",
                gs_protocol::PROTOCOL_VERSION
            );
        }
    }
}
