//! nDraw object decoding and exact guest apply adapters.
//!
//! The decode functions read only the binding and its referenced objects. They
//! neither inspect the executor cache nor emit calls, so a native CommandSink
//! can use them without changing the fallback D3D9 device's state.

pub use crate::apply_shader::{
    ConstantBinding, ConstantKind, ShaderBinding, ShaderStage, TextureBinding, decode_shader, shader,
};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use mtcore::machine::{call_out, pop, push};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateObject<const N: usize> {
    pub object: u32,
    pub hash_0c: u32,
    pub packed: [u32; N],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedState {
    pub blend: StateObject<2>,
    pub depth: StateObject<2>,
    pub raster: StateObject<3>,
    pub blend_rgba_00: u32,
    pub stencil_reference: u8,
    pub winding_flip: bool,
}

fn state<const N: usize>(m: &dyn Mem, object: u32, offset: u32) -> StateObject<N> {
    StateObject {
        object,
        hash_0c: m.u32(object + 12),
        packed: std::array::from_fn(|i| m.u32(object + offset + i as u32 * 4)),
    }
}

/// Translated from 0x00DAA5F0 (DDDA.exe 2017-12-13 Steam).
/// Packed fields are consumed by mtstate::packed and its BLEND/DEPTH/RASTER
/// maps. Identity/hash suppression belongs to the diff/apply half.
pub fn decode_fixed(m: &dyn Mem, binding: u32) -> FixedState {
    let flags = m.u32(binding + 4);
    FixedState {
        blend: state(m, m.u32(binding + 8), 0x20),
        depth: state(m, m.u32(binding + 12), 0x18),
        raster: state(m, m.u32(binding + 16), 0x20 + 12 * ((flags >> 8) & 1)),
        blend_rgba_00: m.u32(binding),
        stencil_reference: flags as u8,
        winding_flip: flags & 0x100 != 0,
    }
}

/// Translated from 0x00DAA5F0 (DDDA.exe 2017-12-13 Steam).
pub fn fixed(m: &mut dyn Mem, c: &mut Cpu) {
    push(m, c, c.ecx);
    push(m, c, c.ebx);
    push(m, c, c.ebp);
    push(m, c, c.esi);
    c.esi = m.u32(c.esp + 0x14);
    c.ebp = m.u32(c.esi + 12);
    push(m, c, c.edi);
    c.edi = m.u32(c.esp + 0x1c);
    c.edx = c.ecx;
    m.set_u32(c.esp + 0x10, c.edx);
    if c.ebp != c.edi {
        c.ecx = m.u32(c.esi + 0x18);
        c.eax = m.u32(c.edi + 8);
        c.ebx = m.u32(c.esi + 4);
        if c.ecx != c.eax {
            let mut factor_call = false;
            if c.ebp != 0 && c.ecx != 0 {
                c.ebp = m.u32(c.ecx + 12);
                if c.ebp != m.u32(c.eax + 12) {
                    c.ecx += 0x20;
                    push(m, c, c.ecx);
                    c.eax += 0x20;
                    push(m, c, c.eax);
                    push(m, c, c.esi);
                    c.ecx = c.edx;
                    call_out(m, c, 0xda7510, 0xdaa63f);
                    c.edx = m.u32(c.esp + 0x10);
                }
                c.eax = m.u32(c.edi);
                c.ecx = m.u32(c.esi + 12);
                if c.eax != m.u32(c.ecx) {
                    push(m, c, c.ecx);
                    c.ecx = c.esp;
                    m.set_u32(c.ecx, c.eax);
                    push(m, c, 0xdaa656);
                    ddnative_base::x86::call_lifted(m, c, 0xda68a0);
                    c.edx = m.u32(c.ebx);
                    c.esp += 4;
                    push(m, c, c.eax);
                    c.eax = m.u32(c.edx + 0xe4);
                    factor_call = true;
                }
            } else {
                push(m, c, 0);
                c.eax += 0x20;
                push(m, c, c.eax);
                push(m, c, c.esi);
                c.ecx = c.edx;
                call_out(m, c, 0xda7510, 0xdaa672);
                c.eax = m.u32(c.edi);
                c.ecx = m.u32(c.ebx);
                c.edx = c.eax;
                c.ebp = c.eax & 255;
                c.edx = (c.edx >> 16) & 255;
                c.ebp <<= 16;
                c.eax &= 0xff00ff00;
                c.edx |= c.ebp;
                c.edx |= c.eax;
                c.eax = m.u32(c.ecx + 0xe4);
                push(m, c, c.edx);
                factor_call = true;
            }
            if factor_call {
                push(m, c, 193);
                push(m, c, c.ebx);
                call_out(m, c, c.eax, 0xdaa69f);
                c.edx = m.u32(c.esp + 0x10);
            }
            c.ecx = m.u32(c.edi + 8);
            m.set_u32(c.esi + 0x18, c.ecx);
        }
        c.ebp = m.u32(c.esi + 0x20);
        c.eax = m.u32(c.edi + 12);
        let mut depth_changed = c.ebp != c.eax;
        if !depth_changed {
            c.ecx = m.u32(c.esi + 12);
            if c.ecx != 0 {
                c.ecx = m.u32(c.ecx + 4) ^ m.u32(c.edi + 4);
                depth_changed = c.ecx & 255 != 0;
            }
        }
        if depth_changed {
            let mut reference_call = false;
            if m.u32(c.esi + 12) != 0 && c.ebp != 0 {
                c.ecx = m.u32(c.ebp + 12);
                if c.ecx != m.u32(c.eax + 12) {
                    c.ebp += 0x18;
                    push(m, c, c.ebp);
                    c.eax += 0x18;
                    push(m, c, c.eax);
                    push(m, c, c.esi);
                    c.ecx = c.edx;
                    call_out(m, c, 0xda77d0, 0xdaa6e6);
                }
                c.edx = m.u32(c.esi + 12);
                c.eax = m.u32(c.edi + 4);
                c.ecx = m.u8(c.edx + 4) as u32;
                c.eax &= 255;
                if c.ecx != c.eax {
                    c.edx = m.u32(c.ebx);
                    push(m, c, c.eax);
                    c.eax = m.u32(c.edx + 0xe4);
                    reference_call = true;
                }
            } else {
                push(m, c, 0);
                c.eax += 0x18;
                push(m, c, c.eax);
                push(m, c, c.esi);
                c.ecx = c.edx;
                call_out(m, c, 0xda77d0, 0xdaa712);
                c.edx = m.u8(c.edi + 4) as u32;
                c.ecx = m.u32(c.ebx);
                c.eax = m.u32(c.ecx + 0xe4);
                push(m, c, c.edx);
                reference_call = true;
            }
            if reference_call {
                push(m, c, 57);
                push(m, c, c.ebx);
                call_out(m, c, c.eax, 0xdaa724);
            }
            c.ecx = m.u32(c.edi + 12);
            m.set_u32(c.esi + 0x20, c.ecx);
        }
        c.edx = m.u32(c.esi + 0x1c);
        c.ebp = m.u32(c.edi + 16);
        let mut raster_changed = c.edx != c.ebp;
        if !raster_changed {
            c.eax = m.u32(c.esi + 12);
            if c.eax != 0 {
                c.eax = m.u32(c.eax + 4) ^ m.u32(c.edi + 4);
                raster_changed = c.eax & 0x100 != 0;
            }
        }
        if raster_changed {
            c.eax = m.u32(c.esi + 12);
            let apply;
            if c.eax != 0 && c.edx != 0 {
                c.ecx = m.u32(c.edi + 4);
                c.eax = m.u32(c.eax + 4);
                c.ecx >>= 8;
                c.eax >>= 8;
                c.ebx = c.ecx ^ c.eax;
                let mut different = c.ebx & 1 != 0;
                if !different {
                    c.ebx = m.u32(c.edx + 12);
                    different = c.ebx != m.u32(c.ebp + 12);
                }
                apply = different;
                if apply {
                    c.eax = (c.eax & 1) * 3;
                    c.edx = c.edx + c.eax * 4 + 0x20;
                    c.eax = (c.ecx & 1) * 3;
                    push(m, c, c.edx);
                    c.ecx = c.ebp + c.eax * 4 + 0x20;
                    push(m, c, c.ecx);
                }
            } else {
                c.eax = (m.u32(c.edi + 4) >> 8) & 1;
                c.edx = c.eax * 3;
                push(m, c, 0);
                c.eax = c.ebp + c.edx * 4 + 0x20;
                push(m, c, c.eax);
                apply = true;
            }
            if apply {
                c.ecx = m.u32(c.esp + 0x18);
                push(m, c, c.esi);
                call_out(m, c, 0xda7b30, 0xdaa7ab);
            }
            c.ecx = m.u32(c.edi + 16);
            m.set_u32(c.esi + 0x1c, c.ecx);
        }
        m.set_u32(c.esi + 12, c.edi);
    }
    c.edi = pop(m, c);
    c.esi = pop(m, c);
    c.ebp = pop(m, c);
    c.ebx = pop(m, c);
    c.ecx = pop(m, c);
    c.esp += 12;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    pub object: u32,
    pub surface_14: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Targets {
    pub color: [View; 4],
    pub depth: View,
    pub viewport: [i32; 4],
    pub scissor: [i32; 4],
    pub dimensions_58: [u16; 2],
}

/// Translated from 0x00DA6FE0 (DDDA.exe 2017-12-13 Steam).
/// Rectangles are the selected raw rectangles, before viewport intersection.
/// Format provenance must be resolved from the surface; not every view's
/// format field is initialized (NDRAW section 9).
pub fn decode_targets(m: &dyn Mem, binding: u32) -> Targets {
    let view = |a| {
        let object = m.u32(a);
        View {
            object,
            surface_14: if object == 0 { 0 } else { m.u32(object + 0x14) },
        }
    };
    let alternate = (m.u32(binding + 0x14) >> 9) & 1;
    Targets {
        color: std::array::from_fn(|i| view(binding + i as u32 * 4)),
        depth: view(binding + 0x10),
        viewport: std::array::from_fn(|i| m.u32(binding + 0x18 + alternate * 16 + i as u32 * 4) as i32),
        scissor: std::array::from_fn(|i| m.u32(binding + 0x38 + alternate * 16 + i as u32 * 4) as i32),
        dimensions_58: [m.u16(binding + 0x58), m.u16(binding + 0x5a)],
    }
}

/// Translated from 0x00DA6FE0 (DDDA.exe 2017-12-13 Steam).
pub fn targets(m: &mut dyn Mem, c: &mut Cpu) {
    c.esp = c.esp.wrapping_sub(0x28);
    push(m, c, c.esi);
    c.esi = m.u32(c.esp + 0x30);
    push(m, c, c.edi);
    c.edi = m.u32(c.esp + 0x38);
    if m.u32(c.esi + 8) != c.edi {
        c.eax = m.u32(c.edi + 0x14);
        c.ecx = m.u32(c.esi + 4);
        c.eax >>= 9;
        push(m, c, c.ebx);
        c.eax &= !0xff | 1;
        push(m, c, c.ebp);
        m.set_u32(c.esp + 0x40, c.ecx);
        m.set_u8(c.esp + 0x3c, c.eax as u8);
        c.ebp = 0;
        c.ebx = c.esi + 0x84;
        for _ in 0..4 {
            c.eax = m.u32(c.edi + c.ebp * 4);
            if m.u32(c.ebx) != c.eax {
                m.set_u32(c.ebx, c.eax);
                c.eax = m.u32(c.edi + c.ebp * 4);
                if c.eax != 0 {
                    c.eax = m.u32(c.eax + 0x14);
                }
                c.edx = m.u32(c.ecx);
                push(m, c, c.eax);
                c.eax = m.u32(c.edx + 0x94);
                push(m, c, c.ebp);
                push(m, c, c.ecx);
                call_out(m, c, c.eax, 0xda7037);
                invalidate_viewport(m, c);
                c.ecx = m.u32(c.esp + 0x40);
            }
            c.ebp += 1;
            c.ebx += 4;
        }
        c.edx = m.u32(c.edi + 0x10);
        c.eax = c.edx;
        if m.u32(c.esi + 0x94) != c.eax {
            m.set_u32(c.esi + 0x94, c.eax);
            if c.eax != 0 {
                c.eax = m.u32(c.eax + 0x14);
            }
            c.edx = m.u32(c.ecx);
            push(m, c, c.eax);
            c.eax = m.u32(c.edx + 0x9c);
            push(m, c, c.ecx);
            call_out(m, c, c.eax, 0xda7096);
            invalidate_viewport(m, c);
        }
        let alternate = if m.u8(c.esp + 0x3c) != 0 { 16 } else { 0 };
        c.edx = m.u32(c.edi + 0x18 + alternate);
        c.ebx = m.u32(c.edi + 0x1c + alternate);
        c.ecx = m.u32(c.edi + 0x20 + alternate);
        c.eax = m.u32(c.edi + 0x24 + alternate);
        c.ebp = m.u32(c.edi + 0x58) & 0xffff;
        m.set_u32(c.esp + 0x28, c.ebp);
        if c.edx as i32 <= 0 {
            c.edx = 0;
        }
        if c.ebx as i32 <= 0 {
            c.ebx = 0;
        }
        c.ebp = m.u32(c.esp + 0x28);
        if c.ecx as i32 >= c.ebp as i32 {
            c.ecx = c.ebp;
        }
        c.ebp = m.u16(c.edi + 0x5a) as u32;
        if c.eax as i32 >= c.ebp as i32 {
            c.eax = c.ebp;
        }
        if c.edx as i32 > c.ecx as i32 {
            c.edx = 0;
            c.ecx = 0;
        }
        if c.ebx as i32 > c.eax as i32 {
            c.ebx = 0;
            c.eax = 0;
        }
        if c.ecx != m.u32(c.esi + 0x6c)
            || c.ebx != m.u32(c.esi + 0x68)
            || c.edx != m.u32(c.esi + 0x64)
            || c.eax != m.u32(c.esi + 0x70)
        {
            c.xmm[0] = [0; 4];
            m.set_u32(c.esi + 0x6c, c.ecx);
            m.set_u32(c.esi + 0x70, c.eax);
            c.ecx = c.ecx.wrapping_sub(c.edx);
            c.eax = c.eax.wrapping_sub(c.ebx);
            m.set_u32(c.esp + 0x28, c.ecx);
            m.set_u32(c.esp + 0x2c, c.eax);
            c.eax = m.u32(c.esp + 0x40);
            m.set_u32(c.esi + 0x64, c.edx);
            m.set_u32(c.esp + 0x20, c.edx);
            c.ecx = c.esp + 0x20;
            m.set_u32(c.esi + 0x68, c.ebx);
            c.edx = m.u32(c.eax);
            c.edx = m.u32(c.edx + 0xbc);
            push(m, c, c.ecx);
            m.set_u32(c.esp + 0x34, c.xmm[0][0]);
            c.xmm[0] = [m.u32(0x1406ce0), 0, 0, 0];
            push(m, c, c.eax);
            m.set_u32(c.esp + 0x2c, c.ebx);
            m.set_u32(c.esp + 0x3c, c.xmm[0][0]);
            call_out(m, c, c.edx, 0xda7174);
        }
        let alternate = if m.u8(c.esp + 0x3c) != 0 { 16 } else { 0 };
        c.ecx = m.u32(c.edi + 0x40 + alternate);
        c.edx = m.u32(c.edi + 0x38 + alternate);
        c.eax = m.u32(c.edi + 0x3c + alternate);
        m.set_u32(c.esp + 0x18, c.ecx);
        c.ecx = m.u32(c.edi + 0x44 + alternate);
        c.ebx = m.u32(c.esi + 0x64);
        m.set_u32(c.esp + 0x1c, c.ecx);
        m.set_u32(c.esp + 0x14, c.eax);
        m.set_u32(c.esp + 0x10, c.edx);
        if c.edx as i32 <= c.ebx as i32 {
            c.edx = c.ebx;
            m.set_u32(c.esp + 0x10, c.edx);
        }
        c.ebx = m.u32(c.esi + 0x68);
        if c.eax as i32 <= c.ebx as i32 {
            c.eax = c.ebx;
            m.set_u32(c.esp + 0x14, c.eax);
        }
        c.ebx = m.u32(c.esi + 0x6c);
        if m.u32(c.esp + 0x18) as i32 >= c.ebx as i32 {
            c.ebp = c.ebx;
            m.set_u32(c.esp + 0x18, c.ebp);
        } else {
            c.ebp = m.u32(c.esp + 0x18);
        }
        c.ebx = m.u32(c.esi + 0x70);
        if c.ecx as i32 >= c.ebx as i32 {
            c.ecx = c.ebx;
            m.set_u32(c.esp + 0x1c, c.ecx);
        }
        if c.edx as i32 > c.ebp as i32 {
            c.edx = 0;
            c.ebp = 0;
            m.set_u32(c.esp + 0x10, 0);
            m.set_u32(c.esp + 0x18, 0);
        }
        if c.eax as i32 > c.ecx as i32 {
            c.eax = 0;
            c.ecx = 0;
            m.set_u32(c.esp + 0x14, 0);
            m.set_u32(c.esp + 0x1c, 0);
        }
        if c.ebp != m.u32(c.esi + 0x7c)
            || c.eax != m.u32(c.esi + 0x78)
            || c.edx != m.u32(c.esi + 0x74)
            || c.ecx != m.u32(c.esi + 0x80)
        {
            m.set_u32(c.esi + 0x78, c.eax);
            c.eax = m.u32(c.esp + 0x40);
            m.set_u32(c.esi + 0x80, c.ecx);
            m.set_u32(c.esi + 0x74, c.edx);
            c.ecx = c.esp + 0x10;
            m.set_u32(c.esi + 0x7c, c.ebp);
            c.edx = m.u32(c.eax);
            c.edx = m.u32(c.edx + 0x12c);
            push(m, c, c.ecx);
            push(m, c, c.eax);
            call_out(m, c, c.edx, 0xda723f);
        }
        c.ebp = pop(m, c);
        m.set_u32(c.esi + 8, c.edi);
        c.ebx = pop(m, c);
    }
    c.edi = pop(m, c);
    c.esi = pop(m, c);
    c.esp = c.esp.wrapping_add(0x34);
}

fn invalidate_viewport(m: &mut dyn Mem, c: &mut Cpu) {
    c.ecx = m.u32(0x18d04e4);
    m.set_u32(c.esi + 0x64, c.ecx);
    c.edx = m.u32(0x18d04e8);
    m.set_u32(c.esi + 0x68, c.edx);
    c.eax = m.u32(0x18d04ec);
    m.set_u32(c.esi + 0x6c, c.eax);
    c.ecx = m.u32(0x18d04f0);
    m.set_u32(c.esi + 0x70, c.ecx);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stream {
    pub object: u32,
    pub handle_20: u32,
    pub bytes_14: u32,
    pub stride: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Streams {
    pub vertex: [Stream; 4],
    pub index_object_28: u32,
    pub index_handle: u32,
    pub instance_flags_24: u32,
}

/// Translated from 0x00DAA7C0 (DDDA.exe 2017-12-13 Steam).
/// Declaration identity belongs to the shader binding, not the streams.
pub fn decode_streams(m: &dyn Mem, binding: u32) -> Streams {
    let vertex = std::array::from_fn(|i| {
        let i = i as u32;
        let object = m.u32(binding + i * 4);
        if object == 0 {
            return Stream::default();
        }
        Stream {
            object,
            handle_20: m.u32(object + 0x20),
            bytes_14: m.u32(object + 0x14),
            stride: m.u8(binding + 0x10 + i) as u32,
            offset: m.u32(binding + 0x14 + i * 4),
        }
    });
    let index_object_28 = m.u32(binding + 0x28);
    Streams {
        vertex,
        index_object_28,
        index_handle: if index_object_28 == 0 {
            0
        } else {
            m.u32(index_object_28 + 0x20)
        },
        instance_flags_24: m.u32(binding + 0x24),
    }
}

/// Exact guest adapter, including scratch writes and registers at COM calls.
/// Translated from 0x00DAA7C0 (DDDA.exe 2017-12-13 Steam).
pub fn streams(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.esp = cpu.esp.wrapping_sub(12);
    push(m, cpu, cpu.ebp);
    cpu.ebp = m.u32(cpu.esp + 0x18);
    push(m, cpu, cpu.edi);
    cpu.edi = m.u32(cpu.esp + 0x18);
    if m.u32(cpu.edi + 0x14) != cpu.ebp {
        cpu.ecx = m.u32(cpu.ebp + 0x28);
        cpu.edx = m.u32(cpu.edi + 4);
        push(m, cpu, cpu.ebx);
        push(m, cpu, cpu.esi);
        cpu.esi = 0;
        cpu.eax = 0;
        m.set_u32(cpu.esp + 0x18, cpu.edx);
        if cpu.ecx != 0 {
            cpu.eax = m.u32(cpu.ecx + 0x20);
        }
        if m.u32(cpu.edi + 0x30) != cpu.eax {
            m.set_u32(cpu.edi + 0x30, cpu.eax);
            cpu.ecx = m.u32(cpu.edx);
            push(m, cpu, cpu.eax);
            push(m, cpu, cpu.edx);
            cpu.edx = m.u32(cpu.ecx + 0x1a0);
            call_out(m, cpu, cpu.edx, 0x00daa801);
        }
        cpu.eax = cpu.edi + 0x54;
        m.set_u32(cpu.esp + 0x24, cpu.esi);
        m.set_u32(cpu.esp + 0x10, cpu.eax);
        for _ in 0..4 {
            cpu.ecx = 0;
            cpu.ebx = 0;
            cpu.edi = 0;
            if m.u32(cpu.ebp + cpu.esi * 4) != 0 {
                cpu.eax = m.u32(cpu.ebp + cpu.esi * 4);
                cpu.ebx = m.u32(cpu.eax + 0x20);
                cpu.eax = m.u32(cpu.eax + 0x14);
                cpu.ecx = m.u32(cpu.ebp + cpu.esi * 4 + 0x14);
                cpu.edi = m.u8(cpu.ebp + cpu.esi + 0x10) as u32;
                cpu.eax = cpu.eax.wrapping_sub(cpu.ecx);
                cpu.edx = cpu.eax % cpu.edi;
                cpu.eax /= cpu.edi;
                let larger = m.u32(cpu.esp + 0x24) < cpu.eax;
                m.set_u32(cpu.esp + 0x14, cpu.eax);
                cpu.eax = cpu.esp + if larger { 0x14 } else { 0x24 };
                cpu.eax = m.u32(cpu.eax);
                m.set_u32(cpu.esp + 0x24, cpu.eax);
                cpu.eax = m.u32(cpu.esp + 0x10);
            }
            if m.u32(cpu.eax - 0x20) != cpu.ebx
                || m.u32(cpu.eax - 0x10) != cpu.edi
                || m.u32(cpu.eax) != cpu.ecx
            {
                push(m, cpu, cpu.edi);
                push(m, cpu, cpu.ecx);
                m.set_u32(cpu.eax - 0x20, cpu.ebx);
                m.set_u32(cpu.eax, cpu.ecx);
                m.set_u32(cpu.eax - 0x10, cpu.edi);
                cpu.eax = m.u32(cpu.esp + 0x20);
                cpu.edx = m.u32(cpu.eax);
                push(m, cpu, cpu.ebx);
                push(m, cpu, cpu.esi);
                push(m, cpu, cpu.eax);
                cpu.eax = m.u32(cpu.edx + 0x190);
                call_out(m, cpu, cpu.eax, 0x00daa87a);
                cpu.eax = m.u32(cpu.esp + 0x10);
            }
            cpu.esi += 1;
            cpu.eax += 4;
            m.set_u32(cpu.esp + 0x10, cpu.eax);
        }
        cpu.eax = m.u32(cpu.esp + 0x20);
        cpu.ecx = m.u32(cpu.esp + 0x24);
        cpu.esi = pop(m, cpu);
        m.set_u32(cpu.eax + 0xe20, cpu.ecx);
        m.set_u32(cpu.eax + 0x14, cpu.ebp);
        cpu.ebx = pop(m, cpu);
    }
    cpu.edi = pop(m, cpu);
    cpu.ebp = pop(m, cpu);
    cpu.esp = cpu.esp.wrapping_add(24);
}
