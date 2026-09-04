//! The "character" stage — everything the clean Dirichlet oscillator is not.
//!
//! The band-limited core gives a mathematically perfect starting point. That is
//! the right *foundation* precisely because you can always dirty a clean
//! signal, but never clean a dirty one. This module is the dirt, all of it
//! **on purpose and under a knob**, not from CPU rounding:
//!
//! * `drive` + `bias` — asymmetric saturation → harmonic fattening, even
//!   harmonics, "tube"-ish thickening.
//! * `fold` — a reflective wavefolder (West-coast / Serge style) → dense,
//!   evolving upper spectrum from a simple source.
//! * `crush` + `downsample` — deliberate quantisation and sample-rate
//!   reduction → the PPG / DX7 / early-digital grit, as a feature.
//!
//! These nonlinear stages generate content above Nyquist. Left at 1×
//! ([`Character::process`]) that content aliases when pushed — intentional
//! "digital character". For a clean result the caller can 2×-oversample the
//! oscillator and run [`Character::process_hq`], which decimates through a
//! short linear-phase half-band FIR ([`HQ_LATENCY`] samples of latency).

use crate::trig::{exp2, floor_f64};

/// Samples of latency added by the HQ (2×) path's decimation FIR. `process`
/// (1×) adds none.
pub const HQ_LATENCY: usize = 3;

// 13-tap linear-phase half-band FIR (windowed sinc, normalised). Even taps are
// zero by the half-band property; only 0, ±1, ±3, ±5 remain. Stopband ≈ −50 dB.
const HB0: f32 = 0.500_105;
const HB1: f32 = 0.284_310;
const HB3: f32 = -0.036_082;
const HB5: f32 = 0.001_719;
const HB_LEN: usize = 13;

/// Character parameters. `Copy`, all in `[0,1]` (or `[-1,1]` for `bias`).
#[derive(Clone, Copy)]
pub struct CharParams {
    /// Pre-shaper gain into the saturator. 0 = clean.
    pub drive: f32,
    /// Waveshaper asymmetry, `-1..1`. Nonzero adds even harmonics.
    pub bias: f32,
    /// Sine-wavefolder depth. 0 = off.
    pub fold: f32,
    /// Bit-crush amount. 0 = 16-bit (off), 1 ≈ 4-bit.
    pub crush: f32,
    /// Sample-rate reduction. 0 = off, 1 ≈ hold every 16 samples.
    pub downsample: f32,
}

impl CharParams {
    pub const CLEAN: CharParams = CharParams {
        drive: 0.0,
        bias: 0.0,
        fold: 0.0,
        crush: 0.0,
        downsample: 0.0,
    };

    #[inline]
    fn is_clean(&self) -> bool {
        self.drive <= 0.0
            && self.bias == 0.0
            && self.fold <= 0.0
            && self.crush <= 0.0
            && self.downsample <= 0.0
    }
}

impl Default for CharParams {
    fn default() -> Self {
        CharParams::CLEAN
    }
}

/// Per-voice character processor. Holds the grit-stage state (DC blocker,
/// sample-and-hold) plus the HQ decimation delay line.
#[derive(Clone, Copy)]
pub struct Character {
    p: CharParams,
    dc_x1: f32,
    dc_y1: f32,
    hold: f32,
    hold_ctr: f32,
    decim: [f32; HB_LEN], // 2× samples, decim[LEN-1] = newest
}

impl Character {
    pub const fn new() -> Self {
        Character {
            p: CharParams::CLEAN,
            dc_x1: 0.0,
            dc_y1: 0.0,
            hold: 0.0,
            hold_ctr: 0.0,
            decim: [0.0; HB_LEN],
        }
    }

    #[inline]
    pub fn set_params(&mut self, p: CharParams) {
        self.p = p;
    }

    #[inline]
    pub fn params(&self) -> CharParams {
        self.p
    }

