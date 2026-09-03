#!/usr/bin/env pwsh
# Build the VST3 + CLAP bundles and validate them with Tracktion pluginval.
#
#   pwsh scripts/validate.ps1 [-Strictness 8] [-Pluginval <path-to-pluginval.exe>]
#
# pluginval exercises the integration points we cannot unit-test:
#   * parameter state save + restore (state recall)
#   * host changing the audio block size on the fly
#   * host changing the sample rate on the fly
#   * no heap allocations during processing (with --validate-in-process)
#   * parameter automation, threading, bus layouts, fuzzing
#
# Get pluginval: https://github.com/Tracktion/pluginval/releases
# (or drop pluginval.exe in  <repo>/harmonic_synth/tools/ , or pass -Pluginval)

[CmdletBinding()]
param(
    [int]$Strictness = 8,
    [string]$Pluginval = ""
)

$ErrorActionPreference = "Stop"
$synthDir = Split-Path $PSScriptRoot -Parent

# --- locate pluginval ---
if (-not $Pluginval) {
    $cmd = Get-Command pluginval -ErrorAction SilentlyContinue
    if ($cmd) {
        $Pluginval = $cmd.Source
    } elseif (Test-Path "$synthDir/tools/pluginval.exe") {
        $Pluginval = "$synthDir/tools/pluginval.exe"
    }
}
if (-not $Pluginval -or -not (Test-Path $Pluginval)) {
    throw "pluginval not found. Install from https://github.com/Tracktion/pluginval/releases, add it to PATH, put it in $synthDir/tools/, or pass -Pluginval <path>."
}

# --- build bundles ---
Push-Location $synthDir
try {
    cargo xtask bundle harmonic_synth --release
    if ($LASTEXITCODE -ne 0) { throw "bundle build failed" }
} finally {
    Pop-Location
}

$bundled = Join-Path $synthDir "target/bundled"
$targets = @(
    (Join-Path $bundled "harmonic_synth.vst3"),
    (Join-Path $bundled "harmonic_synth.clap")
)

# --- run pluginval ---
$failed = $false
foreach ($t in $targets) {
    if (-not (Test-Path $t)) {
        Write-Warning "missing bundle: $t"
        $failed = $true
        continue
    }
    Write-Host "`n=== pluginval (strictness $Strictness): $(Split-Path $t -Leaf) ===" -ForegroundColor Cyan
    & $Pluginval `
        --strictness-level $Strictness `
        --validate-in-process `
        --skip-gui-tests `
        --timeout-ms 120000 `
        --validate $t
    if ($LASTEXITCODE -ne 0) { $failed = $true }
}

if ($failed) {
    throw "pluginval reported failures"
}
Write-Host "`nAll validations passed." -ForegroundColor Green
