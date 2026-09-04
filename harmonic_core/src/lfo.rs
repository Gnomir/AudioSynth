//! A low-frequency oscillator for modulation. `Copy`, `no_std`, no `libm`.
//! Sine / triangle / saw, all phase-aligned (rising through 0 at phase 0) so
//! swapping shape mid-note does not jump.

use crate::trig::{floor_f64, sin_turns_fast};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LfoShape {
    Sine = 0,
    Triangle = 1,
    Saw = 2,
}

impl LfoShape {
    /// For the C ABI. Unknown values fall back to `Sine`.
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => LfoShape::Triangle,
            2 => LfoShape::Saw,
            _ => LfoShape::Sine,
        }
    }
}

/// Key-sync behaviour on note-on.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LfoMode {
    /// Phase snaps to 0 on every note-on (default).
    Retrigger = 0,
    /// Phase keeps running across notes (analog-style, per-voice free-run).
    FreeRun = 1,
}

impl LfoMode {
    /// For the C ABI. Unknown values fall back to `Retrigger`.
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => LfoMode::FreeRun,
            _ => LfoMode::Retrigger,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Lfo {
    phase: f64, // turns, [0, 1)
    inc: f64,   // turns per sample
    shape: LfoShape,
    mode: LfoMode,
}

impl Lfo {
    pub const fn new() -> Self {
        Lfo {
            phase: 0.0,
            inc: 0.0,
            shape: LfoShape::Sine,
            mode: LfoMode::Retrigger,
        }
    }

    /// Rate in Hz. Clamped to `[0, fs/2)`.
    #[inline]
    pub fn set_rate(&mut self, hz: f64, sample_rate: f64) {
        let h = if hz < 0.0 { 0.0 } else { hz };
        self.inc = (h / sample_rate).min(0.499);
    }

    #[inline]
    pub fn set_shape(&mut self, shape: LfoShape) {
        self.shape = shape;
    }

    #[inline]
    pub fn set_mode(&mut self, mode: LfoMode) {
        self.mode = mode;
    }

    #[inline]
    pub fn set_phase(&mut self, turns: f64) {
        self.phase = turns - floor_f64(turns);
    }

    /// Note-on. Restarts from phase 0 in [`LfoMode::Retrigger`]; a no-op in
    /// [`LfoMode::FreeRun`] (the phase keeps running across notes).
    #[inline]
    pub fn retrigger(&mut self) {
        if self.mode == LfoMode::Retrigger {
            self.phase = 0.0;
        }
    }

    /// Advance one sample, return the value in `[-1, 1]`.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let p = self.phase;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        match self.shape {
            // a modulator — 16-bit trig is plenty, and it halves the cost
            LfoShape::Sine => sin_turns_fast(p) as f32,
            LfoShape::Triangle => {
                // rising through 0 at p=0: 0 → +1 → 0 → −1 → 0
                let t = if p < 0.25 {
                    4.0 * p
                } else if p < 0.75 {
                    2.0 - 4.0 * p
                } else {
                    4.0 * p - 4.0
                };
                t as f32
            }
            LfoShape::Saw => {
                // rising ramp, phase-aligned: 0 → +1, wrap to −1 → 0
                let s = if p < 0.5 { 2.0 * p } else { 2.0 * p - 2.0 };
                s as f32
            }
        }
    }
}

impl Default for Lfo {
    fn default() -> Self {
        Lfo::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn cycle(shape: LfoShape, hz: f64) -> Vec<f32> {
        let mut l = Lfo::new();
        l.set_shape(shape);
        l.set_rate(hz, SR);
        let n = (SR / hz) as usize;
        (0..n).map(|_| l.tick()).collect()
    }

    #[test]
    fn all_shapes_stay_in_range_and_have_zero_mean() {
        for shape in [LfoShape::Sine, LfoShape::Triangle, LfoShape::Saw] {
            let c = cycle(shape, 5.0);
            assert!(c.iter().all(|&v| (-1.0001..=1.0001).contains(&v)));
            let mean: f32 = c.iter().sum::<f32>() / c.len() as f32;
            assert!(mean.abs() < 0.02, "mean {mean}");
        }
    }

    #[test]
    fn free_run_mode_survives_retrigger() {
        let mut l = Lfo::new();
        l.set_rate(5.0, SR);
        l.set_mode(LfoMode::FreeRun);
        for _ in 0..1234 {
            l.tick();
        }
        let before = l.tick();
        l.retrigger(); // note-on: must NOT reset the phase
        let after = l.tick();
        // phase advanced by exactly one inc, not jumped back to ~0
        assert!((after - before).abs() < 0.05, "free-run LFO jumped on retrigger");

        // Retrigger mode (default) does snap back
        let mut r = Lfo::new();
        r.set_rate(5.0, SR);
        for _ in 0..1234 {
            r.tick();
        }
        r.retrigger();
        assert!(r.tick().abs() < 1e-6, "retrigger LFO did not restart at 0");
    }

    #[test]
    fn shapes_are_phase_aligned_at_start() {
        for shape in [LfoShape::Sine, LfoShape::Triangle, LfoShape::Saw] {
            let mut l = Lfo::new();
            l.set_shape(shape);
            l.set_rate(2.0, SR);
            assert!(l.tick().abs() < 1e-6, "{:?} not zero at phase 0", shape as u32);
        }
    }

    #[test]
    fn triangle_and_saw_hit_their_peaks() {
        for shape in [LfoShape::Triangle, LfoShape::Saw] {
            let c = cycle(shape, 4.0);
            let max = c.iter().cloned().fold(f32::MIN, f32::max);
            let min = c.iter().cloned().fold(f32::MAX, f32::min);
            assert!(max > 0.95 && min < -0.95, "{:?} peaks {min}..{max}", shape as u32);
        }
    }
}
