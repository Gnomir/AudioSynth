//! A reusable ADSR envelope. Linear attack, one-pole (exponential) decay and
//! release. `Copy`, `no_std`, and no `exp` — the stage coefficients are plain
//! one-pole time constants derived without transcendentals.
//!
//! Used for both the amplitude envelope (with `sustain = 1`, `decay ≈ 0` it
//! behaves like a simple AR) and the dedicated filter envelope.
//!
//! Decay/release coefficients are tuned so a stage of `t` seconds actually
//! *finishes* in ≈ `t` (reaches −80 dB on release, within 0.5 % on decay),
//! not `5·t` as a naïve `1/(t·fs)` one-pole would give.

use crate::trig::exp2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_inc: f32,
    decay_coeff: f32,
    sustain: f32,
    release_coeff: f32,
}

impl Adsr {
    pub const fn new() -> Self {
        Adsr {
            stage: Stage::Idle,
            level: 0.0,
            attack_inc: 1.0,
            decay_coeff: 0.5,
            sustain: 1.0,
            release_coeff: 0.01,
        }
    }

    /// `attack` / `decay` / `release` in seconds (floored at 0.5 ms),
    /// `sustain` in `0..1`. Safe to call on a sounding envelope.
    pub fn set(
        &mut self,
        sample_rate: f64,
        attack_s: f64,
        decay_s: f64,
        sustain: f64,
        release_s: f64,
    ) {
        let a = fmax(attack_s, 0.0005) * sample_rate;
        self.attack_inc = (1.0 / a) as f32;
        // one-pole coeff s.t. the stage is essentially complete after its time:
        //   decay   → within 2^-7.64 ≈ 0.5 % of sustain
        //   release → 2^-13.3 ≈ -80 dB (below the 1e-4 Idle threshold)
        let d = fmax(decay_s, 0.0005) * sample_rate;
        self.decay_coeff = (1.0 - exp2(-7.64 / d)) as f32;
        let r = fmax(release_s, 0.0005) * sample_rate;
        self.release_coeff = (1.0 - exp2(-13.3 / r)) as f32;
        self.sustain = clamp01(sustain as f32);
    }

    /// Note-on. Starts the attack from the *current* level (so a stolen voice
    /// re-triggers without a click).
    #[inline]
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    /// Note-off. Enters the release stage from wherever the level is.
    #[inline]
    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Hard stop (choke / host reset).
    #[inline]
    pub fn choke(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    /// Advance one sample, return the new level.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        match self.stage {
            Stage::Attack => {
                self.level += self.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += self.decay_coeff * (self.sustain - self.level);
                if (self.level - self.sustain).abs() < 1.0e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = self.sustain;
                // sustain of zero = a percussive shape: free the voice even
                // while the key is held
                if self.sustain < 1.0e-4 {
                    self.stage = Stage::Idle;
                }
            }
            Stage::Release => {
                self.level += self.release_coeff * (0.0 - self.level);
                if self.level < 1.0e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
            Stage::Idle => {}
        }
        self.level
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    #[inline]
    pub fn is_releasing(&self) -> bool {
        self.stage == Stage::Release
    }

    #[inline]
    pub fn level(&self) -> f32 {
        self.level
    }
}

impl Default for Adsr {
    fn default() -> Self {
        Adsr::new()
    }
}

#[inline(always)]
fn fmax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

#[inline(always)]
fn clamp01(x: f32) -> f32 {
    // NaN compares false against everything and falls through unchanged
    // without this check — a NaN `sustain` would otherwise latch permanently
    // into the envelope's Sustain stage, which assigns `self.level =
    // self.sustain` every tick with no other guard.
    if x.is_nan() || x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn run(e: &mut Adsr, samples: usize) -> Vec<f32> {
        (0..samples).map(|_| e.tick()).collect()
    }

    #[test]
    fn ar_shape_when_sustain_is_full() {
        let mut e = Adsr::new();
        e.set(SR, 0.01, 0.001, 1.0, 0.05);
        e.trigger();
        let atk = run(&mut e, (0.01 * SR) as usize + 4);
        assert!(*atk.last().unwrap() > 0.99, "attack didn't reach 1: {}", atk.last().unwrap());
        // held: stays at 1
        let held = run(&mut e, 4800);
        assert!(held.iter().all(|&v| (v - 1.0).abs() < 1e-3));
        e.release();
        let rel = run(&mut e, (SR * 0.5) as usize);
        assert!(*rel.last().unwrap() < 1e-3, "release didn't fall: {}", rel.last().unwrap());
        assert!(!e.is_active());
    }

    #[test]
    fn decays_to_sustain_and_holds() {
        let mut e = Adsr::new();
        e.set(SR, 0.002, 0.05, 0.4, 0.1);
        e.trigger();
        run(&mut e, (SR * 0.4) as usize); // through attack + decay
        let held = run(&mut e, 4800);
        assert!(
            held.iter().all(|&v| (v - 0.4).abs() < 0.02),
            "did not settle at sustain: {:?}",
            &held[..3]
        );
    }

    #[test]
    fn zero_sustain_is_percussive_and_frees() {
        let mut e = Adsr::new();
        e.set(SR, 0.002, 0.05, 0.0, 0.2);
        e.trigger();
        run(&mut e, (SR * 1.0) as usize);
        assert!(!e.is_active(), "zero-sustain envelope never freed");
    }

    #[test]
    fn monotone_attack_then_nonincreasing_release() {
        let mut e = Adsr::new();
        e.set(SR, 0.02, 0.001, 1.0, 0.1);
        e.trigger();
        let atk = run(&mut e, (0.02 * SR) as usize);
        for w in atk.windows(2) {
            assert!(w[1] >= w[0] - 1e-6, "attack not monotone");
        }
        e.release();
        let rel = run(&mut e, (0.1 * SR) as usize);
        for w in rel.windows(2) {
            assert!(w[1] <= w[0] + 1e-6, "release not monotone");
        }
    }
}
