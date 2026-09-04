#!/usr/bin/env bash
# Cross-platform bit-exactness verification (RFC-15).
#
# Runs the full `harmonic_core` test suite — including the
# `cross_platform_bit_exact` integration test, which compares the rendered
# signal bit-for-bit against a reference hash produced on x86_64 — on emulated
# ARM Linux targets via Docker + QEMU.
#
#   aarch64-unknown-linux-gnu        — 64-bit ARM (mobile / embedded Linux)
#   armv7-unknown-linux-gnueabihf    — 32-bit ARM hard-float; the VFP `f64`
#                                      instruction set is identical to
#                                      thumbv7em-none-eabihf (Cortex-M4F), so a
#                                      pass here transfers to the bare-metal
#                                      target the firmware build uses.
#
# Requires: Docker with binfmt/QEMU emulation. One-time host setup, if the ARM
# platforms are not yet registered:
#
#   docker run --rm --privileged tonistiigi/binfmt --install arm64,arm
#
# `cross` is deliberately not used: on a Windows host it tries to install a
# Linux rustup toolchain and fails. A plain emulated container needs none.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_IMAGE="${RUST_IMAGE:-rust:1.97-slim}"

run_target() {
    local platform="$1" label="$2"
    echo "=============================================================="
    echo "  $label  ($platform)"
    echo "=============================================================="
    MSYS_NO_PATHCONV=1 docker run --rm --platform "$platform" \
        -v "${CRATE_DIR}:/src:ro" -w /src \
        -e CARGO_TARGET_DIR=/tmp/target \
        "$RUST_IMAGE" \
        bash -c 'rustc -vV | grep host && cargo test --release'
}

run_target linux/arm64   "AArch64 (aarch64-unknown-linux-gnu)"
run_target linux/arm/v7  "ARMv7 hard-float (armv7-unknown-linux-gnueabihf)"

echo
echo "OK — all targets bit-identical to the x86_64 reference."
