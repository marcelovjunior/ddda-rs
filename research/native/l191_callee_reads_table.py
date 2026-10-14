"""L191 audit: the callee-saved registers (EBX, EBP, ESI, EDI) each recorded guest
callee reads before writing them, from its lifted code (l191_callee_inputs.py).
Scans the recorded vectors that the crates' tests read, collects every recorded
call target, and writes crates/mtcore/tests/support/callee_reads.rs.

Targets with a dispatch-block lifted body use the block analysis of
l191_callee_inputs.py. Straight-line lifted bodies (no dispatch block) are read
as one path: a register read before its first write in textual order, with calls
followed into their callees. Targets with no lifted body get all four registers
(the conservative default); the tests' synthetic stub and heap addresses read
nothing. Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import glob
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import l191_callee_inputs as inputs  # noqa: E402

ROOT = Path('E:/ddda-rs-wt/L191')
NOTES = Path('E:/ddda-re/notes')
SAVED = {'ebx': 3, 'ebp': 5, 'esi': 6, 'edi': 7}
# the tests' synthetic stub and heap addresses (not code of the image)
SYNTHETIC = (0x2000_0000, 0x4000_0000)
# the L173 recorders' BEGIN/END markers (record_l173.py): harness callbacks, reads nothing
MARKERS = (0x0120_0000, 0x0120_0020)
# recorded targets whose callee is not known (no lifted body, not a recorder marker):
# the reads are unknown, so the register check is not made. Open: see docs/tasks/L191-haiku-ports.md.
OPEN = {0x0DEAD100: 'routine of the joints recording (L165 job_joints.json, case 480): no lifted body'}
LITERAL = re.compile(r'"(E:/ddda-re/notes/[^"]+\.json)"')
NATIVE_DIR = re.compile(r'native-L\d+')
NAMED = re.compile(r'vectors\("([A-Za-z0-9_]+\.json)"\)')
FUNCTION = re.compile(r'pub fn f_([0-9a-f]{8})\(m: &mut dyn Mem, cpu: &mut Cpu\) \{\n(.*?)\n\}\n', re.S)
CALL = re.compile(r'x86::call_(?:ext|lifted)\(m, cpu, 0x([0-9A-Fa-f]+)')
FN = {}


def function_bodies():
    for f in glob.glob(str(ROOT / 'crates/ddlift/*/src/generated/*.rs')):
        text = Path(f).read_text(encoding='utf-8', errors='replace')
        for m in FUNCTION.finditer(text):
            FN.setdefault(int(m.group(1), 16), m.group(2))


def callee_read_mask(target, depth):
    """Bit i set: the callee reads register i (saved registers only) before writing it."""
    if target in inputs.GLOBAL:
        return sum(1 << SAVED[r] for r in inputs.inputs_of(target) if r in SAVED)
    if target in FN and depth < 6:
        return linear_mask(FN[target], depth + 1)
    return sum(1 << i for i in (3, 5, 6, 7))


def linear_mask(body, depth=0):
    """Saved registers read before written, in textual order (one path, no branches)."""
    written, reads = 0, 0
    for st in re.split(r';|\n', body):
        st = st.strip()
        if not st:
            continue
        call = CALL.search(st)
        if call:
            mask = callee_read_mask(int(call.group(1), 16), depth)
            reads |= mask & ~written
            continue
        m = inputs.ASSIGN.search(st)
        rhs = st[m.end():] if m else st
        if not st.startswith('emit::push(cpu.'):
            for r in inputs.REG.findall(rhs):
                if not written & (1 << inputs.IDX[r]):
                    reads |= 1 << inputs.IDX[r]
        if m:
            written |= 1 << inputs.IDX[m.group(1)]
    return reads


def saved_list(mask):
    return sorted(i for i in (3, 5, 6, 7) if mask & (1 << i))


def vector_files():
    # the literal paths, the named vectors, and every recording of the native-L### directories
    # the tests name (the draw tests read {address}.json from one of them)
    files = set()
    for src in glob.glob(str(ROOT / 'crates/*/tests/**/*.rs'), recursive=True):
        text = Path(src).read_text(encoding='utf-8', errors='replace')
        files.update(LITERAL.findall(text))
        for name in NAMED.findall(text):
            files.update(str(p) for p in NOTES.glob(f'native*/**/{name}'))
        for dirname in set(NATIVE_DIR.findall(text)):
            files.update(str(p) for p in (NOTES / dirname).glob('*.json'))
    return sorted(f for f in files if Path(f).exists())


def targets_in(node, out):
    if isinstance(node, dict):
        for k, v in node.items():
            if k == 'target' and isinstance(v, int):
                out.add(v)
            else:
                targets_in(v, out)
    elif isinstance(node, list):
        for v in node:
            targets_in(v, out)


def main():
    function_bodies()
    found = set()
    files = vector_files()
    for f in files:
        try:
            data = json.loads(Path(f).read_text(encoding='utf-8', errors='replace'))
        except (ValueError, OSError):
            continue
        targets_in(data, found)
    rows = []
    for t in sorted(found):
        if SYNTHETIC[0] <= t < SYNTHETIC[1] or MARKERS[0] <= t < MARKERS[1]:
            rows.append((t, [], 'synthetic'))
        elif t in OPEN:
            rows.append((t, [], 'open'))
        elif t in inputs.GLOBAL:
            rows.append((t, sorted(SAVED[r] for r in inputs.inputs_of(t) if r in SAVED), 'lifted'))
        elif t in FN:
            rows.append((t, saved_list(callee_read_mask(t, 0)), 'straight-line'))
        else:
            rows.append((t, [3, 5, 6, 7], 'unlifted'))
    notes = {
        'lifted': '',
        'straight-line': ' // straight-line body, one path',
        'unlifted': ' // no lifted body: conservative',
        'synthetic': ' // test stub, heap or recorder marker: reads nothing',
        'open': ' // OPEN: ' + 'unknown reads, not checked (see docs)',
    }
    out = ROOT / 'crates/mtcore/tests/support/callee_reads.rs'
    out.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        '//! Generated by research/native/l191_callee_reads_table.py from the lifted code of the',
        '//! guest callees the recorded vectors call: the callee-saved registers each one reads',
        '//! before writing it (EBX=3, EBP=5, ESI=6, EDI=7 as in `[u32; 8]` register arrays).',
        '//! Targets without a lifted body read all four (the conservative default).',
        '',
        '/// The callee-saved registers the callee at `target` reads.',
        "pub fn saved_reads(target: u32) -> &'static [usize] {",
        '    match target {',
    ]
    for t, regs, kind in rows:
        lines.append(f'        0x{t:08x} => &{regs!r},{notes[kind]}')
    lines += ['        _ => &[3, 5, 6, 7],', '    }', '}', '']
    out.write_text('\n'.join(lines), encoding='utf-8')
    counts = {k: sum(1 for r in rows if r[2] == k) for k in notes}
    print(f'{len(files)} vector files, {len(rows)} targets: {counts} -> {out}')


if __name__ == '__main__':
    main()
