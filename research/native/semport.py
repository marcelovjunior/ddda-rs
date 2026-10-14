"""Mechanical register-level translator for semantic ports (L184, s4).

Translates one DDDA.exe function (32-bit x86, integer + SSE scalar/packed
subset, no x87) from the unpacked exe into a Rust function over guest
memory. Registers, XMM registers and flags live in Rust locals (no per-
instruction emulation, no Cpu struct traffic except around guest calls);
the stack stays in guest memory at the original's addresses, so every
guest call sees exactly the frame the original builds. Operation order and
operand widths are the original's, so results are bit-identical.

The generated function is a basic-block dispatcher: `match pc { block => {
...; pc = next; } }`, each instruction a commented Rust statement.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe:
    semport.py <entry> <end_inclusive_last_insn> <rust_fn_name> <out.rs>
"""
import sys
import struct

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
import capstone as cs  # noqa: E402
from capstone.x86 import *  # noqa: E402,F403

md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_32)
md.detail = True

GPR = {}
for full, parts in (('eax', ('al', 'ah', 'ax')), ('ecx', ('cl', 'ch', 'cx')), ('edx', ('dl', 'dh', 'dx')),
                    ('ebx', ('bl', 'bh', 'bx'))):
    GPR[full] = (full, 0, 32)
    GPR[parts[0]] = (full, 0, 8)
    GPR[parts[1]] = (full, 8, 8)
    GPR[parts[2]] = (full, 0, 16)
for full, part in (('esi', 'si'), ('edi', 'di'), ('ebp', 'bp'), ('esp', 'sp')):
    GPR[full] = (full, 0, 32)
    GPR[part] = (full, 0, 16)

JCC = {
    'jz': 'zf', 'je': 'zf', 'jnz': '!zf', 'jne': '!zf', 'jb': 'cf', 'jc': 'cf', 'jnae': 'cf', 'jae': '!cf',
    'jnb': '!cf', 'jnc': '!cf', 'jbe': '(cf || zf)', 'jna': '(cf || zf)', 'ja': '(!cf && !zf)',
    'jnbe': '(!cf && !zf)', 'js': 'sf', 'jns': '!sf', 'jp': 'pf', 'jpe': 'pf', 'jnp': '!pf', 'jpo': '!pf',
    'jl': '(sf != of_)', 'jnge': '(sf != of_)', 'jge': '(sf == of_)', 'jnl': '(sf == of_)',
    'jle': '(zf || sf != of_)', 'jng': '(zf || sf != of_)', 'jg': '(!zf && sf == of_)', 'jnle': '(!zf && sf == of_)',
}


