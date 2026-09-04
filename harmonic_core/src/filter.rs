//! Zero-delay-feedback state-variable filter (trapezoidal / TPT integration),
//! after Andrew Simper (Cytomic), *"Solving the continuous SVF equations using
//! trapezoidal integration"*.
//!
//! Unconditionally stable for every cutoff up to Nyquist and every resonance;
//! low / band / high / notch from the same two integrators. `no_std`, no
//! `libm` — the prewarp `tan(π fc/fs)` is [`crate::trig::tan_turns`].
//!
//! Cutoff and resonance are smoothed **inside** the filter, per sample (~1 ms
//! one-pole), so a host can drop new targets at block rate — or a filter
//! envelope can drop them per sample — with no zipper. Coefficients are
//! rebuilt only on the samples where a target is actually moving.

use crate::trig::{exp2, tan_turns_fast};

/// Filter response.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FilterMode {
    Bypass = 0,
    Low = 1,
    Band = 2,
    High = 3,
    Notch = 4,
}

impl FilterMode {
    /// For the C ABI. Unknown values fall back to `Bypass`.
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => FilterMode::Low,
            2 => FilterMode::Band,
            3 => FilterMode::High,
            4 => FilterMode::Notch,
            _ => FilterMode::Bypass,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Svf {
    sample_rate: f64,
    mode: FilterMode,
    smooth: f64, // one-pole coefficient for the parameter smoothers

    // targets / smoothed parameter state
    cutoff_t: f64,
    cutoff_z: f64,
    res_t: f64,
    res_z: f64,

    // coefficients
    g: f64,
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,

    // integrator state
    ic1: f64,
    ic2: f64,
}

impl Svf {
    /// Cutoff is clamped below this fraction of the sample rate so the prewarp
    /// `tan(π fc/fs)` stays finite.
    pub const MAX_CUTOFF_FRAC: f64 = 0.45;
    pub const MIN_CUTOFF_HZ: f64 = 20.0;

