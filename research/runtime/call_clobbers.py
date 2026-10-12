"""L67: are ecx, edx and the flags a CRT routine leaves observable? For each
call_ext site of the given targets in the generated shards, the code after
the call is scanned along the lifted function's blocks (its `pc = ...`
successors, up to DEPTH blocks): the first access of cpu.ecx/cpu.edx and
of the flags on every path is a write (not observable), a read
(observable), or unknown (the path leaves the function: a return, an
indirect jump, or the depth runs out). `xor r, r` counts as a write.

    python call_clobbers.py 0x01301D90 0x012EE1D0 ...

Read-only: crates/ddlift/*/src/generated/*.rs (the committed lifts).
"""
import glob
import re
import sys
from collections import Counter, defaultdict

DEPTH = 6
targets = {int(a, 16) for a in sys.argv[1:]}
call = re.compile(r'let t = 0x([0-9A-F]+)u32;[^}]*?x86::call_ext\(m, cpu, t\);')
FLAG_WRITE = re.compile(r'cpu\.fl(\s*=[^=]|\.alu_|\.inc_dec|\.logic|\.set_|\.shift|\.rot|\.mul|\.neg|\.bt)')
FLAG_READ = re.compile(r'cpu\.fl\.(cond|cf|zf|sf|of|pf|af|value|flags|get)')
ARM = re.compile(r'\n\s*0x([0-9A-F]+) => \{')
SUCC = re.compile(r'pc = (?:if [^{]*\{ 0x([0-9A-F]+) \} else \{ 0x([0-9A-F]+) \}|0x([0-9A-F]+));\s*continue;')


def first(text, reg):
    if reg == 'flags':
        w, r = FLAG_WRITE.search(text), FLAG_READ.search(text)
        if r and (not w or r.start() < w.start()):
            return 'read', None
        return ('write', None) if w else (None, None)
    for m in re.finditer(r'alu_xor\(cpu\.' + reg + r', cpu\.' + reg + r',|cpu\.' + reg + r'\b(\s*=(?!=))?', text):
        if m.group(0).startswith('alu_xor') or m.group(1):
            return 'write', None
        return 'read', None
    return None, None


def block_end(text):
    ends = [i for i in (text.find('continue;'), text.find('return;')) if i >= 0]
    nxt = ARM.search(text)
    e = min(ends) + 9 if ends else len(text)
    if nxt and nxt.start() < e:
        e = nxt.start()
    return e


def walk(func, arms, text, reg, depth, seen):
    """The first access of `reg` from `text` on: read/write/unknown."""
    end = block_end(text)
    block = text[:end]
    what, _ = first(block, reg)
    if what:
        return what
    if 'return;' in block or depth == 0:
        return 'unknown'
    m = SUCC.search(block)
    if not m:
        return 'unknown'
    succ = [int(x, 16) for x in m.groups() if x]
    outcomes = set()
    for s in succ:
        if s in seen or s not in arms:
            outcomes.add('unknown' if s not in arms else 'write')
            continue
        outcomes.add(walk(func, arms, func[arms[s]:], reg, depth - 1, seen | {s}))
    if 'read' in outcomes:
        return 'read'
    return 'write' if outcomes == {'write'} else 'unknown'


results = defaultdict(Counter)
reads = []
root = sys.path[0] + '/../../crates/ddlift'
for path in glob.glob(root + '/*/src/generated/lifted_*.rs'):
    src = open(path, encoding='utf-8').read()
    for m in call.finditer(src):
        t = int(m.group(1), 16)
        if t not in targets:
            continue
        fstart = src.rfind('\npub fn ', 0, m.start())
        fend = src.find('\npub fn ', m.end())
        func = src[fstart:fend if fend > 0 else len(src)]
        arms = {int(a.group(1), 16): a.end() for a in ARM.finditer(func)}
        rest = func[m.end() - fstart:]
        for reg in ('ecx', 'edx', 'flags'):
            r = walk(func, arms, rest, reg, DEPTH, frozenset())
            results[t][f'{reg}:{r}'] += 1
            if r == 'read':
                reads.append((t, reg, func[:40].strip()))
for t in sorted(targets):
    print(f'{t:#010x}', dict(sorted(results[t].items())))
for r in reads[:20]:
    print('read', hex(r[0]), r[1], r[2])
