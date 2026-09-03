//! `character_demo.wav` — the same held chord run through the character stages
//! one at a time, so you can hear the clean Dirichlet core go from "camertone"
//! to something with teeth:
//!
//!   0–2 s   clean            (the sterile perfect starting point)
//!   2–4 s   drive            (asymmetric saturation → fatten)
//!   4–6 s   fold             (reflective wavefolder → dense upper spectrum)
//!   6–8 s   grit             (bit-crush + downsample → early-digital dirt)
//!   8–11 s  FM sweep          (index 0 → 3, ratio 2) → clangorous
//!   11–14 s feedback sweep    (0 → 0.7) → sine toward saw toward noise
//!   14–17 s everything at once
//!
//!   cargo run --example character_demo --release

use harmonic_core::{CharParams, PolySynth};
use std::fs::File;
use std::io::{BufWriter, Write};

const SR: f64 = 48_000.0;

fn main() -> std::io::Result<()> {
    let mut s: PolySynth<16> = PolySynth::new(SR);
    s.set_gain(0.45);
    s.set_envelope(0.01, 0.3);
    // mid brightness so the character stages have harmonic material to work on
    s.set_rolloff(0.55_f64.powi(2) * (0.9995 - 0.02) + 0.02);

    // a sus2 chord, held the whole time
    for n in [45u8, 52, 57, 64] {
        s.note_on(n, 0.85);
    }

    let secs = 17.0;
    let total = (SR * secs) as usize;
    let mut pcm: Vec<i16> = Vec::with_capacity(total * 2);

    for i in 0..total {
        let t = i as f64 / SR;

        let mut ch = CharParams::CLEAN;
        let (mut fm_idx, mut fb) = (0.0, 0.0);

        if (2.0..4.0).contains(&t) {
            ch.drive = 0.7;
            ch.bias = 0.25;
        } else if (4.0..6.0).contains(&t) {
            ch.fold = 0.6;
        } else if (6.0..8.0).contains(&t) {
            ch.crush = 0.6;
            ch.downsample = 0.45;
        } else if (8.0..11.0).contains(&t) {
            fm_idx = ((t - 8.0) / 3.0) * 3.0;
        } else if (11.0..14.0).contains(&t) {
            fb = ((t - 11.0) / 3.0) * 0.7;
        } else if t >= 14.0 {
            ch = CharParams {
                drive: 0.5,
                bias: 0.2,
                fold: 0.4,
                crush: 0.35,
                downsample: 0.2,
            };
            fm_idx = 1.4;
            fb = 0.3;
        }

        s.set_character(ch);
        s.set_fm(2.0, fm_idx);
        s.set_feedback(fb);

        let [l, r] = s.render_sample();
        pcm.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        pcm.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }

    write_wav_stereo("character_demo.wav", SR as u32, &pcm)?;
    println!("wrote character_demo.wav  ({secs}s stereo {} Hz)", SR as u32);
    Ok(())
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
