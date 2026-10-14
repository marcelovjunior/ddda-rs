"""Generate crates/mtcore/src/semantic_s3b.rs: a semantic form of the shadow
cascade view setup 0x00EE5CE0 (2151 instructions of scalar and packed SSE with
a little x87), translated mechanically from the exe's bytes.

What the translation keeps: every instruction's effect on the guest-visible
state (the ABI: ST0 result, the callee-saved registers, ESP, and every guest
write the original makes outside its own stack frame, each guest call with
its registers, stack arguments and xmm inputs). What it drops: the
original's flag bookkeeping (only the flags a later jump reads are computed),
register spills of the emulator, per-instruction dispatch; the function's own
stack frame lives in a host buffer that is written back around guest calls.
SSE arithmetic is the hardware's own instruction (inline asm), so NaN
operand order and rounding are exactly the original's.

Run: E:/ddda-re/tools/pyenv/Scripts/python.exe research/native/gen_cascade_view.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from capstone import Cs, CS_ARCH_X86, CS_MODE_32  # noqa: E402
from capstone.x86 import X86_OP_REG, X86_OP_IMM, X86_OP_MEM  # noqa: E402

START, END = 0x00EE5CE0, 0x00EE8B20
PROLOGUE_END = 0x00EE5CEC  # first instruction translated: after push ebp / mov ebp, esp / and esp, -16 / sub esp, 0x294
EPILOGUE = (0x00EE8B1A, 0x00EE8B1D)  # mov esp,ebp / pop ebp: replaced by the exit code (the register pops stay)
import os
TRACE = bool(os.environ.get('CASCADE_TRACE'))
OUT = Path(__file__).resolve().parents[2] / ('crates/mtcore/src/semantic_s3b.rs' if not TRACE else 'crates/mtcore/src/semantic_s3b_trace.rs')

md = Cs(CS_ARCH_X86, CS_MODE_32)
md.detail = True
instructions = list(md.disasm(h.code(START, END - START), START))
by_address = {i.address: i for i in instructions}

GPR32 = {'eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp', 'esp'}


def reg(name):
    if name in GPR32:
        return name
    raise NotImplementedError(name)


def xmm(name):
    assert name.startswith('xmm')
    return int(name[3:])


def address(op):
    """Rust expression for a memory operand's guest address."""
    mem = op.mem
    parts = []
    if mem.base:
        parts.append(f'{insn_reg(mem.base)}')
    if mem.index:
        scale = mem.scale
        parts.append(f'{insn_reg(mem.index)}.wrapping_mul({scale})' if scale != 1 else f'{insn_reg(mem.index)}')
    expr = parts[0] if parts else '0u32'
    for part in parts[1:]:
        expr = f'{expr}.wrapping_add({part})'
    if mem.disp or not parts:
        disp = mem.disp & 0xFFFFFFFF
        expr = f'{expr}.wrapping_add({disp:#x})' if parts else f'{disp:#x}u32'
    return expr


def insn_reg(reg_id):
    return md.reg_name(reg_id)


def lane0(i):
    return f'x[{i}]'


# ----------------------------------------------------------------------------
# flag helpers (emitted inline)
def flags_cmp(a, b):
    return (f'{{ let (a, b) = ({a}, {b}); let r = a.wrapping_sub(b); cf = a < b; zf = r == 0; sf = r >> 31 != 0; '
            f'of = ((a ^ b) & (a ^ r)) >> 31 != 0; pf = (r as u8).count_ones() % 2 == 0; }}')


def flags_cmp8(a, b):
    return (f'{{ let (a, b) = ({a} as u8, {b} as u8); let r = a.wrapping_sub(b); cf = a < b; zf = r == 0; sf = r >> 7 != 0; '
            f'of = ((a ^ b) & (a ^ r)) >> 7 != 0; pf = r.count_ones() % 2 == 0; }}')


def flags_logic(r):
    return f'{{ let r: u32 = {r}; cf = false; of = false; zf = r == 0; sf = r >> 31 != 0; pf = (r as u8).count_ones() % 2 == 0; }}'


