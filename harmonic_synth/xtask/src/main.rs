//! `cargo xtask <cmd>`.
//!
//! * `bundle harmonic_synth --release` — the nih-plug bundler (delegated).
//! * `validate [--strictness N] [--pluginval PATH]` — build the VST3 + CLAP
//!   bundles and run Tracktion `pluginval` against them (state recall,
//!   block-size / sample-rate changes on the fly, allocation checks,
//!   parameter automation, fuzzing). Shells out to `scripts/validate.*`.

use std::path::Path;
use std::process::Command;

fn main() -> nih_plug_xtask::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("validate") => validate(args.collect()),
        _ => nih_plug_xtask::main(),
    }
}

fn validate(extra: Vec<String>) -> nih_plug_xtask::Result<()> {
    // xtask/ -> harmonic_synth/
    let synth_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a parent dir")
        .to_path_buf();

    let (program, script): (&str, &str) = if cfg!(windows) {
        ("pwsh", "scripts/validate.ps1")
    } else {
        ("bash", "scripts/validate.sh")
    };

    let status = Command::new(program)
        .arg(script)
        .args(&extra)
        .current_dir(&synth_dir)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to launch {program}: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("validation script exited with {status}"))
    }
}
