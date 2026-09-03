//! The "character" stage — everything the clean Dirichlet oscillator is not.
//!
//! The band-limited core gives a mathematically perfect starting point. That is
//! the right *foundation* precisely because you can always dirty a clean
//! signal, but never clean a dirty one. This module is the dirt, all of it
//! **on purpose and under a knob**, not from CPU rounding:
//!
//! * `drive` + `bias` — asymmetric saturation → harmonic fattening, even
//!   harmonics, "tube"-ish thickening.
//! * `fold` — a sine wavefolder (West-coast / Serge style) → dense, evolving
//!   upper spectrum from a simple source.
//! * `crush` + `downsample` — deliberate quantisation and sample-rate
//!   reduction → the PPG / DX7 / early-digital grit, as a feature.
//!
//! The nonlinear stages generate content above Nyquist and *will* alias when
//! pushed. That is intentional here — the aliasing is part of the sound the
//! way it was on the machines this is chasing. If you want a surgically clean
//! drive, that is an oversampled v2, not this.

use crate::trig::{exp2, floor_f64};

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

/// Per-voice character processor. Holds the small amount of state the grit
/// stages need (DC blocker, sample-and-hold).
#[derive(Clone, Copy)]
pub struct Character {
    p: CharParams,
    dc_x1: f32,
    dc_y1: f32,
    hold: f32,
    hold_ctr: f32,
}

impl Character {
    pub const fn new() -> Self {
        Character {
            p: CharParams::CLEAN,
            dc_x1: 0.0,
            dc_y1: 0.0,
            hold: 0.0,
            hold_ctr: 0.0,
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

    /// Process one sample. Identity (bit-for-bit) when fully clean.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.p.is_clean() {
            return x;
        }

        // 1. asymmetric saturation
        let pre = (x + self.p.bias * 0.3) * (1.0 + self.p.drive * 8.0);
        let mut y = tanh_pade(pre);

        // 2. DC blocker (bias/asymmetry injects DC)
        let hp = y - self.dc_x1 + 0.9995 * self.dc_y1;
        self.dc_x1 = y;
        self.dc_y1 = hp;
        y = hp;

        // 3. reflective wavefolder (Buchla/Serge style), done as an exact
        // triangle mapping so any input depth folds fully in O(1). Identity
        // when `fold == 0`; as it rises the fold threshold drops and the
        // signal bounces off ±thr, adding progressively denser harmonics.
        if self.p.fold > 0.0 {
            let fold = self.p.fold.min(1.0);
            // pre-gain drives the signal past the threshold; threshold also
            // drops. Both are identity-preserving at fold == 0.
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

        // 5. sample-rate reduction (sample & hold)
        if self.p.downsample > 0.0 {
            let factor = 1.0 + 15.0 * self.p.downsample.min(1.0);
            self.hold_ctr += 1.0;
            if self.hold_ctr >= factor {
                self.hold = y;
                self.hold_ctr -= factor;
            }
            y = self.hold;
        }

        y
    }

    /// Clear state (note-on / host reset).
    #[inline]
    pub fn reset(&mut self) {
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        self.hold = 0.0;
        self.hold_ctr = 0.0;
    }
}

impl Default for Character {
    fn default() -> Self {
        Character::new()
    }
}

/// `tanh` via a Padé approximant. ℝ → (−1, 1), near-identity for small `x`.
#[inline]
pub fn tanh_pade(x: f32) -> f32 {
    let x = if x > 4.0 {
        4.0
    } else if x < -4.0 {
        -4.0
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
    fn drive_adds_energy_but_stays_bounded() {
        let mut c = Character::new();
        c.set_params(CharParams {
            drive: 0.8,
            ..CharParams::CLEAN
        });
        let mut peak = 0.0_f32;
        for i in 0..2000 {
            let x = (i as f32 * 0.05).sin() * 0.3; // quiet input
            let y = c.process(x);
            assert!(y.is_finite() && y.abs() <= 1.05);
            peak = peak.max(y.abs());
        }
        // saturation lifts a quiet signal — that's the "fatten"
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
    fn round_f32_behaves() {
        assert_eq!(round_f32(0.4), 0.0);
        assert_eq!(round_f32(0.6), 1.0);
        assert_eq!(round_f32(-0.6), -1.0);
        assert_eq!(round_f32(2.5), 3.0);
    }
}
