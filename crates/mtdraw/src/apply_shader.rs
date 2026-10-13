//! Shader/constant binding half of nDraw apply (0x00DAA8B0).
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};
use mtcore::machine::{call_out, pop, push};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShaderBinding {
    pub flags_00: u32,
    pub metadata_08: u32,
    pub values: u32,
    pub declaration: u32,
    pub vertex: ShaderStage,
    pub pixel: ShaderStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShaderStage {
    pub handle: u32,
    pub descriptors: u32,
    pub summary: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureBinding {
    pub slot: u32,
    pub sampler_object: u32,
    pub texture_object: u32,
    pub texture_handle: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstantKind {
    Float,
    Integer,
    Boolean,
    Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstantBinding {
    pub kind: ConstantKind,
    pub descriptor: u32,
    pub value_index: u32,
    /// Tagged source pointer, or texture object for a derived descriptor.
    pub value: u32,
    pub register: u32,
    pub count: u32,
}

impl ConstantBinding {
    /// Shared L117 range decoding; derived descriptors have texture sources.
    pub fn range(&self) -> Option<mtstate::ConstantRange> {
        (self.kind != ConstantKind::Derived)
            .then(|| mtstate::constant_range(self.descriptor, self.value, self.kind == ConstantKind::Float))
    }

    /// Dimensions or reciprocal dimensions. Unknown kinds retain earlier
    /// scratch contents; counts above one read beyond this temporary float4.
    /// Those cases return None: native callers must decline the command or
    /// preserve the original scratch behavior, never synthesize zeros.
    pub fn dimensions(&self, m: &dyn Mem) -> Option<[u32; 4]> {
        let g = Pages::of(m);
        if self.kind != ConstantKind::Derived || self.count > 1 {
            return None;
        }
        let kind = (self.descriptor >> 8) & 255;
        if kind != 3 && kind != 7 {
            return None;
        }
        Some(mtstate::derived_dimensions(
            [
                g.u16(m, self.value + 0x20) as u32,
                g.u16(m, self.value + 0x22) as u32 & 0x3fff,
                g.u16(m, self.value + 0x26) as u32 & 0x7ff,
            ],
            kind == 3,
        ))
    }
}

impl ShaderStage {
    /// Original descriptor order; vertex slots are relative (0..4), before
    /// adding D3D's 257. Resolving pipeline state can reuse mtstate::SAMPLER.
    pub fn textures<'a>(&self, m: &'a dyn Mem, values: u32) -> impl Iterator<Item = TextureBinding> + 'a {
        let g = Pages::of(m);
        let start = self.descriptors;
        let count = (self.summary >> 8) & 15;
        (0..count).map(move |i| {
            let d = g.u32(m, start + i * 4);
            let sampler_object = g.u32(m, values + ((d >> 12) & 1023) * 4);
            let texture_object = g.u32(m, values + (d >> 22) * 4);
            TextureBinding {
                slot: d & 4095,
                sampler_object,
                texture_object,
                texture_handle: g.u32(m, texture_object + 0x34),
            }
        })
    }

    /// Source descriptions only: no context suppression, conversions or calls.
    pub fn constants<'a>(&self, m: &'a dyn Mem, values: u32) -> impl Iterator<Item = ConstantBinding> + 'a {
        let g = Pages::of(m);
        let counts = [
            (self.summary >> 12) & 127,
            (self.summary >> 19) & 15,
            (self.summary >> 23) & 31,
            self.summary >> 28,
        ];
        let start = self.descriptors + ((self.summary >> 8) & 15) * 4;
        let mut cursor = start;
        counts.into_iter().enumerate().flat_map(move |(section, count)| {
            let begin = cursor;
            cursor += count * 4;
            (0..count).map(move |i| {
                let descriptor = g.u32(m, begin + i * 4);
                let value_index = (descriptor >> 1) & if section == 3 { 127 } else { 31 };
                ConstantBinding {
                    kind: [
                        ConstantKind::Float,
                        ConstantKind::Integer,
                        ConstantKind::Boolean,
                        ConstantKind::Derived,
                    ][section],
                    descriptor,
                    value_index,
                    value: g.u32(m, values + value_index * 4),
                    register: (descriptor >> 16) & 255,
                    count: descriptor >> 24,
                }
            })
        })
    }
}

