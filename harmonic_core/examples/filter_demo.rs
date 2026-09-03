//! `filter_demo.wav` — the state-variable filter on a bright source:
//!
//!   0–5 s    resonant low-pass, cutoff sweeps 200 Hz → 12 kHz → 200 Hz (res 70 %)
//!   5–9 s    band-pass, cutoff sweep (res 60 %)
//!   9–17 s   held chord (long amp release) with a DEDICATED filter ADSR:
//!            +5 oct depth, A 4 ms / D 0.18 s / S 0 / R 0.12 s, re-plucked
//!            every 1.5 s — the filter sweep is fully independent of the amp
//!            envelope, which just sustains underneath.
//!
//!   cargo run --example filter_demo --release

use harmonic_core::{FilterMode, PolySynth};
use std::fs::File;
use std::io::{BufWriter, Write};

const SR: f64 = 48_000.0;

fn main() -> std::io::Result<()> {
    let mut s: PolySynth<16> = PolySynth::new(SR);
    s.set_gain(2.0); // BLIT is peak-normalised, so it needs make-up level
    s.set_rolloff(0.97); // bright, lots of material for the filter
    // amp: slow release so section 3 clearly shows the filter env acting alone
    s.set_amp_adsr(0.006, 0.001, 1.0, 1.2);
    // dedicated filter envelope: a percussive sweep (sustain 0)
    s.set_filter_envelope(0.004, 0.18, 0.0, 0.12);

    let total = (SR * 17.0) as usize;
    let mut pcm: Vec<i16> = Vec::with_capacity(total * 2);

    // section 1+2 hold one low note; section 3 plays a chord
    s.note_on(40, 0.9);
    let mut chord_on = false;
    let repluck = (SR * 1.5) as usize;
    let sec3_start = (SR * 9.0) as usize;

    for i in 0..total {
        let t = i as f64 / SR;

        if t < 5.0 {
            let tri = 1.0 - (2.0 * (t / 5.0) - 1.0).abs();
            let fc = 200.0 * exp2_(tri * 6.0); // 200 Hz .. ~12.8 kHz
            s.set_filter(FilterMode::Low, fc, 0.7, 0.0);
        } else if t < 9.0 {
            let u = (t - 5.0) / 4.0;
            let tri = 1.0 - (2.0 * u - 1.0).abs();
            let fc = 300.0 * exp2_(tri * 5.5);
            s.set_filter(FilterMode::Band, fc, 0.6, 0.0);
        } else {
            if !chord_on {
                s.note_off(40);
                chord_on = true;
            }
            // re-pluck the chord every 1.5 s so the filter envelope retriggers
            if (i - sec3_start).is_multiple_of(repluck) {
                for n in [48u8, 55, 60, 64, 67] {
                    s.note_off(n);
                    s.note_on(n, 0.85);
                }
            }
            s.set_filter(FilterMode::Low, 300.0, 0.8, 5.0); // filter env opens +5 oct
        }

        let [l, r] = s.render_sample();
        pcm.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        pcm.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }

    write_wav_stereo("filter_demo.wav", SR as u32, &pcm)?;
    println!("wrote filter_demo.wav  (17s stereo {} Hz)", SR as u32);
    Ok(())
}

/// local 2^x (examples can't reach the crate's no_std one; std is fine here)
fn exp2_(x: f64) -> f64 {
    x.exp2()
}

fn write_wav_stereo(path: &str, sr: u32, pcm: &[i16]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    let data_bytes = (pcm.len() * 2) as u32;
    let byte_rate = sr * 4;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&4u16.to_le_bytes())?;
    w.write_all(&16u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    for &v in pcm {
        w.write_all(&v.to_le_bytes())?;
    }
    w.flush()
}
