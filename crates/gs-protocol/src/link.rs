//! Vehicle-side UDP endpoint. Non-blocking, so it is safe to poll from a real-time loop.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Instant;

use crate::{
    AckResult, CommandAck, DecodeError, Downlink, Uplink, DEFAULT_VEHICLE_PORT, MAX_PACKET_LEN,
};

/// Binds the vehicle's command port, receives [`Uplink`]s and sends [`Downlink`]s.
///
/// Telemetry goes to whichever ground station was heard from last, so nothing needs
/// to be configured on the vehicle: start the bridge, it heartbeats, telemetry flows.
/// A fixed destination can be given for receive-only ground setups.
pub struct VehicleLink {
    socket: UdpSocket,
    ground: Option<SocketAddr>,
    last_rx: Option<Instant>,
    rx_buf: [u8; MAX_PACKET_LEN],
}

impl VehicleLink {
    pub fn bind(port: u16) -> io::Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            ground: None,
            last_rx: None,
            rx_buf: [0; MAX_PACKET_LEN],
        })
    }

    pub fn bind_default() -> io::Result<Self> {
        Self::bind(DEFAULT_VEHICLE_PORT)
    }

    /// Send telemetry here until a ground station is heard from.
    pub fn set_ground_addr(&mut self, addr: SocketAddr) {
        self.ground = Some(addr);
    }

    pub fn ground_addr(&self) -> Option<SocketAddr> {
        self.ground
    }

    /// Seconds since the last valid uplink packet.
    pub fn link_age_s(&self) -> Option<f32> {
        self.last_rx.map(|t| t.elapsed().as_secs_f32())
    }

    /// Returns the next pending command, or `None` when the socket is drained.
    /// Packets that fail to decode are dropped.
    pub fn poll(&mut self) -> Option<Uplink> {
        loop {
            let (len, from) = match self.socket.recv_from(&mut self.rx_buf) {
                Ok(v) => v,
                // WouldBlock means drained; any other error is treated the same way so
                // a flaky interface can never stall the control loop.
                Err(_) => return None,
            };
            match Uplink::decode(&self.rx_buf[..len]) {
                Ok(cmd) => {
                    self.ground = Some(from);
                    self.last_rx = Some(Instant::now());
                    return Some(cmd);
                }
                Err(DecodeError::VersionMismatch(v)) => {
                    eprintln!("[link] dropping uplink from {from}: protocol version {v}");
                }
                Err(_) => {}
            }
        }
    }

    /// Best-effort send; telemetry is dropped if no ground station is known yet.
    pub fn send(&self, msg: &Downlink) {
        if let Some(addr) = self.ground {
            let _ = self.socket.send_to(&msg.encode(), addr);
        }
    }

    pub fn ack(&self, cmd: &Uplink, time_s: f64, result: AckResult) {
        if cmd.kind.wants_ack() {
            self.send(&Downlink::Ack(CommandAck {
                seq: cmd.seq,
                time_s,
                result,
            }));
        }
    }
}
