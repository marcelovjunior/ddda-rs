//! ABI-level model draw ports. Object offsets and arithmetic come from the
//! 2017-12-13 Steam executable; buffers/tables remain in guest memory.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

struct DrawContext<'a> {
    memory: &'a mut dyn Mem,
    cpu: &'a mut Cpu,
    pages: Pages,
    owner: u32,
    entry: u32,
    frame: u32,
}
impl DrawContext<'_> {
    fn read(&self, a: u32) -> u32 {
        self.pages.u32(self.memory, a)
    }
    fn write(&mut self, a: u32, v: u32) {
        self.pages.set_u32(self.memory, a, v);
    }
    fn field(&self, offset: u32) -> u32 {
        self.read(self.owner.wrapping_add(offset))
    }
    fn set(&mut self, offset: u32, v: u32) {
        self.write(self.owner.wrapping_add(offset), v);
    }
    fn call(&mut self, target: u32, resume: u32, this: u32, args: &[u32]) -> u32 {
        self.cpu.esp = self.frame;
        for &v in args.iter().rev().chain(std::iter::once(&resume)) {
            self.cpu.esp = self.cpu.esp.wrapping_sub(4);
            self.write(self.cpu.esp, v);
        }
        self.cpu.ecx = this;
        crate::semantic::call_guest(self.memory, self.cpu, target);
        self.cpu.eax
    }
    fn allocate_record(&mut self, size: u32, resume: u32) -> u32 {
        if self.field(0x24).wrapping_add(size) > self.field(0x28) {
            let chunk = size.wrapping_add(0xfff) & !0xfff;
            let p = self.call(0xda7d70, resume, self.read(0x18d08f4), &[chunk]);
            self.set(0x24, p);
            self.set(0x28, if p == 0 { 0 } else { p.wrapping_add(chunk) });
        }
        let p = self.field(0x24);
        self.set(0x24, p.wrapping_add(size));
        p
    }
    // MOVQ loads the entire eight bytes before either output dword changes.
    fn copy_record(&mut self, destination_record: u32, source_record: u32, bytes: u32) {
        for k in 0..bytes / 8 {
            let off = k * 8;
            let lo = self.read(source_record.wrapping_add(off));
            let hi = self.read(source_record.wrapping_add(off + 4));
            self.write(destination_record.wrapping_add(off), lo);
            self.write(destination_record.wrapping_add(off + 4), hi);
        }
        if bytes & 4 != 0 {
            let off = bytes & !7;
            let v = self.read(source_record.wrapping_add(off));
            self.write(destination_record.wrapping_add(off), v);
        }
    }
    fn duplicate(&mut self, source_record: u32, kind: u32, resumes: [u32; 4]) -> u32 {
        if kind > 3 {
            return 0;
        }
        let destination_record = self.allocate_record(if kind < 2 { 32 } else { 48 }, resumes[kind as usize]);
        self.copy_record(destination_record, source_record, 28 + kind * 4);
        destination_record
    }
    fn mark_dirty(&mut self, bits: u32) {
        self.set(0x154, self.field(0x154) | bits);
    }
    fn set_shader_binding(&mut self, index: u32, packed: u32) -> bool {
        if self.field(0x2248) == self.read(self.field(0x20).wrapping_add(index.wrapping_mul(8))) {
            return false;
        }
        if self.pages.u8(self.memory, self.owner.wrapping_add(0x5c)) & 0x80 != 0 {
            let log = self
                .owner
                .wrapping_add(0x30c7c)
                .wrapping_add(self.field(0x6c).wrapping_mul(12));
            self.write(log, 0x40a);
            self.write(log.wrapping_add(4), self.field(0x2248));
            self.write(log.wrapping_add(8), self.field(0x224c));
            self.set(0x6c, self.field(0x6c).wrapping_add(1));
        }
        self.set(
            0x2248,
            self.read(self.field(0x20).wrapping_add(index.wrapping_mul(8))),
        );
        self.set(0x224c, packed);
        self.mark_dirty(3);
        true
    }
}

