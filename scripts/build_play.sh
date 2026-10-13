#!/usr/bin/env bash
# build_play.sh: build the game exe (ddplay) for playtesting and keep a copy
# of the exe and its PDB per run (L75). Use this for every play build.
#
#   scripts/build_play.sh [options] TAG [-- extra cargo args]
#
#   TAG                 run name: the exe and PDB go to <target>/runs/TAG
#   --profile P         play-fast (default), play or play-o0
#   --target-dir D      default: $CARGO_TARGET_DIR, else the profile's shared
#                       warm directory (play-fast: C:/ddda-targets/playfast,
#                       play: C:/ddda-targets/play, play-o0: C:/ddda-targets/playo0)
#   --who NAME          heavy.py slot owner (default: $BUILD_PLAY_WHO or head)
#   --no-heavy          already inside a heavy.py slot (or nothing to build)
#   --tries N           attempts (default 3); a compile error stops at once,
#                       a rustc killed by the memory guard is retried
#   --timings           write cargo's HTML timing report (<target>/cargo-timings)
#   --gate              cargo -j 16 with scripts/rustc_gate.rs as RUSTC_WRAPPER:
#                       shard compilers limited by free RAM instead of by -j,
#                       so their threads get all 16 job tokens (see the file)
#
# The environment is fixed here so that every caller hits the same cargo
# fingerprints: RUSTC_BOOTSTRAP=1 for the -Z flags, RUSTFLAGS from
# PLAY_RUSTFLAGS below, no CARGO_INCREMENTAL (the profile decides: runtime
# crates incremental, shards not). Changing RUSTFLAGS rebuilds every crate.
set -u
PLAY_RUSTFLAGS="-Zthreads=8"

profile=play-fast; td=${CARGO_TARGET_DIR:-}; who=${BUILD_PLAY_WHO:-head}
heavy=1; tries=3; timings=; gate=; tag=
while [ $# -gt 0 ]; do
  case $1 in
    --profile) profile=$2; shift 2 ;;
    --target-dir) td=$2; shift 2 ;;
    --who) who=$2; shift 2 ;;
    --no-heavy) heavy=; shift ;;
    --tries) tries=$2; shift 2 ;;
    --timings) timings=--timings; shift ;;
    --gate) gate=--gate; shift ;;
    --) shift; break ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    -*) echo "build_play: unknown option $1" >&2; exit 2 ;;
    *) tag=$1; shift ;;
  esac
done
[ -n "$tag" ] || { echo "build_play: TAG missing (see --help)" >&2; exit 2; }
if [ -z "$td" ]; then
  case $profile in
    play-fast) td=C:/ddda-targets/playfast ;;
    play) td=C:/ddda-targets/play ;;
    play-o0) td=C:/ddda-targets/playo0 ;;
    *) echo "build_play: give --target-dir for profile $profile" >&2; exit 2 ;;
  esac
fi

# the shards compile only inside a machine-wide heavy slot (docs/tasks/AGENT-RULES.md)
if [ -n "$heavy" ] && [ -z "${BUILD_PLAY_IN_HEAVY:-}" ]; then
  py=E:/ddda-re/tools/pyenv/Scripts/python.exe
  [ -x "$py" ] || py=python
  BUILD_PLAY_IN_HEAVY=1 HEAVY_BIG=1 exec "$py" E:/ddda-re/tools/heavy.py "$who" -- \
    "$(cygpath -m "$BASH" 2>/dev/null || echo bash)" "$0" --profile "$profile" --target-dir "$td" --tries "$tries" $timings $gate "$tag" -- "$@"
fi

repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo" || exit 2
# CMake for the emu feature's unicorn build
export PATH="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin:$PATH"
export CARGO_TARGET_DIR=$td RUSTC_BOOTSTRAP=1 RUSTFLAGS=$PLAY_RUSTFLAGS
unset CARGO_INCREMENTAL CARGO_ENCODED_RUSTFLAGS
jobs=()
if [ -n "$gate" ]; then
  # the wrapper is not fingerprinted: building or changing it rebuilds nothing
  gexe=C:/ddda-targets/tools/rustc_gate.exe
  if [ ! -x "$gexe" ] || [ scripts/rustc_gate.rs -nt "$gexe" ]; then
    mkdir -p "$(dirname "$gexe")" && rustc -O --edition=2021 scripts/rustc_gate.rs -o "$gexe" || exit 2
  fi
  free_mb=$(powershell -NoProfile -Command "[int]((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1KB)")
  # a shard rustc peaks at ~2 GB (opt-level 0) to ~3 GB (opt-level 1, split modules);
  # the gate also holds a shard back while less than DDDA_GATE_FREE_MB is free
  slots=$(( (free_mb - 3072) / 2560 )); [ $slots -lt 1 ] && slots=1; [ $slots -gt 8 ] && slots=8
  export RUSTC_WRAPPER=$gexe DDDA_GATE_SHARDS=${DDDA_GATE_SHARDS:-$slots} DDDA_GATE_FREE_MB=${DDDA_GATE_FREE_MB:-3500}
  jobs=(-j 16)
  echo "build_play: gate: $DDDA_GATE_SHARDS shard compilers at once ($free_mb MB free), 16 job tokens"
fi
args=(build --profile "$profile" -p ddplay --features emu "${jobs[@]}" $timings "$@")
logdir=$td/runs/logs; mkdir -p "$logdir"
log=$logdir/$tag.log

t0=$(date +%s)
echo "build_play: $tag: cargo ${args[*]} (target $td, ${jobs[*]:-CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-default}}, $(date +%T))"
for ((attempt = 1; attempt <= tries; attempt++)); do
  ta=$(date +%s)
  cargo "${args[@]}" > "$log" 2>&1
  rc=$?
  compiled=$(grep -cE '^\s+Compiling ' "$log")
  echo "build_play: attempt $attempt: exit $rc in $(( $(date +%s) - ta )) s, $compiled crates compiled:" \
    "$(grep -E '^\s+Compiling ' "$log" | awk '{print $2}' | tr '\n' ' ' | cut -c1-300)"
  [ $rc -eq 0 ] && break
  # a real compile error has rustc diagnostics; a killed rustc only "could not compile"
  if grep -E '^error(\[E[0-9]+\])?: ' "$log" | grep -vqE 'could not compile|process didn.t exit'; then
    grep -E '^error' -A6 "$log" | head -40
    echo "build_play: compile error, not retrying (log $log)"; exit 1
  fi
  tail -5 "$log"
done
if [ $rc -ne 0 ]; then
  echo "build_play: FAILED after $tries attempts, $(( $(date +%s) - t0 )) s (log $log)"; exit 1
fi

out=$td/$profile
run=$td/runs/$tag; mkdir -p "$run"
cp "$out/ddplay.exe" "$out/ddplay.pdb" "$run/" || exit 1
echo "build_play: ok in $(( $(date +%s) - t0 )) s: $run/ddplay.exe" \
  "($(( $(stat -c %s "$run/ddplay.exe") >> 20 )) MB, PDB $(( $(stat -c %s "$run/ddplay.pdb") >> 20 )) MB)"
[ -n "$timings" ] && ls -t "$td"/cargo-timings/cargo-timing-2*.html 2>/dev/null | head -1
exit 0
