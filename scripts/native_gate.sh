#!/usr/bin/env bash
# The commit gate for ddnative and its ddlift dependencies (docs/NATIVE_PIPELINE.md).
# Every differential test must run against its vectors (a missing vector
# file fails), every port's address must be declared by a test, and the
# tracker is rebuilt. Commit only if this exits 0:
#   scripts/native_gate.sh && git commit ...
set -euo pipefail
cd "$(dirname "$0")/.."
# Bound concurrent shard compilers unless the caller deliberately overrides it.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
DDNATIVE_REQUIRE_VECTORS=1 cargo test -q -p ddnative
out=$(python scripts/inventory.py build)
echo "$out"
if echo "$out" | grep -q "ported without a declared test"; then
    echo "native_gate: a port has no declared test (add '// verifies 0x...' above its test)" >&2
    exit 1
fi
python scripts/inventory.py summary
