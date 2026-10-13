//! The frame's command executor of nDraw: thiscall `sRender::execute()`.
//! Translated from 0x00DAB480 (DDDA.exe 2017-12-13 Steam); the render
//! worker 0x00DAC090 calls it once a frame (docs/NDRAW.md, section 3).
//!
//! sRender keeps the frame's commands as lists of 8-byte entries whose
//! second word points to a command record. The executor walks them
//! depth first: a stack of (owner, next entry, end) ranges in its context
//! (built by 0x00DA6DA0), the root list first; a nested-list command
//! pushes a range, an exhausted range pops. For every record it applies
//! the record's targets, fixed states, streams and shaders (the four apply
//! functions, which skip what is already applied) and makes the Direct3D 9
//! calls (BeginScene/EndScene around the frame).
//!
//! Command types (the low nibble of the record's half-word at +2):
//!
//! | type | command |
//! |---|---|
//! | 0 | DrawPrimitive |
//! | 1 | DrawIndexedPrimitive; with sRender+0x30 set, consecutive draws over one index range merge into one call |
//! | 2 | instanced DrawPrimitive (stream frequencies set around the draw) |
//! | 3 | instanced DrawIndexedPrimitive |
//! | 4 | Clear (or, when the colour is not exactly representable, the record's fallback draw) |
//! | 5 | StretchRect between a target and a surface (a following identical copy is merged) |
//! | 6 | a nested command list |
//! | 7 | 0x011481B0 / 0x011481F0 on an object (begin/end of a query span) |
//! | 8-15 | nothing |
//!
//! This port keeps the original's machine behaviour exactly (the contract
//! of crates/mtcore's ports): the same registers, xmm and x87 state at
//! every call and at the return, the same frame stores at the same ESP
//! offsets (the apply functions read the context inside this frame), and
//! every call made as the lifted body makes it (`x86::call_ext`, and
//! `x86::call_lifted` for 0x0067FB40), with the original's return
//! addresses. The frame offsets below are the original's ESP offsets with
//! no argument pushed.

use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use mtcore::machine::{call_out, pop, push};

// --- the functions it calls ----------------------------------------------------

/// thiscall `begin(context*)`: zero the context, take the device and the
/// frame's root command list, set the frame's initial render states.
const BEGIN_CONTEXT: u32 = 0x00DA_6DA0;
/// `end(context*)`: unbind targets, streams, shaders and textures.
const END_CONTEXT: u32 = 0x00DA_6EA0;
/// thiscall `apply(context*, object*)` on sRender: render and depth targets.
const APPLY_TARGETS: u32 = 0x00DA_6FE0;
/// ... fixed-function states (blend, depth-stencil, rasterizer, samplers).
const APPLY_STATES: u32 = 0x00DA_A5F0;
/// ... vertex streams and indices.
const APPLY_STREAMS: u32 = 0x00DA_A7C0;
/// ... vertex declaration, shaders and their constants.
const APPLY_SHADERS: u32 = 0x00DA_A8B0;
/// thiscall `check(HRESULT, const char *expression)`: reports a failed call.
const CHECK_RESULT: u32 = 0x00DA_89B0;
/// thiscall `bool equal(const RECT *)`: two rectangles.
const RECT_EQUAL: u32 = 0x0067_FB40;
/// fastcall on the object of a type-7 command (+4 zero / non-zero).
const SPAN_BEGIN: u32 = 0x0114_81B0;
const SPAN_END: u32 = 0x0114_81F0;
/// The sRender singleton (docs/NDRAW.md).
const S_RENDER: u32 = 0x018D_08F4;
/// The expressions CHECK_RESULT reports (strings in the exe).
const STRETCH_RECT_RECTS: u32 = 0x0142_93B8;
const STRETCH_RECT_WHOLE: u32 = 0x0142_9368;
/// float constants in the exe: 1.0, 255.0, 1/255.
const ONE: u32 = 0x0140_6CE0;
const BYTE_MAX: u32 = 0x0161_C298;
const INV_BYTE_MAX: u32 = 0x0161_C29C;

// --- IDirect3DDevice9 vtable offsets ---------------------------------------------

const STRETCH_RECT: u32 = 0x88;
const BEGIN_SCENE: u32 = 0xA4;
const END_SCENE: u32 = 0xA8;
const CLEAR: u32 = 0xAC;
const DRAW_PRIMITIVE: u32 = 0x144;
const DRAW_INDEXED_PRIMITIVE: u32 = 0x148;
const SET_STREAM_SOURCE: u32 = 0x190;
const SET_STREAM_SOURCE_FREQ: u32 = 0x198;

// --- frame slots (ESP offsets with no argument pushed) ----------------------------

/// a stream object (the instanced draws), the Clear's fallback draw, a
/// StretchRect's next entry, then its target index
const SLOT_C: u32 = 0x0C;
/// scratch: a colour byte, a StretchRect cursor and surface, a mask
const SLOT_10: u32 = 0x10;
/// the pending merged draw's primitive type, then its start index
const BATCH_START: u32 = 0x14;
/// sRender (`this`)
const RENDER: u32 = 0x18;
/// the pending merged draw's index count
const BATCH_COUNT: u32 = 0x1C;
/// sRender+0x30 (byte): merge consecutive indexed draws
const MERGE: u32 = 0x23;
/// the pending merged draw's record (0: none)
const BATCH: u32 = 0x24;
/// the Clear's packed colour bytes
const COLOUR: u32 = 0x28;
/// the Clear's alpha and its colour bytes back as floats
const ALPHA: u32 = 0x2C;
const RED_F: u32 = 0x30;
const GREEN_F: u32 = 0x34;
const BLUE_F: u32 = 0x38;
/// the StretchRect destination rectangle (left, top, right, bottom)
const DEST_RECT: u32 = 0x48;
/// the executor's context (0xE28 bytes) for the apply functions
const CONTEXT: u32 = 0x58;
/// the context's render targets (four objects)
const TARGETS: u32 = 0xE0;
/// the range stack: per level (owner, next entry, end), 12 bytes
const RANGES: u32 = 0x274;
/// the range stack's depth
const DEPTH: u32 = 0xE74;
/// the vertex count of the bound streams (NumVertices = this - base)
const VERTICES: u32 = 0xE78;

// --- helpers -----------------------------------------------------------------------

fn at(cpu: &Cpu, off: u32) -> u32 {
    cpu.esp.wrapping_add(off)
}

/// `call target` with the given return address, through the dispatcher.
fn call(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, return_to: u32) {
    call_out(m, cpu, target, return_to);
}


/// The D3DPRIMITIVETYPE and primitive count of `count` vertices/indices:
/// the original's `DEC; CMP 3; JA; JMP table` as (EAX, ECX), with EDX as
/// the division leaves it (`MUL` by 0xAAAAAAAB for the triangle list).
fn primitives(cpu: &mut Cpu, code: u32, count: u32) {
    cpu.edx = code.wrapping_sub(1);
    cpu.eax = 1;
    cpu.ecx = count;
    match cpu.edx {
        // line list
        0 => {
            cpu.ecx >>= 1;
            cpu.eax = 2;
        }
        // line strip
        1 => {
            cpu.ecx = cpu.ecx.wrapping_sub(1);
            cpu.eax = 3;
        }
        // triangle list: count / 3
        2 => {
            cpu.edx = ((0xAAAA_AAABu64 * cpu.ecx as u64) >> 32) as u32 >> 1;
            cpu.ecx = cpu.edx;
            cpu.eax = 4;
        }
        // triangle strip
        3 => {
            cpu.ecx = cpu.ecx.wrapping_sub(2);
            cpu.eax = 5;
        }
        // anything else: a point list of `count`
        _ => {}
    }
}