class Fn:
    def __init__(self, entry, last, name):
        self.entry, self.name = entry, name
        code = h.code(entry, last - entry + 16)
        self.insns = []
        for i in md.disasm(code, entry):
            self.insns.append(i)
            if i.address >= last:
                break
        assert self.insns[-1].address == last, hex(self.insns[-1].address)
        self.by_addr = {i.address: i for i in self.insns}
        self.out = []
        self.cache = {}      # (address expr, width) -> (variable, registers it depends on)
        self.cache_vars = 0
        self.callee_pops = {}
        self.frame = None   # frame size when the locals live in a Rust array
        self.want_frame = False
        self.esp_at = {}

    # ---- frame localisation ----
    def analyse_frame(self, callee_pops):
        """Find the aligned frame (AND ESP,-16; SUB ESP,k), track ESP's offset
        from it through the CFG and decide whether the locals can live in a
        Rust array (no pointer to the frame is ever formed, no byte access)."""
        self.callee_pops = callee_pops
        insns = self.insns
        idx = {i.address: k for k, i in enumerate(insns)}
        first = None
        for k, i in enumerate(insns):
            if i.mnemonic == 'and' and i.operands[0].type == X86_OP_REG and self.reg_name(i, i.operands[0].reg) == 'esp':
                first = k
                break
        if first is None:
            return
        nxt = None
        for cand in insns[first + 1:first + 8]:
            if cand.mnemonic == 'sub' and cand.operands[0].type == X86_OP_REG and self.reg_name(cand, cand.operands[0].reg) == 'esp':
                nxt = cand
                break
        if nxt is None:
            return
        size = nxt.operands[1].imm
        # escape check: ESP is only a memory base, pushed/popped or adjusted
        for i in insns:
            if i.mnemonic == 'lea' and any(o.type == X86_OP_MEM and o.mem.base and self.reg_name(i, o.mem.base) == 'esp' for o in i.operands):
                return
            if i.mnemonic == 'mov' and i.operands[1].type == X86_OP_REG and self.reg_name(i, i.operands[1].reg) == 'esp' and i.address < insns[first].address + 1:
                continue
            for op in i.operands:
                if op.type == X86_OP_REG and self.reg_name(i, op.reg) in ('esp', 'sp'):
                    if i.mnemonic in ('and', 'sub', 'add', 'push', 'pop', 'ret', 'call'):
                        continue
                    if i.mnemonic == 'mov' and op is i.operands[0]:
                        continue   # mov esp, ebp (epilogue)
                    return
        # forward dataflow of ESP's offset from the aligned frame
        def step(i, off):
            mn = i.mnemonic
            if mn == 'push':
                return off - 4
            if mn == 'pop':
                return off + 4
            if mn in ('sub', 'add') and i.operands[0].type == X86_OP_REG and self.reg_name(i, i.operands[0].reg) == 'esp':
                return off - i.operands[1].imm if mn == 'sub' else off + i.operands[1].imm
            if mn == 'mov' and i.operands[0].type == X86_OP_REG and self.reg_name(i, i.operands[0].reg) == 'esp':
                return None
            if mn == 'call':
                op = i.operands[0]
                key = op.imm if op.type == X86_OP_IMM else 'indirect'
                if key not in self.callee_pops:
                    raise SystemExit(f'give the stack bytes callee {hex(i.address)} pops (--pops=target:bytes): {key}')
                return off + self.callee_pops[key]
            return off
        state = {insns[first + 1].address: 0}
        work = [insns[first + 1].address]
        while work:
            a = work.pop()
            k = idx[a]
            off = state[a]
            while True:
                i = insns[k]
                self.esp_at[i.address] = off
                new = None if off is None else step(i, off)
                if i.mnemonic == 'ret':
                    break
                succs = []
                if i.mnemonic == 'jmp':
                    succs.append(i.operands[0].imm)
                else:
                    if i.mnemonic in JCC:
                        succs.append(i.operands[0].imm)
                    if k + 1 < len(insns):
                        succs.append(insns[k + 1].address)
                fall = insns[k + 1].address if (i.mnemonic != 'jmp' and k + 1 < len(insns)) else None
                for t in succs:
                    if t in state:
                        assert state[t] == new, (hex(i.address), state[t], new)
                    else:
                        state[t] = new
                        if t != fall:
                            work.append(t)
                if fall is None:
                    break
                if fall in self.esp_at:
                    break
                k += 1
                off = new
        self.frame = size

    def slot(self, i, op, width):
        """Index (in dwords) of a frame-local access [esp + disp], or None."""
        if self.frame is None or op.type != X86_OP_MEM:
            return None
        m = op.mem
        if not m.base or self.reg_name(i, m.base) != 'esp' or m.index:
            return None
        off = self.esp_at.get(i.address)
        if off is None:
            return None
        a = off + m.disp
        if not (-self.frame <= a and a + width <= 0):
            return None
        assert (a + self.frame) % 4 == 0 and width in (4, 16), (hex(i.address), a, width)
        return (a + self.frame) // 4

    # ---- operand helpers ----
    def reg_name(self, i, r):
        return i.reg_name(r)

    def addr_expr(self, i, op):
        m = op.mem
        parts = []
        if m.base:
            parts.append(self.reg_name(i, m.base))
        if m.index:
            idx = self.reg_name(i, m.index)
            parts.append(f'{idx}.wrapping_mul({m.scale})' if m.scale != 1 else idx)
        expr = None
        for p in parts:
            expr = p if expr is None else f'{expr}.wrapping_add({p})'
        disp = m.disp & 0xffffffff
        if expr is None:
            return f'0x{disp:08x}u32'
        if disp:
            return f'{expr}.wrapping_add(0x{disp:x})'
        return expr

    def read_gpr(self, name):
        full, shift, bits = GPR[name]
        if bits == 32:
            return full
        mask = (1 << bits) - 1
        return f'(({full} >> {shift}) & 0x{mask:x})'

    def invalidate_reg(self, full):
        for key in [k for k, (_, regs) in self.cache.items() if full in regs]:
            del self.cache[key]

    def cached_load(self, i, op, width):
        """A guest load that is not repeated while nothing it depends on
        changes (no guest store, call or write to its address registers)."""
        a = self.addr_expr(i, op)
        key = (a, width)
        if key in self.cache:
            return self.cache[key][0]
        regs = set()
        if op.mem.base:
            regs.add(GPR[self.reg_name(i, op.mem.base)][0])
        if op.mem.index:
            regs.add(GPR[self.reg_name(i, op.mem.index)][0])
        name = f'ld{self.cache_vars}'
        self.cache_vars += 1
        load = {8: f'pg.u8(m, {a}) as u32', 16: f'pg.u16(m, {a}) as u32', 32: f'pg.u32(m, {a})'}[width]
        self.cache[key] = (name, regs)
        return f'{{ {name} = {load}; {name} }}'

    def write_gpr(self, name, value):
        full, shift, bits = GPR[name]
        self.invalidate_reg(full)
        if bits == 32:
            return f'{full} = {value};'
        mask = (1 << bits) - 1
        return f'{full} = ({full} & !(0x{mask:x}u32 << {shift})) | ((({value}) & 0x{mask:x}) << {shift});'

    def get(self, i, op, bits=None):
        """Rust expression (u32) for an integer operand."""
        if op.type == X86_OP_REG:
            return self.read_gpr(self.reg_name(i, op.reg))
        if op.type == X86_OP_IMM:
            b = bits or op.size * 8
            return f'0x{op.imm & ((1 << b) - 1):x}u32'
        assert op.type == X86_OP_MEM
        sl = self.slot(i, op, 4) if (bits or op.size * 8) == 32 else None
        if sl is not None:
            return f'fr[{sl}].to_bits()'
        b = bits or op.size * 8
        return self.cached_load(i, op, b)

    def put(self, i, op, value):
        if op.type == X86_OP_REG:
            return self.write_gpr(self.reg_name(i, op.reg), value)
        assert op.type == X86_OP_MEM
        sl = self.slot(i, op, 4) if op.size * 8 == 32 else None
        if sl is not None:
            return f'fr[{sl}] = f32::from_bits({value});'
        self.cache.clear()
        a = self.addr_expr(i, op)
        b = op.size * 8
        return {8: f'pg.set_u8(m, {a}, ({value}) as u8);', 16: f'pg.set_u16(m, {a}, ({value}) as u16);',
                32: f'pg.set_u32(m, {a}, {value});'}[b]

    # xmm helpers
    def xmm_index(self, i, op):
        n = self.reg_name(i, op.reg)
        assert n.startswith('xmm'), n
        return int(n[3:])

    def lane0(self, i, op):
        if op.type == X86_OP_REG:
            return f'xmm[{self.xmm_index(i, op)}][0]'
        sl = self.slot(i, op, 4)
        if sl is not None:
            return f'fr[{sl}]'
        return f'f32::from_bits({self.cached_load(i, op, 32)})'

    def vec4(self, i, op):
        if op.type == X86_OP_REG:
            return f'xmm[{self.xmm_index(i, op)}]'
        sl = self.slot(i, op, 16)
        if sl is not None:
            return f'[fr[{sl}], fr[{sl + 1}], fr[{sl + 2}], fr[{sl + 3}]]'
        a = self.addr_expr(i, op)
        parts = []
        for k in range(4):
            key = (f'{a}.wrapping_add({4 * k})', 32)
            if key in self.cache:
                parts.append(f'f32::from_bits({self.cache[key][0]})')
            else:
                regs = set()
                if op.mem.base:
                    regs.add(GPR[self.reg_name(i, op.mem.base)][0])
                if op.mem.index:
                    regs.add(GPR[self.reg_name(i, op.mem.index)][0])
                name = f'ld{self.cache_vars}'
                self.cache_vars += 1
                self.cache[key] = (name, regs)
                parts.append(f'f32::from_bits({{ {name} = pg.u32(m, {key[0]}); {name} }})')
        return '[' + ', '.join(parts) + ']'

    # ---- instruction translation ----
    def tr(self, i):
        mn = i.mnemonic
        ops = i.operands
        L = []
        e = L.append
        if mn in ('mov', 'movzx'):
            e(self.put(i, ops[0], self.get(i, ops[1])))
        elif mn == 'movsx':
            bits = ops[1].size * 8
            sx = {8: 'as i8', 16: 'as i16'}[bits]
            e(self.put(i, ops[0], f'(({self.get(i, ops[1])}) {sx} as i32 as u32)'))
        elif mn == 'lea':
            e(self.put(i, ops[0], self.addr_expr(i, ops[1])))
        elif mn in ('add', 'sub', 'and', 'or', 'xor', 'cmp', 'test'):
            bits = ops[0].size * 8
            a, b = self.get(i, ops[0]), self.get(i, ops[1], bits)
            mask = f'0x{(1 << bits) - 1:x}u32'
            e(f'{{ let (a, b) = ({a}, {b});')
            if mn in ('sub', 'cmp'):
                e(f'(cf, zf, sf, of_, pf) = rt::sub_flags(a, b, {bits});')
                res = f'a.wrapping_sub(b) & {mask}'
            elif mn == 'add':
                e(f'(cf, zf, sf, of_, pf) = rt::add_flags(a, b, {bits});')
                res = f'a.wrapping_add(b) & {mask}'
            else:
                op = {'and': '&', 'or': '|', 'xor': '^', 'test': '&'}[mn]
                res = f'(a {op} b) & {mask}'
                e(f'(cf, zf, sf, of_, pf) = rt::logic_flags({res}, {bits});')
            if mn not in ('cmp', 'test'):
                e('let r = ' + res + ';')
                e(self.put(i, ops[0], 'r'))
            e('}')
        elif mn in ('inc', 'dec'):
            bits = ops[0].size * 8
            a = self.get(i, ops[0])
            mask = f'0x{(1 << bits) - 1:x}u32'
            e(f'{{ let a = {a}; let old_cf = cf;')
            if mn == 'inc':
                e(f'(cf, zf, sf, of_, pf) = rt::add_flags(a, 1, {bits});')
                e(f'let r = a.wrapping_add(1) & {mask};')
            else:
                e(f'(cf, zf, sf, of_, pf) = rt::sub_flags(a, 1, {bits});')
                e(f'let r = a.wrapping_sub(1) & {mask};')
            e('cf = old_cf;')
            e(self.put(i, ops[0], 'r'))
            e('}')
        elif mn == 'shl':
            bits = ops[0].size * 8
            count = ops[1].imm & 31 if ops[1].type == X86_OP_IMM else None
            assert count is not None and count != 0
            e(f'{{ let a = {self.get(i, ops[0])};')
            e(f'let (r, c_f, z_f, s_f, o_f, p_f) = rt::shl(a, {count}, {bits});')
            e('(cf, zf, sf, of_, pf) = (c_f, z_f, s_f, o_f, p_f);')
            e(self.put(i, ops[0], 'r'))
            e('}')
        elif mn == 'push':
            e(f'{{ let v = {self.get(i, ops[0], 32)}; esp = esp.wrapping_sub(4); pg.set_u32(m, esp, v); }}')
            self.cache.clear()
        elif mn == 'pop':
            self.invalidate_reg('esp')
            e(f'{{ let v = pg.u32(m, esp); esp = esp.wrapping_add(4); {self.put(i, ops[0], "v")} }}')
        elif mn == 'lahf':
            self.invalidate_reg('eax')
            e('eax = (eax & 0xffff_00ff) | ((((sf as u32) << 7) | ((zf as u32) << 6) | ((pf as u32) << 2) | 2 | (cf as u32)) << 8);')
        elif mn == 'movss':
            d, s = ops
            if d.type == X86_OP_REG:
                di = self.xmm_index(i, d)
                if s.type == X86_OP_REG:
                    e(f'xmm[{di}][0] = xmm[{self.xmm_index(i, s)}][0];')
                else:
                    e(f'xmm[{di}] = [{self.lane0(i, s)}, 0.0, 0.0, 0.0];')
            else:
                sl = self.slot(i, d, 4)
                if sl is not None:
                    e(f'fr[{sl}] = xmm[{self.xmm_index(i, s)}][0];')
                else:
                    self.cache.clear()
                    e(f'pg.set_u32(m, {self.addr_expr(i, d)}, xmm[{self.xmm_index(i, s)}][0].to_bits());')
        elif mn in ('movaps', 'movups', 'movdqa', 'movdqu'):
            d, s = ops
            if d.type == X86_OP_REG:
                e(f'xmm[{self.xmm_index(i, d)}] = {self.vec4(i, s)};')
            elif self.slot(i, d, 16) is not None:
                sl = self.slot(i, d, 16)
                e(f'{{ let v = xmm[{self.xmm_index(i, s)}]; fr[{sl}] = v[0]; fr[{sl + 1}] = v[1]; fr[{sl + 2}] = v[2]; fr[{sl + 3}] = v[3]; }}')
            else:
                self.cache.clear()
                a = self.addr_expr(i, d)
                e(f'{{ let a = {a}; let v = xmm[{self.xmm_index(i, s)}];')
                for k in range(4):
                    e(f'pg.set_u32(m, a.wrapping_add({4 * k}), v[{k}].to_bits());')
                e('}')
        elif mn in ('mulss', 'addss', 'subss', 'divss'):
            op = {'mulss': '*', 'addss': '+', 'subss': '-', 'divss': '/'}[mn]
            di = self.xmm_index(i, ops[0])
            e(f'xmm[{di}][0] = rt::{mn}(xmm[{di}][0], {self.lane0(i, ops[1])});')
        elif mn == 'sqrtss':
            di = self.xmm_index(i, ops[0])
            e(f'xmm[{di}][0] = {self.lane0(i, ops[1])}.sqrt();')
        elif mn in ('mulps', 'addps', 'subps', 'divps'):
            op = {'mulps': '*', 'addps': '+', 'subps': '-', 'divps': '/'}[mn]
            di = self.xmm_index(i, ops[0])
            e(f'{{ let s = {self.vec4(i, ops[1])}; for k in 0..4 {{ xmm[{di}][k] = rt::{mn[:-2]}ss(xmm[{di}][k], s[k]); }} }}')
        elif mn in ('xorps', 'andps', 'orps'):
            op = {'xorps': '^', 'andps': '&', 'orps': '|'}[mn]
            di = self.xmm_index(i, ops[0])
            e(f'{{ let s = {self.vec4(i, ops[1])}; for k in 0..4 {{ xmm[{di}][k] = f32::from_bits(xmm[{di}][k].to_bits() {op} s[k].to_bits()); }} }}')
        elif mn == 'shufps':
            di = self.xmm_index(i, ops[0])
            e(f'{{ let s = {self.vec4(i, ops[1])}; xmm[{di}] = rt::shufps(xmm[{di}], s, {ops[2].imm}); }}')
        elif mn in ('comiss', 'ucomiss'):
            e(f'{{ let (c_f, z_f, p_f) = rt::comiss({self.lane0(i, ops[0])}, {self.lane0(i, ops[1])}); '
              f'(cf, zf, pf, sf, of_) = (c_f, z_f, p_f, false, false); }}')
        elif mn == 'cvtps2pd':
            di = self.xmm_index(i, ops[0])
            s = ops[1]
            if s.type == X86_OP_REG:
                e(f'{{ let s = xmm[{self.xmm_index(i, s)}]; xmm[{di}] = rt::cvtps2pd(s[0], s[1]); }}')
            else:
                a = self.addr_expr(i, s)
                e(f'{{ let a = {a}; xmm[{di}] = rt::cvtps2pd(f32::from_bits(pg.u32(m, a)), f32::from_bits(pg.u32(m, a.wrapping_add(4)))); }}')
        elif mn == 'cvtsd2ss':
            di = self.xmm_index(i, ops[0])
            s = ops[1]
            if s.type == X86_OP_REG:
                src = f'(((xmm[{self.xmm_index(i, s)}][1].to_bits() as u64) << 32) | xmm[{self.xmm_index(i, s)}][0].to_bits() as u64)'
            else:
                a = self.addr_expr(i, s)
                src = f'(((pg.u32(m, {a}.wrapping_add(4)) as u64) << 32) | pg.u32(m, {a}) as u64)'
            e(f'xmm[{di}][0] = f64::from_bits({src}) as f32;')
        elif mn == 'cvttss2si':
            e(self.put(i, ops[0], f'rt::cvttss2si({self.lane0(i, ops[1])})'))
        elif mn == 'cvtsi2ss':
            di = self.xmm_index(i, ops[0])
            e(f'xmm[{di}][0] = ({self.get(i, ops[1], 32)} as i32) as f32;')
        elif mn == 'imul' and len(ops) == 3:
            e(self.put(i, ops[0], f'(({self.get(i, ops[1], 32)} as i32).wrapping_mul({ops[2].imm} as i32)) as u32'))
        elif mn in ('sete', 'setne') and ops[0].size == 1:
            cond = 'zf' if mn == 'sete' else '!zf'
            e(self.put(i, ops[0], f'({cond}) as u32'))
        elif mn == 'fld' and ops[0].type == X86_OP_MEM and ops[0].size == 4:
            e(f'x87 = {self.get(i, ops[0], 32)};')
        elif mn == 'fstp' and ops[0].type == X86_OP_MEM and ops[0].size == 4:
            e(self.put(i, ops[0], 'rt::x87_copy(x87)'))
        elif mn == 'nop':
            pass
        else:
            raise SystemExit(f'unsupported {hex(i.address)}: {mn} {i.op_str}')
        return L

    def blocks(self):
        leaders = {self.entry}
        for k, i in enumerate(self.insns):
            if i.mnemonic == 'jmp' or i.mnemonic in JCC:
                assert i.operands[0].type == X86_OP_IMM, hex(i.address)
                leaders.add(i.operands[0].imm)
                if k + 1 < len(self.insns):
                    leaders.add(self.insns[k + 1].address)
            if i.mnemonic == 'ret' and k + 1 < len(self.insns):
                leaders.add(self.insns[k + 1].address)
        return sorted(leaders)

    def generate(self, doc):
        leaders = self.blocks()
        o = []
        w = o.append
        w(doc)
        w(f'#[allow(unused_assignments, unused_variables, unused_mut, clippy::all)]')
        w(f'pub fn {self.name}(m: &mut dyn Mem, c: &mut Cpu) {{')
        w('    let pg = Pages::of(m);')
        w('    let (mut eax, mut ecx, mut edx, mut ebx, mut esp, mut ebp, mut esi, mut edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);')
        w('    let mut xmm = c.xmm.map(|v| v.map(f32::from_bits));')
        if self.frame is not None:
            w(f'    let mut fr = [0f32; {self.frame // 4}];')
        w('    let (mut cf, mut zf, mut sf, mut of_, mut pf) = (false, false, false, false, false);')
        w('    let mut x87 = 0u32;')
        w('    //LDDECL')
        w(f'    let mut pc: u32 = 0x{self.entry:08x};')
        w('    loop {')
        w('        #[cfg(feature = "cover")]')
        w('        rt::hit(pc);')
        w('        match pc {')
        lset = set(leaders)
        for k, i in enumerate(self.insns):
            if i.address in lset:
                if k:
                    prev = self.insns[k - 1]
                    if prev.mnemonic not in ('jmp', 'ret'):
                        w(f'                pc = 0x{i.address:08x};')
                        w('                continue;')
                    w('            }')
                w(f'            0x{i.address:08x} => {{')
                self.cache.clear()
            w(f'                // {i.address:08x}: {i.mnemonic} {i.op_str}')
            mn = i.mnemonic
            if mn in JCC:
                w(f'                if {JCC[mn]} {{ pc = 0x{i.operands[0].imm:08x}; continue; }}')
            elif mn == 'jmp':
                w(f'                pc = 0x{i.operands[0].imm:08x};')
                w('                continue;')
            elif mn == 'ret':
                n = i.operands[0].imm if i.operands else 0
                w(f'                esp = esp.wrapping_add(4 + {n});')
                w('                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);')
                w('                c.xmm = xmm.map(|v| v.map(f32::to_bits));')
                w('                return;')
            elif mn == 'call':
                nxt = i.address + i.size
                op = i.operands[0]
                if op.type == X86_OP_IMM:
                    target = f'0x{op.imm:08x}u32'
                else:
                    target = self.get(i, op)
                self.cache.clear()
                w(f'                let target: u32 = {target};')
                w(f'                esp = esp.wrapping_sub(4); pg.set_u32(m, esp, 0x{nxt:08x});')
                w('                (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi) = (eax, ecx, edx, ebx, esp, ebp, esi, edi);')
                w('                c.xmm = xmm.map(|v| v.map(f32::to_bits));')
                w('                call_ext(m, c, target);')
                w('                (eax, ecx, edx, ebx, esp, ebp, esi, edi) = (c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi);')
                w('                xmm = c.xmm.map(|v| v.map(f32::from_bits));')
            else:
                for line in self.tr(i):
                    w('                ' + line)
        w('            }')
        w('            _ => unreachable!("pc {pc:#x}"),')
        w('        }')
        w('    }')
        w('}')
        decl = ' '.join(f'let mut ld{k} = 0u32;' for k in range(self.cache_vars))
        o = [line.replace('    //LDDECL', '    ' + decl) for line in o]
        return '\n'.join(o) + '\n'


def main():
    entry, last = int(sys.argv[1], 16), int(sys.argv[2], 16)
    name, out = sys.argv[3], sys.argv[4]
    fn = Fn(entry, last, name)
    doc = open(sys.argv[5]).read() if len(sys.argv) > 5 else ''
    pops = {}
    want_frame = False
    for a in sys.argv[6:]:
        if a == '--frame':
            want_frame = True
        elif a.startswith('--pops='):
            for kv in a[7:].split(','):
                k, v = kv.split(':')
                pops['indirect' if k == 'indirect' else int(k, 16)] = int(v)
    if want_frame:
        fn.analyse_frame(pops)
    open(out, 'w', newline='\n').write(fn.generate(doc))
    print(f'{len(fn.insns)} instructions, {len(fn.blocks())} blocks -> {out}')


main()