def flags_logic8(r):
    return f'{{ let r: u8 = {r}; cf = false; of = false; zf = r == 0; sf = r >> 7 != 0; pf = r.count_ones() % 2 == 0; }}'


def flags_add(a, b):
    return (f'{{ let (a, b) = ({a}, {b}); let r = a.wrapping_add(b); cf = r < a; zf = r == 0; sf = r >> 31 != 0; '
            f'of = (!(a ^ b) & (a ^ r)) >> 31 != 0; pf = (r as u8).count_ones() % 2 == 0; }}')


def flags_incdec(old, new, inc):
    overflow = f'{old} == 0x7fff_ffff' if inc else f'{old} == 0x8000_0000'
    return f'{{ let r: u32 = {new}; zf = r == 0; sf = r >> 31 != 0; of = {overflow}; pf = (r as u8).count_ones() % 2 == 0; }}'


# stack words each guest call reads (the ones by value); 0x00CFD5E0 reads through pointers into the frame
CALL_ARGS = {0x00FED950: 0, 0x00FED940: 0, 0x00FED920: 0, 0x012F2F20: 0, 0x012F3690: 2, 0x00CFD5E0: 3, 0x00CFC8B0: 4}

CONDITIONS = {
    'ja': '!cf && !zf', 'jbe': 'cf || zf', 'jnc': '!cf', 'jb': 'cf', 'jns': '!sf', 'js': 'sf', 'jnz': '!zf', 'jz': 'zf',
    'jp': 'pf', 'jnp': '!pf', 'jae': '!cf', 'jne': '!zf', 'je': 'zf',
}

SSE_SCALAR = {'mulss': 'mulss', 'addss': 'addss', 'subss': 'subss', 'divss': 'divss'}
SSE_PACKED = {'mulps': 'mulps', 'addps': 'addps', 'subps': 'subps', 'minps': 'minps', 'maxps': 'maxps'}


