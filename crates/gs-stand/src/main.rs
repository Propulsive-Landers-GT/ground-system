use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use gs_stand::config::Config;
use gs_stand::fake::FakeWorld;
use gs_stand::{App, AppOptions};

/// GTPL test-stand adapter: Arduinos + MTV servos behind the gs-protocol UDP link.
#[derive(Parser, Debug)]
#[command(name = "gs-stand", version)]
struct Cli {
    /// Config file. Relative paths inside it resolve against `--root` (default: this file's
    /// grandparent, so `stand/config.toml` makes `stand/sequences` and `logs/` repo-relative).
    #[arg(long, default_value = "stand/config.toml")]
    config: PathBuf,

    /// Directory relative config paths resolve against. Default: the config file's
    /// grandparent, i.e. the repo root for `stand/config.toml`.
    #[arg(long)]
    root: Option<PathBuf>,

    /// UDP port to listen on (overrides `[udp] port`). The bridge is pointed here with
    /// `--stand <host>:<port>`.
    #[arg(long)]
    port: Option<u16>,

    /// Use in-process fake Arduinos and a fake MTV: no hardware needed.
    #[arg(long)]
    fake_arduino: bool,

    /// Dev only: run sequence clocks this many times faster than real time.
    #[arg(long, default_value_t = 1.0)]
    time_scale: f64,

    /// Do not write the per-run CSV.
    #[arg(long)]
    no_csv: bool,

    /// Parse config and sequences, print a summary, exit.
    #[arg(long)]
    check: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();

    let (config, root) = if cli.config.exists() {
        let cfg = Config::load(&cli.config)?;
        let root = cli
            .config
            .canonicalize()?
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .context("config path has no parent")?;
        (cfg, root)
    } else {
        tracing::warn!(
            "{} not found; using built-in defaults",
            cli.config.display()
        );
        (Config::default(), std::env::current_dir()?)
    };
    let root = cli.root.clone().unwrap_or(root);
    if cli.time_scale != 1.0 {
        tracing::warn!("TIME SCALE {}x: sequences run faster than real time (dev only)", cli.time_scale);
    }

    let port = cli.port.unwrap_or(config.udp.port);
    let fake = cli.fake_arduino.then(FakeWorld::new_shared);
    if fake.is_some() {
        tracing::warn!("FAKE ARDUINO mode: no hardware is being driven");
    }

    if cli.check {
        let seq_dir = Config::resolve(&root, &config.sequences.dir);
        let seqs = gs_stand::sequence::load_dir(&seq_dir)?;
        println!("config OK; {} sequences in {}:", seqs.len(), seq_dir.display());
        for (name, s) in &seqs {
            println!("  {name}: {} steps, {:.1} s", s.steps.len(), s.duration_s);
            for st in &s.steps {
                println!("    {}", st.describe());
            }
            if let Some(p) = &s.mtv_profile {
                println!("    MTV profile from T+{:.1} to T+{:.1}", p.start_t, p.end_t());
            }
        }
        return Ok(());
    }

    let mut app = App::new(AppOptions {
        config,
        root,
        udp_port: port,
        time_scale: cli.time_scale,
        fake,
        csv: !cli.no_csv,
    })?;

    // Ctrl-C terminates the process. Like the legacy scripts, that leaves the Arduinos at
    // their last state; send `Abort`/`Disarm` from the ground first if that matters.
    let stop = AtomicBool::new(false);
    app.run(&stop);
    Ok(())
}