/// The four apply calls of a record, as the original makes them: each
/// object pushed, then the context (`lea`), ECX = sRender held in EBX. The
/// registers rotate EDX, EAX, ECX, ... (types 1 and 3).
fn apply_all(m: &mut dyn Mem, cpu: &mut Cpu, record: u32, returns: [u32; 4]) {
    cpu.edx = m.u32(record.wrapping_add(4));
    cpu.ebx = m.u32(at(cpu, RENDER));
    push(m, cpu, cpu.edx);
    cpu.eax = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.eax);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_TARGETS, returns[0]);
    cpu.ecx = m.u32(record.wrapping_add(8));
    push(m, cpu, cpu.ecx);
    cpu.edx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.edx);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_STATES, returns[1]);
    cpu.eax = m.u32(record.wrapping_add(0x10));
    push(m, cpu, cpu.eax);
    cpu.ecx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.ecx);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_STREAMS, returns[2]);
    cpu.edx = m.u32(record.wrapping_add(0xC));
    push(m, cpu, cpu.edx);
    cpu.eax = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.eax);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_SHADERS, returns[3]);
}

/// The same four calls with the registers rotated EAX, ECX, EDX, ...
/// (types 0 and 2).
fn apply_all_alt(m: &mut dyn Mem, cpu: &mut Cpu, record: u32, returns: [u32; 4]) {
    cpu.eax = m.u32(record.wrapping_add(4));
    cpu.ebx = m.u32(at(cpu, RENDER));
    push(m, cpu, cpu.eax);
    cpu.ecx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.ecx);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_TARGETS, returns[0]);
    cpu.edx = m.u32(record.wrapping_add(8));
    push(m, cpu, cpu.edx);
    cpu.eax = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.eax);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_STATES, returns[1]);
    cpu.ecx = m.u32(record.wrapping_add(0x10));
    push(m, cpu, cpu.ecx);
    cpu.edx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.edx);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_STREAMS, returns[2]);
    cpu.eax = m.u32(record.wrapping_add(0xC));
    push(m, cpu, cpu.eax);
    cpu.ecx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.ecx);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_SHADERS, returns[3]);
}

/// `IDirect3DDevice9::DrawPrimitive(type = EAX, start = record+0x18,
/// count = ECX)` as the original pushes it (EDX = the method).
fn draw_primitive(m: &mut dyn Mem, cpu: &mut Cpu, record: u32, return_to: u32) {
    cpu.edx = m.u32(cpu.edi);
    cpu.edx = m.u32(cpu.edx.wrapping_add(DRAW_PRIMITIVE));
    push(m, cpu, cpu.ecx);
    cpu.ecx = m.u32(record.wrapping_add(0x18));
    push(m, cpu, cpu.ecx);
    push(m, cpu, cpu.eax);
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.edx, return_to);
}

/// `DrawIndexedPrimitive(type = EAX, base = record+0x1C, min 0,
/// vertices = VERTICES - base, start = record+0x18, count = ECX)`
/// (EBX = the vtable, EDX = the method).
fn draw_indexed(m: &mut dyn Mem, cpu: &mut Cpu, record: u32, return_to: u32) {
    cpu.edx = m.u32(record.wrapping_add(0x1C));
    cpu.ebx = m.u32(cpu.edi);
    push(m, cpu, cpu.ecx);
    cpu.ecx = m.u32(record.wrapping_add(0x18));
    push(m, cpu, cpu.ecx);
    cpu.ecx = m.u32(at(cpu, VERTICES + 8)).wrapping_sub(cpu.edx);
    push(m, cpu, cpu.ecx);
    push(m, cpu, 0);
    push(m, cpu, cpu.edx);
    cpu.edx = m.u32(cpu.ebx.wrapping_add(DRAW_INDEXED_PRIMITIVE));
    push(m, cpu, cpu.eax);
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.edx, return_to);
}

/// Where the loop goes next.
enum Next {
    /// 0x00DAB4FD: reload the depth, then fetch
    Reload,
    /// 0x00DAB504: fetch with the depth in EBX
    Fetch,
}

/// thiscall `sRender::execute()`: ECX = sRender.
pub fn execute(m: &mut dyn Mem, cpu: &mut Cpu) {
    // prologue: a 16-byte aligned frame of 0xE74 bytes
    push(m, cpu, cpu.ebp);
    cpu.ebp = cpu.esp;
    cpu.esp &= 0xFFFF_FFF0;
    cpu.esp = cpu.esp.wrapping_sub(0xE74);
    push(m, cpu, cpu.ebx);
    cpu.ebx = 0;
    push(m, cpu, cpu.esi);
    push(m, cpu, cpu.edi);
    cpu.esi = cpu.ecx;
    cpu.edi = m.u32(cpu.esi.wrapping_add(0x98)); // the device
    cpu.eax = at(cpu, CONTEXT);
    push(m, cpu, cpu.eax);
    m.set_u32(at(cpu, RENDER + 4), cpu.esi);
    // eight words of the context, zeroed again by BEGIN_CONTEXT's memset
    for k in 0..8 {
        m.set_u32(at(cpu, 0xC0 + 4 * k), cpu.ebx);
    }
    call(m, cpu, BEGIN_CONTEXT, 0x00DA_B4DF);
    device_call(m, cpu, BEGIN_SCENE, 0x00DA_B4EA);
    cpu.eax = (cpu.eax & !0xFF) | m.u8(cpu.esi.wrapping_add(0x30)) as u32;
    m.set_u8(at(cpu, MERGE), cpu.eax as u8);
    m.set_u32(at(cpu, BATCH), cpu.ebx);
    m.set_u32(at(cpu, BATCH_START), cpu.ebx);
    m.set_u32(at(cpu, BATCH_COUNT), cpu.ebx);

    let mut next = Next::Reload;
    loop {
        if let Next::Reload = next {
            cpu.ebx = m.u32(at(cpu, DEPTH));
        }
        // the next entry (0 when the frame's commands are done)
        let Some(entry) = fetch(m, cpu) else { break };
        cpu.ecx = m.u32(entry.wrapping_add(0x14)); // a read the original makes (unused)
        cpu.ecx = m.u32(at(cpu, BATCH));
        cpu.esi = m.u32(entry.wrapping_add(4)); // the record
        if cpu.ecx != 0 {
            match merge_or_flush(m, cpu) {
                Some(n) => {
                    next = n;
                    continue;
                }
                None => {}
            }
        }
        next = dispatch(m, cpu);
    }
    finish(m, cpu);
}

/// `push this; call [vtable + offset]` with ECX = the vtable, EDX = the
/// method (BeginScene, EndScene).
fn device_call(m: &mut dyn Mem, cpu: &mut Cpu, offset: u32, return_to: u32) {
    cpu.ecx = m.u32(cpu.edi);
    cpu.edx = m.u32(cpu.ecx.wrapping_add(offset));
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.edx, return_to);
}