/// Translated from 0x00DAA8B0 (DDDA.exe 2017-12-13 Steam).
pub fn decode_shader(m: &dyn Mem, binding: u32) -> ShaderBinding {
    let g = Pages::of(m);
    let metadata = g.u32(m, binding + 8);
    let descriptors = g.u32(m, metadata + 4);
    let summary = g.u32(m, metadata + 8);
    ShaderBinding {
        flags_00: g.u32(m, binding),
        metadata_08: metadata,
        values: binding + 12,
        declaration: g.u32(m, metadata + 0x18),
        vertex: ShaderStage {
            handle: g.u32(m, metadata + 0x10),
            descriptors,
            summary,
        },
        pixel: ShaderStage {
            handle: g.u32(m, metadata + 0x14),
            descriptors: descriptors + (summary & 255) * 4,
            summary: g.u32(m, metadata + 12),
        },
    }
}

/// Whole guest adapter. Translated from 0x00DAA8B0 (DDDA.exe 2017-12-13 Steam).
pub fn shader(m: &mut dyn Mem, c: &mut Cpu) {
    let g = Pages::of(m);
    c.esp -= 0x74;
    push(m, c, c.esi);
    c.esi = g.u32(m, c.esp + 0x80);
    push(m, c, c.edi);
    c.edi = g.u32(m, c.esp + 0x80);
    g.set_u32(m, c.esp + 0x18, c.ecx);
    if g.u32(m, c.edi + 0x10) != c.esi {
        c.eax = g.u32(m, c.esi + 8);
        g.set_u32(m, c.esp + 8, c.eax);
        c.eax = g.u32(m, c.eax + 0x18);
        push(m, c, c.ebx);
        push(m, c, c.ebp);
        c.ebp = g.u32(m, c.edi + 4);
        if g.u32(m, c.edi + 0x2c) != c.eax {
            g.set_u32(m, c.edi + 0x2c, c.eax);
            c.ecx = g.u32(m, c.ebp);
            c.edx = g.u32(m, c.ecx + 0x15c);
            push(m, c, c.eax);
            push(m, c, c.ebp);
            call_out(m, c, c.edx, 0xdaa8f4);
        }
        c.eax = g.u32(m, c.esi);
        c.ebx = u32::from(c.eax & 0x180000 == 0x180000);
        c.eax = g.u32(m, c.edi);
        c.ecx = (c.eax >> 25) & 1;
        if c.ecx != c.ebx {
            c.edx = c.eax ^ (((c.ebx << 25) ^ c.eax) & 0x2000000);
            g.set_u32(m, c.edi, c.edx);
            if c.ebx != 0 {
                c.eax = g.u32(m, c.ebp);
                c.ecx = g.u32(m, c.eax + 0xe4);
                push(m, c, 1);
                push(m, c, 15);
                push(m, c, c.ebp);
                call_out(m, c, c.ecx, 0xdaa944);
            } else {
                c.edx = g.u32(m, c.ebp);
                c.eax = g.u32(m, c.edx + 0xe4);
                push(m, c, 0);
                push(m, c, 15);
                push(m, c, c.ebp);
                call_out(m, c, c.eax, 0xdaaa16);
            }
        }
        if c.ebx != 0 {
            c.ecx = g.u32(m, c.edi);
            c.eax = g.u32(m, c.esi);
            c.edx = c.eax & 0x7f8;
            c.ebx = c.ecx >> 6;
            c.edx ^= c.ebx;
            if c.edx & 0x7fff8 != 0 {
                c.eax &= 0x7f8;
                c.ecx &= 0xfe0001ff;
                c.eax = (c.eax << 6) | c.ecx;
                g.set_u32(m, c.edi, c.eax);
                c.ecx = g.u32(m, c.esi);
                c.eax = g.u32(m, c.ebp);
                c.eax = g.u32(m, c.eax + 0xe4);
                c.ecx = (c.ecx >> 3) & 255;
                c.edx = g.u32(m, 0x1445a70 + c.ecx * 4);
                push(m, c, c.edx);
                push(m, c, 25);
                push(m, c, c.ebp);
                call_out(m, c, c.eax, 0xdaa99a);
            }
            c.eax = g.u32(m, c.esi);
            c.ecx = g.u32(m, c.edi);
            c.edx = (c.eax >> 10) ^ c.ecx;
            if c.edx & 0x1fe != 0 {
                c.eax = c.ecx ^ (((c.eax >> 10) ^ c.ecx) & 0x1fe);
                g.set_u32(m, c.edi, c.eax);
                c.ecx = g.u32(m, c.esi);
                c.eax = g.u32(m, c.ebp);
                c.edx = g.u32(m, c.eax + 0xe4);
                c.ecx = (c.ecx >> 11) & 255;
                push(m, c, c.ecx);
                push(m, c, 24);
                push(m, c, c.ebp);
                call_out(m, c, c.edx, 0xdaa9d5);
            }
        }
        c.ebx = g.u32(m, c.esp + 0x10);
        c.eax = g.u32(m, c.ebx + 4);
        g.set_u32(m, c.esp + 0x2c, c.eax);
        c.eax = g.u32(m, c.ebx + 0x10);
        g.set_u32(m, c.esp + 0x14, 0);
        if g.u32(m, c.edi + 0x24) != c.eax {
            g.set_u32(m, c.edi + 0x24, c.eax);
            c.ecx = g.u32(m, c.ebp);
            c.edx = g.u32(m, c.ecx + 0x170);
            push(m, c, c.eax);
            push(m, c, c.ebp);
            call_out(m, c, c.edx, 0xdaaa00);
            c.ecx = g.u32(m, c.esp + 0x14);
        } else {
            c.ecx = g.u32(m, c.edi + 0x10).wrapping_add(12);
            g.set_u32(m, c.esp + 0x14, c.ecx);
        }
        c.eax = g.u32(m, c.ebx + 8);
        c.edx = g.u32(m, c.ebx + 4);
        c.eax = c.edx + (c.eax & 255) * 4;
        g.set_u32(m, c.esp + 0x30, c.eax);
        c.eax = g.u32(m, c.ebx + 0x14);
        c.edx = 0;
        g.set_u32(m, c.esp + 0x18, 0);
        if g.u32(m, c.edi + 0x28) != c.eax {
            g.set_u32(m, c.edi + 0x28, c.eax);
            c.ecx = g.u32(m, c.ebp);
            c.edx = g.u32(m, c.ecx + 0x1ac);
            push(m, c, c.eax);
            push(m, c, c.ebp);
            call_out(m, c, c.edx, 0xdaaa52);
            c.ecx = g.u32(m, c.esp + 0x14);
            c.edx = 0;
        } else {
            c.eax = g.u32(m, c.edi + 0x10).wrapping_add(12);
            g.set_u32(m, c.esp + 0x18, c.eax);
        }
        if c.ecx == c.edx {
            g.set_u32(m, c.esp + 0x18, c.edx);
        }
        for off in [0x34, 0x38, 0x3c, 0x40, 0x1c] {
            g.set_u32(m, c.esp + off, c.edx);
        }
        if g.u32(m, c.ebx + 8) & 0xf00 != 0 {
            vertex_textures(m, c);
        }
        c.ebx = 0;
        c.edi += 0x118;
        loop {
            if g.u32(m, c.esp + c.ebx * 4 + 0x34) == 0 && g.u32(m, c.edi - 0x80) != 0 {
                push(m, c, 0);
                c.ecx = c.ebx + 257;
                g.set_u32(m, c.edi, 0);
                c.eax = g.u32(m, c.ebp);
                c.edx = g.u32(m, c.eax + 0x104);
                push(m, c, c.ecx);
                push(m, c, c.ebp);
                call_out(m, c, c.edx, 0xdaab8f);
            }
            c.ebx += 1;
            c.edi += 4;
            if c.ebx >= 4 {
                break;
            }
        }
        vertex_constants(m, c);
        c.ecx = g.u32(m, c.esp + 0x10);
        c.esi = g.u32(m, c.esp + 0x8c);
        g.set_u32(m, c.esp + 0x14, 0);
        if g.u32(m, c.ecx + 12) & 0xf00 != 0 {
            pixel_textures(m, c);
        }
        pixel_constants(m, c);
        c.eax = g.u32(m, c.esp + 0x8c);
        c.ecx = g.u32(m, c.esp + 0x88);
        c.ebp = pop(m, c);
        g.set_u32(m, c.ecx + 0x10, c.eax);
        c.ebx = pop(m, c);
    }
    c.edi = pop(m, c);
    c.esi = pop(m, c);
    c.esp += 0x80;
}

