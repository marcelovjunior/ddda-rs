"""L184 s10: transliterate one small-subset x86 function (integer moves, a
few ALU ops, compare + jump, thiscall CALLs with a stack frame in guest
memory, scalar and packed SSE float math) to a Rust ABI-level port.

Registers are Rust locals, the stack stays in guest memory (callees see
the original's frame), flags are replaced by the compare operands of the
setter the jump needs (no EFLAGS emulation), blocks dispatch through a
`match pc` that rustc turns into a jump table. Every instruction keeps a
comment with its address. Verified afterwards against the original in
Unicorn (research/native/record_p2c_*.py).

    mini_lift.py <entry hex> <rust_fn_name> <out.rs> [<doc comment>]

Reads the disassembly from the Ghidra HTTP server (read-only).
"""
import re
import sys
import urllib.request

REG32 = ['eax', 'ecx', 'edx', 'ebx', 'esp', 'ebp', 'esi', 'edi']
REG8 = {'al': 'eax', 'cl': 'ecx', 'dl': 'edx', 'bl': 'ebx'}


def fetch(entry):
    text = urllib.request.urlopen(f'http://127.0.0.1:8089/disassemble_function?address={entry:08x}').read().decode()
    ins = []
    for line in text.splitlines():
        m = re.match(r'([0-9a-f]{8}): (\w+)\s*(.*?)\s*$', line)
        if m:
            ins.append((int(m.group(1), 16), m.group(2).lower(), m.group(3)))
    return ins


def split_ops(ops):
    out, depth, cur = [], 0, ''
    for ch in ops:
        if ch == '[':
            depth += 1
        elif ch == ']':
            depth -= 1
        if ch == ',' and depth == 0:
            out.append(cur.strip())
            cur = ''
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


