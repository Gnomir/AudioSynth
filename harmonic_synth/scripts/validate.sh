#!/usr/bin/env bash
# Build the VST3 + CLAP bundles and validate them with Tracktion pluginval.
#
#   bash scripts/validate.sh [--strictness N] [--pluginval /path/to/pluginval]
#
# pluginval exercises the integration points we cannot unit-test:
#   * parameter state save + restore (state recall)
#   * host changing the audio block size on the fly
#   * host changing the sample rate on the fly
#   * no heap allocations during processing (with --validate-in-process)
#   * parameter automation, threading, bus layouts, fuzzing
#
# Get pluginval: https://github.com/Tracktion/pluginval/releases
# (or drop the binary in  <repo>/harmonic_synth/tools/ , or pass --pluginval)
set -euo pipefail

strictness=8
pluginval=""
while [ $# -gt 0 ]; do
    case "$1" in
        --strictness) strictness="$2"; shift 2 ;;
        --pluginval)  pluginval="$2";  shift 2 ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

synth_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# --- locate pluginval ---
if [ -z "$pluginval" ]; then
    if command -v pluginval >/dev/null 2>&1; then
        pluginval="$(command -v pluginval)"
    elif [ -x "$synth_dir/tools/pluginval" ]; then
        pluginval="$synth_dir/tools/pluginval"
    fi
fi
if [ -z "$pluginval" ] || [ ! -x "$pluginval" ]; then
    echo "pluginval not found. Install from https://github.com/Tracktion/pluginval/releases," >&2
    echo "add it to PATH, put it in $synth_dir/tools/, or pass --pluginval <path>." >&2
    exit 1
fi

# --- build bundles ---
( cd "$synth_dir" && cargo xtask bundle harmonic_synth --release )

bundled="$synth_dir/target/bundled"
targets=("$bundled/harmonic_synth.vst3" "$bundled/harmonic_synth.clap")

# --- run pluginval ---
failed=0
for t in "${targets[@]}"; do
    if [ ! -e "$t" ]; then
        echo "missing bundle: $t" >&2
        failed=1
        continue
    fi
    echo
    echo "=== pluginval (strictness $strictness): $(basename "$t") ==="
    "$pluginval" \
        --strictness-level "$strictness" \
        --validate-in-process \
        --skip-gui-tests \
        --timeout-ms 120000 \
        --validate "$t" || failed=1
done

if [ "$failed" -ne 0 ]; then
    echo "pluginval reported failures" >&2
    exit 1
fi
echo
echo "All validations passed."
