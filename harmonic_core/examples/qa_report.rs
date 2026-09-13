//! Independent QA measurement harness — NOT part of the crate's own test
//! suite (that lives in tests/ and is already exercised by `cargo test`).
//! This renders a fixed battery of known signals to 32-bit float WAV so an
//! external script (Node, no dependency on this crate) can do frequency-
//! domain analysis without trusting the engine's own self-reported numbers.
//!
//!   cargo run --release --example qa_report
//!
//! Every file name states exactly what was rendered and with what settings,
//! so a reader can reproduce it from the Voice/PolySynth public API alone.

use harmonic_core::{CharParams, FilterMode, LfoShape, PolySynth, Tuning, Voice, Waveform};
use std::fs::File;
use std::io::{BufWriter, Write};

const FS: f64 = 48_000.0;

fn main() -> std::io::Result<()> {
    // 1) Darkest possible geometric tone (rolloff at the demo UI's own
    //    minimum, 0.02) — "how close to a pure sine is the oscillator at
    //    its cleanest setting".
    {
        let mut v = Voice::new(FS);
        v.set_waveform(Waveform::Geometric);
        v.set_frequency(440.0);
        v.set_rolloff(0.02);
        v.set_gain(1.0);
        v.reset();
        render_voice_seconds(&mut v, 2.0, "qa_01_dark_sine_440hz.wav")?;
    }

    // 2) Brightest possible geometric tone (rolloff at the demo UI's own
    //    maximum, 0.965) — full harmonic comb, for frequency-ladder and
    //    aliasing checks.
    {
        let mut v = Voice::new(FS);
        v.set_waveform(Waveform::Geometric);
        v.set_frequency(220.0);
        v.set_rolloff(0.965);
        v.set_gain(1.0);
        v.reset();
        render_voice_seconds(&mut v, 2.0, "qa_02_bright_220hz.wav")?;
    }

    // 3) Same bright tone, but with every "dirty" stage pushed hard AND
    //    HQ off, vs HQ on — the actual aliasing-floor claim, independently
    //    re-measured rather than quoted from prior project docs.
    for (hq, name) in [(false, "qa_03_dirty_hq_off.wav"), (true, "qa_03_dirty_hq_on.wav")] {
        let mut v = Voice::new(FS);
        v.set_waveform(Waveform::Geometric);
        v.set_frequency(110.0);
        v.set_rolloff(0.9);
        v.set_character(CharParams { drive: 0.7, fold: 0.6, crush: 0.5, ..CharParams::CLEAN });
        v.set_hq(hq);
        v.set_gain(0.9);
        v.reset();
        render_voice_seconds(&mut v, 2.0, name)?;
    }

    // 4) 12-TET frequency accuracy: A4 and a handful of MIDI notes, one
    //    note at a time via PolySynth's default (unmodified) tuning.
    {
        let mut synth: PolySynth<8> = PolySynth::new(FS);
        for note in [45u8, 57, 60, 69, 72, 81, 93] {
            // clean, undecorated tone: default tuning, no unison, no character
            synth.reset();
            synth.set_rolloff(0.5);
            synth.note_on(note, 1.0);
            render_poly_seconds(&mut synth, 2.0, &format!("qa_04_tet12_note{note}.wav"))?;
        }
    }

    // 5) Just-intonation frequency accuracy: a 5-limit major triad
    //    (1/1, 5/4, 3/2) over a 220 Hz root, rendered as three SEPARATE
    //    single-note files so each partial ladder is unambiguous in the
    //    FFT (a real chord render is done separately, file 8).
    {
        let ratios_cents = [0.0, 386.3137, 701.9550]; // 5-limit major: 1/1, 5/4, 3/2
        let tuning = Tuning::from_cents(&ratios_cents, 1200.0, 220.0, 60);
        for (i, _) in ratios_cents.iter().enumerate() {
            let mut synth: PolySynth<8> = PolySynth::new(FS);
            synth.set_tuning(tuning);
            synth.set_rolloff(0.5);
            synth.note_on(60 + i as u8, 1.0); // degrees map onto consecutive notes from root
            render_poly_seconds(&mut synth, 2.0, &format!("qa_05_just_degree{i}.wav"))?;
        }
    }

    // 6) Envelope timing: amp ADSR with known attack/decay/sustain/release,
    //    held long enough to reach sustain, then released — so the WAV's
    //    own amplitude envelope can be measured against the nominal times.
    {
        let mut synth: PolySynth<8> = PolySynth::new(FS);
        synth.set_amp_adsr(0.100, 0.150, 0.5, 0.300); // 100ms A, 150ms D, 50% S, 300ms R
        synth.set_rolloff(0.3);
        synth.note_on(69, 1.0);
        let hold_frames = (FS * 0.8) as usize; // well past attack+decay
        let release_frames = (FS * 0.8) as usize; // well past release
        let mut l = vec![0.0f32; hold_frames];
        let mut r = vec![0.0f32; hold_frames];
        synth.render_block(&mut l, &mut r);
        synth.note_off(69);
        let mut l2 = vec![0.0f32; release_frames];
        let mut r2 = vec![0.0f32; release_frames];
        synth.render_block(&mut l2, &mut r2);
        l.extend_from_slice(&l2);
        write_wav_f32("qa_06_envelope_a100_d150_s50_r300.wav", FS as u32, &l)?;
    }

    // 7) Filter response: fixed cutoff, resonance, fed with the brightest
    //    oscillator (rich harmonic content) so the filter's actual response
    //    curve can be reconstructed from the output spectrum.
    for (cutoff, name) in [(500.0, "qa_07_filter_lp_500hz.wav"), (2000.0, "qa_07_filter_lp_2000hz.wav")] {
        let mut v = Voice::new(FS);
        v.set_waveform(Waveform::Geometric);
        v.set_frequency(55.0); // low fundamental so many harmonics sit under the cutoff and above it
        v.set_rolloff(0.965);
        v.set_filter_mode(FilterMode::Low);
        v.set_filter_cutoff(cutoff);
        v.set_filter_resonance(0.1);
        v.set_gain(0.9);
        v.reset();
        render_voice_seconds(&mut v, 2.0, name)?;
    }

    // 8) A real chord (just-intonation triad, all three notes at once,
    //    unison + stereo spread on) — for a genuine stereo-image check.
    {
        let ratios_cents = [0.0, 386.3137, 701.9550];
        let tuning = Tuning::from_cents(&ratios_cents, 1200.0, 220.0, 60);
        let mut synth: PolySynth<8> = PolySynth::new(FS);
        synth.set_tuning(tuning);
        synth.set_rolloff(0.5);
        synth.set_unison(4, 12.0, 0.8, 0.3);
        synth.note_on(60, 0.8);
        synth.note_on(61, 0.8);
        synth.note_on(62, 0.8);
        render_poly_stereo_seconds(&mut synth, 2.0, "qa_08_chord_unison_stereo.wav")?;
    }

    // 9) LFO -> vibrato, a known rate and depth, for modulation-accuracy
    //    checks (the vibrato rate should show up as sidebands / an AM-ish
    //    envelope at exactly the LFO rate).
    {
        let mut v = Voice::new(FS);
        v.set_waveform(Waveform::Geometric);
        v.set_frequency(440.0);
        v.set_rolloff(0.4);
        v.set_lfo(5.0, LfoShape::Sine); // 5 Hz rate
        v.set_lfo_targets(0.0, 40.0, 0.0, 0.0); // 40 cents -> vibrato only
        v.reset();
        render_voice_seconds(&mut v, 2.0, "qa_09_vibrato_5hz_40c.wav")?;
    }

    println!("qa_report: wrote qa_*.wav in the current directory");
    Ok(())
}

