//! Note → frequency mapping for [`PolySynth`](crate::PolySynth).
//!
//! The default is 12-tone equal temperament, A4 = 440 Hz — and on that path the
//! engine is byte-for-byte what it was before this module existed
//! (`PolySynth::note_hz` short-circuits straight to [`crate::midi_to_hz`]).
//!
//! A [`Tuning`] describes any regular scale in the Scala sense: a repeat
//! interval ("period", 1200 cents = an octave, but e.g. 1901.955 for
//! Bohlen-Pierce), a list of scale-degree offsets in cents from the tonic, and
//! a reference (MIDI note, frequency) the whole thing hangs off. The keyboard
//! mapping is the simple linear one: MIDI note `ref_note` is degree 0, each
//! higher key is the next degree, wrapping into the next period.
//!
//! This is *fundamental* retuning — it moves where a played note sits. It does
//! **not** (and with the closed form cannot) move an individual overtone off
//! `k·f0`: the oscillator sums harmonics at integer multiples of whatever
//! frequency it is handed. Inharmonic partial spacing would be a different
//! kernel (`docs/09_ROADMAP.md`).

use crate::trig::exp2;

/// A regular scale: period + degree cents + reference anchor.
///
/// Build one with [`Tuning::equal`] (n-EDO) or [`Tuning::from_cents`] (an
/// arbitrary Scala-style scale), or use [`Tuning::EQUAL_440`] (the default).
/// All constructors sanitise their input — a `Tuning` value is always usable.
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    /// Frequency, in Hz, of `ref_note`. Clamped to `[8, 20000]`.
    ref_hz: f64,
    /// MIDI note that sits at `ref_hz` and is scale degree 0 (the tonic).
    ref_note: u8,
    /// Repeat interval in cents (1200 = octave). Clamped to `[1, 4800]`.
    period_cents: f64,
    /// Cents of each degree from the tonic; `degrees[0]` is forced to `0.0`.
    /// Only the first `n_degrees` entries are used.
    degrees: [f64; Self::MAX_DEGREES],
    /// Number of degrees per period. Clamped to `[1, MAX_DEGREES]`.
    n_degrees: u8,
}

impl Tuning {
    /// Largest scale this can hold. 64 covers 53-EDO / 53-comma Turkish scales,
    /// Bohlen-Pierce, every common just / historical scale, and most Scala
    /// files; larger `.scl` files are truncated by the caller.
    pub const MAX_DEGREES: usize = 64;

    /// 12-tone equal temperament, A4 (MIDI 69) = 440 Hz. The default; the engine
    /// treats this exactly like the pre-tuning `midi_to_hz` path (bit-identical).
    pub const EQUAL_440: Tuning = {
        let mut d = [0.0f64; Self::MAX_DEGREES];
        // 0, 100, 200, … 1100 — exact in f64
        let mut k = 0;
        while k < 12 {
            d[k] = (k as f64) * 100.0;
            k += 1;
        }
        Tuning { ref_hz: 440.0, ref_note: 69, period_cents: 1200.0, degrees: d, n_degrees: 12 }
    };

    /// n-tone equal temperament: `edo` equal steps of `1200 / edo` cents per
    /// octave, one step per MIDI key. `ref_note` (usually 69) sits at `ref_hz`.
    /// `edo` is clamped to `[1, MAX_DEGREES]`, `ref_hz` to `[8, 20000]`.
    pub fn equal(edo: u8, ref_hz: f64, ref_note: u8) -> Tuning {
        let n = clampu(edo, 1, Self::MAX_DEGREES as u8);
        let mut degrees = [0.0f64; Self::MAX_DEGREES];
        let step = 1200.0 / n as f64;
        let mut k = 0usize;
        while k < n as usize {
            degrees[k] = k as f64 * step;
            k += 1;
        }
        Tuning {
            ref_hz: sane_hz(ref_hz),
            ref_note,
            period_cents: 1200.0,
            degrees,
            n_degrees: n,
        }
    }

    /// An arbitrary regular scale from a list of degree cents (measured from the
    /// tonic; a leading `0.0` is optional — it is forced regardless) and a
    /// `period` in cents. `cents` past [`Tuning::MAX_DEGREES`] entries is
    /// ignored; non-finite cents become `0.0`; `period` is clamped to
    /// `[1, 4800]`; `ref_hz` to `[8, 20000]`.
    pub fn from_cents(cents: &[f64], period: f64, ref_hz: f64, ref_note: u8) -> Tuning {
        let mut degrees = [0.0f64; Self::MAX_DEGREES];
        let n = if cents.len() > Self::MAX_DEGREES { Self::MAX_DEGREES } else { cents.len() };
        let mut k = 0usize;
        while k < n {
            let c = cents[k];
            degrees[k] = if c.is_finite() { c } else { 0.0 };
            k += 1;
        }
        degrees[0] = 0.0;
        let n_degrees = if n == 0 { 1 } else { n as u8 };
        Tuning {
            ref_hz: sane_hz(ref_hz),
            ref_note,
            period_cents: clampf(period, 1.0, 4800.0),
            degrees,
            n_degrees,
        }
    }

