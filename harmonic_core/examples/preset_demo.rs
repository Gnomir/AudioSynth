//! Renders four real presets from `harmonic_synth`'s starter bank straight
//! through the public engine API, each playing a short musically sensible
//! phrase — not a parameter-sweep proof, an actual sound a musician would
//! reach for. Mirrors `harmonic_synth::presets::tests::configure` exactly
//! (same formulas, same defaults) so what you hear here is what the plugin
//! actually plays with that preset loaded — not a separate approximation.
//!
//!   cargo run --example preset_demo --release
//!
//! Writes deep-sub.wav, bright-saw-lead.wav, warm-analog-pad.wav, dx-bell.wav
//! to the current directory.

use harmonic_core::{CharParams, FilterMode, LfoMode, LfoShape, PolySynth, Waveform};
use std::fs::File;
use std::io::{BufWriter, Write};

const SR: f64 = 48_000.0;

fn brightness_to_r(b: f32) -> f64 {
    let b = b.clamp(0.0, 1.0) as f64;
    0.02 + (0.9995 - 0.02) * b * b
}

/// One (name, plain-value) pair, exactly as `harmonic_synth::presets::Preset` stores it.
struct P<'a>(&'a [(&'a str, f32)]);

fn configure(engine: &mut PolySynth<8>, p: &P) {
    let g = |id: &str, dflt: f32| -> f32 {
        p.0.iter().find(|(k, _)| *k == id).map(|(_, v)| *v).unwrap_or(dflt)
    };
    engine.set_waveform(match g("oscwave", 0.0) as i32 {
        1 => Waveform::Saw,
        2 => Waveform::Triangle,
        _ => Waveform::Geometric,
    });
    engine.set_rolloff(brightness_to_r(g("bright", 0.35)));
    engine.set_partial_limit(g("partials", 2048.0));
    engine.set_brightness_depth(g("exprbrt", 0.35) as f64);
    engine.set_formant(g("formant", 0.0) as f64);
    engine.set_envelope(g("attack", 0.005) as f64, g("release", 0.18) as f64);
    engine.set_gain(10f64.powf(g("gain", -12.0) as f64 / 20.0));
    let grit = g("grit", 0.0);
    engine.set_character(CharParams {
        drive: g("drive", 0.0),
        bias: 0.25 * g("drive", 0.0),
        fold: g("fold", 0.0),
        crush: grit,
        downsample: grit * 0.8,
    });
    engine.set_fm(g("fmratio", 1.0) as f64, g("fmamt", 0.0) as f64);
    engine.set_feedback(g("feedbk", 0.0) as f64);
    engine.set_filter(
        match g("fltmode", 0.0) as i32 {
            1 => FilterMode::Low,
            2 => FilterMode::Band,
            3 => FilterMode::High,
            4 => FilterMode::Notch,
            _ => FilterMode::Bypass,
        },
        g("fltcut", 12_000.0) as f64,
        g("fltres", 0.0) as f64,
        g("fltenv", 0.0) as f64,
    );
    engine.set_filter_envelope(
        g("featk", 0.004) as f64,
        g("fedec", 0.15) as f64,
        g("fesus", 0.0) as f64,
        g("ferel", 0.25) as f64,
    );
    engine.set_free_running(g("freerun", 0.0) != 0.0);
    engine.set_hq(g("hqmode", 0.0) != 0.0);
    engine.set_unison(
        g("unicnt", 1.0) as u32,
        g("unidet", 12.0) as f64,
        g("unispr", 0.6) as f64,
        g("unidrift", 0.0) as f64,
    );
    engine.set_lfo(
        g("lforate", 5.0) as f64,
        match g("lfoshp", 0.0) as i32 {
            1 => LfoShape::Triangle,
            2 => LfoShape::Saw,
            _ => LfoShape::Sine,
        },
        if g("lfosync", 0.0) != 0.0 { LfoMode::FreeRun } else { LfoMode::Retrigger },
        g("lfobrt", 0.0) as f64 * 0.5,
        g("lfovib", 0.0) as f64,
        g("lfocut", 0.0) as f64,
        g("lfofm", 0.0) as f64,
    );
}

struct Phrase {
    file: &'static str,
    preset: P<'static>,
    /// (note, on_at_s, off_at_s) — off_at None holds until the release tail.
    notes: &'static [(u8, f64, f64)],
    total_s: f64,
}

fn main() -> std::io::Result<()> {
    let phrases = [
        Phrase {
            file: "deep-sub.wav",
            preset: P(&[
                ("bright", 0.12), ("partials", 6.0), ("attack", 0.003), ("release", 0.20),
                ("gain", -6.0), ("fltmode", 1.0), ("fltcut", 500.0), ("fesus", 1.0),
            ]),
            notes: &[(36, 0.0, 2.2)],
            total_s: 3.0,
        },
        Phrase {
            file: "bright-saw-lead.wav",
            preset: P(&[
                ("oscwave", 1.0), ("bright", 0.7), ("attack", 0.004), ("release", 0.12),
                ("gain", -10.0), ("unicnt", 3.0), ("unidet", 10.0), ("unispr", 0.5),
                ("lfovib", 6.0), ("lforate", 5.5),
            ]),
            notes: &[(60, 0.0, 0.45), (63, 0.5, 0.95), (67, 1.0, 1.7)],
            total_s: 2.6,
        },
        Phrase {
            file: "warm-analog-pad.wav",
            preset: P(&[
                ("oscwave", 1.0), ("bright", 0.4), ("attack", 1.2), ("release", 3.0),
                ("gain", -13.0), ("unicnt", 5.0), ("unidet", 16.0), ("unispr", 0.9),
                ("unidrift", 0.5), ("fltmode", 1.0), ("fltcut", 4000.0), ("fltenv", 1.5),
                ("featk", 1.0), ("fedec", 2.0), ("fesus", 0.4), ("ferel", 3.0),
            ]),
            notes: &[(48, 0.0, 3.5), (52, 0.0, 3.5), (55, 0.0, 3.5), (59, 0.0, 3.5)],
            total_s: 7.0,
        },
        Phrase {
            file: "dx-bell.wav",
            preset: P(&[
                ("bright", 0.2), ("attack", 0.001), ("release", 2.5), ("gain", -13.0),
                ("fmamt", 2.2), ("fmratio", 3.5), ("feedbk", 0.0),
                ("fedec", 2.0), ("fesus", 0.0),
            ]),
            notes: &[(60, 0.0, 0.05), (67, 1.1, 1.15)],
            total_s: 4.0,
        },
    ];

    for ph in &phrases {
        let mut s: PolySynth<8> = PolySynth::new(SR);
        configure(&mut s, &ph.preset);

        let total = (SR * ph.total_s) as usize;
        let mut pcm: Vec<i16> = Vec::with_capacity(total * 2);
        let mut on = vec![false; ph.notes.len()];

        for i in 0..total {
            let t = i as f64 / SR;
            for (idx, (note, on_at, off_at)) in ph.notes.iter().enumerate() {
                if !on[idx] && t >= *on_at {
                    s.note_on(*note, 0.85);
                    on[idx] = true;
                }
                if on[idx] && t >= *off_at {
                    s.note_off(*note);
                    on[idx] = false;
                }
            }
            let [l, r] = s.render_sample();
            pcm.push((l.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            pcm.push((r.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }

        write_wav_stereo(ph.file, SR as u32, &pcm)?;
        println!("wrote {}  ({:.1}s stereo {} Hz)", ph.file, ph.total_s, SR as u32);
    }
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
