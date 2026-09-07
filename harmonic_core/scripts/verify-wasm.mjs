// wasm32 bit-exactness cross-check.
//
// Runs `harmonic_core::verify::render_verification` inside the `wasm32-unknown-
// unknown` build and checks its output hashes to exactly the same value as the
// x86-64 / AArch64 / ARMv7-hf reference (`harmonic_core::verify::VERIFY_HASH`).
// Two independent hashes must agree: the one the wasm module computes itself,
// and one this script folds from the raw sample bytes in JS.
//
//   cargo build --no-default-features --release --target wasm32-unknown-unknown
//   node scripts/verify-wasm.mjs
//
// Exit 0 = bit-identical; exit 1 = a wasm f64 op diverged from IEEE-754.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = join(
  here, '..', 'target', 'wasm32-unknown-unknown', 'release', 'harmonic_core.wasm',
);

// Keep in sync with harmonic_core::verify::VERIFY_HASH / VERIFY_2_HASH.
const REFERENCE = 0x272cf9c7ecbaf653n;
const REFERENCE_2 = 0x25660025905bedc4n;

const MASK64 = (1n << 64n) - 1n;
const FNV_PRIME = 0x100000001b3n;
const FNV_OFFSET = 0xcbf29ce484222325n;

function fnv1a(bytes) {
  let h = FNV_OFFSET;
  for (const b of bytes) {
    h = (h ^ BigInt(b)) & MASK64;
    h = (h * FNV_PRIME) & MASK64;
  }
  return h;
}

const hex = (n) => `0x${n.toString(16).padStart(16, '0')}`;

const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const ex = instance.exports;

const ptr = ex.hc_verify_render();
const len = ex.hc_verify_len();
const samples = new Float32Array(ex.memory.buffer, ptr, len);

const jsHash = fnv1a(new Uint8Array(samples.buffer, samples.byteOffset, len * 4));
const wasmHash =
  (BigInt(ex.hc_verify_hash_hi() >>> 0) << 32n) | BigInt(ex.hc_verify_hash_lo() >>> 0);

console.log(`frames rendered : ${len / 2}`);
console.log(`wasm self-hash  : ${hex(wasmHash)}`);
console.log(`js byte-hash    : ${hex(jsHash)}`);
console.log(`reference       : ${hex(REFERENCE)}`);

if (wasmHash !== REFERENCE || jsHash !== REFERENCE) {
  console.error('\nMISMATCH — the wasm32 render is NOT bit-identical to the reference.');
  process.exit(1);
}
console.log('OK — wasm32 render is bit-identical to the x86-64 / ARM reference.\n');

// Second scripted pass — HQ bus, Saw/Triangle, fractional partials, Formant.
const ptr2 = ex.hc_verify_render_2();
const len2 = ex.hc_verify_len();
const samples2 = new Float32Array(ex.memory.buffer, ptr2, len2);
const jsHash2 = fnv1a(new Uint8Array(samples2.buffer, samples2.byteOffset, len2 * 4));
const wasmHash2 =
  (BigInt(ex.hc_verify_hash_hi() >>> 0) << 32n) | BigInt(ex.hc_verify_hash_lo() >>> 0);

console.log(`pass 2 wasm self-hash : ${hex(wasmHash2)}`);
console.log(`pass 2 js byte-hash   : ${hex(jsHash2)}`);
console.log(`pass 2 reference      : ${hex(REFERENCE_2)}`);

if (wasmHash2 !== REFERENCE_2 || jsHash2 !== REFERENCE_2) {
  console.error('\nMISMATCH — the wasm32 pass-2 render is NOT bit-identical to the reference.');
  process.exit(1);
}
console.log('OK — wasm32 pass-2 render is bit-identical to the x86-64 / ARM reference.\n');

// ---------------------------------------------------------------------------
// The browser-integration path: drive one `Voice` through the C ABI exactly as
// contrib/wasm-demo's AudioWorklet does, and check it makes bounded sound.
// ---------------------------------------------------------------------------
const SR = 48_000;
const v = ex.harmonic_wasm_voice();
ex.harmonic_voice_init(v, SR);
ex.harmonic_voice_set_rolloff(v, 0.9);
ex.harmonic_voice_set_gain(v, 0.8);
ex.harmonic_voice_set_filter(v, 1, 4_000, 0.3); // low-pass
ex.harmonic_voice_set_frequency(v, 220);
ex.harmonic_voice_reset(v);

const scratch = ex.harmonic_wasm_scratch();
const QUANTUM = 128;
let peak = 0;
let finite = true;
let energy = 0;
for (let block = 0; block < SR / QUANTUM; block++) { // ~1 s
  ex.harmonic_voice_process(v, scratch, QUANTUM);
  const buf = new Float32Array(ex.memory.buffer, scratch, QUANTUM * 2);
  for (const s of buf) {
    finite &&= Number.isFinite(s);
    peak = Math.max(peak, Math.abs(s));
    energy += s * s;
  }
}
const rms = Math.sqrt(energy / (SR * 2));
console.log(`Voice C ABI : peak ${peak.toFixed(3)}  rms ${rms.toFixed(4)}  finite ${finite}`);

if (!finite || peak <= 0.02 || peak > 1.5 || rms < 2e-3) {
  console.error('\nFAIL — the wasm Voice did not produce clean bounded audio.');
  process.exit(1);
}
console.log('\nOK — the wasm Voice renders clean bounded audio through the C ABI.');
