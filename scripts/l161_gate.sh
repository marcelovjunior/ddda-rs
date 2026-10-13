#!/usr/bin/env bash
# Invoke ONLY through heavy.py with HEAVY_BIG=1 (one shard build at a time).
# Historical full-gate reproduction: head-owned only under the shared train rule.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR=C:/ddda-targets/L161
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export LIFT_FLAG_FIXED_POINT=1
export LIFT_DATA=E:/ddda-re/notes/native-L161
export INVENTORY_DB=E:/ddda-re/notes/L161/inventory.sqlite
export L161_PROFILE_CONFIG=C:/ddda-targets/L161/profiles.toml
stage="${1:?all, gen, opt0 or opt1}"
mkdir -p /c/ddda-targets/L161 /e/ddda-re/notes/L161
if [[ "$stage" == all ]]; then
    E:/ddda-re/tools/pyenv/Scripts/python.exe -m unittest discover -s research/native -p test_lean_emission.py
    E:/ddda-re/tools/pyenv/Scripts/python.exe -m unittest discover -s research/native -p test_lift_codegen.py
    python -m unittest discover -s research/native -p test_register_cache.py
    bash "$0" gen
    bash "$0" opt0
    bash "$0" opt1
    exit
fi
if [[ "$stage" == gen ]]; then
    E:/ddda-re/tools/pyenv/Scripts/python.exe - <<'PY'
import hashlib, json, os, re, shutil, subprocess, time
from pathlib import Path
target = Path(os.environ['LIFT_DATA'])
(target / 'patterns').mkdir(parents=True, exist_ok=True)
source = Path('E:/ddda-re/notes/native')
shutil.copy2(source / 'patterns/lift.json', target / 'patterns/lift.json')
if not (target / 'vectors').exists():
    # A private read-only replay reference; gen writes only its own patterns.
    subprocess.run(['powershell', '-NoProfile', '-Command',
        'New-Item -ItemType Junction -Path E:/ddda-re/notes/native-L161/vectors '
        '-Target E:/ddda-re/notes/native/vectors | Out-Null'], check=True)
shutil.copy2('E:/ddda-re/notes/inventory/inventory.sqlite', os.environ['INVENTORY_DB'])
hot = Path('C:/ddda-targets/L161/hot.txt')
hot.write_text('\n'.join(re.findall(r'\(0x([0-9A-F]+), f_',
    Path('crates/ddlift/lift_hot_00/src/generated/mod.rs').read_text())))
begin = time.monotonic()
subprocess.run(['E:/ddda-re/tools/pyenv/Scripts/python.exe', 'research/native/lift.py',
    'gen', '--hot-functions', str(hot)], check=True)
print('L161 generation seconds', time.monotonic() - begin, flush=True)
receipt = []
for path in sorted(Path('crates/ddlift').glob('*/src/generated/*.rs')):
    data = path.read_bytes()
    receipt.append((str(path), len(data), hashlib.sha256(data).hexdigest()))
Path('E:/ddda-re/notes/L161/generated-receipt.json').write_text(json.dumps(receipt, indent=2))
PY
    exit
fi
[[ "$stage" == opt0 || "$stage" == opt1 ]]
if [[ "$stage" == opt0 ]]; then
    E:/ddda-re/tools/pyenv/Scripts/python.exe -m unittest discover -s research/native -p test_lean_emission.py
    E:/ddda-re/tools/pyenv/Scripts/python.exe -m unittest discover -s research/native -p test_dead_flags.py
fi
level="${stage#opt}"
export NATIVE_PROFILE="l161-o$level"
python - <<'PY'
import os
from pathlib import Path
text = ''
for opt in (0, 1):
    profile = f'l161-o{opt}'
    text += f'[profile.{profile}]\ninherits="play"\n'
    for package in [f'ddlift-{i:02}' for i in range(32)] + ['ddlift-hot-00']:
        text += f'[profile.{profile}.package.{package}]\nopt-level={opt}\nincremental=false\n'
Path(os.environ['L161_PROFILE_CONFIG']).write_text(text)
PY
cargo() { command cargo --config "$L161_PROFILE_CONFIG" "$@"; }
export -f cargo
start=$SECONDS
cargo test --profile "$NATIVE_PROFILE" -p ddnative -p ddnative-base --no-run --message-format=json \
    > "E:/ddda-re/notes/L161/artifacts-$stage.jsonl"
echo "L161 $stage build wall seconds $((SECONDS-start))"
bash scripts/native_gate.sh
