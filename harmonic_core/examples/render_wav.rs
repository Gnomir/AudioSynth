//! Renders a short demo WAV to the current directory.
//!
//!   cargo run --example render_wav --release            # geometric r-sweep
//!   cargo run --example render_wav --release -- saw     # band-limited saw, pitch sweep
//!   cargo run --example render_wav --release -- tri     # band-limited triangle, pitch sweep
//!
//! Geometric: a 110 Hz note whose spectral tilt `r` sweeps dark → bright → dark,
//! so you can hear the O(1) partial count open up without any CPU change.
//! Saw / triangle: a two-octave pitch glide, so you can hear the BLIT staying
//! band-limited (no aliasing whine) as the fundamental climbs.

use harmonic_core::{Voice, Waveform};
use std::fs::File;
use std::io::{BufWriter, Write};

fn main() -> std::io::Result<()> {
    let fs = 48_000.0_f64;
    let mode = std::env::args().nth(1).unwrap_or_default();
    let (wf, name): (Waveform, &str) = match mode.as_str() {
        "saw" => (Waveform::Saw, "harmonic_demo_saw.wav"),
        "tri" | "triangle" => (Waveform::Triangle, "harmonic_demo_triangle.wav"),
        _ => (Waveform::Geometric, "harmonic_demo.wav"),
    };
    let secs = 6.0_f64;
    let total = (fs * secs) as usize;

    let mut v = Voice::new(fs);
    v.set_waveform(wf);
    v.set_gain(0.6);
    v.set_frequency(110.0);
    v.reset();

    let mut pcm: Vec<i16> = Vec::with_capacity(total);
    for i in 0..total {
        let t = i as f64 / total as f64;
        let tri = 1.0 - (2.0 * t - 1.0).abs(); // 0 → 1 → 0

        match wf {
            Waveform::Geometric => {
                // spectral-tilt sweep at a fixed pitch
                v.set_rolloff(0.05 + tri * (0.9995 - 0.05));
            }
            _ => {
                // two-octave pitch glide for the BLIT waves
                v.set_frequency(110.0 * 2.0_f64.powf(2.0 * tri));
            }
        }

        // one voice, centre pan → L == R; undo the −3 dB equal-power law
        let [l, _] = v.render_sample();
        let s = (l * std::f32::consts::SQRT_2).clamp(-1.0, 1.0);
        pcm.push((s * i16::MAX as f32) as i16);
    }

    write_wav(name, fs as u32, &pcm)?;
    println!("wrote {name}  ({secs:.1}s, {} Hz mono, {wf:?})", fs as u32);
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