/// 0x00DAB504: take the entry at the current level's cursor (cursor += 8);
/// past the level's end, pop to the first level with an entry left (its
/// cursor += 8). None: nothing is left, or the entry is null.
fn fetch(m: &mut dyn Mem, cpu: &mut Cpu) -> Option<u32> {
    cpu.ecx = cpu.ebx.wrapping_mul(3);
    let cursor = at(cpu, RANGES + 4).wrapping_add(cpu.ecx.wrapping_mul(4));
    cpu.eax = m.u32(cursor);
    cpu.ecx = cursor;
    cpu.edx = cpu.eax.wrapping_add(8);
    m.set_u32(cpu.ecx, cpu.edx);
    cpu.ebx = m.u32(at(cpu, DEPTH));
    cpu.ecx = cpu.ebx.wrapping_mul(3);
    if cpu.eax >= m.u32(at(cpu, RANGES + 8).wrapping_add(cpu.ecx.wrapping_mul(4))) {
        // this level is done: pop
        if (cpu.ebx as i32) <= 0 {
            return None;
        }
        loop {
            cpu.ebx = cpu.ebx.wrapping_sub(1);
            cpu.eax = cpu.ebx.wrapping_mul(3).wrapping_mul(4);
            m.set_u32(at(cpu, DEPTH), cpu.ebx);
            cpu.edx = m.u32(at(cpu, RANGES + 4).wrapping_add(cpu.eax));
            if cpu.edx < m.u32(at(cpu, RANGES + 8).wrapping_add(cpu.eax)) {
                break;
            }
            if (cpu.ebx as i32) <= 0 {
                return None;
            }
        }
        cpu.ecx = cpu.ebx.wrapping_mul(3);
        let cursor = at(cpu, RANGES + 4).wrapping_add(cpu.ecx.wrapping_mul(4));
        cpu.eax = m.u32(cursor);
        cpu.ecx = cursor;
        cpu.edx = cpu.eax.wrapping_add(8);
        m.set_u32(cpu.ecx, cpu.edx);
        cpu.ebx = m.u32(at(cpu, DEPTH));
    }
    (cpu.eax != 0).then_some(cpu.eax)
}

/// 0x00DAB554: EndScene, then END_CONTEXT(context) on sRender; the epilogue.
fn finish(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.edx = m.u32(cpu.edi);
    cpu.eax = m.u32(cpu.edx.wrapping_add(END_SCENE));
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.eax, 0x00DA_B55F);
    cpu.ecx = at(cpu, CONTEXT);
    push(m, cpu, cpu.ecx);
    cpu.ecx = m.u32(at(cpu, RENDER + 4));
    call(m, cpu, END_CONTEXT, 0x00DA_B56D);
    cpu.edi = pop(m, cpu);
    cpu.esi = pop(m, cpu);
    cpu.ebx = pop(m, cpu);
    cpu.esp = cpu.ebp;
    cpu.ebp = pop(m, cpu);
    mtcore::machine::ret(cpu, 0);
}

/// 0x00DAB5B0: a pending merged draw (ECX = its record) and the next
/// record (ESI). The same draw over the next indices extends it (Some:
/// fetch again); otherwise the pending draw is made and the record
/// dispatched (None).
fn merge_or_flush(m: &mut dyn Mem, cpu: &mut Cpu) -> Option<Next> {
    let pending = cpu.ecx;
    let record = cpu.esi;
    let same = (m.u8(record.wrapping_add(2)) ^ m.u8(pending.wrapping_add(2))) & 0xF == 0
        && m.u32(pending.wrapping_add(0xC)) == m.u32(record.wrapping_add(0xC))
        && m.u32(pending.wrapping_add(0x10)) == m.u32(record.wrapping_add(0x10))
        && m.u32(pending.wrapping_add(8)) == m.u32(record.wrapping_add(8))
        && m.u32(pending.wrapping_add(4)) == m.u32(record.wrapping_add(4))
        && m.u32(pending.wrapping_add(0x1C)) == m.u32(record.wrapping_add(0x1C));
    if same {
        cpu.eax = m.u32(at(cpu, BATCH_COUNT));
        cpu.edx = m.u32(at(cpu, BATCH_START));
        cpu.ecx = (cpu.eax & 1).wrapping_add(cpu.eax);
        // the next start: count rounded up to even, + 2 (a strip continued
        // across a degenerate pair)
        cpu.edx = cpu.ecx.wrapping_add(cpu.edx).wrapping_add(2);
        cpu.ecx = m.u32(record.wrapping_add(0x18));
        if cpu.edx == cpu.ecx {
            cpu.eax = m.u32(record.wrapping_add(0x14));
            m.set_u32(at(cpu, BATCH_COUNT), cpu.eax);
            m.set_u32(at(cpu, BATCH_START), cpu.ecx);
            return Some(Next::Fetch);
        }
        cpu.ecx = m.u32(at(cpu, BATCH));
    } else {
        // (the registers the compares loaded are all reloaded below)
        cpu.eax = m.u32(at(cpu, BATCH_COUNT));
        cpu.ecx = pending;
    }
    // 0x00DAB616: draw the pending indices
    cpu.edx = m.u32(cpu.ecx.wrapping_add(0x18));
    cpu.ecx = m.u32(cpu.ecx.wrapping_add(0xC));
    cpu.eax = cpu.eax.wrapping_sub(cpu.edx);
    cpu.eax = cpu.eax.wrapping_add(m.u32(at(cpu, BATCH_START)));
    m.set_u32(at(cpu, BATCH_COUNT), cpu.edx);
    cpu.edx = cpu.eax;
    cpu.eax = m.u32(cpu.ecx);
    cpu.eax = (cpu.eax >> 21) & 0xFF;
    cpu.eax = cpu.eax.wrapping_sub(1);
    let kind = match cpu.eax {
        0 => {
            cpu.edx >>= 1;
            2
        }
        1 => {
            cpu.edx = cpu.edx.wrapping_sub(1);
            3
        }
        2 => {
            cpu.eax = 0xAAAA_AAAB;
            let p = cpu.eax as u64 * cpu.edx as u64;
            (cpu.eax, cpu.edx) = (p as u32, (p >> 32) as u32 >> 1);
            4
        }
        3 => {
            cpu.edx = cpu.edx.wrapping_sub(2);
            5
        }
        // an unknown primitive: the pending draw is dropped
        _ => 0,
    };
    if kind != 0 {
        m.set_u32(at(cpu, BATCH_START), kind);
        cpu.eax = m.u32(at(cpu, BATCH));
        cpu.eax = m.u32(cpu.eax.wrapping_add(0x1C));
        cpu.ecx = m.u32(cpu.edi);
        cpu.ecx = m.u32(cpu.ecx.wrapping_add(DRAW_INDEXED_PRIMITIVE));
        push(m, cpu, cpu.edx); // count
        cpu.edx = m.u32(at(cpu, BATCH_COUNT + 4)); // start
        push(m, cpu, cpu.edx);
        cpu.edx = m.u32(at(cpu, VERTICES + 8)).wrapping_sub(cpu.eax);
        push(m, cpu, cpu.edx); // vertices
        push(m, cpu, 0); // min index
        push(m, cpu, cpu.eax); // base vertex
        cpu.eax = m.u32(at(cpu, BATCH_START + 0x14)); // type
        push(m, cpu, cpu.eax);
        push(m, cpu, cpu.edi);
        call(m, cpu, cpu.ecx, 0x00DA_B69E);
    }
    cpu.eax = 0;
    m.set_u32(at(cpu, BATCH), 0);
    m.set_u32(at(cpu, BATCH_START), 0);
    m.set_u32(at(cpu, BATCH_COUNT), 0);
    None
}

