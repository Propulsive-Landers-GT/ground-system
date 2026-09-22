//! Which of the two UDP endpoints a command goes to, and which one a downlink packet
//! came from. Pure functions over the configured addresses, so they are testable and
//! the same decision is never made twice in different ways.

use std::net::SocketAddr;

use gs_protocol::{CommandKind, Downlink, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// The Lander flight software or the simulator (`--vehicle`).
    Vehicle,
    /// The test-stand adapter `gs-stand` (`--stand`).
    Stand,
}

impl Endpoint {
    pub fn name(self) -> &'static str {
        match self {
            Endpoint::Vehicle => "vehicle",
            Endpoint::Stand => "test stand",
        }
    }
}

/// An operator command with its destination already decided by [`Endpoints::route`].
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedCommand {
    pub endpoint: Endpoint,
    pub kind: CommandKind,
}

/// The addresses the bridge talks to. Both share the one listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoints {
    pub vehicle: SocketAddr,
    pub stand: Option<SocketAddr>,
}

pub const NO_STAND: &str = "no test stand configured";

impl Endpoints {
    pub fn addr(&self, endpoint: Endpoint) -> Option<SocketAddr> {
        match endpoint {
            Endpoint::Vehicle => Some(self.vehicle),
            Endpoint::Stand => self.stand,
        }
    }

