"""Isolated baseline/cache original-vector replay; no shard or game build.

Outputs selected bodies and filtered private vectors only under
CARGO_TARGET_DIR; no generated shard build. Both variants use identical
external-call replay. The head owns the full-corpus integration build.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'research/native'))
import lift
import lean_emission


def records(path):
    """Stream a JSON array without loading the 400 MB vector set at once."""
    decoder = json.JSONDecoder()
    with path.open() as stream:
        text = ''
        done = False
        while not done:
            more = stream.read(1024 * 1024)
            done = not more
            text += more
            while True:
                text = text.lstrip(' \n\r\t[,')
                if not text or text.startswith(']'):
                    break
                try:
                    record, end = decoder.raw_decode(text)
                except json.JSONDecodeError:
                    if done:
                        raise
                    break
                yield record
                text = text[end:]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--opt', type=int, choices=(0, 1, 2), required=True)
    p.add_argument('--prepare-only', action='store_true')
    p.add_argument('--bench', action='store_true')
    p.add_argument('--asm', action='store_true', help='emit isolated test assembly for inspection')
    p.add_argument('--variant', choices=('registers', 'flags', 'result-zf'), default='flags')
    p.add_argument('--baseline-ref', default='3e72b984', help='git pin for baseline generated bodies')
    p.add_argument('--cpu', type=int, help='pin this process and child benchmark to one Windows logical CPU')
    p.add_argument('--census', type=Path, default=Path('E:/ddda-re/notes/L161/c26-audit/census.json'))
    p.add_argument('--extra-hot', action='store_true', help='also replay DD42C0, DB63E0 and EFD240')
    a = p.parse_args()
    if a.cpu is not None:
        import ctypes
        if os.name != 'nt' or not 0 <= a.cpu < 64:
            p.error('--cpu requires Windows and an index from 0 to 63')
        kernel = ctypes.windll.kernel32
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        kernel.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        if not kernel.SetProcessAffinityMask(kernel.GetCurrentProcess(), 1 << a.cpu):
            raise OSError('SetProcessAffinityMask failed')
    target = Path(os.environ['CARGO_TARGET_DIR'])
    out = target / 'isolated'
    out.mkdir(parents=True, exist_ok=True)
    addresses = json.loads(a.census.read_text())['top'] + ['0x00e0e5e0']
    if a.extra_hot:
        addresses += ['0x00dd42c0', '0x00db63e0', '0x00efd240']
    support = ['0x00d63ff0', '0x00d1b540', '0x00d12e10']
    if a.extra_hot:
        support += ['0x00fed8f0']  # EFD240's original vectors execute this body
    bodies = {}
    pattern = 'pub fn f_(' + '|'.join(address[2:] for address in addresses + support) + ')'
    paths = subprocess.check_output(['git', 'grep', '-l', '-E', pattern, a.baseline_ref, '--',
                                     'crates/ddlift'], cwd=ROOT, text=True).splitlines()
    for path in paths:
        text = subprocess.check_output(['git', 'show', path], cwd=ROOT, text=True)
        for address in addresses + support:
            start = text.find(f'pub fn f_{address[2:]}(')
            if start < 0:
                continue
            end = text.find('// Translated from', start)
            body = text[start:end if end >= 0 else len(text)]
            # Same-shard direct functions are external boundaries in this
            # isolated harness; original vectors must supply every call.
            body = re.sub(r'x86::call_local\(m, cpu, (0x[0-9A-F]+), (?:super::lifted::f_[0-9a-f]+|crate::platform::\w+)\)',
                          r'x86::call_ext(m, cpu, \1)', body)
            body = body.replace('x86::call_lifted(', 'x86::call_ext(')
            bodies[address] = body
    assert len(bodies) == len(addresses + support)
    candidates = {}
    loop_arms = {}
    if a.variant in ('flags', 'result-zf', 'registers'):
        os.environ['LIFT_FLAG_FIXED_POINT'] = '1'
        for record in records(Path(lift.N) / 'patterns/lift.json'):
            address = f"0x{record['addr']:08x}"
            if address in bodies:
                if address == '0x00fedd60' and a.variant == 'result-zf':
                    for loop_entry, loop_exit in ((0xFEDF26,0xFEDF39),(0xFEDF44,0xFEDF57)):
                        arm = next(arm for arm in record['arms']
                                   if int(re.match(r'\s*0x([0-9A-Fa-f]+)',arm)[1],16) == loop_entry)
                        loop_arms[loop_entry] = [arm, f'0x{loop_exit:X} => {{ return; }}']
                os.environ['LIFT_RESULT_ZF'] = '0'
                os.environ['LIFT_REG_LOCALS'] = '0'
                baseline = lift.shard_fn(record) if a.variant in ('result-zf', 'registers') else None
                os.environ['LIFT_RESULT_ZF'] = '1' if a.variant == 'result-zf' else '0'
                os.environ['LIFT_REG_LOCALS'] = '1' if a.variant == 'registers' else '0'
                body = lift.shard_fn(record)
                body = re.sub(r'x86::call_local\(m, cpu, (0x[0-9A-F]+), (?:super::lifted::f_[0-9a-f]+|crate::platform::\w+)\)',
                              r'x86::call_ext(m, cpu, \1)', body)
                candidates[address] = body.replace('x86::call_lifted(', 'x86::call_ext(')
                if baseline is not None:
                    baseline = re.sub(r'x86::call_local\(m, cpu, (0x[0-9A-F]+), (?:super::lifted::f_[0-9a-f]+|crate::platform::\w+)\)',
                                      r'x86::call_ext(m, cpu, \1)', baseline)
                    bodies[address] = baseline.replace('x86::call_lifted(', 'x86::call_ext(')
                    if a.variant == 'result-zf':
                        assert candidates[address] == lean_emission.result_zf(bodies[address]), address
        assert candidates.keys() == bodies.keys()
        if a.variant == 'result-zf':
            removed = {address: body.count('cpu.fl.zf()') - candidates[address].count('cpu.fl.zf()')
                       for address,body in bodies.items() if body != candidates[address]}
            print('exact predicate-only source audit:', removed, flush=True)
    vector_dir = out / 'vectors'
    vector_dir.mkdir(exist_ok=True)
    selected = [record for record in records(Path(lift.N) / 'vectors/lift.json')
                if f"0x{record['addr']:08x}" in addresses]
    assert {record['addr'] for record in selected} == {int(addr, 16) for addr in addresses}
    (vector_dir / 'lift.json').write_text(json.dumps(selected))
    prelude = 'use ddnative_base::{mem::Mem, emit, x86::{self, Cpu, Flags}};\n'
    all_functions = ('pub fn all() -> Vec<(u32, x86::LiftFn)> { vec![' +
                     ','.join(f'({address}, f_{address[2:]})' for address in addresses) + '] }\n'
                     'pub fn dispatch_table() -> std::collections::HashMap<u32,x86::LiftFn> { '
                     '[' + ','.join(f'({addr}, f_{addr[2:]} as x86::LiftFn)' for addr in support) + ', '
                     '(0x012efea0, ddnative_base::platform::memcpy as x86::LiftFn)].into_iter().collect() }\n')
    source = '#![allow(warnings)]\npub use ddnative_base::{mem, platform};\n'
    for variant in ('baseline', 'candidate'):
        changed = 0
        parts = []
        for address, body in bodies.items():
            cached = candidates[address] if variant == 'candidate' else body
            normalized = lambda s: re.sub(r'\s+', '', re.sub(r'(?m)^//.*$', '', s))
            changed += normalized(cached) != normalized(body)
            parts.append(cached)
        source += f'mod {variant} {{\n' + prelude + '\n'.join(parts) + all_functions + '}\n'
        if a.variant == 'result-zf':
            os.environ['LIFT_RESULT_ZF'] = '1' if variant == 'candidate' else '0'
            loops = '\n'.join(lift.rust_fn(address,'isolated recorded loop',arms)
                              for address,arms in loop_arms.items())
            source += f'mod loop_{variant} {{\n' + prelude + loops + '}\n'
        test = lift.lifted_test_source('').replace('//!', '//').replace('use ddnative::generated::lifted;', f'use crate::{variant} as lifted;')
        test = test.replace('ddnative::', 'ddnative_base::')
        if variant == 'baseline':
            test += (ROOT / 'scripts/l161_bench.rs').read_text()
            if a.variant == 'result-zf':
                test += (ROOT / 'scripts/l161_result_bench.rs').read_text()
        source += f'mod test_{variant} {{\n' + test + '}\n'
        print(variant, 'changed bodies', changed, 'vectors', len(selected), flush=True)
    (out / 'isolated.rs').write_text(source)
    (out / 'Cargo.toml').write_text(f'''[package]
name="l161-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{ROOT.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{ROOT.as_posix()}/crates/ddnative-host"}}
serde_json="1"
[[test]]
name="isolated"
path="isolated.rs"
[profile.measure]
inherits="release"
opt-level={a.opt}
debug=1
codegen-units=1
[profile.measure.package.ddnative-base]
opt-level=3
[profile.measure.package.ddnative-host]
opt-level=3
''')
    if not a.prepare_only:
        env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_BUILD_JOBS='4',
                   LIFT_DATA=str(out), DDNATIVE_REQUIRE_VECTORS='1')
        subprocess.run(['cargo', 'test', '--manifest-path', str(out / 'Cargo.toml'), '--profile', 'measure',
                        '--', '--test-threads=1'], env=env, check=True)
        if a.bench:
            subprocess.run(['cargo', 'test', '--manifest-path', str(out / 'Cargo.toml'), '--profile', 'measure',
                            '_cost', '--', '--ignored', '--nocapture', '--test-threads=1'],
                           env=env, check=True, timeout=60)
        if a.asm:
            subprocess.run(['cargo', 'rustc', '--manifest-path', str(out / 'Cargo.toml'),
                            '--test', 'isolated', '--profile', 'measure', '--', '--emit=asm',
                            '-Cllvm-args=-x86-asm-syntax=intel'], env=env, check=True)


if __name__ == '__main__':
    main()