/// 0x00DAB6AC: run one record (ESI) by its type; where the loop goes next.
fn dispatch(m: &mut dyn Mem, cpu: &mut Cpu) -> Next {
    cpu.eax = m.u16(cpu.esi.wrapping_add(2)) as u32 & 0xF;
    match cpu.eax {
        0 => draw(m, cpu),
        1 => draw_indexed_or_merge(m, cpu),
        2 => draw_instanced(m, cpu),
        3 => draw_indexed_instanced(m, cpu),
        4 => clear(m, cpu),
        5 => return stretch_rect(m, cpu),
        6 => nested(m, cpu),
        7 => return span(m, cpu),
        _ => return Next::Fetch,
    }
    Next::Reload
}

/// Type 0: DrawPrimitive.
fn draw(m: &mut dyn Mem, cpu: &mut Cpu) {
    let record = cpu.esi;
    apply_all_alt(m, cpu, record, [0x00DA_B7AA, 0x00DA_B7BA, 0x00DA_B7CA, 0x00DA_B7DA]);
    cpu.edx = m.u32(m.u32(record.wrapping_add(0xC)));
    let count = m.u32(record.wrapping_add(0x14));
    primitives(cpu, (cpu.edx >> 21) & 0xFF, count);
    draw_primitive(m, cpu, record, 0x00DA_BD8C);
}

/// Type 1: DrawIndexedPrimitive, or (merging) the start of a merged draw.
fn draw_indexed_or_merge(m: &mut dyn Mem, cpu: &mut Cpu) {
    let record = cpu.esi;
    apply_all(m, cpu, record, [0x00DA_B6D7, 0x00DA_B6E7, 0x00DA_B6F7, 0x00DA_B707]);
    cpu.ecx = m.u32(record.wrapping_add(0x14));
    if m.u8(at(cpu, MERGE)) != 0 {
        cpu.edx = m.u32(record.wrapping_add(0x18));
        m.set_u32(at(cpu, BATCH), record);
        m.set_u32(at(cpu, BATCH_COUNT), cpu.ecx);
        m.set_u32(at(cpu, BATCH_START), cpu.edx);
        return;
    }
    cpu.edx = m.u32(m.u32(record.wrapping_add(0xC)));
    primitives(cpu, (cpu.edx >> 21) & 0xFF, cpu.ecx);
    draw_indexed(m, cpu, record, 0x00DA_B791);
}

/// The stream frequencies of an instanced draw: per stream with an object,
/// instance data (0x80000001) or indexed geometry (0x40000000 | count),
/// and for instance data the stream itself at its instance offset.
/// `count`: the record's instance count; `stride_of`: the record's word
/// multiplying the stream's stride for the offset. `returns`: (frequency,
/// source) return addresses.
fn set_frequencies(m: &mut dyn Mem, cpu: &mut Cpu, record: u32, count: u32, stride_of: u32, returns: [u32; 2], indexed: bool) {
    cpu.ebx = 0;
    loop {
        cpu.eax = m.u32(record.wrapping_add(0x10));
        cpu.ecx = m.u32(cpu.eax.wrapping_add(cpu.ebx * 4));
        m.set_u32(at(cpu, SLOT_C), cpu.ecx);
        if cpu.ecx != 0 {
            cpu.eax = if m.u8(cpu.eax.wrapping_add(cpu.ebx).wrapping_add(0x24)) != 0 {
                0x8000_0001
            } else {
                m.u32(record.wrapping_add(count)) | 0x4000_0000
            };
            if indexed {
                // 0x00DAB9EA: ECX = vtable, EDX = method
                cpu.ecx = m.u32(cpu.edi);
                cpu.edx = m.u32(cpu.ecx.wrapping_add(SET_STREAM_SOURCE_FREQ));
                push(m, cpu, cpu.eax);
                push(m, cpu, cpu.ebx);
                push(m, cpu, cpu.edi);
                call(m, cpu, cpu.edx, returns[0]);
                cpu.eax = m.u32(record.wrapping_add(0x10));
                if m.u8(cpu.eax.wrapping_add(cpu.ebx).wrapping_add(0x24)) != 0 {
                    cpu.eax = m.u8(cpu.eax.wrapping_add(cpu.ebx).wrapping_add(0x10)) as u32;
                    cpu.edx = m.u32(record.wrapping_add(stride_of));
                    cpu.ecx = m.u32(cpu.edi);
                    cpu.edx = cpu.edx.wrapping_mul(cpu.eax);
                    push(m, cpu, cpu.eax);
                    cpu.eax = m.u32(record.wrapping_add(0x10));
                    cpu.edx = cpu.edx.wrapping_add(m.u32(cpu.eax.wrapping_add(cpu.ebx * 4).wrapping_add(0x14)));
                    cpu.eax = m.u32(at(cpu, SLOT_C + 4));
                    push(m, cpu, cpu.edx);
                    cpu.edx = m.u32(cpu.eax.wrapping_add(0x20));
                    cpu.eax = m.u32(cpu.ecx.wrapping_add(SET_STREAM_SOURCE));
                    push(m, cpu, cpu.edx);
                    push(m, cpu, cpu.ebx);
                    push(m, cpu, cpu.edi);
                    call(m, cpu, cpu.eax, returns[1]);
                }
            } else {
                // 0x00DAB884: EDX = vtable, EAX = method
                cpu.edx = m.u32(cpu.edi);
                push(m, cpu, cpu.eax);
                cpu.eax = m.u32(cpu.edx.wrapping_add(SET_STREAM_SOURCE_FREQ));
                push(m, cpu, cpu.ebx);
                push(m, cpu, cpu.edi);
                call(m, cpu, cpu.eax, returns[0]);
                cpu.ecx = m.u32(record.wrapping_add(0x10));
                if m.u8(cpu.ecx.wrapping_add(cpu.ebx).wrapping_add(0x24)) != 0 {
                    cpu.edx = m.u32(cpu.edi);
                    cpu.eax = cpu.ecx;
                    cpu.eax = m.u8(cpu.eax.wrapping_add(cpu.ebx).wrapping_add(0x10)) as u32;
                    cpu.ecx = m.u32(record.wrapping_add(stride_of));
                    cpu.edx = m.u32(cpu.edx.wrapping_add(SET_STREAM_SOURCE));
                    cpu.ecx = cpu.ecx.wrapping_mul(cpu.eax);
                    push(m, cpu, cpu.eax);
                    cpu.eax = m.u32(record.wrapping_add(0x10));
                    cpu.ecx = cpu.ecx.wrapping_add(m.u32(cpu.eax.wrapping_add(cpu.ebx * 4).wrapping_add(0x14)));
                    cpu.eax = m.u32(at(cpu, SLOT_C + 4));
                    push(m, cpu, cpu.ecx);
                    cpu.ecx = m.u32(cpu.eax.wrapping_add(0x20));
                    push(m, cpu, cpu.ecx);
                    push(m, cpu, cpu.ebx);
                    push(m, cpu, cpu.edi);
                    call(m, cpu, cpu.edx, returns[1]);
                }
            }
        }
        cpu.ebx = cpu.ebx.wrapping_add(1);
        if cpu.ebx >= 4 {
            break;
        }
    }
}

