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
    sample_rate: f64, // *operating* rate — 2× base while the HQ bus is active
    // The rate `set_cutoff`'s musical ceiling is measured against. Set once
    // at construction, never touched by `set_sample_rate` — unlike
    // `sample_rate`, this must NOT double when HQ turns the filter's
    // operating rate to 2×, or the same modulation sweep would open the
    // filter twice as far in HQ mode as at 1× (RFC-19 audit). `sample_rate`
    // itself still has to track the real operating rate for prewarp
    // (`recompute_g`) and the smoother's timing — those need the true rate,
    // only the musical clamp doesn't.
    base_sample_rate: f64,
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
            base_sample_rate: sr,
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

    /// Cutoff target in Hz. Clamped to `[20, 0.45·fs_base]` — against the
    /// *base* rate, not the current operating rate, so the same modulation
    /// sweep reaches the same ceiling whether or not the HQ bus has this
    /// filter running at `2×` (RFC-19 audit: clamping against the doubled
    /// operating rate let HQ-on sweeps open twice as far as HQ-off, an
    /// audible tonal difference the HQ toggle is supposed to be silent
    /// about). Smoothed toward per sample inside [`Self::process`].
    #[inline]
    pub fn set_cutoff(&mut self, hz: f64) {
        let hi = Self::MAX_CUTOFF_FRAC * self.base_sample_rate;
        self.cutoff_t = if hz < Self::MIN_CUTOFF_HZ {
            Self::MIN_CUTOFF_HZ
        } else if hz > hi {
            hi
        } else {
            hz
        };
    }

    /// Reconfigure the filter to run at a different sample rate — for the
    /// unified HQ bus, which processes two subsamples per output sample at
    /// `2×` the base rate (`docs/15_TECHNICAL_SPEC_HQ_BUS.md`). Recomputes the
    /// smoothing time constant and the cutoff clamp bound for the new rate,
    /// re-clamps the cutoff target/smoothed value against it, and rebuilds
    /// coefficients immediately (not lazily on the next moving-target check —
    /// the rate itself changed, not just the target). A no-op when `sample_rate`
    /// already matches. Integrator state (`ic1`/`ic2`) is left alone: this is a
    /// discrete mode switch (HQ toggled on/off), not a per-sample operation, so
    /// a brief settling transient is an acceptable trade against always paying
    /// for a rate check on every sample.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if sample_rate == self.sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        let dt = 1.0 / sample_rate;
        let tau = 0.001;
        self.smooth = 1.0 - dt / (tau + dt);
        // Re-clamp against the *base* rate's musical ceiling, same as
        // `set_cutoff` — not the new operating rate. In practice this is
        // already a no-op (every `cutoff_t`/`cutoff_z` in the crate only
        // ever comes from `set_cutoff`, which enforces this same bound), but
        // `base_sample_rate` never changes here, so keeping it costs nothing
        // and doesn't rely on that invariant holding forever.
        let hi = Self::MAX_CUTOFF_FRAC * self.base_sample_rate;
        if self.cutoff_t > hi {
            self.cutoff_t = hi;
        }
        if self.cutoff_z > hi {
            self.cutoff_z = hi;
        }
        self.recompute_g();
        self.recompute_a();
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
        // g = tan(π fc / fs) ; angle in turns is fc / (2 fs). `set_cutoff` clamps
        // fc to `[20, 0.45 fs]`, so this already lands in `[~1e-5, 0.225]` and the
        // fast bounded-domain rational is exact enough (~4× cheaper on a modulated
        // cutoff than `tan_turns`). The extra `.min(0.225)` is defence in depth:
        // `tan_turns_fast` has a pole at 0.25 turns (by construction — it mirrors
        // `tan` at Nyquist) and returns a *negative* g just past it, which would
        // destabilise the SVF. Rather than trust every future writer of `cutoff_z`
        // to clamp, cap it here — two instructions, and it also turns a stray NaN
        // cutoff into a defined (very dark) filter instead of NaN integrator state.
        let turns = (self.cutoff_z / (2.0 * self.sample_rate)).min(0.225);
        self.g = tan_turns_fast(turns);
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

    #[test]
    fn set_sample_rate_retargets_the_prewarp_and_the_clamp() {
        // Same Hz cutoff, doubled sample rate: `turns = cutoff/(2*sample_rate)`
        // halves, so `g = tan_turns_fast(turns)` must (roughly) halve too —
        // this is the direct, mechanical proof that `recompute_g` re-ran
        // against the *new* rate rather than a stale one.
        let mut s = Svf::new(48_000.0);
        s.set_cutoff(4_000.0);
        s.reset();
        let g48 = s.g;
        s.set_sample_rate(96_000.0);
        let g96 = s.g;
        assert!(
            g96 < g48 * 0.6 && g96 > g48 * 0.4,
            "g should roughly halve when the rate doubles at a fixed Hz cutoff: {g48} -> {g96}"
        );
        // a no-op call at the *new* rate must not perturb it further.
        s.set_sample_rate(96_000.0);
        assert_eq!(s.g, g96, "no-op set_sample_rate changed g");

        // the musical cutoff ceiling must NOT track the new operating rate
        // (RFC-19 audit): it stays pinned to the *base* rate (0.45*48000 =
        // 21 600 Hz) even while running at 2x, so the same modulation sweep
        // opens the filter to the same absolute Hz whether or not HQ is on.
        s.set_cutoff(30_000.0); // > 0.45*48000 — must clamp down, not pass through
        assert!(
            (s.cutoff_t - 0.45 * 48_000.0).abs() < 1.0,
            "musical ceiling moved with the operating rate: {}",
            s.cutoff_t
        );

        // switching back down changes nothing about that ceiling.
        s.set_sample_rate(48_000.0);
        assert!(s.cutoff_t <= 0.45 * 48_000.0 + 1.0, "ceiling drifted: {}", s.cutoff_t);

        // and the filter stays stable and finite through all of this.
        s.set_mode(FilterMode::Low);
        for i in 0..1000 {
            let x = ((i as f64 * 0.05).sin()) as f32;
            assert!(s.process(x).is_finite());
        }
    }

    #[test]
    fn cutoff_ceiling_is_identical_at_1x_and_hq_2x() {
        // RFC-19 audit: the same requested cutoff — including one far past
        // the 1x ceiling, as a fast LFO/envelope sweep would send — must
        // clamp to the *same* Hz value whether the filter is running at its
        // base rate or the HQ bus's 2x. Before this fix, HQ let the same
        // sweep reach twice as far (a real, audible HQ-on/off tonal
        // difference), because the clamp tracked the operating rate.
        let base = 44_100.0;
        for requested in [8_000.0, 19_845.0, 25_000.0, 60_000.0, 500_000.0] {
            let mut s1x = Svf::new(base);
            s1x.set_cutoff(requested);

            let mut s2x = Svf::new(base);
            s2x.set_sample_rate(2.0 * base);
            s2x.set_cutoff(requested);

            assert_eq!(
                s1x.cutoff_t, s2x.cutoff_t,
                "cutoff ceiling differs between 1x and HQ 2x for requested={requested}: {} vs {}",
                s1x.cutoff_t, s2x.cutoff_t
            );
        }
    }
}
