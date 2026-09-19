//! `gs-mock`: a fake vehicle for UI development. Speaks the real protocol through
//! `gs_protocol::VehicleLink` and enforces the same command rules as the flight software.

mod physics;
mod stand;
mod vehicle;

use std::time::{Duration, Instant};

use clap::{ArgAction, Parser};
use gs_protocol::{Downlink, VehicleLink};

use crate::vehicle::Vehicle;

const FLIGHT_PERIOD: Duration = Duration::from_millis(20);
const STAND_PERIOD_S: f64 = 0.05;

#[derive(Parser, Debug)]
#[command(version, about = "Fake GTPL vehicle for ground-station UI development")]
struct Args {
    /// UDP port to listen on for the bridge's heartbeats and commands.
    #[arg(long, default_value_t = gs_protocol::DEFAULT_VEHICLE_PORT)]
    port: u16,

    /// Return to Standby 5 s after landing or termination (`--auto-reset false` to disable).
    #[arg(long, action = ArgAction::Set, default_value_t = true, value_name = "BOOL")]
    auto_reset: bool,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let mut link = VehicleLink::bind(args.port)?;
    eprintln!(
        "[mock] listening on UDP {}; telemetry starts when a bridge heartbeats",
        args.port
    );

    let mut vehicle = Vehicle::new(args.auto_reset);
    let mut ground = None;
    let mut next_stand_s = 0.0;
    let mut next_step = Instant::now();

    loop {
        while let Some(command) = link.poll() {
            let result = vehicle.handle_command(&command.kind);
            link.ack(&command, vehicle.time_s(), result);
        }
        if link.ground_addr() != ground {
            ground = link.ground_addr();
            if let Some(addr) = ground {
                eprintln!("[mock] sending telemetry to {addr}");
            }
        }

        vehicle.step(FLIGHT_PERIOD.as_secs_f64());

        for message in vehicle.drain_outbox() {
            link.send(&message);
        }
        link.send(&Downlink::Flight(vehicle.flight_telemetry(link.link_age_s())));
        if vehicle.time_s() >= next_stand_s {
            next_stand_s += STAND_PERIOD_S;
            link.send(&Downlink::Stand(vehicle.stand_telemetry()));
        }

        // Fixed-rate loop; if the process was suspended, skip ahead instead of bursting.
        next_step += FLIGHT_PERIOD;
        let now = Instant::now();
        if next_step < now {
            next_step = now;
        }
        std::thread::sleep(next_step - now);
    }
}