fn vertex_textures(m: &mut dyn Mem, c: &mut Cpu) {
    let g = Pages::of(m);
    loop {
        c.ecx = g.u32(m, c.esp + 0x1c);
        c.eax = g.u32(m, c.esp + 0x2c);
        c.eax = g.u32(m, c.eax + c.ecx * 4);
        c.edx = (c.eax >> 12) & 1023;
        c.ecx = g.u32(m, c.esi + c.edx * 4 + 12);
        c.edx = c.eax;
        c.eax &= 4095;
        c.edx = g.u32(m, c.esi + (c.edx >> 22) * 4 + 12);
        c.ebx = c.eax;
        c.eax = g.u32(m, c.edi + c.ebx * 4 + 0x98);
        g.set_u32(m, c.esp + 0x24, c.ecx);
        g.set_u32(m, c.esp + 0x28, c.edx);
        if c.eax != c.ecx {
            let mut changed = c.eax == 0;
            if !changed {
                c.edx = g.u32(m, c.eax + 12);
                changed = c.edx != g.u32(m, c.ecx + 12);
            }
            if changed {
                if c.eax != 0 {
                    c.eax += 0x30;
                }
                push(m, c, c.eax);
                c.eax = g.u32(m, c.esp + 0x28) + 0x30;
                push(m, c, c.eax);
                c.ecx = c.ebx + 257;
                push(m, c, c.ecx);
                c.ecx = g.u32(m, c.esp + 0x2c);
                push(m, c, c.edi);
                call_out(m, c, 0xda7250, 0xdaaafe);
                c.ecx = g.u32(m, c.esp + 0x24);
            }
            g.set_u32(m, c.edi + c.ebx * 4 + 0x98, c.ecx);
        }
        c.edx = g.u32(m, c.esp + 0x28);
        if g.u32(m, c.edi + c.ebx * 4 + 0x118) != c.edx {
            c.eax = c.edx;
            g.set_u32(m, c.edi + c.ebx * 4 + 0x118, c.eax);
            c.edx = g.u32(m, c.eax + 0x34);
            c.ecx = g.u32(m, c.ebp);
            c.ecx = g.u32(m, c.ecx + 0x104);
            push(m, c, c.edx);
            c.eax = c.ebx + 257;
            push(m, c, c.eax);
            push(m, c, c.ebp);
            call_out(m, c, c.ecx, 0xdaab36);
        }
        c.edx = g.u32(m, c.edi + c.ebx * 4 + 0x118);
        c.ecx = g.u32(m, c.esp + 0x10);
        c.eax = g.u32(m, c.esp + 0x1c);
        g.set_u32(m, c.esp + c.ebx * 4 + 0x34, c.edx);
        c.edx = g.u32(m, c.ecx + 8) >> 8;
        c.eax += 1;
        c.edx &= 15;
        g.set_u32(m, c.esp + 0x1c, c.eax);
        if c.eax >= c.edx {
            break;
        }
    }
}