/// After an instanced draw: every stream with an object back to frequency
/// 1, instance-data streams back to their own offset.
fn reset_frequencies(m: &mut dyn Mem, cpu: &mut Cpu, record: u32, returns: [u32; 2]) {
    cpu.ebx = 0;
    loop {
        cpu.eax = m.u32(record.wrapping_add(0x10));
        cpu.eax = m.u32(cpu.eax.wrapping_add(cpu.ebx * 4));
        m.set_u32(at(cpu, SLOT_C), cpu.eax);
        if cpu.eax != 0 {
            cpu.ecx = m.u32(cpu.edi);
            cpu.edx = m.u32(cpu.ecx.wrapping_add(SET_STREAM_SOURCE_FREQ));
            push(m, cpu, 1);
            push(m, cpu, cpu.ebx);
            push(m, cpu, cpu.edi);
            call(m, cpu, cpu.edx, returns[0]);
            cpu.eax = m.u32(record.wrapping_add(0x10));
            if m.u8(cpu.eax.wrapping_add(cpu.ebx).wrapping_add(0x24)) != 0 {
                cpu.edx = m.u8(cpu.eax.wrapping_add(cpu.ebx).wrapping_add(0x10)) as u32;
                cpu.eax = m.u32(cpu.eax.wrapping_add(cpu.ebx * 4).wrapping_add(0x14));
                cpu.ecx = m.u32(cpu.edi);
                cpu.ecx = m.u32(cpu.ecx.wrapping_add(SET_STREAM_SOURCE));
                push(m, cpu, cpu.edx);
                cpu.edx = m.u32(at(cpu, SLOT_C + 4));
                push(m, cpu, cpu.eax);
                cpu.eax = m.u32(cpu.edx.wrapping_add(0x20));
                push(m, cpu, cpu.eax);
                push(m, cpu, cpu.ebx);
                push(m, cpu, cpu.edi);
                call(m, cpu, cpu.ecx, returns[1]);
            }
        }
        cpu.ebx = cpu.ebx.wrapping_add(1);
        if cpu.ebx >= 4 {
            break;
        }
    }
}

/// Type 2: instanced DrawPrimitive (instances: +0x1C, offset step: +0x20).
fn draw_instanced(m: &mut dyn Mem, cpu: &mut Cpu) {
    let record = cpu.esi;
    apply_all_alt(m, cpu, record, [0x00DA_B82A, 0x00DA_B83A, 0x00DA_B84A, 0x00DA_B85A]);
    set_frequencies(m, cpu, record, 0x1C, 0x20, [0x00DA_B891, 0x00DA_B8C5], false);
    cpu.edx = m.u32(m.u32(record.wrapping_add(0xC)));
    let count = m.u32(record.wrapping_add(0x14));
    primitives(cpu, (cpu.edx >> 21) & 0xFF, count);
    // 0x00DAB919: DrawPrimitive, EDX = vtable then method
    cpu.edx = m.u32(cpu.edi);
    cpu.edx = m.u32(cpu.edx.wrapping_add(DRAW_PRIMITIVE));
    push(m, cpu, cpu.ecx);
    cpu.ecx = m.u32(record.wrapping_add(0x18));
    push(m, cpu, cpu.ecx);
    push(m, cpu, cpu.eax);
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.edx, 0x00DA_B92A);
    reset_frequencies(m, cpu, record, [0x00DA_B94C, 0x00DA_B975]);
}

/// Type 3: instanced DrawIndexedPrimitive (instances: +0x20, offset step:
/// +0x24).
fn draw_indexed_instanced(m: &mut dyn Mem, cpu: &mut Cpu) {
    let record = cpu.esi;
    apply_all(m, cpu, record, [0x00DA_B994, 0x00DA_B9A4, 0x00DA_B9B4, 0x00DA_B9C4]);
    set_frequencies(m, cpu, record, 0x20, 0x24, [0x00DA_B9F7, 0x00DA_BA29], true);
    cpu.edx = m.u32(m.u32(record.wrapping_add(0xC)));
    let count = m.u32(record.wrapping_add(0x14));
    primitives(cpu, (cpu.edx >> 21) & 0xFF, count);
    draw_indexed(m, cpu, record, 0x00DA_BA9E);
    reset_frequencies(m, cpu, record, [0x00DA_BABC, 0x00DA_BAE5]);
}

