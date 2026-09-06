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

// Keep in sync with harmonic_core::verify::VERIFY_HASH.
const REFERENCE = 0xc7f786d40586da75n;

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
console.log('\nOK — wasm32 render is bit-identical to the x86-64 / ARM reference.');
