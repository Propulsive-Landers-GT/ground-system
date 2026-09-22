//! `gs-bridge`: the ground-side server. See `docs/DESIGN.md`.

mod hub;
mod link_stats;
mod live;
mod messages;
mod recorder;
mod replay;
mod web;

use std::io::IsTerminal;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::hub::Hub;
use crate::live::LiveLink;
use crate::recorder::Recorder;
use crate::replay::Replay;

#[derive(Parser, Debug)]
#[command(version, about = "GTPL ground-station bridge")]
struct Args {
    /// Address of the vehicle or sim that heartbeats and commands are sent to.
    #[arg(long, value_name = "IP:PORT", default_value = "127.0.0.1:8888")]
    vehicle: SocketAddr,

    /// UDP port telemetry is received on.
    #[arg(long, value_name = "PORT", default_value_t = gs_protocol::DEFAULT_GROUND_PORT)]
    listen_udp: u16,

    /// HTTP port for the web UI and the `/ws` WebSocket.
    #[arg(long, value_name = "PORT", default_value_t = 8080)]
    http: u16,

    /// Directory holding the built web UI.
    #[arg(long, value_name = "PATH", default_value = "web/dist")]
    web_dir: PathBuf,

    /// Directory session logs are written to.
    #[arg(long, value_name = "PATH", default_value = "logs")]
    log_dir: PathBuf,

    /// Do not write a session log.
    #[arg(long)]
    no_record: bool,

    /// Play back a recorded session instead of talking to a vehicle.
    #[arg(long, value_name = "FILE.jsonl")]
    replay: Option<PathBuf>,

    /// Playback speed multiplier.
    #[arg(long, value_name = "FACTOR", default_value_t = 1.0, requires = "replay")]
    replay_speed: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .init();
    let args = Args::parse();

    let hub = Arc::new(Hub::new());
    let http_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, args.http));
    let listener = TcpListener::bind(http_addr)
        .await
        .with_context(|| format!("binding HTTP port {}", args.http))?;

    let commands = match &args.replay {
        Some(path) => {
            let replay = Replay::load(path, args.replay_speed)?;
            tokio::spawn(replay.run(hub.clone()));
            None
        }
        None => Some(start_live_link(&args, hub.clone()).await?),
    };

    info!("web UI on http://localhost:{0}/  WebSocket on ws://localhost:{0}/ws", args.http);
    if !args.web_dir.join("index.html").is_file() {
        warn!(
            "{} has no index.html: run `npm run build` in web/ to get the UI",
            args.web_dir.display()
        );
    }

    let app = web::router(web::AppState {
        hub,
        commands,
        web_dir: args.web_dir,
    });
    axum::serve(listener, app)
        .await
        .context("HTTP server failed")
}

/// Binds the ground UDP port and spawns the link task. Returns the command queue into it.
async fn start_live_link(
    args: &Args,
    hub: Arc<Hub>,
) -> anyhow::Result<mpsc::Sender<gs_protocol::CommandKind>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, args.listen_udp))
        .await
        .with_context(|| format!("binding UDP port {}", args.listen_udp))?;
    info!(
        "listening for telemetry on UDP {}, heartbeating vehicle at {}",
        args.listen_udp, args.vehicle
    );

    let recorder = if args.no_record {
        None
    } else {
        let recorder = Recorder::create(&args.log_dir)
            .with_context(|| format!("creating session log in {}", args.log_dir.display()))?;
        info!("recording to {}", recorder.active_path().unwrap_or_default());
        Some(recorder)
    };

    let (commands_tx, commands) = mpsc::channel(64);
    let link = LiveLink {
        socket,
        vehicle_addr: args.vehicle,
        hub,
        recorder,
        commands,
    };
    tokio::spawn(link.run());
    Ok(commands_tx)
}
