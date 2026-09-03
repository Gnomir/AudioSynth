//! Drives `PolySynth` the same way the nih-plug wrapper does — a timed list of
//! note-on / note-off events plus brightness automation — and writes
//! `poly_demo.wav`: a four-chord phrase with the brightness opening across it.
//!
//!   cargo run --example poly_demo --release

use harmonic_core::PolySynth;
use std::fs::File;
use std::io::{BufWriter, Write};

const SR: f64 = 48_000.0;

#[derive(Clone, Copy)]
enum Ev {
    On(u8),
    Off(u8),
}

fn main() -> std::io::Result<()> {
    let mut synth: PolySynth<16> = PolySynth::new(SR);
    synth.set_gain(0.5);
    synth.set_envelope(0.008, 0.35);

    // ii–V–I–vi in C
    let chords: [&[u8]; 4] = [
        &[62, 65, 69],
        &[55, 59, 62, 65],
        &[60, 64, 67, 72],
        &[57, 60, 64],
    ];
    let step = 1.6_f64;
    let sustain = 1.45_f64;

    // Build a sample-indexed event list, like MIDI events arriving at the host.
    let mut events: Vec<(usize, Ev)> = Vec::new();
    for (i, chord) in chords.iter().enumerate() {
        let on = (i as f64 * step * SR) as usize;
        let off = ((i as f64 * step + sustain) * SR) as usize;
        for &n in *chord {
            events.push((on, Ev::On(n)));
            events.push((off, Ev::Off(n)));
        }
    }
    events.sort_by_key(|(t, _)| *t);

    let total = (chords.len() as f64 * step * SR) as usize + SR as usize;
    let mut pcm: Vec<i16> = Vec::with_capacity(total * 2);
    let mut ev_idx = 0;

    for i in 0..total {
        while ev_idx < events.len() && events[ev_idx].0 <= i {
            match events[ev_idx].1 {
                Ev::On(n) => synth.note_on(n, 0.9),
                Ev::Off(n) => synth.note_off(n),
            }
            ev_idx += 1;
        }

        let b = 0.1 + 0.75 * (i as f64 / total as f64);
        synth.set_rolloff(brightness_to_r(b));

        let [l, r] = synth.render_sample();
        pcm.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        pcm.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }

    write_wav_stereo("poly_demo.wav", SR as u32, &pcm)?;
    println!(
        "wrote poly_demo.wav  ({:.1}s stereo {} Hz, ii-V-I-vi, brightness 0.1 -> 0.85)",
        total as f64 / SR,
        SR as u32
    );
    Ok(())
}

fn brightness_to_r(b: f64) -> f64 {
    let b = b.clamp(0.0, 1.0);
    0.02 + (0.9995 - 0.02) * b * b
}

fn write_wav_stereo(path: &str, sr: u32, pcm: &[i16]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    let data_bytes = (pcm.len() * 2) as u32;
    let byte_rate = sr * 2 * 2;
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
    for &s in pcm {
        w.write_all(&s.to_le_bytes())?;
    }
    w.flush()
}
