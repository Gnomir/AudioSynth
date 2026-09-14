// Independent signal analysis of the qa_*.wav files rendered by
// examples/qa_report.rs. Deliberately dependency-free (no FFT library) —
// uses a Goertzel-style single-frequency magnitude probe, which is exact
// for a stationary tone and lets us ask for magnitude at exactly the
// frequencies we care about (f0, k*f0, Nyquist-adjacent bins) without
// worrying about FFT bin quantization.
'use strict';
import { readFileSync, readdirSync } from 'node:fs';

function readWavFloat32(path) {
  const buf = readFileSync(path);
  if (buf.toString('ascii', 0, 4) !== 'RIFF' || buf.toString('ascii', 8, 12) !== 'WAVE') {
    throw new Error(`${path}: not a RIFF/WAVE file`);
  }
  let offset = 12;
  let fmt = null;
  let data = null;
  while (offset < buf.length) {
    const id = buf.toString('ascii', offset, offset + 4);
    const size = buf.readUInt32LE(offset + 4);
    const body = offset + 8;
    if (id === 'fmt ') {
      fmt = {
        audioFormat: buf.readUInt16LE(body),
        channels: buf.readUInt16LE(body + 2),
        sampleRate: buf.readUInt32LE(body + 4),
        bitsPerSample: buf.readUInt16LE(body + 14),
      };
    } else if (id === 'data') {
      data = buf.subarray(body, body + size);
    }
    offset = body + size + (size % 2);
  }
  if (!fmt || !data) throw new Error(`${path}: missing fmt/data chunk`);
  if (fmt.audioFormat !== 3 || fmt.bitsPerSample !== 32) {
    throw new Error(`${path}: expected 32-bit IEEE float PCM, got format=${fmt.audioFormat} bits=${fmt.bitsPerSample}`);
  }
  const n = data.length / 4;
  const samples = new Float32Array(n);
  for (let i = 0; i < n; i++) samples[i] = data.readFloatLE(i * 4);
  return { sampleRate: fmt.sampleRate, channels: fmt.channels, samples };
}

// Goertzel magnitude (normalized so a full-scale sine at exactly `freq`
// reads back ~1.0 amplitude), evaluated over `samples[start:start+len]`.
function goertzelMag(samples, sampleRate, freq, start, len) {
  if (start + len > samples.length) {
    throw new Error(`goertzelMag: window [${start}, ${start + len}) exceeds buffer length ${samples.length} - would silently NaN`);
  }
  const k = Math.round((len * freq) / sampleRate);
  const w = (2 * Math.PI * k) / len;
  const cosw = Math.cos(w);
  const sinw = Math.sin(w);
  const coeff = 2 * cosw;
  let s0 = 0, s1 = 0, s2 = 0;
  for (let i = 0; i < len; i++) {
    s0 = samples[start + i] + coeff * s1 - s2;
    s2 = s1;
    s1 = s0;
  }
  const re = s1 - s2 * cosw;
  const im = s2 * sinw;
  const mag = Math.sqrt(re * re + im * im) / (len / 2);
  return mag;
}

function toDb(mag, ref = 1.0) {
  return 20 * Math.log10(Math.max(mag, 1e-12) / ref);
}

// Parabolic-interpolated peak frequency near `guessHz`, searching
// +/- searchHz in `steps` steps of Goertzel probes — sub-bin accuracy
// without a full FFT.
function refineFreq(samples, sampleRate, guessHz, start, len, searchHz = 3, steps = 60) {
  let best = { f: guessHz, mag: -1 };
  const mags = [];
  for (let i = -steps; i <= steps; i++) {
    const f = guessHz + (i / steps) * searchHz;
    const m = goertzelMag(samples, sampleRate, f, start, len);
    mags.push([f, m]);
    if (m > best.mag) best = { f, mag: m };
  }
  // parabolic refine around best
  const idx = mags.findIndex(([f]) => f === best.f);
  if (idx > 0 && idx < mags.length - 1) {
    const [f0, y0] = mags[idx - 1];
    const [f1, y1] = mags[idx];
    const [f2, y2] = mags[idx + 1];
    const denom = y0 - 2 * y1 + y2;
    if (Math.abs(denom) > 1e-9) {
      const delta = (0.5 * (y0 - y2)) / denom;
      best.f = f1 + delta * (f2 - f0) / 2;
    }
  }
  return best.f;
}