def emit_instruction(i, nxt):
    """Rust statements for one instruction; `nxt` is the fall-through address."""
    m, ops = i.mnemonic, i.operands
    o = i.op_str
    out = []
    emit = out.append
    size = ops[0].size if ops else 4

    def src128(op):
        if op.type == X86_OP_REG:
            return f'x[{xmm(insn_reg(op.reg))}]'
        return f'rd128!({address(op)})'

    def src32f(op):
        if op.type == X86_OP_REG:
            return f'x[{xmm(insn_reg(op.reg))}]'
        return f'ss!(rd32!({address(op)}))'

    if m == 'mov':
        d, s = ops
        if d.type == X86_OP_REG and d.size == 4:
            if s.type == X86_OP_REG:
                emit(f'{reg(insn_reg(d.reg))} = {reg(insn_reg(s.reg))};')
            elif s.type == X86_OP_IMM:
                emit(f'{reg(insn_reg(d.reg))} = {s.imm & 0xFFFFFFFF:#x};')
            else:
                emit(f'{reg(insn_reg(d.reg))} = rd32!({address(s)});')
        elif d.type == X86_OP_MEM and d.size == 4:
            value = reg(insn_reg(s.reg)) if s.type == X86_OP_REG else f'{s.imm & 0xFFFFFFFF:#x}'
            emit(f'wr32!({address(d)}, {value});')
        elif d.type == X86_OP_REG and d.size == 1 and insn_reg(d.reg) == 'bl':
            emit(f'ebx = (ebx & !0xff) | {s.imm & 0xff:#x};')
        else:
            raise NotImplementedError(f'{i.address:x} {m} {o}')
    elif m == 'lea':
        emit(f'{reg(insn_reg(ops[0].reg))} = {address(ops[1])};')
    elif m in ('add', 'sub', 'xor', 'and', 'cmp', 'test', 'inc', 'dec'):
        emit_integer(i, emit)
    elif m == 'push':
        emit(f'esp = esp.wrapping_sub(4); wr32!(esp, {reg(insn_reg(ops[0].reg))});')
    elif m == 'pop':
        emit(f'{reg(insn_reg(ops[0].reg))} = rd32!(esp); esp = esp.wrapping_add(4);')
    elif m == 'nop':
        pass
    elif m == 'lahf':
        emit('{ let ah = ((sf as u32) << 7) | ((zf as u32) << 6) | ((pf as u32) << 2) | 2 | (cf as u32); eax = (eax & !0xff00) | (ah << 8); }')
    elif m in ('comiss', 'ucomiss'):
        a = lane0_f(ops[0])
        b = src32_lane(ops[1])
        emit(f'{{ let (a, b) = ({a}, {b}); of = false; sf = false; if a.is_nan() || b.is_nan() {{ zf = true; pf = true; cf = true; }} '
             f'else if a > b {{ zf = false; pf = false; cf = false; }} else if a < b {{ zf = false; pf = false; cf = true; }} '
             f'else {{ zf = true; pf = false; cf = false; }} }}')
    elif m in SSE_SCALAR:
        d = xmm(insn_reg(ops[0].reg))
        emit(f'{{ let s = {src32f(ops[1])}; sse2!("{SSE_SCALAR[m]}", x[{d}], s); }}')
    elif m == 'sqrtss':
        d = xmm(insn_reg(ops[0].reg))
        emit(f'{{ let s = {src32f(ops[1])}; sse2!("sqrtss", x[{d}], s); }}')
    elif m in SSE_PACKED:
        d = xmm(insn_reg(ops[0].reg))
        emit(f'{{ let s = {src128(ops[1])}; sse2!("{SSE_PACKED[m]}", x[{d}], s); }}')
    elif m == 'cvtps2pd':
        d = xmm(insn_reg(ops[0].reg))
        emit(f'{{ let s = {src128(ops[1])}; sse2!("cvtps2pd", x[{d}], s); }}')
    elif m == 'shufps':
        d = xmm(insn_reg(ops[0].reg))
        emit(f'{{ let s = {src128(ops[1])}; sse_shufps!(x[{d}], s, {ops[2].imm}); }}')
    elif m == 'movss':
        d, s = ops
        if d.type == X86_OP_REG and s.type == X86_OP_MEM:
            emit(f'x[{xmm(insn_reg(d.reg))}] = ss!(rd32!({address(s)}));')
        elif d.type == X86_OP_REG and s.type == X86_OP_REG:
            dst, src = xmm(insn_reg(d.reg)), xmm(insn_reg(s.reg))
            emit(f'x[{dst}] = from_bits128((bits128(x[{dst}]) & !0xffff_ffffu128) | (bits128(x[{src}]) & 0xffff_ffffu128));')
        elif d.type == X86_OP_MEM and s.type == X86_OP_REG:
            emit(f'wr32!({address(d)}, lane0_bits(x[{xmm(insn_reg(s.reg))}]));')
        else:
            raise NotImplementedError(f'{i.address:x} {m} {o}')
    elif m == 'movaps':
        d, s = ops
        if d.type == X86_OP_REG and s.type == X86_OP_REG:
            emit(f'x[{xmm(insn_reg(d.reg))}] = x[{xmm(insn_reg(s.reg))}];')
        elif d.type == X86_OP_REG:
            emit(f'x[{xmm(insn_reg(d.reg))}] = rd128!({address(s)});')
        else:
            emit(f'wr128!({address(d)}, x[{xmm(insn_reg(s.reg))}]);')
    elif m == 'movsd':
        d, s = ops
        assert d.type == X86_OP_MEM and s.type == X86_OP_REG
        emit(f'wr64!({address(d)}, lane01_bits(x[{xmm(insn_reg(s.reg))}]));')
    elif m in ('xorps', 'andps'):
        d, s = ops
        symbol = '^' if m == 'xorps' else '&'
        dst = xmm(insn_reg(d.reg))
        emit(f'x[{dst}] = from_bits128(bits128(x[{dst}]) {symbol} bits128({src128(s)}));')
    elif m in ('fild', 'fadd', 'fmul', 'fdivr', 'fdiv', 'fst', 'fstp', 'fld', 'fdivrp'):
        emit_x87(i, emit)
    elif m == 'jmp':
        emit(f'pc = {ops[0].imm:#x}; continue;')
    elif m in CONDITIONS:
        emit(f'pc = if {CONDITIONS[m]} {{ {ops[0].imm:#x} }} else {{ {nxt:#x} }}; continue;')
    elif m == 'call':
        emit(f'call_guest!({ops[0].imm:#x}, {nxt:#x}, {CALL_ARGS[ops[0].imm]});')
    elif m == 'ret':
        emit('break \'run;')
    else:
        raise NotImplementedError(f'{i.address:x} {m} {o}')
    return out