    pub fn new(sample_rate: f64) -> Self {
        // Callers (Voice / PolySynth) already validate; this is a defensive
        // fallback for direct construction.
        let sr = crate::validate_sample_rate(sample_rate).0;
        // ~1 ms smoother: fast enough for a snappy filter envelope, slow enough
        // to kill the zipper from block-rate cutoff automation.
        let dt = 1.0 / sr;
        let tau = 0.001;
        let smooth = 1.0 - dt / (tau + dt);

        let mut s = Svf {
            sample_rate: sr,
            mode: FilterMode::Bypass,
            smooth,
            cutoff_t: 0.4 * sr,
            cutoff_z: 0.4 * sr,
            res_t: 0.0,
            res_z: 0.0,
            g: 0.0,
            k: 2.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        s.recompute_g();
        s.recompute_k();
        s.recompute_a();
        s
    }

    #[inline]
    pub fn set_mode(&mut self, mode: FilterMode) {
        self.mode = mode;
    }

    #[inline]
    pub fn mode(&self) -> FilterMode {
        self.mode
    }

    /// Cutoff target in Hz. Clamped to `[20, 0.45·fs]`. Smoothed toward per
    /// sample inside [`Self::process`].
    #[inline]
    pub fn set_cutoff(&mut self, hz: f64) {
        let hi = Self::MAX_CUTOFF_FRAC * self.sample_rate;
        self.cutoff_t = if hz < Self::MIN_CUTOFF_HZ {
            Self::MIN_CUTOFF_HZ
        } else if hz > hi {
            hi
        } else {
            hz
        };
    }

    /// Resonance target `0..1` → Q ∈ `[0.5, 32]`, exponential. Capped; does not
    /// self-oscillate. Smoothed per sample.
    #[inline]
    pub fn set_resonance(&mut self, r: f64) {
        self.res_t = if r < 0.0 {
            0.0
        } else if r > 1.0 {
            1.0
        } else {
            r
        };
    }

    #[inline]
    fn recompute_g(&mut self) {
        // g = tan(π fc / fs) ; angle in turns is fc / (2 fs) ∈ [~1e-5, 0.225]
        // (cutoff clamped to `[20, 0.45 fs]`), so the fast bounded-domain
        // rational is exact enough here and ~4× cheaper on a modulated cutoff.
        self.g = tan_turns_fast(self.cutoff_z / (2.0 * self.sample_rate));
    }

    #[inline]
    fn recompute_k(&mut self) {
        let q = 0.5 * exp2(self.res_z * 6.0); // 0.5 .. 32
        self.k = 1.0 / q;
    }

    #[inline]
    fn recompute_a(&mut self) {
        let a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a1 = a1;
        self.a2 = self.g * a1;
        self.a3 = self.g * self.a2;
    }

    /// Snap smoothers to their targets and clear integrator state
    /// (note-on / host reset).
    #[inline]
    pub fn reset(&mut self) {
        self.cutoff_z = self.cutoff_t;
        self.res_z = self.res_t;
        self.recompute_g();
        self.recompute_k();
        self.recompute_a();
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Process one sample. Per-sample parameter smoothing + coefficient rebuild
    /// happen here, but only while a target is actually moving.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // ---- per-sample parameter interpolation ----
        let moving_c = (self.cutoff_t - self.cutoff_z).abs() > 1.0e-3;
        let moving_r = (self.res_t - self.res_z).abs() > 1.0e-6;
        if moving_c {
            self.cutoff_z += (1.0 - self.smooth) * (self.cutoff_t - self.cutoff_z);
            self.recompute_g();
        }
        if moving_r {
            self.res_z += (1.0 - self.smooth) * (self.res_t - self.res_z);
            self.recompute_k();
        }
        if moving_c || moving_r {
            self.recompute_a();
        }

        if self.mode == FilterMode::Bypass {
            return x;
        }

        // ---- Cytomic SVF step ----
        let v0 = x as f64;
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;

        let out = match self.mode {
            FilterMode::Low => v2,
            FilterMode::Band => v1,
            FilterMode::High => v0 - self.k * v1 - v2,
            FilterMode::Notch => v0 - self.k * v1,
            FilterMode::Bypass => v0,
        };
        out as f32
    }
}

impl Default for Svf {
    fn default() -> Self {
        Svf::new(48_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(mode: FilterMode, cutoff: f64, res: f64, f_hz: f64) -> f64 {
        let sr = 48_000.0;
        let mut s = Svf::new(sr);
        s.set_mode(mode);
        s.set_cutoff(cutoff);
        s.set_resonance(res);
        s.reset();
        let mut acc = 0.0;
        let n = 8192;
        for i in 0..2048 {
            let x = (i as f64 * core::f64::consts::TAU * f_hz / sr).sin() as f32;
            s.process(x);
        }
        for i in 2048..(2048 + n) {
            let x = (i as f64 * core::f64::consts::TAU * f_hz / sr).sin() as f32;
            let y = s.process(x) as f64;
            acc += y * y;
        }
        (acc / n as f64).sqrt()
    }

    #[test]
    fn bypass_is_identity() {
        let mut s = Svf::new(48_000.0);
        s.set_mode(FilterMode::Bypass);
        s.reset();
        for i in -500..500 {
            let x = i as f32 / 250.0;
            assert_eq!(s.process(x), x);
        }
    }

    #[test]
    fn lowpass_passes_low_blocks_high() {
        let low = response(FilterMode::Low, 1000.0, 0.0, 100.0);
        let high = response(FilterMode::Low, 1000.0, 0.0, 10_000.0);
        assert!(low > 0.5, "LP killed the passband: {low}");
        assert!(high < 0.05, "LP let 10 kHz through: {high}");
    }

    #[test]
    fn highpass_blocks_low_passes_high() {
        let low = response(FilterMode::High, 1000.0, 0.0, 80.0);
        let high = response(FilterMode::High, 1000.0, 0.0, 12_000.0);
        assert!(low < 0.05, "HP let 80 Hz through: {low}");
        assert!(high > 0.5, "HP killed the passband: {high}");
    }

    #[test]
    fn bandpass_peaks_near_cutoff() {
        let at = response(FilterMode::Band, 2000.0, 0.6, 2000.0);
        let below = response(FilterMode::Band, 2000.0, 0.6, 200.0);
        let above = response(FilterMode::Band, 2000.0, 0.6, 16_000.0);
        assert!(at > below * 3.0 && at > above * 3.0, "BP not peaked: {below} {at} {above}");
    }

    #[test]
    fn resonance_lifts_the_corner() {
        let flat = response(FilterMode::Low, 1000.0, 0.0, 1000.0);
        let resonant = response(FilterMode::Low, 1000.0, 1.0, 1000.0);
        assert!(resonant > flat * 2.0, "resonance did nothing: {flat} -> {resonant}");
    }

    #[test]
    fn per_sample_smoothing_removes_the_zipper() {
        // Alternate the cutoff target between two far-apart values every sample,
        // as a hostile block-rate automation would. The smoother must keep the
        // output continuous (no sample-to-sample jump beyond a small bound).
        let sr = 48_000.0;
        let mut s = Svf::new(sr);
        s.set_mode(FilterMode::Low);
        s.set_resonance(0.4);
        s.reset();
        let mut prev = 0.0_f32;
        let mut worst_jump = 0.0_f32;
        for i in 0..20_000 {
            s.set_cutoff(if i % 2 == 0 { 300.0 } else { 8000.0 });
            let x = ((i as f64 * 0.02).sin() * 0.6) as f32;
            let y = s.process(x);
            assert!(y.is_finite());
            worst_jump = worst_jump.max((y - prev).abs());
            prev = y;
        }
        assert!(worst_jump < 0.35, "zipper: worst sample jump {worst_jump}");
    }

    #[test]
    fn stable_under_cutoff_and_resonance_sweep() {
        let sr = 48_000.0;
        let mut s = Svf::new(sr);
        s.set_mode(FilterMode::Low);
        s.set_resonance(1.0);
        s.reset();
        for i in 0..200_000 {
            let fc = 20.0 + (i as f64 * 0.11) % 21_000.0;
            s.set_cutoff(fc);
            let x = ((i as f64 * 0.03).sin() * 0.8) as f32;
            let y = s.process(x);
            assert!(y.is_finite() && y.abs() < 20.0, "blew up at i={i}: {y}");
        }
    }
}