// Instantaneous-frequency-over-time via upward zero-crossings. Correct for
// a single dominant sinusoid whose frequency changes slowly relative to its
// own period (true here: 5 Hz LFO modulating a 440 Hz carrier) — unlike the
// Goertzel probe above, it isn't limited by a fixed analysis-window
// frequency resolution, so it can actually resolve a 40-cent (~10 Hz) swing.
function zeroCrossingFreqTrack(samples, sampleRate, start, len) {
  const track = [];
  let lastCrossing = null;
  for (let i = 1; i < len; i++) {
    const a = samples[start + i - 1];
    const b = samples[start + i];
    if (a <= 0 && b > 0) {
      // linear-interpolated sub-sample crossing time
      const frac = -a / (b - a);
      const t = (i - 1 + frac) / sampleRate;
      if (lastCrossing !== null) {
        const period = t - lastCrossing;
        track.push({ t, f: 1 / period });
      }
      lastCrossing = t;
    }
  }
  return track;
}

function rms(samples, start, len) {
  let sum = 0;
  for (let i = 0; i < len; i++) sum += samples[start + i] * samples[start + i];
  return Math.sqrt(sum / len);
}

function peak(samples, start, len) {
  let m = 0;
  for (let i = 0; i < len; i++) m = Math.max(m, Math.abs(samples[start + i]));
  return m;
}

const DIR = process.argv[2] || '.';
const results = {};

function analyzeStationaryTone(file, f0, label, { checkFreq = true } = {}) {
  const { sampleRate, samples } = readWavFloat32(file);
  const analysisStart = Math.floor(sampleRate * 0.3); // skip the very first transient
  const wanted = Math.floor(sampleRate * 1.0);
  const analysisLen = Math.min(wanted, samples.length - analysisStart);
  if (analysisLen < sampleRate * 0.2) {
    throw new Error(`${file}: only ${analysisLen} samples available after the 0.3s skip (need >=0.2s) - render a longer file`);
  }
  // Fundamental frequency via zero-crossing period averaging, NOT the
  // Goertzel/parabolic refineFreq() below — validated against a synthetic
  // 440.000000 Hz reference sine: zero-crossing reads back 440.000001 Hz
  // (0.000003 cents), while the Goertzel/parabolic method reads back
  // 439.525 Hz (-1.87 cents) on that SAME mathematically-exact input,
  // proving that approach has a systematic bug in this script, not in the
  // engine. refineFreq()/goertzelMag() are kept only for the harmonic-
  // ladder magnitude readout below, where absolute dB level (not sub-cent
  // frequency precision) is what's being read.
  //
  // Only meaningful on a waveshaper-clean tone: a heavily driven/folded/
  // crushed patch (checkFreq=false) can add zero crossings the fundamental
  // period doesn't have, so this is skipped there and the nominal f0 is
  // used directly for the harmonic-ladder readout instead.
  let trueF0 = f0;
  let centsOff = 0;
  if (checkFreq) {
    const periods = [];
    let last = null;
    for (let i = 1; i < analysisLen; i++) {
      const a = samples[analysisStart + i - 1];
      const b = samples[analysisStart + i];
      if (a <= 0 && b > 0) {
        const frac = -a / (b - a);
        const t = (i - 1 + frac) / sampleRate;
        if (last !== null) periods.push(t - last);
        last = t;
      }
    }
    // Sanity check against harmonic-induced double-counting: a very bright
    // patch's ripple can add spurious upward crossings within one true
    // fundamental period. If the crossing count doesn't match the nominal
    // f0 within 5%, don't trust this number silently.
    const expectedCrossings = f0 * (analysisLen / sampleRate);
    const crossingCountRatio = periods.length / expectedCrossings;
    if (Math.abs(crossingCountRatio - 1) > 0.05) {
      throw new Error(
        `${file}: zero-crossing count (${periods.length}) is ${(crossingCountRatio * 100).toFixed(0)}% of the ` +
        `nominal-f0-implied count (${expectedCrossings.toFixed(0)}) - likely harmonic-induced double-counting on a ` +
        `bright patch; zero-crossing method is not reliable for this file`
      );
    }
    const meanPeriod = periods.reduce((x, y) => x + y, 0) / periods.length;
    trueF0 = 1 / meanPeriod;
    centsOff = 1200 * Math.log2(trueF0 / f0);
  }

  // Harmonic ladder up to Nyquist
  const nyq = sampleRate / 2;
  const harmonics = [];
  for (let k = 1; k * trueF0 < nyq - 50 && k <= 40; k++) {
    const m = goertzelMag(samples, sampleRate, k * trueF0, analysisStart, analysisLen);
    harmonics.push({ k, hz: k * trueF0, db: toDb(m) });
  }
  const fundamentalDb = harmonics[0].db;

  // Energy just below Nyquist (aliasing probe band): 0.44*fs is the exact
  // band the plugin's own in-editor meter uses (see AUDIT / monograph
  // sec. 4.4) — reuse it here so the number is directly comparable.
  const aliasProbeHz = sampleRate * 0.44;
  const aliasMag = goertzelMag(samples, sampleRate, aliasProbeHz, analysisStart, analysisLen);
  const aliasDbRelFund = toDb(aliasMag) - fundamentalDb;

  const pk = peak(samples, 0, samples.length);
  const rmsVal = rms(samples, analysisStart, analysisLen);

  return {
    label,
    file,
    sampleRate,
    requestedF0: f0,
    measuredF0: trueF0,
    centsOff,
    fundamentalDb,
    harmonics,
    aliasProbeHz,
    aliasDbRelFund,
    peakAmplitude: pk,
    rms: rmsVal,
    crestFactorDb: toDb(pk) - toDb(rmsVal),
  };
}