/// Type 4: Clear(0, NULL, flags, colour, z = +8, stencil = +0xC) of the
/// targets and states of the fallback draw at +0x20. The colour (floats at
/// +0x10..+0x1C, red green blue alpha) is packed to bytes (`* 255`,
/// truncated, saturated at 1.0); when a channel does not round trip exactly
/// (byte / 255 != the float) or the write mask (+4) asks for fewer colour
/// channels than the target has, and the record clears colour, the
/// fallback draw is made instead (a full-screen quad).
fn clear(m: &mut dyn Mem, cpu: &mut Cpu) {
    let record = cpu.esi;
    cpu.ebx = m.u32(record.wrapping_add(0x20)); // the fallback draw
    cpu.edx = m.u32(cpu.ebx.wrapping_add(4));
    cpu.ecx = m.u32(at(cpu, RENDER));
    push(m, cpu, cpu.edx);
    cpu.eax = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.eax);
    m.set_u32(at(cpu, SLOT_C + 8), cpu.ebx);
    call(m, cpu, APPLY_TARGETS, 0x00DA_BB09);
    cpu.ecx = m.u32(cpu.ebx.wrapping_add(8));
    push(m, cpu, cpu.ecx);
    cpu.ecx = m.u32(at(cpu, RENDER + 4));
    cpu.edx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.edx);
    call(m, cpu, APPLY_STATES, 0x00DA_BB1B);

    // the colour bytes (SSE, as the original)
    cpu.xmm[4] = [m.u32(record.wrapping_add(0x10)), 0, 0, 0];
    cpu.xmm[3] = [m.u32(ONE), 0, 0, 0];
    let red_over = x86::comiss_hw::<true>(cpu.xmm[4][0], cpu.xmm[3][0]);
    cpu.xmm[2] = [m.u32(record.wrapping_add(0x1C)), 0, 0, 0];
    cpu.xmm[5] = [m.u32(record.wrapping_add(0x14)), 0, 0, 0];
    cpu.xmm[6] = [m.u32(record.wrapping_add(0x18)), 0, 0, 0];
    cpu.xmm[0] = [m.u32(BYTE_MAX), 0, 0, 0];
    m.set_u32(at(cpu, ALPHA), cpu.xmm[2][0]);
    cpu.xmm[1] = scaled(cpu, red_over.cond("be"), 4);
    cpu.ecx = m.u32(at(cpu, COLOUR));
    cpu.eax = x86::cvttss2si(cpu.xmm[1][0]);
    cpu.edx = cpu.eax & 0xFF;
    cpu.ecx = (cpu.ecx & 0xFFFF_FF00) | cpu.edx;
    let green = x86::comiss_hw::<true>(cpu.xmm[5][0], cpu.xmm[3][0]);
    cpu.xmm[1] = scaled(cpu, green.cond("be"), 5);
    cpu.eax = (x86::cvttss2si(cpu.xmm[1][0]) & 0xFF) << 8;
    cpu.ecx &= 0xFFFF_00FF;
    cpu.eax |= cpu.ecx;
    let blue = x86::comiss_hw::<true>(cpu.xmm[6][0], cpu.xmm[3][0]);
    cpu.xmm[1] = scaled(cpu, blue.cond("be"), 6);
    cpu.ecx = (x86::cvttss2si(cpu.xmm[1][0]) & 0xFF) << 16;
    cpu.eax &= 0xFF00_FFFF;
    cpu.ecx |= cpu.eax;
    let alpha = x86::comiss_hw::<true>(cpu.xmm[2][0], cpu.xmm[3][0]);
    if !alpha.cond("a") {
        cpu.xmm[2][0] = x86::mulss(cpu.xmm[2][0], cpu.xmm[0][0]);
        cpu.xmm[0] = cpu.xmm[2];
    }
    cpu.edx = x86::cvttss2si(cpu.xmm[0][0]);
    cpu.eax = (cpu.edx & 0xFF) << 24;
    cpu.ecx &= 0x00FF_FFFF;
    cpu.eax |= cpu.ecx;
    // the bytes back to floats (x87, as the original: byte * (1/255)); the
    // original's FADD of 2^32 for a negative integer cannot happen (the
    // bytes are 0..255) and is left out
    cpu.edx = cpu.eax & 0xFF;
    m.set_u32(at(cpu, SLOT_10), cpu.edx);
    m.set_u32(at(cpu, COLOUR), cpu.eax);
    cpu.st.memory::<0x00db, 4, false>(m, at(cpu, SLOT_10)); // FILD red
    cpu.st.memory::<0x00d9, 4, false>(m, INV_BYTE_MAX); // FLD 1/255
    cpu.ecx = cpu.eax;
    cpu.st.register::<0xc9dc>(); // FMUL ST(1), ST
    cpu.ecx = (cpu.ecx >> 8) & 0xFF;
    cpu.st.register::<0xc9d9>(); // FXCH
    m.set_u32(at(cpu, SLOT_10), cpu.ecx);
    cpu.st.memory::<0x18d9, 4, true>(m, at(cpu, RED_F)); // FSTP
    cpu.ebx = cpu.eax;
    cpu.st.memory::<0x00db, 4, false>(m, at(cpu, SLOT_10)); // FILD green
    cpu.ebx = (cpu.ebx >> 16) & 0xFF;
    m.set_u32(at(cpu, SLOT_10), cpu.ebx);
    cpu.st.register::<0xc9d8>(); // FMUL ST, ST(1)
    cpu.st.memory::<0x18d9, 4, true>(m, at(cpu, GREEN_F));
    cpu.st.memory::<0x00db, 4, false>(m, at(cpu, SLOT_10)); // FILD blue
    cpu.st.register::<0xc9d8>();
    cpu.eax >>= 24;
    m.set_u32(at(cpu, SLOT_10), cpu.eax);
    cpu.st.memory::<0x18d9, 4, true>(m, at(cpu, BLUE_F));
    cpu.st.memory::<0x00db, 4, false>(m, at(cpu, SLOT_10)); // FILD alpha
    // 0x00DABC45: alpha byte / 255 against the alpha float
    cpu.eax = m.u32(at(cpu, SLOT_C));
    cpu.st.register::<0xc9de>(); // FMULP
    cpu.ecx = m.u32(cpu.eax.wrapping_add(4));
    cpu.st.memory::<0x00d9, 4, false>(m, at(cpu, ALPHA)); // FLD alpha
    cpu.ecx = m.u32(cpu.ecx.wrapping_add(0x14));
    cpu.st.register::<0xc9d9>(); // FXCH
    cpu.ecx &= 7;
    // the target's channels as a mask: (1 << channels) - 1
    cpu.eax = 1u32.wrapping_shl(cpu.ecx & 31).wrapping_sub(1);
    let equal_alpha = cpu.st.compare_flags::<0xe9df>(); // FUCOMIP
    cpu.st.register::<0xd8dd>(); // FSTP ST0
    m.set_u32(at(cpu, SLOT_10), cpu.eax);
    let equal = |cpu: &mut Cpu, f: x86::Flags| -> bool {
        // LAHF; TEST AH, 0x44; JP: on only when equal and ordered
        cpu.eax = (cpu.eax & !0xFF00) | ((f.lahf() as u32) << 8);
        (cpu.eax >> 8) & 0x44 == 0x40
    };
    let exact = equal(cpu, equal_alpha)
        && {
            cpu.xmm[0] = [m.u32(at(cpu, RED_F)), 0, 0, 0];
            let f = x86::comiss_hw::<false>(cpu.xmm[0][0], cpu.xmm[4][0]);
            equal(cpu, f)
        }
        && {
            cpu.xmm[0] = [m.u32(at(cpu, GREEN_F)), 0, 0, 0];
            let f = x86::comiss_hw::<false>(cpu.xmm[0][0], cpu.xmm[5][0]);
            equal(cpu, f)
        }
        && {
            cpu.xmm[0] = [m.u32(at(cpu, BLUE_F)), 0, 0, 0];
            let f = x86::comiss_hw::<false>(cpu.xmm[0][0], cpu.xmm[6][0]);
            equal(cpu, f)
        }
        && {
            cpu.eax = m.u32(at(cpu, SLOT_10));
            cpu.ecx = cpu.eax & m.u32(record.wrapping_add(4));
            cpu.ecx == cpu.eax
        };
    if !exact {
        // only depth or stencil to clear: the colour does not matter
        cpu.eax = m.u32(record.wrapping_add(4));
        cpu.ecx = cpu.eax & 0x30;
        if cpu.ecx != cpu.eax {
            return fallback_draw(m, cpu);
        }
    }
    // 0x00DABCB6: D3DCLEAR flags: target (any colour channel), zbuffer, stencil
    cpu.ecx = m.u32(record.wrapping_add(4));
    cpu.eax = 0;
    if cpu.ecx & 0xF != 0 {
        cpu.eax = 1;
    }
    if cpu.ecx & 0x10 != 0 {
        cpu.eax |= 2;
    }
    if cpu.ecx & 0x20 != 0 {
        cpu.eax |= 4;
    }
    cpu.ecx = m.u32(record.wrapping_add(0xC));
    cpu.st.memory::<0x00d9, 4, false>(m, record.wrapping_add(8)); // FLD z
    cpu.esi = m.u32(at(cpu, COLOUR));
    push(m, cpu, cpu.ecx); // stencil
    push(m, cpu, cpu.ecx); // z (stored over it below)
    cpu.ecx = m.u32(cpu.edi);
    cpu.st.memory::<0x18d9, 4, true>(m, cpu.esp); // FSTP [esp]
    // D3DCOLOR: alpha and green from the bytes, red (EDX) and blue (EBX)
    cpu.edx <<= 16;
    cpu.esi &= 0xFF00_FF00;
    cpu.edx |= cpu.esi;
    cpu.edx |= cpu.ebx;
    push(m, cpu, cpu.edx);
    cpu.edx = m.u32(cpu.ecx.wrapping_add(CLEAR));
    push(m, cpu, cpu.eax);
    push(m, cpu, 0);
    push(m, cpu, 0);
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.edx, 0x00DA_BD02);
}

