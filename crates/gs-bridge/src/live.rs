//! The live UDP link: receives downlink packets from the vehicle and the test stand,
//! heartbeats both and forwards browser commands. One task owns the socket and all
//! link state, so nothing is locked.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gs_protocol::{CommandKind, DecodeError, Downlink, Uplink, HEARTBEAT_HZ, MAX_PACKET_LEN};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, MissedTickBehavior};
use tracing::{info, warn};

use crate::hub::Hub;
use crate::link_stats::LinkStats;
use crate::messages::{ControlMessage, LogRecord, SentCommand, ServerMessage};
use crate::recorder::{unix_time_s, Recorder};
use crate::routing::{Endpoint, Endpoints, RoutedCommand};

const VERSION_WARNING_INTERVAL: Duration = Duration::from_secs(5);
/// An outage longer than this may have been a vehicle reboot, so params are re-requested.
const REBOOT_GAP: Duration = Duration::from_secs(5);

/// What browsers ask of the link task.
#[derive(Debug)]
pub enum LinkRequest {
    /// An operator command, already validated and routed.
    Command(RoutedCommand),
    /// A recording control; the outcome goes back to the asking client.
    Control {
        control: ControlMessage,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

pub struct LiveLink {
    pub socket: UdpSocket,
    pub endpoints: Endpoints,
    pub hub: Arc<Hub>,
    /// Where recordings are created.
    pub log_dir: PathBuf,
    /// A recording already started from the command line, if any.
    pub recorder: Option<Recorder>,
    pub requests: mpsc::Receiver<LinkRequest>,
}

impl LiveLink {
    pub async fn run(self) {
        let Self {
            socket,
            endpoints,
            hub,
            log_dir,
            recorder,
            mut requests,
        } = self;
        let mut state = LinkState {
            socket,
            endpoints,
            vehicle: Peer::new(Endpoint::Vehicle, endpoints.vehicle),
            stand: endpoints.stand.map(|addr| Peer::new(Endpoint::Stand, addr)),
            hub,
            log_dir,
            recorder,
            next_seq: 1,
            last_version_warning: None,
        };

        // Heartbeats and link status share one 2 Hz tick.
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
                        // E.g. an ICMP "port unreachable" surfacing while a peer is down.
                        warn!("UDP receive error: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                Some(request) = requests.recv() => match request {
                    LinkRequest::Command(command) => state.send_command(command).await,
                    LinkRequest::Control { control, reply } => {
                        // The client may have gone away; nothing to do about that.
                        let _ = reply.send(state.on_control(control));
                    }
                },
                _ = tick.tick() => state.on_tick().await,
            }
        }
    }
}

/// One remote endpoint: its address, packet statistics and connection state.
struct Peer {
    endpoint: Endpoint,
    addr: SocketAddr,
    stats: LinkStats,
    connected: bool,
    disconnected_at: Option<Instant>,
}

impl Peer {
    fn new(endpoint: Endpoint, addr: SocketAddr) -> Self {
        Self {
            endpoint,
            addr,
            stats: LinkStats::default(),
            connected: false,
            disconnected_at: None,
        }
    }

    /// Records a packet; returns `true` when this is the first one after an outage.
    fn on_packet(&mut self, now: Instant, from: SocketAddr) -> bool {
        self.stats.on_packet(now);
        if self.connected {
            return false;
        }
        self.connected = true;
        info!(
            "{} connected: receiving telemetry from {from}",
            self.endpoint.name()
        );
        true
    }

    fn check_timeout(&mut self, now: Instant) {
        if self.connected && !self.stats.connected(now) {
            self.connected = false;
            self.disconnected_at = Some(now);
            warn!(
                "{} disconnected: no telemetry for over 1 s",
                self.endpoint.name()
            );
        }
    }
}

struct LinkState {
    socket: UdpSocket,
    endpoints: Endpoints,
    vehicle: Peer,
    stand: Option<Peer>,
    hub: Arc<Hub>,
    log_dir: PathBuf,
    recorder: Option<Recorder>,
    next_seq: u32,
    last_version_warning: Option<Instant>,
}