fn pixel_textures(m: &mut dyn Mem, c: &mut Cpu) {
    let g = Pages::of(m);
    c.ebx = g.u32(m, c.esp + 0x88);
    loop {
        c.ecx = g.u32(m, c.esp + 0x14);
        c.eax = g.u32(m, c.esp + 0x30);
        c.eax = g.u32(m, c.eax + c.ecx * 4);
        c.edx = (c.eax >> 12) & 1023;
        c.ecx = g.u32(m, c.esi + c.edx * 4 + 12);
        c.edx = c.eax;
        c.eax &= 4095;
        c.edx = g.u32(m, c.esi + (c.edx >> 22) * 4 + 12);
        c.edi = c.eax;
        c.eax = g.u32(m, c.ebx + c.edi * 4 + 0xd8);
        g.set_u32(m, c.esp + 0x2c, c.ecx);
        g.set_u32(m, c.esp + 0x1c, c.edx);
        if c.eax != c.ecx {
            let mut changed = c.eax == 0;
            if !changed {
                c.edx = g.u32(m, c.eax + 12);
                changed = c.edx != g.u32(m, c.ecx + 12);
            }
            if changed {
                if c.eax != 0 {
                    c.eax += 0x30;
                }
                c.ecx = g.u32(m, c.esp + 0x20);
                push(m, c, c.eax);
                c.eax = g.u32(m, c.esp + 0x30) + 0x30;
                push(m, c, c.eax);
                push(m, c, c.edi);
                push(m, c, c.ebx);
                call_out(m, c, 0xda7250, 0xdaafc8);
                c.ecx = g.u32(m, c.esp + 0x2c);
            }
            c.edx = g.u32(m, c.esp + 0x1c);
            g.set_u32(m, c.ebx + c.edi * 4 + 0xd8, c.ecx);
        }
        if g.u32(m, c.ebx + c.edi * 4 + 0x158) != c.edx {
            c.eax = g.u32(m, c.esp + 0x1c);
            g.set_u32(m, c.ebx + c.edi * 4 + 0x158, c.eax);
            c.edx = g.u32(m, c.eax + 0x34);
            c.ecx = g.u32(m, c.ebp);
            c.eax = g.u32(m, c.ecx + 0x104);
            push(m, c, c.edx);
            push(m, c, c.edi);
            push(m, c, c.ebp);
            call_out(m, c, c.eax, 0xdaaffc);
            c.edx = g.u32(m, c.esp + 0x1c);
        }
        c.ecx = g.u32(m, c.esp + 0x20);
        if g.u8(m, c.ecx + 0x31) != 0 {
            c.eax = g.u32(m, c.edx + 0x20) >> 30;
            let enabled = c.eax & 1 != 0 && g.u32(m, c.edx + 0x2c) & 0x80000 == 0;
            c.ecx = g.u32(m, c.ebp);
            c.edx = g.u32(m, c.ecx + 0x114);
            push(m, c, u32::from(enabled));
            push(m, c, 11);
            push(m, c, c.edi);
            push(m, c, c.ebp);
            call_out(m, c, c.edx, if enabled { 0xdab02e } else { 0xdab050 });
            c.eax = 1;
            c.ecx = c.edi;
            c.eax <<= c.ecx & 31;
            if enabled {
                let v = g.u32(m, c.ebx + 0x198) | c.eax;
                g.set_u32(m, c.ebx + 0x198, v);
            } else {
                c.eax = !c.eax;
                let v = g.u32(m, c.ebx + 0x198) & c.eax;
                g.set_u32(m, c.ebx + 0x198, v);
            }
        }
        c.ecx = g.u32(m, c.esp + 0x10);
        c.edx = g.u32(m, c.ecx + 12);
        c.eax = g.u32(m, c.esp + 0x14);
        c.edx >>= 8;
        c.eax += 1;
        c.edx &= 15;
        g.set_u32(m, c.esp + 0x14, c.eax);
        if c.eax >= c.edx {
            break;
        }
    }
}

