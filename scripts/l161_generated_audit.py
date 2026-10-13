"""Compare generated source to its git baseline without exporting game code.

Print only counts and hashes. In particular, verify the restartable step
bodies are byte-identical and memory-helper sites were not removed.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
FUNCTION = re.compile(r'(?m)^pub fn ([fs]_[0-9a-f]{8})\(')
MEMORY = re.compile(r'\bm\.fast_(?:u8|u16|u32|set_u8|set_u16|set_u32)\(|'
                    r'\bemit::(?:push|pop|load64|load128|store64|store128)\(|'
                    r'\bcpu\.st\.(?:real_memory|memory)::')
FLAGS = re.compile(r'\bFlags::(?:new|partial)\(|\bcpu\.fl\.(?:alu_\w+|inc_dec)\(')


def inventory(files):
    bodies = {}
    totals = Counter()
    compiled = set()
    for path, data in files:
        if '/src/generated/' not in path or not path.endswith('.rs'):
            continue
        text = data.decode('utf-8').replace('\r\n', '\n')
        if path.endswith('/mod.rs'):
            compiled.update((Path(path).parent / (name + '.rs')).as_posix()
                            for name in re.findall(r'\bmod (lifted_\d+);', text))
            continue
        if Path(path).as_posix() not in compiled:
            continue
        for match in FUNCTION.finditer(text):
            end = text.find('// Translated from', match.end())
            body = text[match.start():end if end >= 0 else len(text)].rstrip()
            name = match[1]
            if name in bodies:
                raise ValueError('duplicate generated body: ' + name)
            bodies[name] = dict(sha256=hashlib.sha256(body.encode()).hexdigest(),
                               flags=len(FLAGS.findall(body)), memory=len(MEMORY.findall(body)))
            totals['flags'] += bodies[name]['flags']
            totals['memory'] += bodies[name]['memory']
    return bodies, totals


def git_files(ref):
    paths = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', ref, '--', 'crates/ddlift'],
                                    cwd=ROOT, text=True).splitlines()
    process = subprocess.Popen(['git', 'cat-file', '--batch'], cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    try:
        for path in sorted(paths, key=lambda p: not p.endswith('/mod.rs')):
            if '/src/generated/' not in path or not path.endswith('.rs'):
                continue
            process.stdin.write(f'{ref}:{path}\n'.encode())
            process.stdin.flush()
            header = process.stdout.readline().split()
            if len(header) != 3 or header[1] != b'blob':
                raise ValueError(header)
            data = process.stdout.read(int(header[2]))
            assert process.stdout.read(1) == b'\n'
            yield path, data
    finally:
        process.stdin.close()
        process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline-ref', default='3e72b984')
    args = parser.parse_args()
    before, old_totals = inventory(git_files(args.baseline_ref))
    paths = sorted((ROOT / 'crates/ddlift').glob('*/src/generated/*.rs'), key=lambda p: p.name != 'mod.rs')
    after, new_totals = inventory((p.as_posix(), p.read_bytes()) for p in paths)
    assert before.keys() == after.keys(), 'function inventory changed'
    changed = [name for name in before if before[name]['sha256'] != after[name]['sha256']]
    steps = [name for name in before if name.startswith('s_')]
    assert not set(steps).intersection(changed), 'restartable step body changed'
    assert all(before[name]['memory'] == after[name]['memory'] for name in before), 'memory helper count changed'
    print(json.dumps(dict(baseline=args.baseline_ref, whole_functions=len(before)-len(steps),
        steps=len(steps), changed_functions=len(changed), before=old_totals, after=new_totals,
        flags_removed=old_totals['flags']-new_totals['flags'],
        per_function={name: before[name]['flags']-after[name]['flags'] for name in changed}), indent=2))


if __name__ == '__main__':
    main()