fn render_voice_seconds(v: &mut Voice, secs: f64, name: &str) -> std::io::Result<()> {
    let n = (FS * secs) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    v.render_block(&mut l, &mut r);
    write_wav_f32(name, FS as u32, &l)
}

fn render_poly_seconds<const N: usize>(
    synth: &mut PolySynth<N>,
    secs: f64,
    name: &str,
) -> std::io::Result<()> {
    let n = (FS * secs) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    synth.render_block(&mut l, &mut r);
    write_wav_f32(name, FS as u32, &l)
}

fn render_poly_stereo_seconds<const N: usize>(
    synth: &mut PolySynth<N>,
    secs: f64,
    name: &str,
) -> std::io::Result<()> {
    let n = (FS * secs) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    synth.render_block(&mut l, &mut r);
    write_wav_f32_stereo(name, FS as u32, &l, &r)
}

// ---- minimal 32-bit float PCM WAV writer (mono + stereo) ----

fn write_wav_f32(path: &str, sr: u32, samples: &[f32]) -> std::io::Result<()> {
    write_wav_f32_channels(path, sr, 1, samples)
}

fn write_wav_f32_stereo(path: &str, sr: u32, l: &[f32], r: &[f32]) -> std::io::Result<()> {
    let mut interleaved = Vec::with_capacity(l.len() * 2);
    for i in 0..l.len() {
        interleaved.push(l[i]);
        interleaved.push(r[i]);
    }
    write_wav_f32_channels(path, sr, 2, &interleaved)
}

fn write_wav_f32_channels(path: &str, sr: u32, channels: u16, samples: &[f32]) -> std::io::Result<()> {
    let mut f = BufWriter::new(File::create(path)?);
    let byte_rate = sr * channels as u32 * 4;
    let block_align = channels * 4;
    let data_bytes = (samples.len() * 4) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_bytes).to_le_bytes())?;
    f.write_all(b"WAVE")?;
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&3u16.to_le_bytes())?; // IEEE float
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&block_align.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_bytes.to_le_bytes())?;
    for s in samples {
        f.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}