// Translated from 0x00ff1920 (DDDA.exe 2017-12-13 Steam).
/// Build the auxiliary shadow draw state and append copies of the selected
/// draw records. All guest callees retain their original ABI and return sites.
pub fn shadow_draws(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let pages = Pages::of(memory);
    let entry = cpu.esp;
    let owner = cpu.ecx;
    let mut draw = DrawContext {
        memory,
        cpu,
        pages,
        owner,
        entry,
        frame: entry.wrapping_sub(0x3c),
    };
    shadow_draws_inner(&mut draw);
    draw.cpu.esp = entry.wrapping_add(4);
}

fn shadow_draws_inner(draw: &mut DrawContext<'_>) {
    let flags = draw.field(0x5c);
    draw.cpu.eax = flags;
    if flags & 0x40 != 0 {
        return;
    }
    if flags & 0x10 != 0 {
        draw.cpu.eax = flags & !0x10;
        draw.set(0x5c, draw.cpu.eax);
        return;
    }
    let end = draw.field(0x48);
    draw.set(0x5c, flags & !8);
    let start = draw.field(0x60);
    draw.cpu.eax = start;
    if draw.field(0xc8) & 0x400 != 0 {
        draw.frame = draw.entry.wrapping_sub(0x30);
        draw.call(0xfeef80, 0xff1967, draw.owner, &[]);
        draw.frame = draw.entry.wrapping_sub(0x3c);
    }
    if draw.field(0x1dc) == 0 {
        return;
    }
    let pass = draw.field(0x164) & 31;
    draw.cpu.eax = pass;
    let bit = match pass {
        11 | 12 => 0x10,
        15 | 16 => 0x20,
        _ => return,
    };
    if draw.pages.u8(draw.memory, draw.owner.wrapping_add(0xc8)) & bit == 0 || draw.field(0x190) & 0x100 == 0
    {
        return;
    }
    let material = draw.read(draw.field(0x194).wrapping_add(12)).wrapping_add(16);
    if draw.read(material) & 3 != 3 {
        return;
    }
    let shadow_pass = match pass {
        16 => 17,
        15 => 15,
        _ => 3,
    };
    if pass != 16 {
        draw.cpu.eax = shadow_pass;
    }
    if draw.field(0x1a8) == 0 {
        let out = draw.allocate_record(0x60, 0xff1a26);
        // REP MOVSD, including its sequential overlap and DF behavior.
        for i in 0..23u32 {
            let off = if draw.cpu.df {
                0u32.wrapping_sub(i * 4)
            } else {
                i * 4
            };
            let v = draw.read(draw.owner.wrapping_add(0xb4).wrapping_add(off));
            draw.write(out.wrapping_add(off), v);
        }
        draw.write(out.wrapping_add(20), (draw.read(out.wrapping_add(20)) & !6) | 1);
        if draw.field(0x1d4) == 0 {
            draw.cpu.eax = draw.field(0x1d8);
            draw.write(out, draw.cpu.eax);
            draw.write(out.wrapping_add(16), draw.field(0x1dc));
        } else {
            let mode = if shadow_pass == 15 { 17 } else { shadow_pass };
            let value = draw.call(
                0x67fb90,
                if shadow_pass == 15 { 0xff1a99 } else { 0xff1a7f },
                draw.field(0x1d4),
                &[mode, 0],
            );
            draw.write(out, value);
            let value = draw.call(
                0xc6cbd0,
                if shadow_pass == 15 { 0xff1aa8 } else { 0xff1a8d },
                draw.field(0x1d4),
                &[mode],
            );
            draw.write(out.wrapping_add(16), value);
        }
        if draw.read(out.wrapping_add(16)) == 0 {
            return;
        }
        draw.set(0x1a8, out);
    }
    let mut alpha = draw.pages.u8(draw.memory, draw.owner.wrapping_add(0x114)) as u32;
    draw.cpu.eax = draw.read(material);
    let mut sampler_offset = 0xdc0;
    if draw.cpu.eax & 0x40 != 0 && draw.cpu.eax & 0x7f8000 != 0 {
        draw.cpu.eax = (draw.read(material.wrapping_add(4)) & 15).wrapping_sub(1);
        sampler_offset = match draw.cpu.eax {
            0 => 0xda0,
            1 => 0xd28,
            2 | 5 => {
                alpha = 1;
                0xda8
            }
            3 | 6 => {
                alpha = 1;
                0xdb0
            }
            4 => {
                alpha = 255;
                0xdb8
            }
            _ => 0xdc0,
        };
    }
    let sampler = draw.field(sampler_offset);
    if draw.field(0x1ac) == 0 {
        let out = draw.allocate_record(32, 0xff1b5a);
        draw.cpu.eax = out;
        draw.copy_record(out, draw.owner.wrapping_add(0x110), 20);
        draw.write(out.wrapping_add(8), sampler);
        draw.set(0x1ac, out);
    }
    if draw.field(0x1a4) == 0 {
        let state = draw.field(0x154);
        let index = draw.field(0x224c) & 0xfff;
        let saved = (draw.read(draw.field(0x1c).wrapping_add(index * 8)) << 12) | index;
        let old = draw.field(0xcc8) & !15;
        let bias = if shadow_pass == 3 { draw.read(0x143a7f8) } else { 0 };
        // Preserve x87 extended intermediate alpha in the subtraction/division
        // and comparison. Rounding it to f32 before those operations differs.
        let mut operand = [0u8; 10];
        operand[..4].copy_from_slice(&((state >> 11) & 255).to_le_bytes());
        draw.cpu.st.exec::<0x00db>(&mut operand);
        operand[..4].copy_from_slice(&draw.read(0x161c29c).to_le_bytes());
        draw.cpu.st.exec::<0x00d9>(&mut operand);
        draw.cpu.st.register::<0xc9dc>();
        draw.cpu.st.register::<0xc9d9>();
        draw.cpu.st.exec::<0x10d9>(&mut operand);
        let a = u32::from_le_bytes(operand[..4].try_into().unwrap());
        operand[..4].copy_from_slice(&alpha.to_le_bytes());
        draw.cpu.st.exec::<0x00db>(&mut operand);
        draw.cpu.st.register::<0xcade>();
        draw.cpu.st.register::<0xc9d9>();
        draw.cpu.st.exec::<0x18d9>(&mut operand);
        let b = u32::from_le_bytes(operand[..4].try_into().unwrap());
        operand[..4].copy_from_slice(&draw.read(0x143a7f4).to_le_bytes());
        draw.cpu.st.exec::<0x00d9>(&mut operand);
        draw.cpu.st.register::<0xe1d8>();
        draw.cpu.st.register::<0xe8d9>();
        draw.cpu.st.register::<0xf1de>();
        draw.cpu.st.exec::<0x18d9>(&mut operand);
        let reciprocal = u32::from_le_bytes(operand[..4].try_into().unwrap());
        operand[..4].copy_from_slice(&draw.read(old).to_le_bytes());
        draw.cpu.st.exec::<0x00d9>(&mut operand);
        let compare = draw.cpu.st.compare_flags::<0xe9df>().snapshot().0;
        draw.cpu.st.register::<0xd8dd>();
        let equal = compare & 0x44 == 0x40;
        if !equal
            || f32::from_bits(draw.read(old.wrapping_add(4))) != f32::from_bits(b)
            || f32::from_bits(draw.read(old.wrapping_add(8))) != f32::from_bits(bias)
            || f32::from_bits(draw.read(old.wrapping_add(12))) != f32::from_bits(reciprocal)
        {
            let descriptor = draw.read(draw.field(0x18).wrapping_add(0x568));
            let size = (draw.pages.u16(draw.memory, descriptor.wrapping_add(24)) as u32 * 4 + 15) & !15;
            let out = draw.allocate_record(size, 0xff1cdf);
            draw.set(0xcc8, out);
            draw.mark_dirty(1);
            if out != 0 {
                let len = draw.pages.u16(draw.memory, descriptor.wrapping_add(24)) as u32 * 4;
                let source = draw.read(descriptor.wrapping_add(36));
                crate::semantic::copy_guest(draw.memory, draw.pages, out, source, len);
                draw.cpu.eax = out; // memcpy returns its destination
                for (i, v) in [a, b, bias, reciprocal].into_iter().enumerate() {
                    draw.write(out.wrapping_add(i as u32 * 4), v);
                }
                operand[..4].copy_from_slice(&draw.field(0x17c).to_le_bytes());
                draw.cpu.st.exec::<0x00d9>(&mut operand);
                draw.cpu.st.exec::<0x18d9>(&mut operand);
                draw.write(
                    out.wrapping_add(16),
                    u32::from_le_bytes(operand[..4].try_into().unwrap()),
                );
            }
        }
        if draw.field(0x154) & 0x100000 == 0 {
            draw.set_shader_binding(0x40a, 0x40a);
        } else if state & 4 != 0 {
            draw.set_shader_binding(0x414, 0xcd3ad414);
        } else {
            let key = draw.read(0x1447b5cu32.wrapping_add(((draw.field(0x154) >> 3) & 255) * 4));
            draw.set_shader_binding(key & 0xfff, ((key ^ 0x84f11fff) & !0xfff) | (key & 0xfff));
        }
        let texture = draw.read(draw.field(0x1a8));
        let format = if texture == 0 {
            13
        } else {
            (draw.read(texture.wrapping_add(32)) >> 20) & 15
        };
        let mode = draw.pages.u8(draw.memory, draw.owner.wrapping_add(0x169));
        let shader = draw.call(
            0xe132b0,
            0xff1fb6,
            draw.read(0x18d12cc),
            &[
                draw.owner.wrapping_add(4),
                draw.field(0x1cc),
                if mode == 255 { 0 } else { mode as u32 },
                draw.owner.wrapping_add(0x1f8),
                draw.owner.wrapping_add(0x1bc),
                format,
                draw.field(0x1d0),
            ],
        );
        draw.cpu.eax = draw.field(0x20);
        if draw.set_shader_binding(saved & 0xfff, ((saved ^ 0x84f11fff) & !0xfff) | (saved & 0xfff)) {
            draw.cpu.eax = draw.field(0x2248);
        }
        if shader == 0 {
            return;
        }
        let count = draw.read(shader.wrapping_add(32));
        let out = draw.allocate_record(count.wrapping_mul(4).wrapping_add(27) & !15, 0xff2097);
        draw.cpu.eax = out;
        draw.copy_record(out, draw.owner.wrapping_add(0x154), 12);
        draw.write(out.wrapping_add(8), shader);
        draw.write(out, (draw.read(out) & 0xfff00007) | (state & 0x7fff8));
        let keys = draw.read(shader.wrapping_add(28));
        for i in 0..count {
            let k = draw.read(keys.wrapping_add(i.wrapping_mul(4))) & 0xfff;
            let v = draw.field(0x1f8 + k * 8);
            draw.write(out.wrapping_add(12).wrapping_add(i.wrapping_mul(4)), v);
        }
        draw.set(0x1a4, out);
    }
    for i in start..end {
        let source = draw.field(0x44).wrapping_add(i.wrapping_mul(8));
        let source_record = draw.read(source.wrapping_add(4));
        let kind = draw.pages.u16(draw.memory, source_record.wrapping_add(2)) as u32 & 15;
        draw.cpu.eax = kind;
        if kind > 3 {
            continue;
        }
        let out = draw.duplicate(source_record, kind, [0xff216c, 0xff21cf, 0xff2236, 0xff22a0]);
        for (offset, field) in [(12, 0x1a4), (4, 0x1a8), (8, 0x1ac)] {
            draw.write(out.wrapping_add(offset), draw.field(field));
        }
        let count = draw.field(0x48);
        let dest = draw.field(0x44).wrapping_add(count.wrapping_mul(8));
        draw.set(0x48, count.wrapping_add(1));
        draw.write(dest, (draw.read(dest) & 0x7ffffff) | (draw.field(0x164) << 27));
        let packed = draw.field(0x164);
        let offset = draw.field(0x178);
        draw.write(dest.wrapping_add(4), out);
        draw.write(
            dest,
            ((packed >> 5).wrapping_add(offset) & 0x7ffffff) | (shadow_pass << 27),
        );
        let key = (draw.read(source) >> 5).wrapping_add(draw.field(0x178));
        draw.write(dest, (key & 0x7ffffff) | (draw.read(dest) & !0x7ffffff));
        draw.cpu.eax = dest;
    }
}

