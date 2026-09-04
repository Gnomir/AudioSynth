#!/usr/bin/env pwsh
# Build the bundles and validate them:
#   * .vst3  -> Tracktion  pluginval   (strictness 8)
#   * .clap  -> free-audio  clap-validator
#
#   pwsh scripts/validate.ps1 [-Strictness 8]
#
# Both tools cover the integration points we cannot unit-test: parameter state
# save + restore, host changing block size / sample rate on the fly, no heap
# allocations during processing, automation, threading, fuzzing.
#
# Tools are looked for on PATH, then in  <repo>/harmonic_synth/tools/  — the
# tools/ dir is gitignored; drop the binaries there or let this script fetch
# them with -Fetch.
#
#   pluginval:      https://github.com/Tracktion/pluginval/releases
#   clap-validator: https://github.com/free-audio/clap-validator/releases

[CmdletBinding()]
param(
    [int]$Strictness = 8,
    [switch]$Fetch
)

$ErrorActionPreference = "Stop"
$synthDir = Split-Path $PSScriptRoot -Parent
$tools = Join-Path $synthDir "tools"

function Find-Tool([string]$name, [string]$url) {
    $exe = "$name.exe"
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $local = Join-Path $tools $exe
    if (Test-Path $local) { return $local }
    if ($Fetch) {
        New-Item -ItemType Directory -Force -Path $tools | Out-Null
        $zip = Join-Path $tools "$name.zip"
        Write-Host "fetching $name ..." -ForegroundColor Yellow
        Invoke-WebRequest -Uri $url -OutFile $zip
        Expand-Archive -Path $zip -DestinationPath $tools -Force
        Remove-Item $zip
        if (Test-Path $local) { return $local }
    }
    throw "$name not found. Install it, drop $exe in $tools\, or re-run with -Fetch. ($url)"
}

# --- resolve tools ---
$pvUrl = "https://github.com/Tracktion/pluginval/releases/download/v1.0.4/pluginval_Windows.zip"
$cvUrl = "https://github.com/free-audio/clap-validator/releases/download/0.4.1/clap-validator-0.4.1-127-g152b982-windows.zip"
$pluginval = Find-Tool "pluginval" $pvUrl
$clapval   = Find-Tool "clap-validator" $cvUrl

# --- build ---
Push-Location $synthDir
try {
    cargo xtask bundle harmonic_synth --release
    if ($LASTEXITCODE -ne 0) { throw "bundle build failed" }
} finally { Pop-Location }

$bundled = Join-Path $synthDir "target/bundled"
$failed = $false

# --- VST3: pluginval ---
Write-Host "`n=== pluginval (strictness $Strictness): harmonic_synth.vst3 ===" -ForegroundColor Cyan
& $pluginval --strictness-level $Strictness --validate-in-process `
             --timeout-ms 120000 --validate (Join-Path $bundled "harmonic_synth.vst3")
if ($LASTEXITCODE -ne 0) { $failed = $true }

# --- CLAP: clap-validator ---
# The CLAP wrapper's `ext_state_load` fix (bounded allocation + post-load host
# param rescan) is carried by `vendor/nih-plug` via [patch], so the full
# state-* suite runs with no `--exclude`. See docs/10_NIH_PLUG_CLAP_BUGS.md.
Write-Host "`n=== clap-validator: harmonic_synth.clap ===" -ForegroundColor Cyan
& $clapval validate (Join-Path $bundled "harmonic_synth.clap")
if ($LASTEXITCODE -ne 0) { $failed = $true }

if ($failed) { throw "validation reported failures" }
Write-Host "`nAll validations passed." -ForegroundColor Green
