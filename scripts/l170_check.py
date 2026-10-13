"""Isolated opt2 ALU-inline A/B: original vectors, asm counts and fixture timings.

No shards or game run. Generated bodies/vectors/assembly stay in the private
CARGO_TARGET_DIR. Existing generated bodies are used to isolate this one pass.
Guest calls use the same recorded external-call seam in both variants.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'research/native'))
import inline_flags
import lift
from l161_check import records

ADDRESSES = ['00dd42c0', '00efd240', '00db63e0', '00fee320']
# Closed leaves executed by the recorder (including EFD240's size getter).
SUPPORT = ['00d63ff0', '00d1b540', '00d12e10', '00fed8f0']


def counts(text):
    calls = [line.strip() for line in text.splitlines()
             if re.match(r'\s*callq?\s', line)
             or re.match(r'\s*jmpq?\s+_(?:R|Z)', line)]
    result = Counter()
    for line in calls:
        if 'fast_' in line:
            category = 'memory'
        elif 'Flags' in line:
            category = 'flags'
        elif 'x87' in line:
            category = 'x87'
        elif 'sse2' in line:
            category = 'sse'
        elif 'call_' in line or re.search(r'f_[0-9a-f]{8}', line):
            category = 'guest_call'
        elif 'push' in line or 'pop' in line:
            category = 'push_pop'
        elif '*' in line:
            category = 'indirect'
        else:
            category = 'other'
        result[category] += 1
    result['instructions'] = sum(bool(re.match(r'^\s+[a-z][a-z0-9]+\s', line))
                                 for line in text.splitlines())
    result['calls'] = len(calls)
    return dict(result)


def assembly_report(target, out, instructions):
    asm = max((target / 'measure/deps').glob('l170_isolated-*.s'), key=lambda p: p.stat().st_mtime)
    assembly = asm.read_text()
    result = {}
    for variant in ('baseline', 'candidate'):
        for address in ADDRESSES:
            # Include out-of-line split parts; opt2 may inline different parts
            # after changing helper visibility. Counting only the root is wrong.
            matches = list(re.finditer(r'(?m)^(\S*' + variant + r'\S*f_' + address + r'(?:_p\d+)?):\n', assembly))
            assert matches, (variant, address)
            text = '\n'.join(assembly[m.start():assembly.index('.seh_endproc', m.start())] for m in matches)
            (out / f'{variant}-{address}.s').write_text(text)
            entry = counts(text)
            entry['guest_instructions'] = instructions[address]
            entry['host_functions'] = len(matches)
            result[f'{variant}-{address}'] = entry
    (out / 'assembly-counts.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bench', action='store_true')
    parser.add_argument('--analyze-only', action='store_true')
    args = parser.parse_args()
    target = Path(os.environ['CARGO_TARGET_DIR'])
    out = target / 'isolated'
    (out / 'vectors').mkdir(parents=True, exist_ok=True)
    if args.analyze_only:
        old = json.loads((out / 'assembly-counts.json').read_text())
        assembly_report(target, out, {a: old[f'baseline-{a}']['guest_instructions'] for a in ADDRESSES})
        return
    pattern = 'pub fn f_(' + '|'.join(ADDRESSES + SUPPORT) + ')'
    paths = subprocess.check_output(['rg', '-l', pattern, 'crates/ddlift'], cwd=ROOT, text=True).splitlines()
    bodies = {}
    for path in paths:
        source = (ROOT / path).read_text()
        for address in ADDRESSES + SUPPORT:
            start = source.find(f'pub fn f_{address}(')
            if start < 0:
                continue
            end = source.find('// Translated from', start)
            body = source[start:end if end >= 0 else len(source)]
            body = re.sub(r'x86::call_local\(m, cpu, (0x[0-9A-F]+), (?:super::lifted::f_[0-9a-f]+|crate::platform::\w+)\)',
                          r'x86::call_ext(m, cpu, \1)', body)
            bodies[address] = body.replace('x86::call_lifted(', 'x86::call_ext(')
    assert set(bodies) == set(ADDRESSES + SUPPORT), bodies.keys()
    assert not any('_inline(' in body for body in bodies.values()), 'baseline must use default helpers'
    instructions = {f"{r['addr']:08x}": len(r['body'])
                    for r in records(Path(lift.N) / 'patterns/lift.json')
                    if f"{r['addr']:08x}" in ADDRESSES}
    assert set(instructions) == set(ADDRESSES)
    selected = [record for record in records(Path(lift.N) / 'vectors/lift.json')
                if f"{record['addr']:08x}" in ADDRESSES]
    assert {record['addr'] for record in selected} == {int(a, 16) for a in ADDRESSES}
    assert all(not c.get('terminal') and not c.get('machine') for c in selected), 'benchmark needs returning, non-machine fixtures'
    (out / 'vectors/lift.json').write_text(json.dumps(selected))
    print('original vectors', dict(Counter(f"{r['addr']:08x}" for r in selected)), flush=True)
    prelude = 'use ddnative_base::{mem::Mem, emit, x86::{self, Cpu, Flags}};\n'
    functions = ('pub fn all() -> Vec<(u32,x86::LiftFn)> { vec![' +
                 ','.join(f'(0x{a}, f_{a})' for a in ADDRESSES) + '] }\n'
                 'pub fn dispatch_table() -> std::collections::HashMap<u32,x86::LiftFn> { [' +
                 ','.join(f'(0x{a}, f_{a} as x86::LiftFn)' for a in SUPPORT) +
                 ', (0x012efea0, ddnative_base::platform::memcpy as x86::LiftFn)].into_iter().collect() }\n')
    source = '#![allow(warnings)]\npub use ddnative_base::{mem, platform};\n'
    for variant in ('baseline', 'candidate'):
        os.environ['LIFT_INLINE_FLAGS'] = str(int(variant == 'candidate'))
        parts = []
        for address, body in bodies.items():
            result = inline_flags.retarget(body)
            result = result.replace('pub fn f_', '#[inline(never)]\npub fn f_')
            parts.append(result)
            if address in ADDRESSES:
                print(variant, address, 'retargeted', result.count('_inline('), flush=True)
        source += f'pub mod {variant} {{\n' + prelude + '\n'.join(parts) + functions + '}\n'
        test = lift.lifted_test_source('').replace('//!', '//').replace(
            'use ddnative::generated::lifted;', f'use crate::{variant} as lifted;').replace('ddnative::', 'ddnative_base::')
        if variant == 'baseline':
            bench = (ROOT / 'scripts/l161_bench.rs').read_text().split('#[test]\n#[ignore = "bounded paired long-loop')[0]
            # Preserve the established timing boundary and add complete final
            # register/lazy-flag/memory A/B checks outside the timed call.
            bench = bench.replace('let mut m = PageMemory', 'let mut expected = None;\n        let mut m = PageMemory')
            bench = bench.replace('let mut nanos = 0;', 'let mut last_cpu = None;\n                let mut nanos = 0;')
            bench = bench.replace('black_box(cpu.eax);', 'black_box(cpu.eax);\n                    last_cpu = Some(cpu);')
            bench = bench.replace('                eprintln!("l161-cost', '''                let cpu = last_cpu.unwrap();
                let fl = cpu.fl;
                let result = (cpu.exit(), (fl.kind, fl.a, fl.b, fl.r, fl.bits, fl.cf_in),
                    cpu.xmm, cpu.df, cpu.fs_base, cpu.sys_flags,
                    initial.keys().map(|a| (*a, m.u8(*a))).collect::<Vec<_>>());
                if let Some(ref want) = expected { assert_eq!(&result, want); }
                else { expected = Some(result); }
                eprintln!("l161-cost''')
            test += bench.replace('l161-cost', 'l170-cost')
        source += f'#[cfg(test)] mod test_{variant} {{\n' + test + '}\n'
    (out / 'isolated.rs').write_text(source)
    (out / 'Cargo.toml').write_text(f'''[package]
name="l170-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{ROOT.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{ROOT.as_posix()}/crates/ddnative-host"}}
serde_json="1"
[lib]
path="isolated.rs"
[profile.measure]
inherits="release"
opt-level=2
debug=0
codegen-units=1
[profile.measure.package.ddnative-base]
opt-level=3
[profile.measure.package.ddnative-host]
opt-level=3
''')
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_BUILD_JOBS='4',
               LIFT_DATA=str(out), DDNATIVE_REQUIRE_VECTORS='1')
    common = ['--manifest-path', str(out / 'Cargo.toml'), '--profile', 'measure']
    subprocess.run(['cargo', 'test', *common, '--', '--test-threads=1', '--nocapture'], env=env, check=True)
    subprocess.run(['cargo', 'rustc', *common, '--lib', '--', '--emit=asm'], env=env, check=True)
    assembly_report(target, out, instructions)
    if args.bench:
        subprocess.run(['cargo', 'test', *common, 'paired_cost', '--', '--ignored', '--nocapture', '--test-threads=1'],
                       env=env, check=True, timeout=60)


if __name__ == '__main__':
    main()