impl LinkState {
    fn on_control(&mut self, control: ControlMessage) -> Result<(), String> {
        match control {
            ControlMessage::StartRecording { name } => {
                if let Some(recorder) = &self.recorder {
                    return Err(format!(
                        "already recording to {}",
                        recorder.dir().display()
                    ));
                }
                let recorder = Recorder::start(&self.log_dir, name.as_deref())
                    .map_err(|e| format!("could not start recording: {e}"))?;
                info!("recording to {}", recorder.dir().display());
                self.recorder = Some(recorder);
            }
            ControlMessage::StopRecording => {
                let Some(recorder) = self.recorder.take() else {
                    return Err("not recording".into());
                };
                info!("recording to {} stopped", recorder.dir().display());
                recorder.stop();
            }
        }
        // Tell every client straight away rather than at the next 2 Hz tick.
        self.publish_link(Instant::now());
        Ok(())
    }

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
        let endpoint = self.endpoints.attribute(from, &packet);
        let peer = match endpoint {
            Endpoint::Vehicle => &mut self.vehicle,
            // `attribute` only names the stand when one is configured.
            Endpoint::Stand => self.stand.as_mut().unwrap_or(&mut self.vehicle),
        };
        let just_connected = peer.on_packet(now, from);
        // Loss and rate come from the flight sequence number, which only the vehicle has.
        if let (Endpoint::Vehicle, Downlink::Flight(flight)) = (endpoint, &packet) {
            peer.stats.on_flight_seq(flight.seq, now);
        }

        let message = ServerMessage::from(packet);
        self.hub.publish(&message);
        if let Some(recorder) = &mut self.recorder {
            recorder.record(&LogRecord::Down {
                t: unix_time_s(),
                message,
            });
        }

        if just_connected && endpoint == Endpoint::Vehicle {
            // The vehicle only sends params on request or change, so ask when it first
            // appears. A short dropout (a stalled flight loop, a radio fade) is the same
            // vehicle with the same params; only a long one could have been a reboot.
            let rebooted = self
                .vehicle
                .disconnected_at
                .is_none_or(|t| now.duration_since(t) > REBOOT_GAP);
            if rebooted {
                self.send_command(RoutedCommand {
                    endpoint: Endpoint::Vehicle,
                    kind: CommandKind::RequestParams,
                })
                .await;
            }
        }
    }

    async fn on_tick(&mut self) {
        let now = Instant::now();
        self.vehicle.check_timeout(now);
        if let Some(stand) = &mut self.stand {
            stand.check_timeout(now);
        }

        // Both endpoints send telemetry to whoever last heartbeated them.
        self.send_uplink(Endpoint::Vehicle, CommandKind::Heartbeat)
            .await;
        if self.stand.is_some() {
            self.send_uplink(Endpoint::Stand, CommandKind::Heartbeat).await;
        }

        if let Some(recorder) = &mut self.recorder {
            recorder.flush();
            if recorder.failed() {
                // Already logged by the recorder; drop it so a new one can be started.
                self.recorder = None;
            }
        }
        self.publish_link(now);
    }

    fn publish_link(&mut self, now: Instant) {
        let recording = self.recorder.as_ref().and_then(Recorder::active_path);
        let stand = self
            .stand
            .as_ref()
            .map(|peer| peer.stats.stand_status(now, peer.addr.to_string()));
        let status = self.vehicle.stats.status(
            now,
            self.vehicle.addr.to_string(),
            recording,
            stand,
        );
        self.hub.publish(&ServerMessage::Link(status));
    }

    /// Sends an operator command, echoes it to every client and logs it.
    async fn send_command(&mut self, command: RoutedCommand) {
        let RoutedCommand { endpoint, kind } = command;
        let Some(seq) = self.send_uplink(endpoint, kind.clone()).await else {
            self.hub.publish(&ServerMessage::error(format!(
                "command could not be sent to the {}",
                endpoint.name()
            )));
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

    /// Heartbeats and commands to both endpoints share one counter so `seq` is
    /// monotonic on the wire and acks are unambiguous whichever side they come from.
    async fn send_uplink(&mut self, endpoint: Endpoint, kind: CommandKind) -> Option<u32> {
        let Some(addr) = self.endpoints.addr(endpoint) else {
            warn!("no {} configured; dropping {kind:?}", endpoint.name());
            return None;
        };
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let is_heartbeat = kind == CommandKind::Heartbeat;
        let packet = Uplink { seq, kind }.encode();
        match self.socket.send_to(&packet, addr).await {
            Ok(_) => Some(seq),
            Err(e) => {
                // A down interface would otherwise log twice a second.
                if !is_heartbeat {
                    warn!("UDP send to {} ({addr}) failed: {e}", endpoint.name());
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
