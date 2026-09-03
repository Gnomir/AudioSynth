//! Renders a short demo to `harmonic_demo.wav` in the current directory:
//! a 110 Hz note whose spectral tilt `r` sweeps dark → bright → dark, so you
//! can hear the O(1) partial count open up without any CPU change.
//!
//!   cargo run --example render_wav --release

use harmonic_core::Voice;
use std::fs::File;
use std::io::{BufWriter, Write};

fn main() -> std::io::Result<()> {
    let fs = 48_000.0_f64;
    let secs = 6.0_f64;
    let total = (fs * secs) as usize;

    let mut v = Voice::new(fs);
    v.set_frequency(110.0);
    v.set_gain(0.6);
    v.reset();

    let mut pcm: Vec<i16> = Vec::with_capacity(total);
    for i in 0..total {
        let t = i as f64 / total as f64;
        // triangle sweep of r over [0.05, 0.9995]
        let tri = 1.0 - (2.0 * t - 1.0).abs();
        let r = 0.05 + tri * (0.9995 - 0.05);
        v.set_rolloff(r);

        // one voice, centre pan → L == R; undo the −3 dB equal-power law
        let [l, _] = v.render_sample();
        let s = (l * std::f32::consts::SQRT_2).clamp(-1.0, 1.0);
        pcm.push((s * i16::MAX as f32) as i16);
    }

    write_wav("harmonic_demo.wav", fs as u32, &pcm)?;
    println!(
        "wrote harmonic_demo.wav  ({:.1}s, {} Hz mono, r sweeps 0.05 -> 0.9995 -> 0.05)",
        secs, fs as u32
    );
    Ok(())
}

fn write_wav(path: &str, sr: u32, pcm: &[i16]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    let data_bytes = (pcm.len() * 2) as u32;
    let byte_rate = sr * 2;

    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // PCM chunk size
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&1u16.to_le_bytes())?; // mono
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // block align
    w.write_all(&16u16.to_le_bytes())?; // bits per sample
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    for &s in pcm {
        w.write_all(&s.to_le_bytes())?;
    }
    w.flush()
}