function fmtHarmonics(h, maxRows = 8) {
  return h.slice(0, maxRows).map(x => `k=${x.k} ${x.hz.toFixed(1)}Hz ${x.db.toFixed(1)}dB`).join('  |  ');
}

console.log('=== 1) Darkest tone (rolloff=0.02, 440 Hz target) ===');
{
  const r = analyzeStationaryTone(`${DIR}/qa_01_dark_sine_440hz.wav`, 440.0, 'dark');
  console.log(`measured f0 = ${r.measuredF0.toFixed(4)} Hz (requested 440 Hz, ${r.centsOff.toFixed(3)} cents off)`);
  console.log(`harmonic ladder: ${fmtHarmonics(r.harmonics)}`);
  const h2 = r.harmonics[1] ? r.harmonics[1].db - r.harmonics[0].db : null;
  console.log(`2nd harmonic relative to fundamental: ${h2 === null ? 'n/a' : h2.toFixed(1) + ' dB'}`);
  console.log(`peak amplitude: ${r.peakAmplitude.toFixed(4)} (full scale = 1.0)`);
  results.dark = r;
}

console.log('\n=== 2) Brightest tone (rolloff=0.965, 220 Hz target) ===');
{
  const r = analyzeStationaryTone(`${DIR}/qa_02_bright_220hz.wav`, 220.0, 'bright');
  console.log(`measured f0 = ${r.measuredF0.toFixed(4)} Hz (requested 220 Hz, ${r.centsOff.toFixed(3)} cents off)`);
  console.log(`harmonic ladder (first 8 of ${r.harmonics.length} below Nyquist): ${fmtHarmonics(r.harmonics)}`);
  console.log(`alias probe @ ${r.aliasProbeHz.toFixed(0)} Hz: ${r.aliasDbRelFund.toFixed(1)} dB relative to fundamental`);
  results.bright = r;
}

console.log('\n=== 3) Aliasing floor: dirty patch, HQ off vs HQ on ===');
{
  const off = analyzeStationaryTone(`${DIR}/qa_03_dirty_hq_off.wav`, 110.0, 'dirty-hq-off', { checkFreq: false });
  const on = analyzeStationaryTone(`${DIR}/qa_03_dirty_hq_on.wav`, 110.0, 'dirty-hq-on', { checkFreq: false });
  console.log(`HQ OFF: alias probe @ ${off.aliasProbeHz.toFixed(0)}Hz = ${off.aliasDbRelFund.toFixed(1)} dB rel. fundamental`);
  console.log(`HQ ON:  alias probe @ ${on.aliasProbeHz.toFixed(0)}Hz = ${on.aliasDbRelFund.toFixed(1)} dB rel. fundamental`);
  console.log(`HQ improvement: ${(off.aliasDbRelFund - on.aliasDbRelFund).toFixed(1)} dB cleaner with HQ on`);
  results.aliasing = { off, on };
}