class Gen:
    def __init__(self, ins):
        self.ins = ins
        self.calls = {}
        self.next = {ins[i][0]: (ins[i + 1][0] if i + 1 < len(ins) else None) for i in range(len(ins))}

    # ---- operand helpers
    @staticmethod
    def mem(op):
        """-> (size, address expression) or None."""
        m = re.match(r'(?:(\w+) ptr )?\[(\w+)(?: \+ (-?0x[0-9a-f]+))?\]$', op)
        if not m:
            return None
        size, reg, disp = m.groups()
        base = reg.lower()
        d = int(disp, 16) if disp else 0
        addr = base if d == 0 else (f'{base}.wrapping_add(0x{d:x})' if d > 0 else f'{base}.wrapping_sub(0x{-d:x})')
        return size, addr

    @staticmethod
    def is_stack(op):
        return bool(re.search(r'\[esp\b', op, re.I))

    def val(self, op, size=4):
        """read operand as u32 expression."""
        op = op.strip()
        low = op.lower()
        if low in REG32:
            return low
        if low in REG8:
            return f'({REG8[low]} & 0xff)'
        if re.match(r'^-?0x[0-9a-f]+$', low):
            v = int(low, 16) & 0xFFFFFFFF
            return f'0x{v:x}u32'
        mm = self.mem(op)
        if mm:
            sz, addr = mm
            if sz in ('dword', 'float', None):
                return f'ld!({addr})' if self.is_stack(op) else f'pg.u32(m, {addr})'
            if sz == 'byte':
                return f'u32::from(pg.u8(m, {addr}))'
            if sz == 'word':
                return f'u32::from(pg.u16(m, {addr}))'
        raise SystemExit(f'operand {op}')

    def store(self, op, expr):
        low = op.lower()
        if low in REG32:
            return f'{low} = {expr};'
        mm = self.mem(op)
        if mm:
            sz, addr = mm
            if sz in ('dword', 'float', None):
                return f'st!({addr}, {expr});' if self.is_stack(op) else f'pg.set_u32(m, {addr}, {expr});'
            if sz == 'byte':
                return f'pg.set_u8(m, {addr}, ({expr}) as u8);'
        raise SystemExit(f'store {op}')

    def x(self, op):
        m = re.match(r'XMM(\d)$', op.strip(), re.I)
        return int(m.group(1)) if m else None

    def f32_of(self, op):
        """scalar float source: xmm low lane or memory dword."""
        r = self.x(op)
        if r is not None:
            return f'f32::from_bits(x[{r}][0])'
        mm = self.mem(op)
        if self.is_stack(op):
            return f'f32::from_bits(ld!({mm[1]}))'
        return f'f32::from_bits(pg.u32(m, {mm[1]}))'

    def quad(self, op):
        """128-bit source as [u32;4] expression."""
        r = self.x(op)
        if r is not None:
            return f'x[{r}]'
        mm = self.mem(op)
        a = mm[1]
        if self.is_stack(op):
            return f'[ld!({a}), ld!({a}.wrapping_add(4)), ld!({a}.wrapping_add(8)), ld!({a}.wrapping_add(12))]'
        return f'[pg.u32(m, {a}), pg.u32(m, {a}.wrapping_add(4)), pg.u32(m, {a}.wrapping_add(8)), pg.u32(m, {a}.wrapping_add(12))]'

    # ---- one instruction -> (statements, flag-setter or None, terminator)
    def emit(self, a, mn, ops, setter):
        o = split_ops(ops)
        nxt = self.next[a]
        S = []
        if mn == 'push':
            S.append(f'esp = esp.wrapping_sub(4); st!(esp, {self.val(o[0])});')
        elif mn == 'pop':
            S.append(f'{o[0].lower()} = ld!(esp); esp = esp.wrapping_add(4);')
        elif mn == 'mov':
            S.append(self.store(o[0], self.val(o[1])))
        elif mn == 'lea':
            mm = self.mem(o[1])
            S.append(f'{o[0].lower()} = {mm[1]};')
        elif mn == 'sub' and o[0].lower() == 'esp':
            S.append(f'esp = esp.wrapping_sub({self.val(o[1])}); frame_lo = esp.wrapping_sub(0x40);')
        elif mn in ('and', 'or', 'add', 'sub'):
            op = {'and': '&', 'or': '|'}.get(mn)
            e = f'{self.val(o[0])} {op} {self.val(o[1])}' if op else f'{self.val(o[0])}.wrapping_{mn}({self.val(o[1])})'
            S.append(self.store(o[0], e))
            setter = ('res', o[0].lower())
        elif mn == 'shr':
            S.append(self.store(o[0], f'{self.val(o[0])} >> {int(o[1], 16)}'))
            setter = ('res', o[0].lower())
        elif mn == 'cmp':
            S.append(f'ia = {self.val(o[0])}; ib = {self.val(o[1])};')
            setter = ('cmp',)
        elif mn == 'test':
            S.append(f'ia = {self.val(o[0])} & {self.val(o[1])};')
            setter = ('test',)
        elif mn == 'comiss':
            S.append(f'fa = {self.f32_of(o[0])}; fb = {self.f32_of(o[1])};')
            setter = ('comiss',)
        elif mn == 'movss':
            d, s = self.x(o[0]), self.x(o[1])
            if d is not None and s is None:
                S.append(f'x[{d}] = [{self.val(o[1])}, 0, 0, 0];')
            elif d is not None:
                S.append(f'x[{d}][0] = x[{s}][0];')
            else:
                S.append(self.store(o[0], f'x[{s}][0]'))
        elif mn == 'movaps':
            d, s = self.x(o[0]), self.x(o[1])
            if d is not None:
                S.append(f'x[{d}] = {self.quad(o[1])};')
            else:
                mm = self.mem(o[0])
                ad = mm[1]
                store = 'st!(' if self.is_stack(o[0]) else 'pg.set_u32(m, '
                S.append(f'let q = x[{s}]; for (k, w) in q.into_iter().enumerate() {{ {store}{ad}.wrapping_add(4 * k as u32), w); }}')
        elif mn in ('mulss', 'addss', 'subss', 'divss'):
            d = self.x(o[0])
            sym = {'mulss': '*', 'addss': '+', 'subss': '-', 'divss': '/'}[mn]
            S.append(f'x[{d}][0] = (f32::from_bits(x[{d}][0]) {sym} {self.f32_of(o[1])}).to_bits();')
        elif mn in ('mulps', 'addps'):
            d = self.x(o[0])
            sym = '*' if mn == 'mulps' else '+'
            S.append(f'{{ let s = {self.quad(o[1])}; for k in 0..4 {{ x[{d}][k] = (f32::from_bits(x[{d}][k]) {sym} f32::from_bits(s[k])).to_bits(); }} }}')
        elif mn == 'shufps':
            d, s, imm = self.x(o[0]), self.x(o[1]), int(o[2], 16)
            S.append(f'{{ let (dv, sv) = (x[{d}], x[{s}]); x[{d}] = [dv[{imm & 3}], dv[{(imm >> 2) & 3}], sv[{(imm >> 4) & 3}], sv[{(imm >> 6) & 3}]]; }}')
        elif mn == 'sqrtss':
            d = self.x(o[0])
            S.append(f'x[{d}][0] = {self.f32_of(o[1])}.sqrt().to_bits();')
        elif mn == 'call':
            tgt = int(o[0], 16)
            spec = self.calls[tgt]
            pre = f'esp = esp.wrapping_sub(4); st!(esp, 0x{nxt:08x}); flush!(esp, {4 + 4 * spec["nargs"]});'
            if spec['out']:
                pre += f' flush!(ld!(esp.wrapping_add(4)), {spec["out"]});'
            post = f' refill!(ecx, {spec["in"]});' if spec['in'] else ''
            S.append(
                'c.eax = eax; c.ecx = ecx; c.edx = edx; c.ebx = ebx; c.ebp = ebp; c.esi = esi; c.edi = edi;'
                f' {pre} c.esp = esp;'
                f' x86::call_ext(m, c, 0x{tgt:08x});'
                f' eax = c.eax; ecx = c.ecx; edx = c.edx; ebx = c.ebx; ebp = c.ebp; esi = c.esi; edi = c.edi; esp = c.esp;{post}')
        elif mn == 'ret':
            n = int(o[0], 16) if o else 0
            S.append(f'c.eax = eax; c.ecx = ecx; c.edx = edx; c.ebx = ebx; c.ebp = ebp; c.esi = esi; c.edi = edi; c.esp = esp.wrapping_add({4 + n}); return;')
        elif mn == 'jmp':
            S.append(f'pc = 0x{int(o[0], 16):08x}; continue;')
        elif mn.startswith('j'):
            t = int(o[0], 16)
            cond = self.cond(mn, setter)
            S.append(f'pc = if {cond} {{ 0x{t:08x} }} else {{ 0x{nxt:08x} }}; continue;')
        elif mn == 'nop':
            pass
        else:
            raise SystemExit(f'unsupported {mn} {ops} at {a:x}')
        return S, setter

    def cond(self, mn, setter):
        k = setter[0]
        if k == 'cmp':
            return {'jz': 'ia == ib', 'jnz': 'ia != ib', 'jle': '(ia as i32) <= (ib as i32)', 'jbe': 'ia <= ib', 'ja': 'ia > ib',
                    'jl': '(ia as i32) < (ib as i32)', 'jge': '(ia as i32) >= (ib as i32)', 'jg': '(ia as i32) > (ib as i32)'}[mn]
        if k == 'test':
            return {'jz': 'ia == 0', 'jnz': 'ia != 0', 'jle': '(ia as i32) <= 0', 'jg': '(ia as i32) > 0'}[mn]
        if k == 'res':
            r = setter[1]
            return {'jz': f'{r} == 0', 'jnz': f'{r} != 0'}[mn]
        if k == 'comiss':
            return {'jbe': '!(fa > fb)', 'ja': 'fa > fb', 'jz': 'fa == fb || fa.is_nan() || fb.is_nan()'}[mn]
        raise SystemExit(f'cond {mn} {setter}')

    def run(self, entry, fn, doc):
        ins = self.ins
        targets = {entry}
        for i, (a, mn, ops) in enumerate(ins):
            if mn == 'jmp' or (mn.startswith('j')):
                targets.add(int(split_ops(ops)[0], 16))
                if i + 1 < len(ins):
                    targets.add(ins[i + 1][0])
            if mn == 'ret' and i + 1 < len(ins):
                targets.add(ins[i + 1][0])
        body = []
        setter = None
        for i, (a, mn, ops) in enumerate(ins):
            if a in targets:
                if body:
                    body.append(f'        pc = 0x{a:08x}; continue;\n    }}')
                body.append(f'    0x{a:08x} => {{')
                setter = None
            S, setter_new = self.emit(a, mn, ops, setter)
            if setter_new is not None:
                setter = setter_new
            for s in S:
                body.append(f'        {s}  // {a:08x}: {mn} {ops}')
        body.append('    }')
        out = []
        out.append(f'{doc}')
        out.append('#[allow(unused_assignments, unused_variables, unused_mut, unreachable_code)]')
        out.append(f'pub fn {fn}(m: &mut dyn Mem, c: &mut Cpu) {{')
        out.append('    let pg = Pages::of(m);')
        out.append('    let (mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);')
        out.append('    let mut x = [[0u32; 4]; 8];')
        out.append('    // the frame below the aligned ESP (locals and pushes) lives in an array, flushed to guest memory for callees')
        out.append('    let mut fr = [0u32; FRAME_WORDS];')
        out.append('    let mut frame_lo = 0u32;')
        out.append('    macro_rules! in_frame { ($a:expr) => { $a.wrapping_sub(frame_lo) < (FRAME_WORDS as u32) * 4 } }')
        out.append('    macro_rules! ld { ($a:expr) => {{ let a: u32 = $a; if in_frame!(a) { fr[((a - frame_lo) >> 2) as usize] } else { pg.u32(m, a) } }} }')
        out.append('    macro_rules! st { ($a:expr, $v:expr) => {{ let (a, v): (u32, u32) = ($a, $v); if in_frame!(a) { fr[((a - frame_lo) >> 2) as usize] = v; } else { pg.set_u32(m, a, v); } }} }')
        out.append('    macro_rules! flush { ($a:expr, $n:expr) => {{ let a: u32 = $a; if in_frame!(a) { let mut k = 0u32; while k < $n { let w = fr[((a + k - frame_lo) >> 2) as usize]; pg.set_u32(m, a + k, w); k += 4; } } }} }')
        out.append('    macro_rules! refill { ($a:expr, $n:expr) => {{ let a: u32 = $a; if in_frame!(a) { let mut k = 0u32; while k < $n { fr[((a + k - frame_lo) >> 2) as usize] = pg.u32(m, a + k); k += 4; } } }} }')
        out.append('    let (mut ia, mut ib) = (0u32, 0u32);')
        out.append('    let (mut fa, mut fb) = (0f32, 0f32);')
        out.append(f'    let mut pc: u32 = 0x{entry:08x};')
        out.append('    loop {')
        out.append('        match pc {')
        out.extend(('    ' + l) if not l.startswith('    }') else ('    ' + l) for l in body)
        out.append('            _ => unreachable!(),')
        out.append('        }')
        out.append('    }')
        out.append('}')
        return '\n'.join(out)


def main():
    entry = int(sys.argv[1], 16)
    fn, path = sys.argv[2], sys.argv[3]
    doc = sys.argv[4] if len(sys.argv) > 4 else ''
    g = Gen(fetch(entry))
    # --call TARGET:NARGS[:out=SIZE][:in=SIZE]: callee stack arguments (it pops them), bytes it reads
    # through its first argument (flushed from the frame first), bytes it writes through ECX (read back)
    for a in sys.argv[5:]:
        parts = a.split(':')
        spec = {'nargs': int(parts[1]), 'out': 0, 'in': 0}
        for extra in parts[2:]:
            k, v = extra.split('=')
            spec[k] = int(v)
        g.calls[int(parts[0], 16)] = spec
    open(path, 'w').write(g.run(entry, fn, doc) + '\n')


main()
