#!/usr/bin/env bash
# Build the bundles and validate them:
#   * .vst3  -> Tracktion  pluginval   (strictness 8)
#   * .clap  -> free-audio  clap-validator
#
#   bash scripts/validate.sh [--strictness N]
#
# Both cover parameter state save + restore, host block-size / sample-rate
# changes on the fly, no allocations during processing, automation, fuzzing.
#
# Tools: PATH first, then <repo>/harmonic_synth/tools/ (gitignored).
#   pluginval:      https://github.com/Tracktion/pluginval/releases
#   clap-validator: https://github.com/free-audio/clap-validator/releases
set -euo pipefail

strictness=8
while [ $# -gt 0 ]; do
    case "$1" in
        --strictness) strictness="$2"; shift 2 ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

synth_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tools="$synth_dir/tools"

find_tool() {  # name
    if command -v "$1" >/dev/null 2>&1; then command -v "$1"; return; fi
    if [ -x "$tools/$1" ]; then echo "$tools/$1"; return; fi
    echo "$1 not found. Install it or drop the binary in $tools/" >&2
    exit 1
}
pluginval="$(find_tool pluginval)"
clapval="$(find_tool clap-validator)"

( cd "$synth_dir" && cargo xtask bundle harmonic_synth --release )
bundled="$synth_dir/target/bundled"
failed=0

echo; echo "=== pluginval (strictness $strictness): harmonic_synth.vst3 ==="
"$pluginval" --strictness-level "$strictness" --validate-in-process --skip-gui-tests \
             --timeout-ms 120000 --validate "$bundled/harmonic_synth.vst3" || failed=1

# state-{reproducibility,invalid-random} excluded — nih-plug's CLAP state
# wrapper (rev de421011) skips the post-load host param rescan and doesn't
# bound-check the state length. VST3 state is fine (pluginval above).
echo; echo "=== clap-validator: harmonic_synth.clap ==="
"$clapval" validate --exclude '^state-(reproducibility|invalid-random)' \
           "$bundled/harmonic_synth.clap" || failed=1

if [ "$failed" -ne 0 ]; then echo "validation reported failures" >&2; exit 1; fi
echo; echo "All validations passed (excluded CLAP state tests — see note)."