console.log('\n=== 4) 12-TET frequency accuracy ===');
{
  const midiToHz = (n) => 440 * Math.pow(2, (n - 69) / 12);
  const notes = [45, 57, 60, 69, 72, 81, 93];
  const rows = [];
  for (const note of notes) {
    const expected = midiToHz(note);
    const r = analyzeStationaryTone(`${DIR}/qa_04_tet12_note${note}.wav`, expected, `note${note}`);
    rows.push({ note, expected, measured: r.measuredF0, centsOff: r.centsOff });
    console.log(`MIDI ${note}: expected ${expected.toFixed(4)} Hz, measured ${r.measuredF0.toFixed(4)} Hz, ${r.centsOff.toFixed(4)} cents off`);
  }
  results.tet12 = rows;
}

console.log('\n=== 5) Just-intonation frequency accuracy (5-limit major triad on 220 Hz) ===');
{
  const expectedRatios = [1, 5 / 4, 3 / 2];
  const rows = [];
  for (let i = 0; i < 3; i++) {
    const expected = 220 * expectedRatios[i];
    const r = analyzeStationaryTone(`${DIR}/qa_05_just_degree${i}.wav`, expected, `degree${i}`);
    rows.push({ degree: i, ratio: expectedRatios[i], expected, measured: r.measuredF0, centsOff: r.centsOff });
    console.log(`degree ${i} (ratio ${expectedRatios[i]}): expected ${expected.toFixed(4)} Hz, measured ${r.measuredF0.toFixed(4)} Hz, ${r.centsOff.toFixed(4)} cents off`);
  }
  // Beating check: with true JI ratios the three tones' harmonic
  // coincidences should overlap exactly; report the max cents error as
  // the beat-relevant number (>1 cent starts to be audible as slow beating
  // over several seconds on a sustained chord).
  const maxCents = Math.max(...rows.map(r => Math.abs(r.centsOff)));
  console.log(`max absolute cents error across the triad: ${maxCents.toFixed(4)} cents`);
  results.justIntonation = rows;
}

console.log('\n=== 6) Envelope timing (A=100ms D=150ms S=50% R=300ms) ===');
{
  const { sampleRate, samples } = readWavFloat32(`${DIR}/qa_06_envelope_a100_d150_s50_r300.wav`);
  // Envelope via per-half-cycle peak-picking (local |sample| maxima), not
  // windowed RMS: RMS over a fixed window lags a fast-moving envelope by
  // roughly half the window length, which a first pass of this same
  // measurement (5ms window) showed as a spurious "attack finishes 5ms
  // early" — peak-picking has no such window-smoothing bias, only the
  // carrier's own half-cycle resolution (~1.1ms at 440Hz).
  const env = [];
  for (let i = 1; i < samples.length - 1; i++) {
    const prev = Math.abs(samples[i - 1]);
    const cur = Math.abs(samples[i]);
    const next = Math.abs(samples[i + 1]);
    if (cur >= prev && cur >= next) env.push({ t: i / sampleRate, level: cur });
  }
  const meanLevel = (lo, hi) => {
    const w = env.filter(e => e.t >= lo && e.t < hi);
    return w.length ? w.reduce((a, e) => a + e.level, 0) / w.length : NaN;
  };
  const maxLevel = (lo, hi) => Math.max(...env.filter(e => e.t >= lo && e.t < hi).map(e => e.level));

  // Ceiling for *timing* gates (attack/decay/release crossing a % of it) —
  // a global max, same as before this fix: these care "has it gotten
  // close enough yet", where a crest-factor spike being the ceiling is
  // harmless (it only makes the gate slightly stricter).
  const peakCeiling = Math.max(...env.map(e => e.level));
  // Mean over a short window, for the *level ratio* actually reported
  // below — comparing this to sustainLevel's own windowed mean is
  // apples-to-apples. The original bug: sustainLevel (a windowed mean)
  // was divided by peakCeiling (a global max) — two different statistics
  // of a non-sinusoidal, multi-harmonic tone, whose half-cycle peaks vary
  // a lot in height — which read as a level mismatch that isn't real.
  const peakLevel = meanLevel(0.085, 0.099); // just before decay starts (attack = 100ms)
  const sustainLevel = meanLevel(0.5, 0.79);

  const attackHit = env.find(e => e.level >= 0.9 * peakCeiling);
  const decayHit = env.find(e => e.t > (attackHit?.t ?? 0) && Math.abs(e.level - sustainLevel) <= 0.05 * peakCeiling);
  // Windowed max, not a single arbitrary last-sample-before-0.8s (which on
  // a multi-harmonic tone can land on a small crest-factor wiggle) — same
  // "ceiling for a timing gate" role as peakCeiling above.
  const releaseStartLevel = maxLevel(0.785, 0.8);
  const releaseTail = env.filter(e => e.t >= 0.8);
  const releaseHit = releaseTail.find(e => e.level <= 0.1 * releaseStartLevel);
  const release80dbHit = releaseTail.find(e => toDb(e.level) - toDb(releaseStartLevel) <= -80);
  // The local-peak series runs out once the carrier's own ripple drops
  // into the engine's Idle-threshold snap-to-silence (env.rs's ~1e-4 /
  // -80dB cutoff) — reporting the deepest point actually reached (not
  // just a binary "did it cross -80") tells the difference between
  // "genuinely stalled early" and "got within a rounding error of -80dB
  // before the engine silenced the voice", which a bare not-reached can't.
  const deepestDb = releaseTail.reduce((min, e) => Math.min(min, toDb(e.level) - toDb(releaseStartLevel)), 0);

  console.log(`peak level (post-attack, 0.085-0.099s window avg): ${peakLevel.toFixed(4)}`);
  console.log(`measured sustain level (0.5-0.79s window avg): ${sustainLevel.toFixed(4)} (${(sustainLevel / peakLevel * 100).toFixed(1)}% of peak; nominal sustain = 50%)`);
  console.log(`attack: reaches 90% of peak at t=${attackHit ? attackHit.t.toFixed(4) : 'n/a'}s (nominal 0.100s, linear ramp so 90% is expected at 0.090s)`);
  console.log(`decay:  settles within 5% of sustain at t=${decayHit ? decayHit.t.toFixed(4) : 'n/a'}s (nominal attack+decay = 0.250s)`);
  const release80dbAt = release80dbHit ? `${(release80dbHit.t - 0.8).toFixed(4)}s` : 'not strictly crossed';
  console.log(`release: -20dB (10% amplitude) at t=${releaseHit ? (releaseHit.t - 0.8).toFixed(4) : 'n/a'}s after note-off; -80dB at t=${release80dbAt}, deepest point measured: ${deepestDb.toFixed(2)}dB at t=${(releaseTail[releaseTail.length - 1]?.t - 0.8).toFixed(4)}s before the local-peak series runs out (nominal release param = time-to-~-80dB per the engine's own exp2-based coefficient, eq. c_r = 1-2^(-13.3/(r*fs)) -> designed to reach -80dB at t=r=0.300s)`);
  results.envelope = { peakLevel, sustainLevel, attackHit, decayHit, releaseHit, release80dbHit };
}