    /// `true` iff this is exactly [`Tuning::EQUAL_440`] — the engine's fast path.
    #[inline]
    pub fn is_equal_440(&self) -> bool {
        if self.ref_hz != 440.0
            || self.ref_note != 69
            || self.period_cents != 1200.0
            || self.n_degrees != 12
        {
            return false;
        }
        let mut k = 0usize;
        while k < 12 {
            if self.degrees[k] != (k as f64) * 100.0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// Frequency, in Hz, for a MIDI note under this scale. Always finite and
    /// positive. Pitch bend / unison detune are applied by the caller on top.
    #[inline]
    pub fn hz(&self, note: u8) -> f64 {
        let n = self.n_degrees.max(1) as i32;
        let rel = note as i32 - self.ref_note as i32;
        let period = rel.div_euclid(n);
        let degree = rel.rem_euclid(n) as usize; // 0 ..< n ≤ MAX_DEGREES
        let cents = period as f64 * self.period_cents + self.degrees[degree];
        let hz = self.ref_hz * exp2(cents / 1200.0);
        // exp2 is finite for finite input and ref_hz is already sane, but a
        // huge |cents| (only reachable via from_cents with wild input) could
        // still overflow — keep the contract absolute.
        if hz.is_finite() && hz > 0.0 {
            hz
        } else {
            8.0
        }
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning::EQUAL_440
    }
}

#[inline]
fn sane_hz(hz: f64) -> f64 {
    if hz.is_finite() {
        clampf(hz, 8.0, 20_000.0)
    } else {
        440.0
    }
}

#[inline]
fn clampf(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_nan() || x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

#[inline]
fn clampu(x: u8, lo: u8, hi: u8) -> u8 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_to_hz;

    #[test]
    fn equal_440_is_bit_identical_to_midi_to_hz() {
        let t = Tuning::EQUAL_440;
        assert!(t.is_equal_440());
        for n in 0u8..=127 {
            assert_eq!(
                t.hz(n).to_bits(),
                midi_to_hz(n as f32).to_bits(),
                "note {n}: {} vs {}",
                t.hz(n),
                midi_to_hz(n as f32)
            );
        }
    }

    #[test]
    fn equal_edo_matches_the_definition() {
        // 12-EDO built via `equal` also lands on 12-TET frequencies
        let t = Tuning::equal(12, 440.0, 69);
        for n in [21u8, 33, 45, 57, 69, 81, 93, 108] {
            assert!((t.hz(n) - midi_to_hz(n as f32)).abs() < 1e-9);
        }
        // 24-EDO: one MIDI key = a quarter tone, so note 69+1 is 50 cents up
        let q = Tuning::equal(24, 440.0, 69);
        assert!((q.hz(69) - 440.0).abs() < 1e-9);
        assert!((q.hz(70) - 440.0 * exp2(50.0 / 1200.0)).abs() < 1e-9);
        assert!((q.hz(93) - 440.0 * exp2(1200.0 / 1200.0)).abs() < 1e-9); // 24 steps = an octave
    }

    #[test]
    fn just_intonation_puts_the_fifth_at_a_pure_3_2() {
        // 5-limit chromatic, rooted at C4 = MIDI 60
        let ji: [f64; 12] = [
            0.0, 111.731, 203.910, 315.641, 386.314, 498.045, 590.224, 701.955, 813.686, 884.359,
            1017.596, 1088.269,
        ];
        let c4 = midi_to_hz(60.0);
        let t = Tuning::from_cents(&ji, 1200.0, c4, 60);
        // tonic unchanged
        assert!((t.hz(60) - c4).abs() < 1e-6);
        // the fifth (G, degree 7) is a pure 3:2 above the tonic
        assert!((t.hz(67) / t.hz(60) - 1.5).abs() < 1e-3);
        // the major third (E, degree 4) is a pure 5:4
        assert!((t.hz(64) / t.hz(60) - 1.25).abs() < 1e-3);
        // octave still doubles
        assert!((t.hz(72) / t.hz(60) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn bohlen_pierce_repeats_at_the_tritave() {
        // 13 equal steps of the 3:1 "tritave" (1901.955 cents)
        let period = 1901.955;
        let steps: [f64; 13] = core::array::from_fn(|k| k as f64 * period / 13.0);
        let t = Tuning::from_cents(&steps, period, 440.0, 69);
        assert!((t.hz(69) - 440.0).abs() < 1e-9);
        // 13 keys up = ×3 exactly
        assert!((t.hz(69 + 13) / t.hz(69) - 3.0).abs() < 1e-3);
    }

    #[test]
    fn hostile_scales_still_produce_finite_positive_frequencies() {
        let wild = [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e9, -1e9, 0.0];
        let t = Tuning::from_cents(&wild, f64::NAN, f64::NAN, 0);
        for n in 0u8..=127 {
            let h = t.hz(n);
            assert!(h.is_finite() && h > 0.0, "note {n} → {h}");
        }
        // ref_hz NaN fell back to 440, period NaN fell back to 1 cent
        assert!(!t.is_equal_440());
    }

    #[test]
    fn reference_frequency_scales_the_whole_scale() {
        let t432 = Tuning::equal(12, 432.0, 69);
        assert!((t432.hz(69) - 432.0).abs() < 1e-9);
        assert!((t432.hz(60) - midi_to_hz(60.0) * (432.0 / 440.0)).abs() < 1e-6);
    }
}