/// `xmm[src] > 1.0 ? 255.0 : xmm[src] * 255.0` (`JBE` on `comiss`: an
/// unordered value scales).
fn scaled(cpu: &Cpu, scale: bool, src: usize) -> [u32; 4] {
    if scale {
        let mut v = cpu.xmm[src];
        v[0] = x86::mulss(v[0], cpu.xmm[0][0]);
        v
    } else {
        cpu.xmm[0]
    }
}

/// 0x00DABD07: the Clear's fallback draw (a DrawPrimitive record at
/// SLOT_C): its states, streams and shaders (its targets are applied).
fn fallback_draw(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.esi = m.u32(at(cpu, SLOT_C));
    let record = cpu.esi;
    cpu.eax = m.u32(record.wrapping_add(8));
    cpu.ebx = m.u32(at(cpu, RENDER));
    push(m, cpu, cpu.eax);
    cpu.ecx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.ecx);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_STATES, 0x00DA_BD1F);
    cpu.edx = m.u32(record.wrapping_add(0x10));
    push(m, cpu, cpu.edx);
    cpu.eax = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.eax);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_STREAMS, 0x00DA_BD2F);
    cpu.ecx = m.u32(record.wrapping_add(0xC));
    push(m, cpu, cpu.ecx);
    cpu.edx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.edx);
    cpu.ecx = cpu.ebx;
    call(m, cpu, APPLY_SHADERS, 0x00DA_BD3F);
    cpu.edx = m.u32(m.u32(record.wrapping_add(0xC)));
    let count = m.u32(record.wrapping_add(0x14));
    primitives(cpu, (cpu.edx >> 21) & 0xFF, count);
    draw_primitive(m, cpu, record, 0x00DA_BD8C);
}

/// Type 5: StretchRect. Types 0..3 (+0xC): a rectangle (+0x10) of the
/// context's target of that index to a rectangle at (+0x20, +0x24) of the
/// surface at (+4)->+0x44[+8]; otherwise the whole of target 3 to the
/// surface (+4)->+0x48[+8], outside the scene. A next entry with the same
/// copy (same type, surfaces and rectangles) is consumed with this one.
fn stretch_rect(m: &mut dyn Mem, cpu: &mut Cpu) -> Next {
    let record = cpu.esi;
    cpu.eax = m.u32(record.wrapping_add(0x28));
    cpu.ecx = m.u32(cpu.eax.wrapping_add(4));
    push(m, cpu, cpu.ecx);
    cpu.ecx = m.u32(at(cpu, RENDER + 4));
    cpu.edx = at(cpu, CONTEXT + 4);
    push(m, cpu, cpu.edx);
    call(m, cpu, APPLY_TARGETS, 0x00DA_BDA6);
    merge_copy(m, cpu, record);
    // 0x00DABEA5
    cpu.eax = m.u32(record.wrapping_add(0xC)) & 0xF;
    m.set_u32(at(cpu, SLOT_C), cpu.eax);
    if cpu.eax < 4 {
        cpu.eax = m.u32(record.wrapping_add(4));
        cpu.edx = m.u32(cpu.eax.wrapping_add(0x44));
        cpu.ecx = m.u32(record.wrapping_add(8));
        cpu.eax = m.u32(cpu.edx.wrapping_add(cpu.ecx.wrapping_mul(4))); // the surface
        cpu.ebx = m.u32(record.wrapping_add(0x20));
        cpu.ecx = m.u32(record.wrapping_add(0x18)).wrapping_sub(m.u32(record.wrapping_add(0x10)));
        cpu.edx = m.u32(record.wrapping_add(0x1C)).wrapping_sub(m.u32(record.wrapping_add(0x14)));
        m.set_u32(at(cpu, SLOT_10), cpu.eax);
        cpu.eax = record.wrapping_add(0x10); // the source rectangle
        cpu.esi = m.u32(record.wrapping_add(0x24));
        m.set_u32(at(cpu, DEST_RECT), cpu.ebx);
        push(m, cpu, STRETCH_RECT_RECTS);
        m.set_u32(at(cpu, DEST_RECT + 8), cpu.esi);
        cpu.ebx = cpu.ebx.wrapping_add(cpu.ecx);
        cpu.ecx = m.u32(at(cpu, SLOT_10 + 4));
        cpu.esi = cpu.esi.wrapping_add(cpu.edx);
        cpu.edx = m.u32(at(cpu, SLOT_C + 4));
        cpu.edx = m.u32(at(cpu, TARGETS).wrapping_add(cpu.edx.wrapping_mul(4)));
        push(m, cpu, 0); // D3DTEXF_NONE
        m.set_u32(at(cpu, DEST_RECT + 0x10), cpu.ebx);
        m.set_u32(at(cpu, DEST_RECT + 0x14), cpu.esi);
        cpu.ecx = m.u32(cpu.ecx.wrapping_add(0x14)); // the surface's handle
        cpu.esi = m.u32(S_RENDER);
        cpu.edx = m.u32(cpu.edx.wrapping_add(0x14)); // the target's handle
        cpu.ebx = at(cpu, DEST_RECT + 8);
        push(m, cpu, cpu.ebx);
        push(m, cpu, cpu.ecx);
        push(m, cpu, cpu.eax);
        m.set_u32(at(cpu, SLOT_10 + 0x14), cpu.esi);
        cpu.esi = m.u32(cpu.edi);
        cpu.eax = m.u32(cpu.esi.wrapping_add(STRETCH_RECT));
        push(m, cpu, cpu.edx);
        push(m, cpu, cpu.edi);
        call(m, cpu, cpu.eax, 0x00DA_BF2A);
        cpu.ecx = m.u32(at(cpu, SLOT_10 + 4));
        push(m, cpu, cpu.eax);
        call(m, cpu, CHECK_RESULT, 0x00DA_BF34);
        return Next::Reload;
    }
    // 0x00DABF39: the whole target, between EndScene and BeginScene
    cpu.ecx = m.u32(record.wrapping_add(4));
    cpu.edx = m.u32(record.wrapping_add(8));
    cpu.eax = m.u32(cpu.ecx.wrapping_add(0x48));
    cpu.ecx = m.u32(cpu.edi);
    cpu.esi = m.u32(cpu.eax.wrapping_add(cpu.edx.wrapping_mul(4)));
    cpu.edx = m.u32(cpu.ecx.wrapping_add(END_SCENE));
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.edx, 0x00DA_BF50);
    cpu.eax = m.u32(cpu.esi.wrapping_add(0x14));
    cpu.ecx = m.u32(at(cpu, TARGETS + 0xC));
    cpu.ecx = m.u32(cpu.ecx.wrapping_add(0x14));
    cpu.edx = m.u32(cpu.edi);
    cpu.esi = m.u32(S_RENDER);
    push(m, cpu, STRETCH_RECT_WHOLE);
    push(m, cpu, 0);
    push(m, cpu, 0);
    push(m, cpu, cpu.eax);
    cpu.eax = m.u32(cpu.edx.wrapping_add(STRETCH_RECT));
    push(m, cpu, 0);
    push(m, cpu, cpu.ecx);
    push(m, cpu, cpu.edi);
    call(m, cpu, cpu.eax, 0x00DA_BF7B);
    push(m, cpu, cpu.eax);
    cpu.ecx = cpu.esi;
    call(m, cpu, CHECK_RESULT, 0x00DA_BF83);
    device_call(m, cpu, BEGIN_SCENE, 0x00DA_BF8E);
    Next::Fetch
}