// Translated from 0x00ff14d0 (DDDA.exe 2017-12-13 Steam).
/// Replay material changes backwards for each additional draw pass.
pub fn replay_draw_passes(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let pages = Pages::of(memory);
    let entry = cpu.esp;
    let owner = cpu.ecx;
    let mut draw = DrawContext {
        memory,
        cpu,
        pages,
        owner,
        entry,
        frame: entry.wrapping_sub(0x38),
    };
    let flags = draw.field(0x5c);
    draw.cpu.eax = flags;
    if flags & 0x40 != 0 {
        draw.cpu.eax = flags & !0x40;
        draw.set(0x5c, draw.cpu.eax);
        draw.cpu.esp = entry.wrapping_add(4);
        return;
    }
    if flags & 0x80 != 0 {
        let passes = draw.field(0x68).wrapping_sub(1);
        let end = draw.field(0x48);
        draw.set(0x5c, flags & !0x80);
        let start = draw.field(0x64);
        draw.cpu.eax = start;
        let count = end.wrapping_sub(start);
        for pass in 0..passes {
            let callback = draw.field(0x78);
            let target = draw.read(draw.read(callback).wrapping_add(0x34));
            if draw.call(target, 0xff152d, callback, &[owner, pass]) & 255 == 0 {
                continue;
            }
            let material = draw.read(draw.field(0x20).wrapping_add((draw.field(0x1cc) & 0xfff) * 8));
            let mut cursor = draw.field(0x6c);
            let mut previous = u32::MAX;
            let mut rejected = false;
            let mut i = count.wrapping_sub(1);
            while (i as i32) >= 0 {
                let source = draw
                    .field(0x44)
                    .wrapping_add(start.wrapping_add(i).wrapping_mul(8));
                let source_record = draw.read(source.wrapping_add(4));
                while cursor > draw.read(source_record) >> 20 {
                    cursor = cursor.wrapping_sub(1);
                    let log = owner.wrapping_add(0x30c7c).wrapping_add(cursor.wrapping_mul(12));
                    let tag = draw.read(log);
                    let index = tag & 0x7fffffff;
                    if tag & 0x80000000 != 0 {
                        draw.set(
                            0x1bcu32.wrapping_add(index.wrapping_mul(4)),
                            draw.read(log.wrapping_add(8)),
                        );
                    } else {
                        draw.set(
                            0x1f8u32.wrapping_add(index.wrapping_mul(8)),
                            draw.read(log.wrapping_add(4)),
                        );
                        draw.set(
                            0x1fcu32.wrapping_add(index.wrapping_mul(8)),
                            draw.read(log.wrapping_add(8)),
                        );
                    }
                    draw.mark_dirty(3);
                }
                if previous != cursor {
                    previous = cursor;
                    let mode = draw.pages.u8(draw.memory, owner.wrapping_add(0x169)) as u32;
                    rejected = draw.call(0xfee320, 0xff160e, owner, &[material, mode]) & 255 == 0;
                }
                if rejected {
                    let dest = draw
                        .field(0x44)
                        .wrapping_add(draw.field(0x48).wrapping_add(i).wrapping_mul(8));
                    draw.write(dest, (draw.read(dest) & 0x7ffffff) | (draw.field(0x164) << 27));
                    draw.write(
                        dest,
                        (draw.read(dest) & !0x7ffffff) | (draw.read(source) & 0x7ffffff),
                    );
                    let out = draw.allocate_record(16, 0xff1686);
                    draw.write(out, (draw.read(out) & 0xfffaffff) | 0xa0000);
                    draw.write(dest.wrapping_add(4), out);
                } else {
                    let kind = draw.pages.u16(draw.memory, source_record.wrapping_add(2)) as u32 & 15;
                    let out = draw.duplicate(source_record, kind, [0xff16d6, 0xff1735, 0xff1798, 0xff17fe]);
                    for (offset, field) in [(12, 0x198), (8, 0x194), (4, 0x1a0)] {
                        draw.write(out.wrapping_add(offset), draw.field(field));
                    }
                    let dest = draw
                        .field(0x44)
                        .wrapping_add(draw.field(0x48).wrapping_add(i).wrapping_mul(8));
                    draw.write(dest, (draw.read(dest) & 0x7ffffff) | (draw.field(0x164) << 27));
                    draw.write(
                        dest,
                        (draw.read(dest) & !0x7ffffff) | (draw.read(source) & 0x7ffffff),
                    );
                    draw.write(dest.wrapping_add(4), out);
                }
                i = i.wrapping_sub(1);
            }
            let callback = draw.field(0x78);
            draw.set(0x48, draw.field(0x48).wrapping_add(count));
            let target = draw.read(draw.read(callback).wrapping_add(0x38));
            draw.call(target, 0xff18d2, callback, &[owner, pass]);
        }
    }
    draw.set(0x5c, draw.field(0x5c) & 0xffffff1f);
    draw.set(0x78, 0);
    draw.cpu.esp = entry.wrapping_add(4);
}

