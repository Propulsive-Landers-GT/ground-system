//! `gs-bridge`: the ground-side server. See `docs/DESIGN.md`.

mod hub;
mod link_stats;
mod live;
mod messages;
mod recorder;
mod replay;
mod routing;
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
use crate::routing::Endpoints;
use crate::web::LinkHandle;

#[derive(Parser, Debug)]
#[command(version, about = "GTPL ground-station bridge")]
struct Args {
    /// Address of the vehicle or sim that heartbeats and flight commands are sent to.
    #[arg(long, value_name = "IP:PORT", default_value = "127.0.0.1:8888")]
    vehicle: SocketAddr,

    /// Address of the test-stand adapter (gs-stand). Stand and valve commands go here.
    #[arg(long, value_name = "IP:PORT")]
    stand: Option<SocketAddr>,

    /// UDP port telemetry is received on.
    #[arg(long, value_name = "PORT", default_value_t = gs_protocol::DEFAULT_GROUND_PORT)]
    listen_udp: u16,

    /// HTTP port for the web UI and the `/ws` WebSocket.
    #[arg(long, value_name = "PORT", default_value_t = 8080)]
    http: u16,

    /// Directory holding the built web UI.
    #[arg(long, value_name = "PATH", default_value = "web/dist")]
    web_dir: PathBuf,

    /// Directory recordings are written to (one subdirectory per recording).
    #[arg(long, value_name = "PATH", default_value = "logs")]
    log_dir: PathBuf,

    /// Start a recording at launch, optionally named. Otherwise recording is started
    /// from the browser.
    #[arg(long, value_name = "NAME", num_args = 0..=1, default_missing_value = "")]
    record: Option<String>,

    /// Play back a recording (its directory or `session.jsonl`) instead of talking to
    /// a vehicle.
    #[arg(long, value_name = "DIR|FILE.jsonl")]
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

    let link = match &args.replay {
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
        link,
        web_dir: args.web_dir,
    });
    axum::serve(listener, app)
        .await
        .context("HTTP server failed")
}

/// Binds the ground UDP port and spawns the link task. Returns the handle browsers use
/// to reach it.
async fn start_live_link(args: &Args, hub: Arc<Hub>) -> anyhow::Result<LinkHandle> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, args.listen_udp))
        .await
        .with_context(|| format!("binding UDP port {}", args.listen_udp))?;
    let endpoints = Endpoints {
        vehicle: args.vehicle,
        stand: args.stand,
    };
    if let Some(stand) = args.stand {
        if stand == args.vehicle {
            anyhow::bail!("--stand and --vehicle must be different addresses");
        }
        info!(
            "listening for telemetry on UDP {}, heartbeating vehicle at {} and test stand at {stand}",
            args.listen_udp, args.vehicle
        );
    } else {
        info!(
            "listening for telemetry on UDP {}, heartbeating vehicle at {} (no test stand)",
            args.listen_udp, args.vehicle
        );
    }

    let recorder = match &args.record {
        Some(name) => {
            let recorder = Recorder::start(&args.log_dir, Some(name))
                .with_context(|| format!("creating recording in {}", args.log_dir.display()))?;
            info!("recording to {}", recorder.dir().display());
            Some(recorder)
        }
        None => {
            info!("not recording; start one from the UI or with --record");
            None
        }
    };

    let (tx, requests) = mpsc::channel(64);
    let link = LiveLink {
        socket,
        endpoints,
        hub,
        log_dir: args.log_dir.clone(),
        recorder,
        requests,
    };
    tokio::spawn(link.run());
    Ok(LinkHandle { endpoints, tx })
}
