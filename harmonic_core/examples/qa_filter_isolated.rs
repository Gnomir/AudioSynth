//! Isolated filter frequency response — NOT driven through an oscillator.
//!
//! `qa_report.rs`'s filter test (§7) drives `Svf` through a `Geometric`
//! oscillator with `rolloff = 0.965`, which already tilts harmonic
//! amplitude by itself before the filter touches it — so a "-3dB relative
//! to the passband harmonics" reading there conflates the oscillator's own
//! spectral tilt with the filter's actual response. This renders `Svf`'s
//! impulse response directly (a flat-spectrum unit impulse in, nothing
//! else in the signal path) so the true response can be measured with
//! zero oscillator involvement.
//!
//!   cargo run --release --example qa_filter_isolated

use harmonic_core::{FilterMode, Svf};
use std::fs::File;
use std::io::{BufWriter, Write};

const FS: f64 = 48_000.0;

fn main() -> std::io::Result<()> {
    for (cutoff, name) in [
        (500.0, "qa_isolated_filter_lp_500hz.wav"),
        (2000.0, "qa_isolated_filter_lp_2000hz.wav"),
    ] {
        let mut f = Svf::new(FS);
        f.set_mode(FilterMode::Low);
        f.set_cutoff(cutoff);
        f.set_resonance(0.1); // same resonance as qa_report.rs's §7, for comparability
        f.reset();

        let n = FS as usize; // 1s — resonance is low, the impulse response settles well inside this
        let mut out = vec![0.0f32; n];
        for (i, sample) in out.iter_mut().enumerate() {
            let x = if i == 0 { 1.0 } else { 0.0 };
            *sample = f.process(x);
        }
        write_wav_f32(name, FS as u32, &out)?;
    }
    println!("qa_filter_isolated: wrote qa_isolated_filter_lp_{{500,2000}}hz.wav in the current directory");
    Ok(())
}

fn write_wav_f32(path: &str, sr: u32, samples: &[f32]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    let data_len = (samples.len() * 4) as u32;
    let riff_len = 36 + data_len;
    w.write_all(b"RIFF")?;
    w.write_all(&riff_len.to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?; // IEEE float
    w.write_all(&1u16.to_le_bytes())?; // mono
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&(sr * 4).to_le_bytes())?; // byte rate
    w.write_all(&4u16.to_le_bytes())?; // block align
    w.write_all(&32u16.to_le_bytes())?; // bits per sample
    w.write_all(b"data")?;
    w.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        w.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}