// Translated from 0x00fefb90 (DDDA.exe 2017-12-13 Steam).
/// Bind one render target and update its viewport constants. Four stack words
/// are consumed (RET 0x10); the fourth is unused by this entry.
pub fn bind_render_target(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let pages = Pages::of(memory);
    let entry = cpu.esp;
    let owner = cpu.ecx;
    let mut draw = DrawContext {
        memory,
        cpu,
        pages,
        owner,
        entry,
        frame: entry.wrapping_sub(0x38),
    };
    bind_render_target_inner(&mut draw);
    draw.cpu.esp = entry.wrapping_add(20);
}

fn bind_render_target_inner(draw: &mut DrawContext<'_>) {
    let texture = draw.read(draw.entry.wrapping_add(4));
    let surface = draw.read(draw.entry.wrapping_add(8));
    let flags = draw.read(draw.entry.wrapping_add(12));
    draw.cpu.eax = flags;
    if draw.field(0xb4) == texture
        && draw.field(0xc4) == surface
        && (draw.field(0xc8) >> 4) & 0xfff == flags
        && draw.field(0xc8) & 7 == 1
    {
        return;
    }
    if surface != 0 {
        if flags & 1 != 0 {
            draw.set(0x1d8, texture);
            draw.set(0x1dc, surface);
        }
        if flags & 0x43 != 0 && draw.pages.u8(draw.memory, surface.wrapping_add(32)) & 0x80 != 0 {
            let resource = draw.read(draw.read(draw.read(surface.wrapping_add(16)).wrapping_add(0x44)));
            draw.set(0x1d8, resource);
            draw.set(0x1dc, surface);
            if resource != 0 {
                draw.write(resource.wrapping_add(4), draw.read(0x18d2ce8).wrapping_add(1));
            }
        }
    }
    let packed = ((flags & 0xfff) << 4) | (draw.field(0xc8) & 0xffff0009) | 1;
    draw.set(0xc4, surface);
    draw.set(0xc8, packed);
    let format = if texture == 0 {
        13
    } else {
        let frame = draw.read(0x18d2ce8).wrapping_add(1);
        let kind = (draw.read(texture.wrapping_add(32)) >> 20) & 15;
        draw.write(texture.wrapping_add(4), frame);
        kind
    };
    if draw.field(0x170) != format {
        draw.mark_dirty(3);
        draw.set(0x170, format);
    }
    if surface != 0 {
        draw.write(surface.wrapping_add(4), draw.read(0x18d2ce8).wrapping_add(1));
    }
    if draw.field(0xb4) != texture {
        draw.set(0xb4, texture);
        if texture != 0 {
            let remap = draw.read(texture.wrapping_add(32)) & 0x80000000 != 0;
            let mut table = 0x1447b7cu32;
            let mut index = draw.read(table) & 0xffff;
            while index != 0 {
                let source = if remap {
                    draw.pages.u16(draw.memory, table.wrapping_add(2)) as u32
                } else {
                    index
                };
                let bank = draw.field(0x20).wrapping_add(source * 8);
                draw.set(0x1f8 + index * 8, draw.read(bank));
                let value = draw.read(bank.wrapping_add(4));
                table = table.wrapping_add(4);
                draw.set(0x1fc + index * 8, value);
                index = draw.read(table) & 0xffff;
            }
        }
    }
    for off in [0xb8, 0xbc, 0xc0] {
        draw.set(off, 0);
    }
    draw.set(0xc8, draw.field(0xc8) | 8);
    let viewport_flags = draw.field(0xc8);
    let (width, height) = if texture != 0 {
        let v = draw.read(texture.wrapping_add(24));
        (v & 0xffff, (v >> 16) & 0x7fff)
    } else if surface != 0 {
        (
            draw.read(surface.wrapping_add(24)) & 0xffff,
            draw.pages.u16(draw.memory, surface.wrapping_add(26)) as u32,
        )
    } else {
        (1, 1)
    };
    if draw.pages.u16(draw.memory, draw.owner.wrapping_add(0x10c)) as u32 == width
        && draw.pages.u16(draw.memory, draw.owner.wrapping_add(0x10e)) as u32 == height
    {
        let descriptor = draw.read(draw.field(0x18).wrapping_add(0x54c));
        let size = (draw.pages.u16(draw.memory, descriptor.wrapping_add(24)) as u32 * 4 + 15) & !15;
        let out = draw.allocate_record(size, 0xfefdda);
        let len = draw.pages.u16(draw.memory, descriptor.wrapping_add(24)) as u32 * 4;
        let source = draw.field(0xc90);
        crate::semantic::copy_guest(draw.memory, draw.pages, out, source, len);
        draw.cpu.eax = out; // memcpy returns its destination
        draw.set(0xc90, out);
        draw.mark_dirty(1);
        if out != 0 {
            let one = draw.read(0x1406ce0);
            let write = x86::cvtsi2ss(width);
            draw.write(out.wrapping_add(16), write);
            let h = x86::cvtsi2ss(height);
            draw.write(out.wrapping_add(20), h);
            draw.write(out.wrapping_add(24), x86::divss(one, write));
            draw.write(out.wrapping_add(28), x86::divss(one, h));
        }
    } else {
        draw.pages
            .set_u16(draw.memory, draw.owner.wrapping_add(0x10c), width as u16);
        draw.pages
            .set_u16(draw.memory, draw.owner.wrapping_add(0x10e), height as u16);
        let stored_width = draw.field(0x10c) & 0xffff;
        let rect = if viewport_flags & 0x200 == 0 { 0xcc } else { 0xdc };
        let [left, top, right, bottom] = [0, 4, 8, 12].map(|off| draw.field(rect + off));
        let dx = x86::cvtsi2ss(right.wrapping_sub(left));
        let dy = x86::cvtsi2ss(bottom.wrapping_sub(top));
        let wf = x86::cvtsi2ss(stored_width);
        let hf = x86::cvtsi2ss(height & 0xffff);
        let half = draw.read(0x1406cf0);
        let sx = x86::mulss(x86::divss(dx, wf), half);
        let sy = x86::mulss(x86::divss(dy, hf), half);
        let ox = x86::divss(x86::divss(x86::cvtsi2ss(left), wf), sx);
        let oy = x86::divss(x86::divss(x86::cvtsi2ss(top), hf), sy);
        let descriptor = draw.read(draw.field(0x18).wrapping_add(0x54c));
        let size = (draw.pages.u16(draw.memory, descriptor.wrapping_add(24)) as u32 * 4 + 15) & !15;
        let out = draw.allocate_record(size, 0xfeffaf);
        draw.cpu.eax = out.wrapping_add(size);
        draw.set(0xc90, out);
        draw.mark_dirty(1);
        if out != 0 {
            let len = draw.pages.u16(draw.memory, descriptor.wrapping_add(24)) as u32 * 4;
            let source = draw.read(descriptor.wrapping_add(36));
            crate::semantic::copy_guest(draw.memory, draw.pages, out, source, len);
            draw.cpu.eax = out;
            let sign = draw.read(0x161c280);
            let one = draw.read(0x1406ce0);
            draw.write(out, x86::addss(ox, one));
            draw.write(out.wrapping_add(4), x86::addss(oy, one) ^ sign);
            draw.write(out.wrapping_add(8), sx);
            draw.write(out.wrapping_add(12), sy ^ sign);
            draw.write(out.wrapping_add(32), x86::divss(one, dx));
            draw.write(out.wrapping_add(36), x86::divss(one, dy));
            let wf = x86::cvtsi2ss(width);
            let hf = x86::cvtsi2ss(height);
            draw.write(out.wrapping_add(16), wf);
            draw.write(out.wrapping_add(20), hf);
            draw.write(out.wrapping_add(24), x86::divss(one, wf));
            draw.write(out.wrapping_add(28), x86::divss(one, hf));
        }
    }
}
