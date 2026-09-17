//! `tpt-visual` — command-line interface for the TPT AV visual stack.
//!
//! A thin wrapper over the [`tpt_av_visual`] facade: renders timeline JSON
//! documents, probes the GPU, and generates session presets. No engine logic
//! lives here.

mod presets;

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use tpt_av_visual::prelude::*;
use tpt_av_visual::Error;

/// Command-line interface for the TPT AV visual stack.
#[derive(Parser)]
#[command(name = "tpt-visual", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a timeline JSON document to an MJPEG AVI video file.
    Render {
        /// Path to the session JSON document.
        session: String,
        /// Number of frames to render from the playhead (session start).
        #[arg(long, default_value_t = 60)]
        frames: u64,
        /// Output AVI path.
        #[arg(long, default_value = "out.avi")]
        out: String,
        /// JPEG quality for the AVI export (1–100).
        #[arg(long, default_value_t = 90)]
        quality: u8,
    },
    /// Describe the GPU the engine will use.
    ProbeGpu,
    /// Write a session preset JSON document.
    New {
        /// Output JSON path.
        out: String,
        /// Preset name (see `presets list`).
        #[arg(long, default_value = "single-clip")]
        preset: String,
    },
    /// List the bundled session presets.
    Presets {
        #[command(subcommand)]
        command: Option<PresetsCommand>,
    },
}

#[derive(Subcommand)]
enum PresetsCommand {
    /// List presets (default when no subcommand is given).
    List,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Render {
            session,
            frames,
            out,
            quality,
        } => render(&session, &out, frames, quality),
        Command::ProbeGpu => probe_gpu(),
        Command::New { out, preset } => new_session(&out, &preset),
        Command::Presets { command: None } => presets_list(),
        Command::Presets {
            command: Some(PresetsCommand::List),
        } => presets_list(),
    }
}

fn render(session_path: &str, out: &str, frames: u64, quality: u8) -> anyhow::Result<()> {
    let session =
        Session::from_json_path(session_path).with_context(|| format!("reading {session_path}"))?;

    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => bail!("no compatible GPU device is available"),
        Err(e) => return Err(Error::from(e).into()),
    };
    renderer.attach_default_decoders()?;

    let bytes = renderer
        .render_frames_to_avi(out, frames, quality)
        .map_err(Error::from)?;
    println!("wrote {out} ({bytes} bytes, {frames} frames)");
    Ok(())
}

fn probe_gpu() -> anyhow::Result<()> {
    match tpt_av_visual::probe_gpu() {
        Some(gpu) => {
            println!("adapter:     {}", gpu.adapter);
            println!("backend:     {}", gpu.backend);
            println!("device type: {}", gpu.device_type);
            println!("driver:      {}", gpu.driver);
            Ok(())
        }
        None => {
            eprintln!("no compatible GPU device is available");
            std::process::exit(1);
        }
    }
}

fn new_session(out: &str, preset: &str) -> anyhow::Result<()> {
    let Some((_, _, json)) = presets::PRESETS.iter().find(|(n, _, _)| *n == preset) else {
        bail!("unknown preset {preset:?}; run `tpt-visual presets list` for options");
    };
    std::fs::write(out, json)?;
    println!("wrote {out} (preset {preset:?})");
    Ok(())
}

fn presets_list() -> anyhow::Result<()> {
    for (name, description, _) in presets::PRESETS {
        println!("{name:<20} {description}");
    }
    Ok(())
}