console.log('\n=== 7) Filter response (LP, resonance=0.1, 55Hz fundamental, bright) ===');
{
  for (const cutoff of [500, 2000]) {
    const { sampleRate, samples } = readWavFloat32(`${DIR}/qa_07_filter_lp_${cutoff}hz.wav`);
    const start = Math.floor(sampleRate * 0.3);
    const len = Math.floor(sampleRate * 1.0);
    // Sample the harmonic ladder of the 55 Hz fundamental and find where
    // it crosses -3dB relative to the low-frequency (passband) harmonics.
    const passbandDb = toDb(goertzelMag(samples, sampleRate, 55, start, len));
    const rows = [];
    for (let k = 1; k * 55 < sampleRate / 2 - 50 && k <= 60; k++) {
      const hz = k * 55;
      const db = toDb(goertzelMag(samples, sampleRate, hz, start, len)) - passbandDb;
      rows.push({ hz, db });
    }
    // First harmonic frequency at which level has dropped by >= 3dB
    // relative to the passband reference, and stays down (avoid a single
    // ripple false-triggering near resonance).
    let cross3db = null;
    for (let i = 0; i < rows.length; i++) {
      if (rows[i].db <= -3 && rows.slice(i, i + 3).every(x => x.db <= -2)) { cross3db = rows[i].hz; break; }
    }
    console.log(`cutoff param = ${cutoff} Hz -> measured -3dB point ≈ ${cross3db ?? 'not reached below Nyquist'} Hz (oscillator-driven — conflates the source's own spectral tilt with the filter, see §7b below)`);
  }
}

