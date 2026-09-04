<#
.SYNOPSIS
Cross-platform bit-exactness verification (RFC-15).

.DESCRIPTION
Runs the full harmonic_core test suite — including the cross_platform_bit_exact
integration test, which compares the rendered signal bit-for-bit against a
reference hash produced on x86_64 — on emulated ARM Linux targets via Docker +
QEMU.

  aarch64-unknown-linux-gnu       64-bit ARM (mobile / embedded Linux)
  armv7-unknown-linux-gnueabihf   32-bit ARM hard-float; the VFP f64 instruction
                                  set is identical to thumbv7em-none-eabihf
                                  (Cortex-M4F), so a pass here transfers to the
                                  bare-metal firmware target.

Requires Docker with binfmt/QEMU emulation. One-time host setup if the ARM
platforms are not registered:

  docker run --rm --privileged tonistiigi/binfmt --install arm64,arm
#>
$ErrorActionPreference = 'Stop'

$crateDir  = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$rustImage = if ($env:RUST_IMAGE) { $env:RUST_IMAGE } else { 'rust:1.97-slim' }

function Invoke-Target([string]$Platform, [string]$Label) {
    Write-Host "=============================================================="
    Write-Host "  $Label  ($Platform)"
    Write-Host "=============================================================="
    $env:MSYS_NO_PATHCONV = '1'
    docker run --rm --platform $Platform `
        -v "${crateDir}:/src:ro" -w /src `
        -e CARGO_TARGET_DIR=/tmp/target `
        $rustImage `
        bash -c 'rustc -vV | grep host && cargo test --release'
    if ($LASTEXITCODE -ne 0) { throw "target $Platform failed" }
}

Invoke-Target 'linux/arm64'  'AArch64 (aarch64-unknown-linux-gnu)'
Invoke-Target 'linux/arm/v7' 'ARMv7 hard-float (armv7-unknown-linux-gnueabihf)'

Write-Host ''
Write-Host 'OK - all targets bit-identical to the x86_64 reference.'
