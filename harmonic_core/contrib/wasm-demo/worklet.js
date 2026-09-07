// AudioWorkletProcessor: a small polyphonic synth on top of harmonic_core.
//
// The engine is `no_std` + zero-dependency. The main thread fetches
// `harmonic_core.wasm` (built for `wasm32-unknown-unknown`) and hands the bytes
// in via `processorOptions` — an AudioWorkletGlobalScope has no `fetch`. This
// processor instantiates it and takes the module's static voice pool
// (`harmonic_wasm_voice_at` — a no_std cdylib has no allocator). Each note gets
// one `Voice` via the C ABI; **voice allocation, stealing and the per-voice
// attack/release gate all live here in JS**, because that is the host's job —
// identical in this page and in a Daisy Seed firmware.

class HarmonicSynthProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.ready = false;
    this.patch = { waveform: 0, brightness: 0.86, partials: 600, formant: 0, hq: 0 };
    this.filter = [1, 6500, 0.18]; // mode, cutoff, resonance
    this.lfo = [5, 0, 0];          // rate, vibrato cents, cutoff octaves
    this.age = 0;
    this.port.onmessage = (e) => this._onMessage(e.data);

    WebAssembly.instantiate(options?.processorOptions?.wasm, {})
      .then((r) => this._boot(r))
      .catch((err) => this.port.postMessage({ type: 'error', message: String(err) }));
  }

  _boot({ instance }) {
    const ex = instance.exports;
    this.ex = ex;
    this.scratch = ex.harmonic_wasm_scratch();
    this.pool = ex.harmonic_wasm_pool_size();
    this.voices = [];
    for (let i = 0; i < this.pool; i++) {
      const ptr = ex.harmonic_wasm_voice_at(i);
      ex.harmonic_voice_init(ptr, sampleRate);
      ex.harmonic_voice_set_gain(ptr, 0.32); // headroom for chords
      this.voices.push({ ptr, note: null, gate: 0, gateTarget: 0, age: 0 });
    }
    this.atkC = 1 - Math.exp(-1 / (0.004 * sampleRate));
    this.relC = 1 - Math.exp(-1 / (0.13 * sampleRate));
    this._applyAll();
    this.ready = true;
    this.port.postMessage({ type: 'ready', pool: this.pool });
  }

  _onMessage(m) {
    if (!this.ready) return;
    if (m.type === 'noteOn') this._noteOn(m.hz, m.note);
    else if (m.type === 'noteOff') this._noteOff(m.note);
    else if (m.type === 'allOff') this.voices.forEach((v) => { v.gateTarget = 0; v.note = null; });
    else if (m.type === 'param') { this._stashParam(m.name, m.value); this._applyAll(); }
  }

  _noteOn(hz, note) {
    // free voice → oldest releasing → oldest overall
    let v = this.voices.find((x) => x.gateTarget === 0 && x.gate < 0.001);
    if (!v) v = this.voices.filter((x) => x.gateTarget === 0).sort((a, b) => a.age - b.age)[0];
    if (!v) v = this.voices.slice().sort((a, b) => a.age - b.age)[0];
    v.note = note;
    v.age = ++this.age;
    v.gateTarget = 1;
    this.ex.harmonic_voice_set_frequency(v.ptr, hz);
    if (v.gate < 0.001) this.ex.harmonic_voice_reset(v.ptr);
  }

  _noteOff(note) {
    for (const v of this.voices) if (v.note === note && v.gateTarget === 1) { v.gateTarget = 0; v.note = null; }
  }

  _stashParam(name, value) {
    switch (name) {
      case 'brightness': case 'partials': case 'formant': this.patch[name] = value; break;
      case 'waveform': this.patch.waveform = value | 0; break;
      case 'hq': this.patch.hq = value ? 1 : 0; break;
      case 'filterMode': this.filter[0] = value | 0; break;
      case 'cutoff': this.filter[1] = value; break;
      case 'resonance': this.filter[2] = value; break;
      case 'lfoRate': this.lfo[0] = value; break;
      case 'lfoVibrato': this.lfo[1] = value; break;
      case 'lfoCutoff': this.lfo[2] = value; break;
    }
  }

  _applyAll() {
    const ex = this.ex, p = this.patch, f = this.filter, l = this.lfo;
    for (const v of this.voices) {
      ex.harmonic_voice_set_waveform(v.ptr, p.waveform);
      ex.harmonic_voice_set_rolloff(v.ptr, p.brightness);
      ex.harmonic_voice_set_partial_limit(v.ptr, p.partials);
      ex.harmonic_voice_set_formant(v.ptr, p.formant);
      ex.harmonic_voice_set_hq(v.ptr, p.hq);
      ex.harmonic_voice_set_filter(v.ptr, f[0], f[1], f[2]);
      ex.harmonic_voice_set_lfo(v.ptr, l[0], 0, 1, 0, l[1], l[2], 0);
    }
  }

  process(_inputs, outputs) {
    if (!this.ready) return true;
    const out = outputs[0];
    const L = out[0], R = out[1] ?? out[0];
    const frames = L.length;
    L.fill(0); R.fill(0);

    for (const v of this.voices) {
      if (v.gate < 1e-4 && v.gateTarget === 0) continue;
      this.ex.harmonic_voice_process(v.ptr, this.scratch, frames);
      const src = new Float32Array(this.ex.memory.buffer, this.scratch, frames * 2);
      let g = v.gate;
      const c = v.gateTarget > g ? this.atkC : this.relC;
      for (let i = 0; i < frames; i++) {
        g += c * (v.gateTarget - g);
        L[i] += src[i * 2] * g;
        R[i] += src[i * 2 + 1] * g;
      }
      v.gate = g;
    }
    return true;
  }
}

registerProcessor('harmonic-synth', HarmonicSynthProcessor);