    /// One sample at 1×. Bit-for-bit identity when fully clean; aliases when
    /// the nonlinear stages are pushed.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.p.is_clean() {
            return x;
        }
        self.stage(x, 1.0)
    }

    /// One output sample from two 2×-oversampled oscillator samples (`lo` =
    /// on-grid, `hi` = the half-step in between). The nonlinear stages run at
    /// 2×; a half-band FIR then decimates back, keeping the alias products
    /// generated above the base Nyquist out of the audible band.
    /// Adds [`HQ_LATENCY`] samples of latency.
    ///
    /// This is the standalone per-voice HQ path (used by [`crate::Voice::
    /// render_sample`] directly, and by the C ABI's `harmonic_voice_process`).
    /// `PolySynth`'s unified HQ bus uses [`Character::process_hq_pair`]
    /// instead — it runs the same nonlinear stages but skips this decimation,
    /// because the bus decimates once for the whole mix, not once per voice.
    #[inline]
    pub fn process_hq(&mut self, lo: f32, hi: f32) -> f32 {
        let (a, b) = self.process_hq_pair(lo, hi);
        self.decimate(a, b)
    }

    /// Like [`Character::process_hq`], but returns both 2×-rate samples
    /// un-decimated — for `PolySynth`'s unified HQ bus, which sums many
    /// voices' pairs and decimates once for the whole stereo mix instead of
    /// once per voice (`docs/15_TECHNICAL_SPEC_HQ_BUS.md`).
    #[inline]
    pub fn process_hq_pair(&mut self, lo: f32, hi: f32) -> (f32, f32) {
        if self.p.is_clean() {
            (lo, hi)
        } else {
            (self.stage(lo, 2.0), self.stage(hi, 2.0))
        }
    }

    /// The nonlinear chain for one sample. `sh_mult` scales the sample-and-hold
    /// period so its wall-clock hold time is preserved when called at 2×.
    #[inline]
    fn stage(&mut self, x: f32, sh_mult: f32) -> f32 {
        // 1. asymmetric saturation
        let pre = (x + self.p.bias * 0.3) * (1.0 + self.p.drive * 8.0);
        let mut y = tanh_pade(pre);

        // 2. DC blocker (bias/asymmetry injects DC). The feedback coefficient
        // sets a fixed real-world cutoff, fc = (1-R)/(2π)·f_op ≈ 3.82 Hz at
        // 1×. Called at 2× (`sh_mult == 2.0`, inside the HQ path) `f_op`
        // doubles, so `R` must be √-scaled (from R = exp(-2π·fc/f_op)) to
        // keep fc fixed — otherwise it silently doubles to ~7.6 Hz, shifting
        // sub-bass phase whenever HQ toggles on. Only two `sh_mult` values
        // are ever passed (1.0, 2.0), so a plain branch beats computing a
        // general `powf` (forbidden in `no_std` anyway).
        let dc_r = if sh_mult == 2.0 { 0.999_749_97 } else { 0.9995 };
        let hp = y - self.dc_x1 + dc_r * self.dc_y1;
        self.dc_x1 = y;
        self.dc_y1 = hp;
        y = hp;

        // 3. reflective wavefolder (Buchla/Serge), exact triangle mapping so any
        // depth folds fully in O(1). Identity at `fold == 0`.
        if self.p.fold > 0.0 {
            let fold = self.p.fold.min(1.0);
            let g = 1.0 + 4.0 * fold;
            let thr = (1.0 - 0.97 * fold) as f64;
            let period = 4.0 * thr;
            let mut u = ((y * g) as f64 + thr) / period;
            u -= floor_f64(u);
            let tri = if u < 0.5 {
                period * u - thr
            } else {
                3.0 * thr - period * u
            };
            y = (tri / thr) as f32;
        }

        // 4. bit crush: 12-bit (barely there) down to ~2-bit (destroyed)
        if self.p.crush > 0.0 {
            let bits = 12.0 - 10.0 * self.p.crush.min(1.0);
            let levels = exp2((bits - 1.0) as f64) as f32;
            y = round_f32(y * levels) / levels;
        }

        // 5. sample-rate reduction (sample & hold). Bypassed below a tiny
        // epsilon, not just `> 0.0`: the discrete hold/skip counter below is
        // an event trigger, not a continuous fade — at a downsample value
        // that's nominally "off" but not exactly 0.0 (LFO/host smoothing
        // passing through e.g. 0.001), `factor` sits just above `sh_mult`,
        // so the counter skips an update roughly once every ~1/(factor -
        // sh_mult) samples: a periodic single-sample "hiccup" (~700 Hz at
        // downsample=0.001 @ 48 kHz), not an inaudible near-transparent
        // pass-through as the knob position would suggest.
        if self.p.downsample > 1.0e-4 {
            let factor = (1.0 + 15.0 * self.p.downsample.min(1.0)) * sh_mult;
            self.hold_ctr += 1.0;
            if self.hold_ctr >= factor {
                self.hold = y;
                self.hold_ctr -= factor;
            }
            y = self.hold;
        }

        y
    }

    /// Push two 2× samples, return one decimated (base-rate) sample.
    #[inline]
    fn decimate(&mut self, a: f32, b: f32) -> f32 {
        let d = &mut self.decim;
        d.copy_within(2..HB_LEN, 0); // shift left by 2
        d[HB_LEN - 2] = a;
        d[HB_LEN - 1] = b;
        HB0 * d[6] + HB1 * (d[5] + d[7]) + HB3 * (d[3] + d[9]) + HB5 * (d[1] + d[11])
    }

    /// Clear all state (note-on / host reset).
    #[inline]
    pub fn reset(&mut self) {
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.hold = 0.0;
        self.hold_ctr = 0.0;
        self.decim = [0.0; HB_LEN];
    }
}