fn vertex_constants(m: &mut dyn Mem, c: &mut Cpu) {
    let g = Pages::of(m);
    c.eax = g.u32(m, c.esp + 0x10);
    c.eax = g.u32(m, c.eax + 8);
    c.edx = g.u32(m, c.esp + 0x2c);
    c.ecx = (c.eax >> 8) & 15;
    c.edi = 0;
    c.ebx = c.edx + c.ecx * 4;
    if c.eax & 0x7f000 != 0 {
        loop {
            c.ecx = g.u32(m, c.esp + 0x14);
            let mut changed = c.ecx == 0;
            if !changed {
                c.eax = (g.u32(m, c.ebx + c.edi * 4) >> 1) & 31;
                c.ecx = g.u32(m, c.ecx + c.eax * 4);
                changed = c.ecx != g.u32(m, c.esi + c.eax * 4 + 12);
            }
            if changed {
                float_upload(m, c, false);
            }
            c.eax = g.u32(m, c.esp + 0x10);
            c.ecx = g.u32(m, c.eax + 8);
            c.esi = g.u32(m, c.esp + 0x8c);
            c.ecx >>= 12;
            c.edi += 1;
            c.ecx &= 127;
            if c.edi >= c.ecx {
                break;
            }
        }
    }
    c.edx = g.u32(m, c.esp + 0x10);
    c.eax = g.u32(m, c.edx + 8);
    c.ecx = (c.eax >> 12) & 127;
    c.edi = 0;
    c.ebx += c.ecx * 4;
    c.ecx = g.u32(m, c.esp + 0x14);
    for (section, mask, shift, count_mask, slot, return_to) in [
        (0, 0x780000, 19, 15, 0x180, 0xdaad5a),
        (1, 0xf800000, 23, 31, 0x188, 0xdaadd1),
    ] {
        if section == 1 {
            c.eax = g.u32(m, c.edx + 8);
            c.edi = (c.eax >> 19) & 15;
            c.ebx += c.edi * 4;
            c.edi = 0;
        }
        if c.eax & mask != 0 {
            loop {
                let mut changed = c.ecx == 0;
                if !changed {
                    c.eax = (g.u32(m, c.ebx + c.edi * 4) >> 1) & 31;
                    c.edx = g.u32(m, c.ecx + c.eax * 4);
                    changed = c.edx != g.u32(m, c.esi + c.eax * 4 + 12);
                }
                if changed {
                    integer_upload(m, c, slot, return_to);
                    c.ecx = g.u32(m, c.esp + 0x14);
                }
                c.edx = g.u32(m, c.esp + 0x10);
                c.eax = g.u32(m, c.edx + 8) >> shift;
                c.edi += 1;
                c.eax &= count_mask;
                if c.edi >= c.eax {
                    break;
                }
            }
        }
    }
    c.eax = g.u32(m, c.edx + 8);
    c.ecx = (c.eax >> 23) & 31;
    c.esi = 0;
    c.ebx += c.ecx * 4;
    if c.eax & 0xf0000000 != 0 {
        c.edi = g.u32(m, c.esp + 0x8c);
        c.ecx = g.u32(m, c.esp + 0x14);
        derived_constants(m, c, false);
    }
}