def lane0_f(op):
    return f'lane0_f32(x[{xmm(insn_reg(op.reg))}])'


def src32_lane(op):
    if op.type == X86_OP_REG:
        return f'lane0_f32(x[{xmm(insn_reg(op.reg))}])'
    return f'f32::from_bits(rd32!({address(op)}))'


def operand_value(op, width=4):
    if op.type == X86_OP_REG:
        name = insn_reg(op.reg)
        if name in GPR32:
            return name
        if name == 'bl':
            return '(ebx & 0xff)'
        if name == 'ah':
            return '((eax >> 8) & 0xff)'
        raise NotImplementedError(name)
    if op.type == X86_OP_IMM:
        return f'{op.imm & (0xFFFFFFFF if width == 4 else 0xFF):#x}'
    if width == 1:
        return f'(rd8!({address(op)}) as u32)'
    return f'rd32!({address(op)})'


def emit_integer(i, emit):
    m, ops = i.mnemonic, i.operands
    if m in ('inc', 'dec'):
        r = reg(insn_reg(ops[0].reg))
        new = f'{r}.wrapping_add(1)' if m == 'inc' else f'{r}.wrapping_sub(1)'
        emit(f'{{ let old = {r}; {r} = {new}; {flags_incdec("old", r, m == "inc")} }}')
        return
    d, s = ops[0], ops[1]
    width = d.size
    if m == 'cmp':
        a, b = operand_value(d, width), operand_value(s, width)
        emit(flags_cmp8(a, b) if width == 1 else flags_cmp(a, b))
    elif m == 'test':
        a, b = operand_value(d, width), operand_value(s, width)
        emit(flags_logic8(f'({a} & {b}) as u8') if width == 1 else flags_logic(f'{a} & {b}'))
    elif width == 1 and d.type == X86_OP_REG:
        name = insn_reg(d.reg)
        assert name == 'bl' and m == 'xor', (i.address, m, name)
        emit('ebx = ebx & !0xff; zf = true; cf = false; of = false; sf = false; pf = true;')
    else:
        r = reg(insn_reg(d.reg))
        value = operand_value(s, 4)
        if m == 'add':
            emit(f'{{ let b = {value}; {flags_add(r, "b")} {r} = {r}.wrapping_add(b); }}')
        elif m == 'sub':
            emit(f'{{ let b = {value}; {flags_cmp(r, "b")} {r} = {r}.wrapping_sub(b); }}')
        elif m == 'xor':
            emit(f'{{ let b = {value}; {r} ^= b; {flags_logic(r)} }}')
        elif m == 'and':
            emit(f'{{ let b = {value}; {r} &= b; {flags_logic(r)} }}')
        else:
            raise NotImplementedError(f'{i.address:x} {m}')


def emit_x87(i, emit):
    b = i.bytes
    opcode = b[0]
    modrm = b[1]
    memory = modrm < 0xC0
    op = opcode | ((modrm & 0x38) << 8 if memory else modrm << 8)
    if memory:
        a = address(i.operands[0])
        reads = i.mnemonic in ('fild', 'fadd', 'fmul', 'fdivr', 'fdiv', 'fld')
        if reads:
            emit(f'{{ let mut d = [0u8; 10]; d[..4].copy_from_slice(&rd32!({a}).to_le_bytes()); c.st.exec::<{op:#06x}>(&mut d); }}')
        else:
            emit(f'{{ let mut d = [0u8; 10]; c.st.exec::<{op:#06x}>(&mut d); wr32!({a}, u32::from_le_bytes([d[0], d[1], d[2], d[3]])); }}')
    else:
        emit(f'{{ let mut d = [0u8; 10]; c.st.exec::<{op:#06x}>(&mut d); }}')