impl Default for Character {
    fn default() -> Self {
        Character::new()
    }
}

/// `tanh` via a Padé approximant. ℝ → (−1, 1), near-identity for small `x`.
///
/// `x(27 + x²)/(27 + 9x²)` has `f(±3) = ±1`, `f'(±3) = 0` **and** `f''(±3) = 0`
/// — beyond `|x| = 3` it turns back up (overshoots 1, then diverges). So the
/// input is clamped at `±3`, where the join to the flat `±1` region is
/// C²-continuous: no derivative kink, no high-frequency aliasing when driven
/// hard. (Clamping further out, e.g. at `±4`, would leave a first-derivative
/// discontinuity.) This matches [`crate::poly::soft_clip`].
#[inline]
pub fn tanh_pade(x: f32) -> f32 {
    let x = if x > 3.0 {
        3.0
    } else if x < -3.0 {
        -3.0
    } else {
        x
    };
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[inline(always)]
fn round_f32(v: f32) -> f32 {
    // nearest integer via truncating cast; `as` saturates, so out-of-range is safe
    let bias = if v >= 0.0 { 0.5 } else { -0.5 };
    (v + bias) as i32 as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_params_are_bit_identity() {
        let mut c = Character::new();
        c.set_params(CharParams::CLEAN);
        for i in -1000..1000 {
            let x = i as f32 / 500.0;
            assert_eq!(c.process(x), x);
        }
    }

    #[test]
    fn tanh_pade_joins_the_clamp_smoothly() {
        // No first-derivative kink at the ±3 clamp: the numeric slope just
        // inside must match the slope just outside (which is 0 — flat ±1).
        let h = 1.0e-4_f32;
        let slope_in = (tanh_pade(3.0) - tanh_pade(3.0 - h)) / h;
        let slope_out = (tanh_pade(3.0 + h) - tanh_pade(3.0)) / h;
        assert!(slope_in.abs() < 2.0e-3, "slope into the clamp = {slope_in}");
        assert!(slope_out.abs() < 1.0e-6, "slope past the clamp = {slope_out}");
        // and it never overshoots ±1
        for i in 0..5000 {
            let x = i as f32 * 0.01; // 0 … 50, well past the clamp
            let y = tanh_pade(x);
            assert!(y <= 1.0 + 1.0e-6 && tanh_pade(-x) >= -1.0 - 1.0e-6, "overshoot at x={x}: {y}");
        }
    }

    #[test]
    fn sample_and_hold_state_is_cleared_on_reset() {
        // A driven+downsampled voice, then reset: the S&H `hold` must not bleed
        // the previous note's tail into the new attack.
        let mut c = Character::new();
        c.set_params(CharParams {
            downsample: 1.0,
            drive: 0.6,
            ..CharParams::CLEAN
        });
        for i in 0..500 {
            c.process((i as f32 * 0.3).sin() * 0.8); // load `hold` with something loud
        }
        c.reset();
        // first sample after reset, fed silence, must be silence — not the tail
        assert_eq!(c.process(0.0), 0.0, "S&H bled the previous note after reset");
    }

    #[test]
    fn dc_blocker_time_constant_matches_between_1x_and_2x_paths() {
        // `bias` injects a DC offset (only when `drive` is also nonzero — the
        // whole chain is skipped when clean), which the DC blocker then decays
        // toward zero. Compare that decay between `process` (1×) and
        // `process_hq_pair` (2×, one call = one wall-clock 1×-sample worth of
        // time — its `.1` "hi" output lands at the same instant `process`'s
        // single output would) at matched call counts. If the 2× coefficient
        // weren't rate-compensated (RFC-19 audit), the 2× curve would decay
        // roughly twice as fast and diverge well before either settles.
        let params = CharParams { drive: 0.6, bias: 0.5, ..CharParams::CLEAN };
        let mut c1x = Character::new();
        c1x.set_params(params);
        let mut c2x = Character::new();
        c2x.set_params(params);

        let mut worst_mid_transient = 0.0_f32;
        for i in 0..2000 {
            let y1x = c1x.process(0.0);
            let y2x = c2x.process_hq_pair(0.0, 0.0).1;
            if i == 200 {
                // deep in the transient (tau ~ 42 ms ~ 2000 samples @ 48k) —
                // this is exactly where a mismatched time constant would show
                worst_mid_transient = (y1x - y2x).abs();
            }
            if i == 1999 {
                assert!((y1x - y2x).abs() < 1.0e-4, "1x/2x DC-blocker diverged at settle: {y1x} vs {y2x}");
            }
        }
        assert!(
            worst_mid_transient < 5.0e-3,
            "1x/2x DC-blocker time constants disagree mid-transient: delta {worst_mid_transient}"
        );
    }

    #[test]
    fn tiny_downsample_is_bypassed_not_jittered() {
        // A `downsample` value that's nominally "off" (e.g. mid-LFO-sweep
        // through near-zero) but not exactly 0.0 must not engage the S&H
        // counter — that's a discrete event trigger, not a continuous fade,
        // and just above threshold it skips an update roughly once every
        // ~1/(factor - sh_mult) samples: a periodic single-sample "hiccup",
        // not silence. Below the bypass epsilon, output must match the
        // `downsample == 0.0` case bit-for-bit (drive stays nonzero so the
        // whole chain isn't skipped by `is_clean()`).
        let base = CharParams { drive: 0.6, ..CharParams::CLEAN };
        let mut c_off = Character::new();
        c_off.set_params(base);
        let mut c_tiny = Character::new();
        // just under the bypass epsilon; skip period ~1/(15·downsample) ≈
        // 740 samples, so 5000 samples reliably crosses it several times —
        // large enough that a shorter loop could miss the one differing
        // sample entirely and pass for the wrong reason.
        c_tiny.set_params(CharParams { downsample: 9.0e-5, ..base });

        for i in 0..5000 {
            let x = (i as f32 * 0.07).sin() * 0.5;
            let y_off = c_off.process(x);
            let y_tiny = c_tiny.process(x);
            assert_eq!(y_off, y_tiny, "tiny downsample diverged from off at sample {i}");
        }
    }

    #[test]
    fn drive_adds_energy_but_stays_bounded() {
        let mut c = Character::new();
        c.set_params(CharParams {
            drive: 0.8,
            ..CharParams::CLEAN
        });
        let mut peak = 0.0_f32;
        for i in 0..2000 {
            let x = (i as f32 * 0.05).sin() * 0.3;
            let y = c.process(x);
            assert!(y.is_finite() && y.abs() <= 1.05);
            peak = peak.max(y.abs());
        }
        assert!(peak > 0.3, "drive did nothing: {peak}");
    }

    #[test]
    fn fold_and_grit_stay_finite_and_bounded() {
        let mut c = Character::new();
        c.set_params(CharParams {
            drive: 0.5,
            bias: 0.4,
            fold: 0.7,
            crush: 0.6,
            downsample: 0.5,
        });
        for i in 0..48_000 {
            let x = (i as f32 * 0.017).sin();
            let y = c.process(x);
            assert!(y.is_finite() && y.abs() <= 1.2, "i={i} y={y}");
        }
    }

    #[test]
    fn hq_path_is_bounded_and_reduces_alias_energy() {
        // Feed a near-Nyquist tone hard into the folder. At 1× the fold
        // products alias down as low-frequency energy; at 2× + decimation
        // there is measurably less of it.
        let params = CharParams {
            drive: 0.6,
            fold: 0.8,
            ..CharParams::CLEAN
        };
        let sr = 48_000.0_f64;
        let f = 9_000.0_f64; // high, so fold harmonics land above Nyquist

        let mut lo_only = Character::new();
        lo_only.set_params(params);
        let mut hq = Character::new();
        hq.set_params(params);

        // crude LF-energy proxy: running |sample| below a slow one-pole
        let mut lf_1x = 0.0f64;
        let mut lf_hq = 0.0f64;
        for i in 0..20_000 {
            let p = i as f64 * f / sr;
            let s0 = (p * core::f64::consts::TAU).sin() as f32;
            let s1 = ((p + 0.5) * core::f64::consts::TAU).sin() as f32;

            let y1 = lo_only.process(s0);
            let yq = hq.process_hq(s0, s1);
            assert!(y1.is_finite() && yq.is_finite() && y1.abs() <= 1.5 && yq.abs() <= 1.5);

            // low-pass the rectified output ~ energy below a few kHz
            lf_1x += 0.001 * ((y1.abs() as f64) - lf_1x);
            lf_hq += 0.001 * ((yq.abs() as f64) - lf_hq);
        }
        assert!(lf_hq < lf_1x, "HQ did not reduce alias energy: 1x={lf_1x:.4} hq={lf_hq:.4}");
    }

    #[test]
    fn round_f32_behaves() {
        assert_eq!(round_f32(0.4), 0.0);
        assert_eq!(round_f32(0.6), 1.0);
        assert_eq!(round_f32(-0.6), -1.0);
        assert_eq!(round_f32(2.5), 3.0);
    }
}
