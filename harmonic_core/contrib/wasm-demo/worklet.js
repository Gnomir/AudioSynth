// AudioWorkletProcessor that runs one harmonic_core `Voice` through the C ABI.
//
// The engine is `no_std` + zero-dependency. The main thread fetches
// `harmonic_core.wasm` (built for `wasm32-unknown-unknown`) and hands the bytes
// in via `processorOptions` — an AudioWorkletGlobalScope has no `fetch`. This
// processor instantiates it, takes the module's static voice slot and scratch
// buffer (`harmonic_wasm_*` — a no_std cdylib has no allocator), and calls
// `harmonic_voice_process` once per render quantum. The attack/release gate
// lives here in JS, because the core `Voice` has no amplitude envelope — that is
// the host's job, exactly as it is on a Daisy Seed.

class HarmonicVoiceProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.ready = false;
    this.gateTarget = 0; // 0..1, set by note on/off messages
    this.gate = 0;
    this.attackCoeff = 0;
    this.releaseCoeff = 0;

    this.port.onmessage = (e) => this._onMessage(e.data);

    const bytes = options?.processorOptions?.wasm;
    WebAssembly.instantiate(bytes, {})
      .then((r) => this._boot(r))
      .catch((err) => this.port.postMessage({ type: 'error', message: String(err) }));
  }

  _boot({ instance }) {
    const ex = instance.exports;
    this.ex = ex;
    this.voice = ex.harmonic_wasm_voice();
    this.scratch = ex.harmonic_wasm_scratch();
    ex.harmonic_voice_init(this.voice, sampleRate);
    ex.harmonic_voice_set_gain(this.voice, 0.85);

    // ~4 ms attack, ~120 ms release on the JS gate
    this.attackCoeff = 1 - Math.exp(-1 / (0.004 * sampleRate));
    this.releaseCoeff = 1 - Math.exp(-1 / (0.12 * sampleRate));

    this.ready = true;
    this.port.postMessage({ type: 'ready' });
  }

  _onMessage(m) {
    if (!this.ready) return;
    const ex = this.ex;
    const v = this.voice;
    switch (m.type) {
      case 'noteOn':
        ex.harmonic_voice_set_frequency(v, m.hz);
        if (this.gateTarget === 0) ex.harmonic_voice_reset(v);
        this.gateTarget = 1;
        break;
      case 'noteOff':
        this.gateTarget = 0;
        break;
      case 'param':
        this._param(m.name, m.value);
        break;
    }
  }

  _param(name, value) {
    const ex = this.ex;
    const v = this.voice;
    switch (name) {
      case 'brightness': ex.harmonic_voice_set_rolloff(v, value); break;
      case 'partials':   ex.harmonic_voice_set_partial_limit(v, value); break;
      case 'formant':    ex.harmonic_voice_set_formant(v, value); break;
      case 'waveform':   ex.harmonic_voice_set_waveform(v, value | 0); break;
      case 'hq':         ex.harmonic_voice_set_hq(v, value ? 1 : 0); break;
      case 'filterMode': this._filterMode = value | 0; this._pushFilter(); break;
      case 'cutoff':     this._cutoff = value; this._pushFilter(); break;
      case 'resonance':  this._resonance = value; this._pushFilter(); break;
      case 'lfoRate':    this._lfoRate = value; this._pushLfo(); break;
      case 'lfoVibrato': this._lfoVibrato = value; this._pushLfo(); break;
      case 'lfoCutoff':  this._lfoCutoff = value; this._pushLfo(); break;
    }
  }

  _pushFilter() {
    this.ex.harmonic_voice_set_filter(
      this.voice,
      this._filterMode ?? 1,
      this._cutoff ?? 6000,
      this._resonance ?? 0.2,
    );
  }

  _pushLfo() {
    this.ex.harmonic_voice_set_lfo(
      this.voice,
      this._lfoRate ?? 5,
      0, // sine
      1, // free-run
      0,
      this._lfoVibrato ?? 0,
      this._lfoCutoff ?? 0,
      0,
    );
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    const frames = out[0].length;
    if (!this.ready) return true;

    this.ex.harmonic_voice_process(this.voice, this.scratch, frames);
    const buf = new Float32Array(this.ex.memory.buffer, this.scratch, frames * 2);

    let g = this.gate;
    const coeff = this.gateTarget > g ? this.attackCoeff : this.releaseCoeff;
    const L = out[0];
    const R = out[1] ?? out[0];
    for (let i = 0; i < frames; i++) {
      g += coeff * (this.gateTarget - g);
      L[i] = buf[i * 2] * g;
      R[i] = buf[i * 2 + 1] * g;
    }
    this.gate = g;
    return true;
  }
}

registerProcessor('harmonic-voice', HarmonicVoiceProcessor);