fn pixel_constants(m: &mut dyn Mem, c: &mut Cpu) {
    let g = Pages::of(m);
    c.eax = g.u32(m, c.ecx + 12);
    c.edi = g.u32(m, c.esp + 0x30);
    c.edx = (c.eax >> 8) & 15;
    c.ebx = c.edi + c.edx * 4;
    c.edi = 0;
    if c.eax & 0x7f000 != 0 {
        loop {
            c.ecx = g.u32(m, c.esp + 0x18);
            let mut changed = c.ecx == 0;
            if !changed {
                c.eax = (g.u32(m, c.ebx + c.edi * 4) >> 1) & 31;
                c.ecx = g.u32(m, c.ecx + c.eax * 4);
                changed = c.ecx != g.u32(m, c.esi + c.eax * 4 + 12);
            }
            if changed {
                float_upload(m, c, true);
            }
            c.edx = g.u32(m, c.esp + 0x10);
            c.eax = g.u32(m, c.edx + 12);
            c.esi = g.u32(m, c.esp + 0x8c);
            c.eax >>= 12;
            c.edi += 1;
            c.eax &= 127;
            if c.edi >= c.eax {
                break;
            }
        }
        c.ecx = c.edx;
    }
    c.eax = g.u32(m, c.ecx + 12);
    c.edx = (c.eax >> 12) & 127;
    c.edi = 0;
    c.ebx += c.edx * 4;
    for (section, mask, shift, count_mask, slot, return_to) in [
        (0, 0x780000, 19, 15, 0x1bc, 0xdab22e),
        (1, 0xf800000, 23, 31, 0x1c4, 0xdab2a5),
    ] {
        if section == 1 {
            c.eax = g.u32(m, c.ecx + 12);
            c.edx = (c.eax >> 19) & 15;
            c.edi = 0;
            c.ebx += c.edx * 4;
        }
        if c.eax & mask != 0 {
            loop {
                c.edx = g.u32(m, c.esp + 0x18);
                let mut changed = c.edx == 0;
                if !changed {
                    c.eax = (g.u32(m, c.ebx + c.edi * 4) >> 1) & 31;
                    c.edx = g.u32(m, c.edx + c.eax * 4);
                    changed = c.edx != g.u32(m, c.esi + c.eax * 4 + 12);
                }
                if changed {
                    integer_upload(m, c, slot, return_to);
                    c.ecx = g.u32(m, c.esp + 0x10);
                }
                c.edx = g.u32(m, c.ecx + 12) >> shift;
                c.edi += 1;
                c.edx &= count_mask;
                if c.edi >= c.edx {
                    break;
                }
            }
        }
    }
    c.ecx = g.u32(m, c.ecx + 12);
    c.eax = (c.ecx >> 23) & 31;
    c.esi = 0;
    c.ebx += c.eax * 4;
    if c.ecx & 0xf0000000 != 0 {
        c.edi = g.u32(m, c.esp + 0x8c);
        c.ecx = g.u32(m, c.esp + 0x18);
        derived_constants(m, c, true);
    }
}

