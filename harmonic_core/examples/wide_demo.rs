//! `wide_demo.wav` — everything from tasks 1–3 at once: a stereo unison pad.
//!
//!   0–8 s   Cmaj9 chord, 7-voice unison per note, ±18 ct detune, wide spread,
//!           free-running phase, gentle sine-LFO vibrato, resonant LP with a
//!           slow filter-envelope bloom on each note.
//!
//!   cargo run --example wide_demo --release

use harmonic_core::{FilterMode, LfoMode, LfoShape, PolySynth};
use std::fs::File;
use std::io::{BufWriter, Write};

const SR: f64 = 48_000.0;

fn main() -> std::io::Result<()> {
    let mut s: PolySynth<48> = PolySynth::new(SR);
    s.set_gain(0.7);
    s.set_rolloff(0.55_f64.powi(2) * (0.9995 - 0.02) + 0.02);
    s.set_free_running(true);
    s.set_unison(7, 18.0, 0.95);
    s.set_lfo(4.5, LfoShape::Sine, LfoMode::Retrigger, 0.0, 8.0, 0.0, 0.0); // 8-cent vibrato
    s.set_amp_adsr(0.35, 0.2, 0.8, 1.2);
    s.set_filter(FilterMode::Low, 700.0, 0.55, 2.6);
    s.set_filter_envelope(0.5, 1.6, 0.35, 1.0);

    for n in [48u8, 52, 55, 59, 62] {
        s.note_on(n, 0.8);
    }

    let total = (SR * 8.0) as usize;
    let mut pcm: Vec<i16> = Vec::with_capacity(total * 2);
    let off_at = (SR * 6.2) as usize;

    for i in 0..total {
        if i == off_at {
            for n in [48u8, 52, 55, 59, 62] {
                s.note_off(n);
            }
        }
        let [l, r] = s.render_sample();
        pcm.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        pcm.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }

    write_wav_stereo("wide_demo.wav", SR as u32, &pcm)?;
    println!("wrote wide_demo.wav  (8s stereo {} Hz, 7× unison pad)", SR as u32);
    Ok(())
}

fn write_wav_stereo(path: &str, sr: u32, pcm: &[i16]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    let data_bytes = (pcm.len() * 2) as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&(sr * 4).to_le_bytes())?;
    w.write_all(&4u16.to_le_bytes())?;
    w.write_all(&16u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    for &v in pcm {
        w.write_all(&v.to_le_bytes())?;
    }
    w.flush()
}
