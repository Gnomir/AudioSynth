//! A cheap real-time spectrum display for the editor. No FFT and no extra
//! dependency: a bank of resonant band-pass [`Svf`]s with per-band envelope
//! followers, written to a lock-free shared array the GUI polls.

use atomic_float::AtomicF32;
use harmonic_core::{FilterMode, Svf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Number of display bands (≈ ⅓-octave, 35 Hz … 17.5 kHz).
pub const BANDS: usize = 30;

const F_LO: f64 = 35.0;
const F_HI: f64 = 17_500.0;

/// Band centre frequency, log-spaced.
pub fn band_hz(i: usize) -> f64 {
    let t = i as f64 / (BANDS - 1) as f64;
    F_LO * (F_HI / F_LO).powf(t)
}

/// The shared magnitudes, one linear-gain value per band. Written by the audio
/// thread, read by the GUI — atomics, never a lock.
pub struct AnalyzerBands {
    levels: [AtomicF32; BANDS],
}

impl AnalyzerBands {
    pub fn new() -> Arc<Self> {
        Arc::new(AnalyzerBands {
            levels: std::array::from_fn(|_| AtomicF32::new(0.0)),
        })
    }

    #[inline]
    pub fn get(&self, i: usize) -> f32 {
        self.levels[i].load(Ordering::Relaxed)
    }
}

/// Audio-thread side: the filter bank + followers. `Box`ed by the plugin so the
/// ~4 KB of filter state does not bloat the `Plugin` struct inline.
pub struct SpectrumAnalyzer {
    filters: [Svf; BANDS],
    env: [f32; BANDS],
    atk: f32,
    rel: f32,
    bands: Arc<AnalyzerBands>,
}

impl SpectrumAnalyzer {
    pub fn new(bands: Arc<AnalyzerBands>) -> Self {
        SpectrumAnalyzer {
            filters: std::array::from_fn(|_| Svf::new(48_000.0)),
            env: [0.0; BANDS],
            atk: 0.0,
            rel: 0.0,
            bands,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        for (i, f) in self.filters.iter_mut().enumerate() {
            *f = Svf::new(sr);
            f.set_mode(FilterMode::Band);
            f.set_cutoff(band_hz(i).min(0.45 * sr));
            f.set_resonance(0.55); // Q ≈ 5 → ~⅓-octave bandwidth
            f.reset();
        }
        let dt = 1.0 / sr as f32;
        self.atk = 1.0 - (-dt / 0.005).exp(); // ~5 ms
        self.rel = 1.0 - (-dt / 0.150).exp(); // ~150 ms
        self.env = [0.0; BANDS];
    }

    /// Feed one (mono-summed) master sample. RT-safe: no allocation.
    #[inline]
    pub fn feed(&mut self, x: f32) {
        for i in 0..BANDS {
            let mag = self.filters[i].process(x).abs();
            let e = &mut self.env[i];
            let coeff = if mag > *e { self.atk } else { self.rel };
            *e += coeff * (mag - *e);
            self.bands.levels[i].store(*e, Ordering::Relaxed);
        }
    }
}