/// Float register lists, including the per-register integer conversion path.
fn float_upload(m: &mut dyn Mem, c: &mut Cpu, pixel: bool) {
    let g = Pages::of(m);
    c.edx = g.u32(m, c.ebx + c.edi * 4);
    c.eax = (c.edx >> 1) & 31;
    let count_slot = if pixel { 0x20 } else { 0x1c };
    let pointer_slot = if pixel { 0x28 } else { 0x24 };
    if pixel {
        c.eax = g.u32(m, c.esi + c.eax * 4 + 12);
        c.esi = g.u8(m, c.ebx + c.edi * 4 + 3) as u32;
        c.ecx = c.eax & !15;
        c.eax &= 15;
        g.set_u32(m, c.esp + 0x28, c.ecx);
        g.set_u32(m, c.esp + 0x20, c.esi);
        if c.eax != 0 {
            c.eax <<= 4;
            g.set_u32(m, c.esp + 0x30, c.eax);
            let shorter = c.eax < c.esi;
            g.set_u32(m, c.esp + 0x2c, c.esi);
            c.eax = c.esp + if shorter { 0x30 } else { 0x2c };
            c.esi = g.u32(m, c.eax);
            g.set_u32(m, c.esp + 0x20, c.esi);
        }
    } else {
        c.esi = g.u32(m, c.esi + c.eax * 4 + 12);
        c.eax = g.u8(m, c.ebx + c.edi * 4 + 3) as u32;
        g.set_u32(m, c.esp + 0x1c, c.eax);
        c.ecx = c.esi & !15;
        c.eax = c.esi & 15;
        g.set_u32(m, c.esp + 0x24, c.ecx);
        if c.eax != 0 {
            c.esi = g.u32(m, c.esp + 0x1c);
            c.eax <<= 4;
            g.set_u32(m, c.esp + 0x2c, c.eax);
            let shorter = c.eax < c.esi;
            g.set_u32(m, c.esp + 0x28, c.esi);
            c.eax = c.esp + if shorter { 0x2c } else { 0x28 };
            c.eax = g.u32(m, c.eax);
            g.set_u32(m, c.esp + 0x1c, c.eax);
        }
    }
    c.eax = (c.edx >> 6) & 1023;
    c.ecx += c.eax * 4;
    if c.edx & 1 != 0 {
        c.esi = 0;
        while c.esi < g.u32(m, c.esp + count_slot) {
            c.ecx = (g.u32(m, c.ebx + c.edi * 4) >> 6) & 1023;
            c.eax = g.u32(m, c.esp + pointer_slot);
            c.edx = c.ecx + c.esi * 4;
            c.xmm[0] = [g.u32(m, c.eax + c.edx * 4), 0, 0, 0];
            c.ecx = g.u32(m, c.ebp);
            c.ecx = g.u32(m, c.ecx + if pixel { 0x1b4 } else { 0x178 });
            c.eax += c.edx * 4;
            // MOVD cleared the upper lanes. CVTSI2SS gives the identical low
            // result and MXCSR effects; converting the three zero lanes is exact.
            c.xmm[0] = [x86::cvtsi2ss(c.xmm[0][0]), 0, 0, 0];
            g.set_u32(m, c.esp + 0x34, c.xmm[0][0]);
            for (source, dest) in [(4, 0x38), (8, 0x3c)] {
                c.xmm[0] = [g.u32(m, c.eax + source), 0, 0, 0];
                c.xmm[0] = [x86::cvtsi2ss(c.xmm[0][0]), 0, 0, 0];
                g.set_u32(m, c.esp + dest, c.xmm[0][0]);
            }
            c.xmm[0] = [g.u32(m, c.eax + 12), 0, 0, 0];
            c.eax = g.u8(m, c.ebx + c.edi * 4 + 2) as u32;
            push(m, c, 1);
            c.edx = c.esp + 0x38;
            push(m, c, c.edx);
            c.eax += c.esi;
            push(m, c, c.eax);
            c.xmm[0] = [x86::cvtsi2ss(c.xmm[0][0]), 0, 0, 0];
            push(m, c, c.ebp);
            g.set_u32(m, c.esp + 0x50, c.xmm[0][0]);
            call_out(m, c, c.ecx, if pixel { 0xdab18a } else { 0xdaacac });
            c.esi += 1;
        }
    } else if pixel {
        c.eax = g.u8(m, c.ebx + c.edi * 4 + 2) as u32;
        c.edx = g.u32(m, c.ebp);
        push(m, c, c.esi);
        push(m, c, c.ecx);
        c.ecx = g.u32(m, c.edx + 0x1b4);
        push(m, c, c.eax);
        push(m, c, c.ebp);
        call_out(m, c, c.ecx, 0xdab1a7);
    } else {
        c.eax = g.u32(m, c.esp + 0x1c);
        c.edx = g.u32(m, c.ebp);
        c.edx = g.u32(m, c.edx + 0x178);
        push(m, c, c.eax);
        push(m, c, c.ecx);
        c.ecx = g.u8(m, c.ebx + c.edi * 4 + 2) as u32;
        push(m, c, c.ecx);
        push(m, c, c.ebp);
        call_out(m, c, c.edx, 0xdaaccd);
    }
}

fn integer_upload(m: &mut dyn Mem, c: &mut Cpu, slot: u32, return_to: u32) {
    let g = Pages::of(m);
    c.edx = g.u8(m, c.ebx + c.edi * 4 + 3) as u32;
    c.eax = g.u32(m, c.ebx + c.edi * 4);
    c.ecx = g.u32(m, c.ebp);
    push(m, c, c.edx);
    c.edx = (c.eax >> 1) & 31;
    c.edx = g.u32(m, c.esi + c.edx * 4 + 12);
    c.eax >>= 6;
    c.edx &= !15;
    c.eax &= 1023;
    c.eax = c.edx + c.eax * 4;
    c.edx = g.u8(m, c.ebx + c.edi * 4 + 2) as u32;
    push(m, c, c.eax);
    c.eax = g.u32(m, c.ecx + slot);
    push(m, c, c.edx);
    push(m, c, c.ebp);
    call_out(m, c, c.eax, return_to);
}