/// 0x00DABDA6: peek at the next entry of the current level; a StretchRect
/// with the same type, surfaces and rectangles is consumed (the cursor
/// moves past it; past the level's end the stack pops as `fetch` does).
fn merge_copy(m: &mut dyn Mem, cpu: &mut Cpu, record: u32) {
    cpu.ebx = m.u32(at(cpu, DEPTH));
    cpu.eax = cpu.ebx.wrapping_mul(3).wrapping_mul(4);
    cpu.ecx = at(cpu, RANGES + 4).wrapping_add(cpu.eax);
    m.set_u32(at(cpu, SLOT_10), cpu.ecx);
    cpu.ecx = m.u32(cpu.ecx);
    m.set_u32(at(cpu, SLOT_C), cpu.ecx);
    if cpu.ecx >= m.u32(at(cpu, RANGES + 8).wrapping_add(cpu.eax)) || cpu.ecx == 0 {
        return;
    }
    cpu.eax = m.u32(cpu.ecx.wrapping_add(4)); // the next record
    cpu.ecx = (cpu.ecx & !0xFF) | (m.u8(cpu.eax.wrapping_add(2)) & 0xF) as u32;
    if cpu.ecx & 0xFF != 5 {
        return;
    }
    cpu.eax = m.u32(at(cpu, SLOT_C));
    cpu.edx = m.u32(record.wrapping_add(0x20));
    cpu.eax = m.u32(cpu.eax.wrapping_add(4));
    if cpu.edx != m.u32(cpu.eax.wrapping_add(0x20)) {
        return;
    }
    cpu.edx = m.u32(at(cpu, SLOT_C));
    cpu.ecx = m.u32(record.wrapping_add(0x24));
    cpu.eax = m.u32(cpu.edx.wrapping_add(4));
    if cpu.ecx != m.u32(cpu.eax.wrapping_add(0x24)) {
        return;
    }
    cpu.ecx = m.u32(record.wrapping_add(0xC));
    cpu.eax = m.u32(cpu.edx.wrapping_add(4));
    if cpu.ecx != m.u32(cpu.eax.wrapping_add(0xC)) {
        return;
    }
    cpu.eax = cpu.edx;
    cpu.eax = m.u32(cpu.eax.wrapping_add(4)).wrapping_add(0x10);
    push(m, cpu, cpu.eax);
    cpu.ecx = record.wrapping_add(0x10);
    push(m, cpu, 0x00DA_BE34);
    x86::call_lifted(m, cpu, RECT_EQUAL);
    if cpu.eax & 0xFF == 0 {
        return;
    }
    cpu.eax = m.u32(at(cpu, SLOT_C));
    cpu.edx = m.u32(record.wrapping_add(4));
    cpu.ecx = m.u32(cpu.eax.wrapping_add(4));
    if cpu.edx != m.u32(cpu.ecx.wrapping_add(4)) {
        return;
    }
    // consume it
    cpu.edx = m.u32(at(cpu, SLOT_10));
    cpu.ecx = cpu.eax.wrapping_add(8);
    m.set_u32(cpu.edx, cpu.ecx);
    cpu.ebx = m.u32(at(cpu, DEPTH));
    cpu.ecx = cpu.ebx.wrapping_mul(3);
    if cpu.eax < m.u32(at(cpu, RANGES + 8).wrapping_add(cpu.ecx.wrapping_mul(4))) {
        return;
    }
    if (cpu.ebx as i32) <= 0 {
        return;
    }
    loop {
        cpu.ebx = cpu.ebx.wrapping_sub(1);
        cpu.eax = cpu.ebx.wrapping_mul(3).wrapping_mul(4);
        m.set_u32(at(cpu, DEPTH), cpu.ebx);
        cpu.edx = m.u32(at(cpu, RANGES + 4).wrapping_add(cpu.eax));
        if cpu.edx < m.u32(at(cpu, RANGES + 8).wrapping_add(cpu.eax)) {
            break;
        }
        if (cpu.ebx as i32) <= 0 {
            return;
        }
    }
    cpu.eax = cpu.ebx.wrapping_mul(3);
    let cursor = at(cpu, RANGES + 4).wrapping_add(cpu.eax.wrapping_mul(4));
    m.set_u32(cursor, m.u32(cursor).wrapping_add(8));
    cpu.ebx = m.u32(at(cpu, DEPTH));
    cpu.eax = cursor;
}

/// Type 6: a nested list (+8 entries, +0xC count, +4 owner) as the next
/// level of the stack.
fn nested(m: &mut dyn Mem, cpu: &mut Cpu) {
    let record = cpu.esi;
    cpu.eax = m.u32(record.wrapping_add(8));
    cpu.edx = m.u32(record.wrapping_add(0xC));
    cpu.ebx = cpu.ebx.wrapping_add(1);
    m.set_u32(at(cpu, DEPTH), cpu.ebx);
    cpu.ecx = cpu.ebx.wrapping_mul(3);
    m.set_u32(at(cpu, RANGES + 4).wrapping_add(cpu.ecx.wrapping_mul(4)), cpu.eax);
    cpu.ecx = cpu.eax.wrapping_add(cpu.edx.wrapping_mul(8));
    cpu.eax = m.u32(at(cpu, DEPTH));
    cpu.edx = cpu.eax.wrapping_mul(3);
    m.set_u32(at(cpu, RANGES + 8).wrapping_add(cpu.edx.wrapping_mul(4)), cpu.ecx);
    cpu.eax = m.u32(at(cpu, DEPTH));
    cpu.ecx = m.u32(record.wrapping_add(4));
    cpu.eax = cpu.eax.wrapping_mul(3);
    m.set_u32(at(cpu, RANGES).wrapping_add(cpu.eax.wrapping_mul(4)), cpu.ecx);
}

/// Type 7: a query span on the object at +8 (+4: 0 begins, else ends).
fn span(m: &mut dyn Mem, cpu: &mut Cpu) -> Next {
    let record = cpu.esi;
    cpu.ecx = m.u32(record.wrapping_add(8));
    if cpu.ecx != 0 {
        if m.u32(record.wrapping_add(4)) == 0 {
            call(m, cpu, SPAN_BEGIN, 0x00DA_BFEE);
        } else {
            call(m, cpu, SPAN_END, 0x00DA_BFF8);
        }
    }
    Next::Fetch
}