    /// Where an operator command goes. `Stand(_)` only the stand can act on; `SetValve`
    /// prefers the real stand over the vehicle's checkout valve map when both exist;
    /// everything else is a flight command.
    pub fn route(&self, kind: &CommandKind) -> Result<Endpoint, &'static str> {
        match kind {
            CommandKind::Stand(_) => match self.stand {
                Some(_) => Ok(Endpoint::Stand),
                None => Err(NO_STAND),
            },
            CommandKind::SetValve { .. } if self.stand.is_some() => Ok(Endpoint::Stand),
            _ => Ok(Endpoint::Vehicle),
        }
    }

    /// Which endpoint a downlink packet belongs to. The source address is authoritative
    /// when it matches a configured endpoint; otherwise (NAT, a vehicle replying from a
    /// different interface) the packet's own `Source` tag decides, and `StandStatus`
    /// only ever comes from the stand. Without a configured stand everything is the
    /// vehicle's, so its counters and connect/disconnect tracking stay in one place.
    pub fn attribute(&self, from: SocketAddr, packet: &Downlink) -> Endpoint {
        if self.stand.is_none() {
            return Endpoint::Vehicle;
        }
        if self.stand == Some(from) {
            return Endpoint::Stand;
        }
        if from == self.vehicle {
            return Endpoint::Vehicle;
        }
        match packet {
            Downlink::StandStatus(_) => Endpoint::Stand,
            Downlink::Flight(f) if f.source == Source::Stand => Endpoint::Stand,
            Downlink::Stand(s) if s.source == Source::Stand => Endpoint::Stand,
            _ => Endpoint::Vehicle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gs_protocol::{
        AckResult, CommandAck, StandCommand, StandMode, StandOutput, StandStatus,
        StandTelemetry, ValveId,
    };

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn with_stand() -> Endpoints {
        Endpoints {
            vehicle: addr("10.0.0.2:8888"),
            stand: Some(addr("10.0.0.3:8888")),
        }
    }

    fn without_stand() -> Endpoints {
        Endpoints {
            vehicle: addr("10.0.0.2:8888"),
            stand: None,
        }
    }

    fn set_valve() -> CommandKind {
        CommandKind::SetValve {
            id: ValveId::Omv,
            open: true,
        }
    }

    #[test]
    fn stand_commands_go_to_the_stand() {
        let e = with_stand();
        for cmd in [
            StandCommand::Arm,
            StandCommand::Disarm,
            StandCommand::Abort,
            StandCommand::SetMtvPercent(50.0),
            StandCommand::SetOutput {
                id: StandOutput::Igniter,
                on: true,
            },
            StandCommand::StartSequence("hotfire".into()),
        ] {
            assert_eq!(e.route(&CommandKind::Stand(cmd)), Ok(Endpoint::Stand));
        }
    }

    #[test]
    fn stand_commands_error_without_a_stand() {
        let e = without_stand();
        assert_eq!(
            e.route(&CommandKind::Stand(StandCommand::Arm)),
            Err("no test stand configured")
        );
    }

    #[test]
    fn set_valve_prefers_the_stand_when_configured() {
        assert_eq!(with_stand().route(&set_valve()), Ok(Endpoint::Stand));
        assert_eq!(without_stand().route(&set_valve()), Ok(Endpoint::Vehicle));
    }

    #[test]
    fn everything_else_goes_to_the_vehicle() {
        for e in [with_stand(), without_stand()] {
            for kind in [
                CommandKind::Heartbeat,
                CommandKind::Arm,
                CommandKind::Disarm,
                CommandKind::Launch,
                CommandKind::Abort,
                CommandKind::SetPhase(gs_protocol::FlightPhase::Descent),
                CommandKind::RequestParams,
                CommandKind::SetMpcWeights(None),
                CommandKind::SetControlMode(gs_protocol::ControlMode::Jog),
                CommandKind::Jog(gs_protocol::JogSetpoint {
                    gimbal_theta: 0.0,
                    gimbal_phi: 0.0,
                    thrust: 0.0,
                    rcs: 0,
                }),
            ] {
                assert_eq!(e.route(&kind), Ok(Endpoint::Vehicle), "{kind:?}");
            }
        }
    }

    #[test]
    fn addr_lookup_follows_configuration() {
        let e = with_stand();
        assert_eq!(e.addr(Endpoint::Vehicle), Some(addr("10.0.0.2:8888")));
        assert_eq!(e.addr(Endpoint::Stand), Some(addr("10.0.0.3:8888")));
        assert_eq!(without_stand().addr(Endpoint::Stand), None);
    }

    fn ack() -> Downlink {
        Downlink::Ack(CommandAck {
            seq: 1,
            time_s: 0.0,
            result: AckResult::Accepted,
        })
    }

    fn stand_status() -> Downlink {
        Downlink::StandStatus(StandStatus {
            time_s: 0.0,
            mode: StandMode::Safe,
            actuation_link_ok: true,
            loadcell_link_ok: true,
            sequences: Vec::new(),
            sequence: None,
        })
    }

    fn stand_telemetry(source: Source) -> Downlink {
        Downlink::Stand(StandTelemetry {
            time_s: 0.0,
            source,
            channels: Vec::new(),
            valves: Vec::new(),
            outputs_on: Vec::new(),
            mtv_percent: None,
        })
    }

    #[test]
    fn source_address_attributes_packets() {
        let e = with_stand();
        // The address wins over the content in both directions.
        assert_eq!(e.attribute(addr("10.0.0.2:8888"), &stand_status()), Endpoint::Vehicle);
        assert_eq!(e.attribute(addr("10.0.0.3:8888"), &ack()), Endpoint::Stand);
        assert_eq!(
            e.attribute(addr("10.0.0.3:8888"), &stand_telemetry(Source::Sim)),
            Endpoint::Stand
        );
    }

    #[test]
    fn unknown_address_falls_back_to_the_source_tag() {
        let e = with_stand();
        let stranger = addr("192.168.1.9:4000");
        assert_eq!(e.attribute(stranger, &stand_status()), Endpoint::Stand);
        assert_eq!(
            e.attribute(stranger, &stand_telemetry(Source::Stand)),
            Endpoint::Stand
        );
        assert_eq!(
            e.attribute(stranger, &stand_telemetry(Source::Sim)),
            Endpoint::Vehicle
        );
        assert_eq!(e.attribute(stranger, &ack()), Endpoint::Vehicle);

        let mut flight = crate::messages::tests::sample_flight(1);
        flight.source = Source::Stand;
        assert_eq!(e.attribute(stranger, &Downlink::Flight(flight)), Endpoint::Stand);
        assert_eq!(
            e.attribute(
                stranger,
                &Downlink::Flight(crate::messages::tests::sample_flight(1))
            ),
            Endpoint::Vehicle
        );
    }

    #[test]
    fn without_a_stand_everything_is_the_vehicle() {
        let e = without_stand();
        assert_eq!(e.attribute(addr("1.2.3.4:5"), &stand_status()), Endpoint::Vehicle);
        assert_eq!(
            e.attribute(addr("1.2.3.4:5"), &stand_telemetry(Source::Stand)),
            Endpoint::Vehicle
        );
    }
}