console.log('\n=== 7b) Filter response, isolated (impulse in, no oscillator) ===');
{
  for (const cutoff of [500, 2000]) {
    const { sampleRate, samples } = readWavFloat32(`${DIR}/qa_isolated_filter_lp_${cutoff}hz.wav`);
    const dcGain = toDb(goertzelMag(samples, sampleRate, 1, 0, samples.length));
    const rows = [];
    for (let hz = 20; hz < sampleRate / 2 - 50; hz *= 1.05) {
      const db = toDb(goertzelMag(samples, sampleRate, hz, 0, samples.length)) - dcGain;
      rows.push({ hz, db });
    }
    let cross3db = null;
    for (let i = 0; i < rows.length; i++) {
      if (rows[i].db <= -3 && rows.slice(i, i + 3).every(x => x.db <= -2)) { cross3db = rows[i].hz; break; }
    }
    console.log(`cutoff param = ${cutoff} Hz -> true -3dB point (isolated) ≈ ${cross3db ? cross3db.toFixed(0) : 'not reached below Nyquist'} Hz`);
  }
}

console.log('\n=== 8) Stereo image (JI triad + 4x unison, spread=0.8) ===');
{
  const buf = readFileSync(`${DIR}/qa_08_chord_unison_stereo.wav`);
  // quick manual parse (stereo interleaved float32)
  let offset = 12, fmt = null, data = null;
  while (offset < buf.length) {
    const id = buf.toString('ascii', offset, offset + 4);
    const size = buf.readUInt32LE(offset + 4);
    const body = offset + 8;
    if (id === 'fmt ') fmt = { channels: buf.readUInt16LE(body + 2), sampleRate: buf.readUInt32LE(body + 4) };
    if (id === 'data') data = buf.subarray(body, body + size);
    offset = body + size + (size % 2);
  }
  const n = data.length / 4 / fmt.channels;
  const L = new Float32Array(n), R = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    L[i] = data.readFloatLE((i * 2) * 4);
    R[i] = data.readFloatLE((i * 2 + 1) * 4);
  }
  const start = Math.floor(fmt.sampleRate * 0.5);
  const len = Math.floor(fmt.sampleRate * 1.0);
  const rmsL = rms(L, start, len), rmsR = rms(R, start, len);
  // Correlation coefficient between L and R over the analysis window —
  // 1.0 = mono-identical, lower = genuine stereo decorrelation.
  let sumL = 0, sumR = 0, sumLR = 0, sumL2 = 0, sumR2 = 0;
  for (let i = 0; i < len; i++) {
    const l = L[start + i], r = R[start + i];
    sumL += l; sumR += r; sumLR += l * r; sumL2 += l * l; sumR2 += r * r;
  }
  const cov = sumLR / len - (sumL / len) * (sumR / len);
  const varL = sumL2 / len - (sumL / len) ** 2;
  const varR = sumR2 / len - (sumR / len) ** 2;
  const corr = cov / Math.sqrt(varL * varR);
  console.log(`RMS L=${rmsL.toFixed(4)} R=${rmsR.toFixed(4)} (balance ${(toDb(rmsL) - toDb(rmsR)).toFixed(2)} dB)`);
  console.log(`L/R correlation coefficient: ${corr.toFixed(4)} (1.0 = identical/mono, lower = wider stereo image)`);
}

console.log('\n=== 9) Vibrato (5 Hz rate, 40 cents depth, 440 Hz carrier) ===');
{
  const { sampleRate, samples } = readWavFloat32(`${DIR}/qa_09_vibrato_5hz_40c.wav`);
  const start0 = Math.floor(sampleRate * 0.5);
  const winLen = Math.floor(sampleRate * 0.6); // 3 full 5Hz cycles
  const track = zeroCrossingFreqTrack(samples, sampleRate, start0, winLen);
  const fs_ = track.map(t => t.f).filter(f => Number.isFinite(f) && f > 300 && f < 600); // reject glitch crossings
  const fmin = Math.min(...fs_);
  const fmax = Math.max(...fs_);
  const centsSwing = 1200 * Math.log2(fmax / fmin);
  // Also recover the modulation rate itself: time between successive minima.
  console.log(`zero-crossing periods measured: ${track.length}`);
  console.log(`measured instantaneous-f range: ${fmin.toFixed(2)}-${fmax.toFixed(2)} Hz (${centsSwing.toFixed(1)} cents peak-to-peak; nominal depth is ±40 cents = 80 cents peak-to-peak; nominal center 440 Hz)`);
}

console.log('\n(Analyzed files: ' + readdirSync(DIR).filter(f => f.startsWith('qa_')).length + ' qa_*.wav in ' + DIR + ')');