# ----------------------------------------------------------------------------
def main():
    leaders = {PROLOGUE_END}
    for i in instructions:
        if i.address < PROLOGUE_END:
            continue
        if i.mnemonic == 'jmp' or i.mnemonic in CONDITIONS:
            leaders.add(i.operands[0].imm)
            leaders.add(i.address + i.size)
        if i.mnemonic == 'call':
            pass
    leaders = sorted(a for a in leaders if a in by_address and a < END)
    body = []
    addresses = [i.address for i in instructions if i.address >= PROLOGUE_END]
    for index, start in enumerate(leaders):
        body.append(f'            {start:#010x} => {{')
        a = start
        while a < END:
            i = by_address[a]
            nxt = a + i.size
            if EPILOGUE[0] <= a < EPILOGUE[1]:
                a = nxt
                continue
            lines = emit_instruction(i, nxt)
            body.append(f'                // {a:08x}: {i.mnemonic} {i.op_str}')
            if TRACE:
                body.append(f'                trace!({a:#x});')
            for line in lines:
                body.append('                ' + line)
            if i.mnemonic in ('jmp', 'ret') or i.mnemonic in CONDITIONS:
                break
            if nxt in leaders:
                body.append(f'                pc = {nxt:#x}; continue;')
                break
            a = nxt
        body.append('            }')
    text = HEADER + '\n'.join(body) + FOOTER
    OUT.write_text(text)
    print(f'{len(instructions)} instructions, {len(leaders)} blocks -> {OUT}')