/// Derived dimensions use the original x87 integer load/divide for width,
/// SSE conversions/divisions for height/depth, including their status bits.
fn derived_constants(m: &mut dyn Mem, c: &mut Cpu, pixel: bool) {
    let g = Pages::of(m);
    loop {
        let mut changed = c.ecx == 0;
        if !changed {
            c.eax = (g.u32(m, c.ebx + c.esi * 4) >> 1) & 127;
            c.edx = g.u32(m, c.ecx + c.eax * 4);
            changed = c.edx != g.u32(m, c.edi + c.eax * 4 + 12);
        }
        if changed {
            c.eax = g.u8(m, c.ebx + c.esi * 4 + 1) as u32;
            if c.eax == 3 || c.eax == 7 {
                let reciprocal = c.eax == 3;
                let temporary = if pixel { 0x30 } else { 0x2c };
                c.eax = (g.u32(m, c.ebx + c.esi * 4) >> 1) & 127;
                c.eax = g.u32(m, c.edi + c.eax * 4 + 12);
                c.ecx = g.u16(m, c.eax + 0x20) as u32;
                g.set_u32(m, c.esp + temporary, c.ecx);
                if reciprocal {
                    c.xmm[0] = [g.u32(m, 0x1406ce0), 0, 0, 0];
                    c.st.memory::<0x00db, 4, false>(m, c.esp + temporary);
                    c.ecx = 0x3fff;
                    c.st.register::<0xe8d9>();
                    c.xmm[1] = [0; 4];
                    c.st.register::<0xf1de>();
                    c.xmm[2] = c.xmm[0];
                } else {
                    c.ecx = 0x3fff;
                    c.st.memory::<0x00db, 4, false>(m, c.esp + temporary);
                    c.xmm[0] = [0; 4];
                }
                c.st.memory::<0x18d9, 4, true>(m, c.esp + 0x44);
                c.edx = (g.u16(m, c.eax + 0x22) as u32) & c.ecx;
                if reciprocal {
                    c.xmm[1][0] = x86::cvtsi2ss(c.edx);
                    c.xmm[2][0] = x86::divss(c.xmm[2][0], c.xmm[1][0]);
                    g.set_u32(m, c.esp + 0x48, c.xmm[2][0]);
                } else {
                    c.xmm[0][0] = x86::cvtsi2ss(c.edx);
                    g.set_u32(m, c.esp + 0x48, c.xmm[0][0]);
                }
                c.eax = (c.eax & !0xffff) | (g.u16(m, c.eax + 0x26) as u32);
                c.ecx = 0x7ff;
                c.eax = (c.eax & !0xffff) | (c.eax & c.ecx);
                c.edx = c.eax & 0xffff;
                if reciprocal {
                    c.xmm[1] = [0; 4];
                    c.xmm[1][0] = x86::cvtsi2ss(c.edx);
                    c.xmm[0][0] = x86::divss(c.xmm[0][0], c.xmm[1][0]);
                } else {
                    c.xmm[0] = [0; 4];
                    c.xmm[0][0] = x86::cvtsi2ss(c.edx);
                }
                g.set_u32(m, c.esp + 0x4c, c.xmm[0][0]);
                c.xmm[0] = [0; 4];
                g.set_u32(m, c.esp + 0x50, 0);
            }
            c.ecx = g.u8(m, c.ebx + c.esi * 4 + 3) as u32;
            c.eax = g.u32(m, c.ebp);
            push(m, c, c.ecx);
            c.ecx = g.u8(m, c.ebx + c.esi * 4 + 2) as u32;
            c.edx = c.esp + 0x48;
            push(m, c, c.edx);
            c.edx = g.u32(m, c.eax + if pixel { 0x1b4 } else { 0x178 });
            push(m, c, c.ecx);
            push(m, c, c.ebp);
            call_out(m, c, c.edx, if pixel { 0xdab3ed } else { 0xdaaf1d });
            c.ecx = g.u32(m, c.esp + if pixel { 0x18 } else { 0x14 });
        }
        c.eax = g.u32(m, c.esp + 0x10);
        c.edx = g.u32(m, c.eax + if pixel { 12 } else { 8 });
        c.esi += 1;
        c.edx >>= 28;
        if c.esi >= c.edx {
            break;
        }
    }
}
