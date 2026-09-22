//! `gs-mock`: a fake vehicle (default) or a fake test-stand adapter (`--stand`) for UI
//! development. Speaks the real protocol through `gs_protocol::VehicleLink` and enforces
//! the same command rules as the flight software / `gs-stand`. Run one of each on
//! different ports to exercise a bridge started with both `--vehicle` and `--stand`.

mod physics;
mod stand;
mod test_stand;
mod vehicle;

use std::time::{Duration, Instant};

use clap::{ArgAction, Parser};
use gs_protocol::{Downlink, VehicleLink};

use crate::test_stand::TestStand;
use crate::vehicle::Vehicle;

const FLIGHT_PERIOD: Duration = Duration::from_millis(20);
const STAND_PERIOD: Duration = Duration::from_millis(50);
const STAND_PERIOD_S: f64 = 0.05;
const STAND_STATUS_PERIOD_S: f64 = 0.2;

#[derive(Parser, Debug)]
#[command(version, about = "Fake GTPL vehicle or test stand for ground-station UI development")]
struct Args {
    /// UDP port to listen on for the bridge's heartbeats and commands.
    #[arg(long, default_value_t = gs_protocol::DEFAULT_VEHICLE_PORT)]
    port: u16,

    /// Behave as the test-stand adapter (gs-stand) instead of a vehicle.
    #[arg(long)]
    stand: bool,

    /// Vehicle only: return to Standby 5 s after landing or termination
    /// (`--auto-reset false` to disable).
    #[arg(long, action = ArgAction::Set, default_value_t = true, value_name = "BOOL")]
    auto_reset: bool,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let link = VehicleLink::bind(args.port)?;
    let role = if args.stand { "test stand" } else { "vehicle" };
    eprintln!(
        "[mock] {role} listening on UDP {}; telemetry starts when a bridge heartbeats",
        args.port
    );
    if args.stand {
        run_stand(link)
    } else {
        run_vehicle(link, args.auto_reset)
    }
}

fn run_vehicle(mut link: VehicleLink, auto_reset: bool) -> ! {
    let mut vehicle = Vehicle::new(auto_reset);
    let mut ground = None;
    let mut next_stand_s = 0.0;
    let mut pacer = Pacer::new(FLIGHT_PERIOD);

    loop {
        while let Some(command) = link.poll() {
            let result = vehicle.handle_command(&command.kind);
            link.ack(&command, vehicle.time_s(), result);
        }
        log_ground_change(&link, &mut ground);

        vehicle.step(FLIGHT_PERIOD.as_secs_f64());

        for message in vehicle.drain_outbox() {
            link.send(&message);
        }
        link.send(&Downlink::Flight(vehicle.flight_telemetry(link.link_age_s())));
        if vehicle.time_s() >= next_stand_s {
            next_stand_s += STAND_PERIOD_S;
            link.send(&Downlink::Stand(vehicle.stand_telemetry()));
        }

        pacer.wait();
    }
}

fn run_stand(mut link: VehicleLink) -> ! {
    let mut stand = TestStand::new();
    let mut ground = None;
    let mut next_status_s = 0.0;
    let mut pacer = Pacer::new(STAND_PERIOD);

    loop {
        while let Some(command) = link.poll() {
            let result = stand.handle_command(&command.kind);
            link.ack(&command, stand.time_s(), result);
        }
        log_ground_change(&link, &mut ground);

        stand.step(STAND_PERIOD.as_secs_f64());

        for message in stand.drain_outbox() {
            link.send(&message);
        }
        link.send(&Downlink::Stand(stand.telemetry()));
        if stand.time_s() >= next_status_s {
            next_status_s += STAND_STATUS_PERIOD_S;
            link.send(&Downlink::StandStatus(stand.status()));
        }

        pacer.wait();
    }
}

fn log_ground_change(link: &VehicleLink, ground: &mut Option<std::net::SocketAddr>) {
    if link.ground_addr() != *ground {
        *ground = link.ground_addr();
        if let Some(addr) = ground {
            eprintln!("[mock] sending telemetry to {addr}");
        }
    }
}

/// Fixed-rate loop timing; if the process was suspended, skips ahead instead of bursting.
struct Pacer {
    period: Duration,
    next: Instant,
}

impl Pacer {
    fn new(period: Duration) -> Self {
        Self {
            period,
            next: Instant::now(),
        }
    }

    fn wait(&mut self) {
        self.next += self.period;
        let now = Instant::now();
        if self.next < now {
            self.next = now;
        }
        std::thread::sleep(self.next - now);
    }
}