HEADER = r'''//! Semantic form of 0x00EE5CE0, the shadow cascade view setup, generated by
//! research/native/gen_cascade_view.py from the exe's instructions (see that
//! file for what the translation keeps and drops). Do not edit by hand.
#![allow(unused_assignments, unused_variables, unused_mut, unused_macros, clippy::all)]
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};
use std::arch::asm;
use std::arch::x86_64::{__m128, _mm_set_ss};

/// Bytes of the original's stack frame kept in a host buffer: its locals,
/// the registers it pushes and the arguments it pushes for calls.
const WINDOW: usize = 0x3c0;

struct Window {
    base: u32,
    bytes: [u8; WINDOW],
}

impl Window {
    #[inline(always)]
    fn offset(&self, address: u32, width: usize) -> Option<usize> {
        let offset = address.wrapping_sub(self.base) as usize;
        (offset + width <= WINDOW).then_some(offset)
    }
}

/// `memcpy` into guest memory: in-page runs through the page table, the rest byte by byte.
fn store_guest(m: &mut dyn Mem, pg: Pages, address: u32, bytes: &[u8]) {
    let mut done = 0usize;
    while done < bytes.len() {
        let at = address.wrapping_add(done as u32);
        let run = ((4096 - (at & 4095)) as usize).min(bytes.len() - done);
        if let Some(target) = pg.bytes(at, run, ddnative_base::mem::WRITE) {
            let mut i = 0usize;
            while i + 4 <= run && (target as usize + i) & 3 == 0 {
                let word = u32::from_le_bytes([bytes[done + i], bytes[done + i + 1], bytes[done + i + 2], bytes[done + i + 3]]);
                // SAFETY: `bytes()` returned `run` writable bytes inside one backed page
                unsafe { ddnative_base::mem::store_at(target.add(i), 4, word) };
                i += 4;
            }
            while i < run {
                // SAFETY: as above
                unsafe { ddnative_base::mem::store_at(target.add(i), 1, bytes[done + i] as u32) };
                i += 1;
            }
        } else {
            for i in 0..run {
                m.set_u8(at.wrapping_add(i as u32), bytes[done + i]);
            }
        }
        done += run;
    }
}

#[inline(always)]
fn lane0_f32(v: __m128) -> f32 {
    f32::from_bits(lane0_bits(v))
}
#[inline(always)]
fn lane0_bits(v: __m128) -> u32 {
    // SAFETY: __m128 is 16 bytes
    unsafe { std::mem::transmute::<__m128, [u32; 4]>(v)[0] }
}
#[inline(always)]
fn lane01_bits(v: __m128) -> u64 {
    // SAFETY: __m128 is 16 bytes
    unsafe { std::mem::transmute::<__m128, [u64; 2]>(v)[0] }
}
#[inline(always)]
fn bits128(v: __m128) -> u128 {
    // SAFETY: __m128 is 16 bytes
    unsafe { std::mem::transmute::<__m128, u128>(v) }
}
#[inline(always)]
fn from_bits128(v: u128) -> __m128 {
    // SAFETY: __m128 is 16 bytes
    unsafe { std::mem::transmute::<u128, __m128>(v) }
}

// Translated from 0x00ee5ce0 (DDDA.exe 2017-12-13 Steam).
/// 0x00EE5CE0, `light->setup_cascade_view(a, b, c, source, mode, count) -> ST0`
/// (thiscall, `ret 0x18`, result in ST0): the per-cascade view setup of the
/// shadow pass. From the light's direction (`+0x160..+0x168`) it builds an
/// orthonormal basis, reads the source camera's matrices through
/// 0x00FED950 / 0x00FED940 / 0x00FED920, fits the view volume (packed
/// transforms of the frustum corners, min / max reductions, the cascade
/// split distances through the CRT `pow` 0x012F2F20 and `tan` 0x012F3690),
/// fills the three output matrices through 0x00CFD5E0 / 0x00CFC8B0 and
/// leaves the near-plane value in ST0.
pub fn setup_cascade_view(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame: push ebp; and esp, -16; sub esp, 0x294
    let aligned = entry.wrapping_sub(4) & !15;
    let mut esp = aligned.wrapping_sub(0x294);
    let ebp = entry.wrapping_sub(4);
    let mut win = Window { base: aligned.wrapping_sub(WINDOW as u32), bytes: [0u8; WINDOW] };
    let (mut eax, mut ecx, mut edx) = (c.eax, c.ecx, c.edx);
    let (mut ebx, mut esi, mut edi) = (c.ebx, c.esi, c.edi);
    let mut x: [__m128; 8] = [from_bits128(0); 8];
    let (mut cf, mut zf, mut pf, mut sf, mut of) = (false, false, false, false, false);

    macro_rules! rd32 { ($a:expr) => {{ let a: u32 = $a; match win.offset(a, 4) { Some(o) => u32::from_le_bytes([win.bytes[o], win.bytes[o + 1], win.bytes[o + 2], win.bytes[o + 3]]), None => pg.u32(&*m, a) } }}; }
    macro_rules! rd8 { ($a:expr) => {{ let a: u32 = $a; match win.offset(a, 1) { Some(o) => win.bytes[o], None => pg.u8(&*m, a) } }}; }
    macro_rules! rd128 { ($a:expr) => {{ let a: u32 = $a; match win.offset(a, 16) { Some(o) => { let mut b = [0u8; 16]; b.copy_from_slice(&win.bytes[o..o + 16]); from_bits128(u128::from_le_bytes(b)) } None => { let mut b = [0u8; 16]; m.read_bytes(a, &mut b); from_bits128(u128::from_le_bytes(b)) } } }}; }
    macro_rules! wr32 { ($a:expr, $v:expr) => {{ let (a, v): (u32, u32) = ($a, $v); match win.offset(a, 4) { Some(o) => win.bytes[o..o + 4].copy_from_slice(&v.to_le_bytes()), None => pg.set_u32(m, a, v) } }}; }
    macro_rules! wr64 { ($a:expr, $v:expr) => {{ let (a, v): (u32, u64) = ($a, $v); match win.offset(a, 8) { Some(o) => win.bytes[o..o + 8].copy_from_slice(&v.to_le_bytes()), None => { pg.set_u32(m, a, v as u32); pg.set_u32(m, a.wrapping_add(4), (v >> 32) as u32); } } }}; }
    macro_rules! wr128 { ($a:expr, $v:expr) => {{ let (a, v): (u32, __m128) = ($a, $v); let b = bits128(v).to_le_bytes(); match win.offset(a, 16) { Some(o) => win.bytes[o..o + 16].copy_from_slice(&b), None => store_guest(m, pg, a, &b) } }}; }
    macro_rules! ss { ($bits:expr) => { unsafe { _mm_set_ss(f32::from_bits($bits)) } }; }
    macro_rules! sse2 {
        ($op:literal, $d:expr, $s:expr) => {{
            let mut d = $d;
            let s = $s;
            // SAFETY: a register-only SSE instruction; MXCSR is the live one, as in the original
            unsafe { asm!(concat!($op, " {0}, {1}"), inout(xmm_reg) d, in(xmm_reg) s, options(pure, nomem, nostack, preserves_flags)) };
            $d = d;
        }};
    }
    macro_rules! sse_shufps {
        ($d:expr, $s:expr, $imm:literal) => {{
            let mut d = $d;
            let s = $s;
            // SAFETY: as above
            unsafe { asm!("shufps {0}, {1}, {imm}", inout(xmm_reg) d, in(xmm_reg) s, imm = const $imm, options(pure, nomem, nostack, preserves_flags)) };
            $d = d;
        }};
    }
    // a guest call: write the frame back, push the return address, run the
    // callee with the registers and the xmm inputs, read the results back
    macro_rules! call_guest {
        ($target:expr, $next:expr, $args:expr) => {{
            let low = esp.wrapping_sub(win.base) as usize;
            // the callees that take pointers into the frame (0x00CFD5E0) see all of it, the others only their stack arguments
            let pointers = $target == 0x00cfd5e0;
            let top = if pointers { WINDOW } else { (low + 4 * $args).min(WINDOW) };
            store_guest(m, pg, esp, &win.bytes[low.min(WINDOW)..top]);
            pg.set_u32(m, esp.wrapping_sub(4), $next);
            c.esp = esp.wrapping_sub(4);
            (c.eax, c.ecx, c.edx, c.ebx, c.esi, c.edi) = (eax, ecx, edx, ebx, esi, edi);
            for k in 0..8 {
                // SAFETY: __m128 is four u32 lanes
                c.xmm[k] = unsafe { std::mem::transmute::<__m128, [u32; 4]>(x[k]) };
            }
            call_ext(m, c, $target);
            (eax, ecx, edx) = (c.eax, c.ecx, c.edx);
            for k in 0..8 {
                // SAFETY: as above
                x[k] = unsafe { std::mem::transmute::<[u32; 4], __m128>(c.xmm[k]) };
            }
            esp = c.esp;
            let low = esp.wrapping_sub(win.base) as usize;
            if pointers && low < WINDOW {
                m.read_bytes(esp, &mut win.bytes[low..]);
            }
        }};
    }

    macro_rules! trace { ($a:expr) => { if std::env::var_os("CASCADE_TRACE").is_some() { eprintln!("T {:x} {:08x} {:08x} {:08x} {:08x} {:08x} | {}", $a, eax, ecx, edx, esi, edi, (0..8).map(|k| format!("{:08x}", lane0_bits(x[k]))).collect::<Vec<_>>().join(" ")); } }; }
    let mut pc: u32 = 0x00ee5cec;
    'run: loop {
        match pc {
'''

FOOTER = r'''
            other => unreachable!("block {other:#x}"),
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = (ebx, saved.1, esi, edi);
    c.eax = eax;
    c.esp = entry.wrapping_add(4 + 0x18);
}
'''

if __name__ == '__main__':
    main()
