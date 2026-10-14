"""L191: registers a guest callee reads before writing them, from its lifted code.
Explores every path from the entry block (a path's state = registers written so far);
a read of a register not yet written on some path makes it an input of the callee."""
import re, glob, sys
ROOT = 'E:/ddda-rs-wt/L191/crates/ddlift'
BLOCK = re.compile(r'^\s*(0x[0-9A-Fa-f]+)\s*=>\s*\{(.*)$')
REGS = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi']
IDX = {r: i for i, r in enumerate(REGS)}
REG = re.compile(r'cpu\.(eax|ecx|edx|ebx|esp|ebp|esi|edi)\b')
ASSIGN = re.compile(r'cpu\.(eax|ecx|edx|ebx|esp|ebp|esi|edi)\s*=\s*(?!=)')
NEXT = re.compile(r'pc = (0x[0-9A-Fa-f]+);')
COND = re.compile(r'pc = if (.*?) \{ (0x[0-9A-Fa-f]+) \} else \{ (0x[0-9A-Fa-f]+) \}')
GLOBAL = {}
for f in glob.glob(ROOT + '/*/src/generated/*.rs'):
    for ln in open(f, encoding='utf-8', errors='replace'):
        m = BLOCK.match(ln)
        if m:
            GLOBAL.setdefault(int(m.group(1), 16), m.group(2))

def block_effect(text, written):
    """Returns (reads_before_write mask, written mask after, successors, ends)."""
    reads = 0
    stmts = text.split(';')
    succ = []
    ends = False
    for st in stmts:
        s = st.strip()
        if not s:
            continue
        m = ASSIGN.search(s)
        rhs = s[m.end():] if m else s
        lhs_reg = m.group(1) if m else None
        # reads in the right-hand side (or the whole statement)
        for r in REG.findall(rhs):
            if not written & (1 << IDX[r]):
                reads |= 1 << IDX[r]
        if lhs_reg:
            written |= 1 << IDX[lhs_reg]
        # pops write registers: `cpu.X = emit::pop(` is an assignment above; calls clobber
        if 'x86::call_' in s:
            # a call reads its arguments (already counted) and clobbers caller-saved registers
            written |= (1 << IDX['eax']) | (1 << IDX['ecx']) | (1 << IDX['edx'])
    c = COND.search(text)
    if c:
        for r in REG.findall(c.group(1)):
            if not written & (1 << IDX[r]):
                reads |= 1 << IDX[r]
        succ = [int(c.group(2), 16), int(c.group(3), 16)]
    else:
        n = NEXT.search(text)
        if n:
            succ = [int(n.group(1), 16)]
        elif 'return' in text:
            ends = True
    return reads, written, succ, ends

def inputs_of(entry):
    seen = set()
    result = 0
    work = [(entry, 0)]
    while work:
        addr, written = work.pop()
        if (addr, written) in seen:
            continue
        seen.add((addr, written))
        text = GLOBAL.get(addr)
        if text is None:
            continue  # an unknown block: treated as no further reads
        reads, w2, succ, ends = block_effect(text, written)
        result |= reads
        for s in succ:
            work.append((s, w2))
    return [REGS[i] for i in range(8) if result & (1 << i)]

if __name__ == '__main__':
    for a in sys.argv[1:]:
        t = int(a, 16)
        print(a, inputs_of(t) if t in GLOBAL else 'no lifted body')
