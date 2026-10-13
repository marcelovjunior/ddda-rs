//! L174 ABI-level material and dependency-key ports. Guest addresses and
//! constants below come from the cited Steam executable functions.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

// Translated from 0x00E0E5E0 (DDDA.exe 2017-12-13 Steam).
/// Salted word FNV-1; byte tails are ignored. Keep the three original
/// spills because a valid input can point into those stack slots.
pub fn dependency_hash(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let pages = Pages::of(memory);
    let entry_stack = cpu.esp;
    let mut hash = pages.u32(memory, entry_stack.wrapping_add(12));
    pages.set_u32(memory, entry_stack.wrapping_sub(4), cpu.esi);
    let count = pages.u32(memory, entry_stack.wrapping_add(8)) >> 2;
    if count != 0 {
        pages.set_u32(memory, entry_stack.wrapping_sub(8), cpu.ebx);
        pages.set_u32(memory, entry_stack.wrapping_sub(12), cpu.edi);
        let mut input = pages.u32(memory, entry_stack.wrapping_add(4));
        for _ in 0..count {
            // E0E603/E0E609: code-embedded multiplier and salt.
            let word = pages.u32(memory, input) ^ 0x14da_14da;
            for shift in [0, 8, 16, 24] {
                hash = hash.wrapping_mul(0x0100_0193) ^ ((word >> shift) & 255);
            }
            input = input.wrapping_add(4);
        }
    }
    cpu.eax = hash;
    cpu.esp = entry_stack.wrapping_add(16);
}

fn dirty(memory: &mut dyn Mem, pages: Pages, draw: u32, bits: u32) {
    let value = pages.u32(memory, draw.wrapping_add(0x154));
    pages.set_u32(memory, draw.wrapping_add(0x154), value | bits);
}

fn color_dirty(memory: &mut dyn Mem, pages: Pages, draw: u32) {
    let value = pages.u32(memory, draw.wrapping_add(0x114));
    pages.set_u32(memory, draw.wrapping_add(0x114), (value & 0xfff) | 0x1000);
}

/// Save a state pair in the original twelve-byte command record. Reload
/// the cursor at the same stores as the original (records may alias it).
/// Layout and masks: 010657D2..01065832 and the matching state-save blocks
/// at 01066D15, 01066DA6, 01066E8D and 01066F17 in the cited Steam build.
fn save_pair(memory: &mut dyn Mem, pages: Pages, draw: u32, offset: u32, tag: u32) {
    if pages.u8(memory, draw.wrapping_add(0x5c)) & 0x80 == 0 {
        return;
    }
    let record = |memory: &dyn Mem| {
        draw.wrapping_add(0x30c7c)
            .wrapping_add(pages.u32(memory, draw.wrapping_add(0x6c)).wrapping_mul(12))
    };
    let a = record(memory);
    let old = pages.u32(memory, a);
    pages.set_u32(memory, a, (old & (0x8000_0000 | tag)) | tag);
    let a = record(memory);
    let old = pages.u32(memory, a);
    pages.set_u32(memory, a, old & 0x7fff_ffff);
    let first = pages.u32(memory, draw.wrapping_add(offset));
    let a = record(memory);
    pages.set_u32(memory, a.wrapping_add(4), first);
    let second = pages.u32(memory, draw.wrapping_add(offset + 4));
    pages.set_u32(memory, a.wrapping_add(8), second);
    let cursor = pages.u32(memory, draw.wrapping_add(0x6c));
    pages.set_u32(memory, draw.wrapping_add(0x6c), cursor.wrapping_add(1));
}

fn call(
    memory: &mut dyn Mem,
    cpu: &mut Cpu,
    pages: Pages,
    frame: u32,
    target: u32,
    this: u32,
    args: &[u32],
    resume: u32,
) {
    cpu.esp = frame;
    for value in args.iter().rev().copied().chain(std::iter::once(resume)) {
        cpu.esp = cpu.esp.wrapping_sub(4);
        pages.set_u32(memory, cpu.esp, value);
    }
    cpu.ecx = this;
    x86::call_ext(memory, cpu, target);
}

// Translated from 0x010655A0 (DDDA.exe 2017-12-13 Steam).
/// Apply material state to a draw context, then call its original finalizer.
/// Material +0x10 is the dependency key; +0x14 packs render flags, +0x40
/// contains RGBA floats, and +0x30 addresses twelve-byte override records
/// (slot in the high twelve bits, then two value words). Draw +0x1f8 is
/// the eight-byte value bank; +0x154 contains its dirty/render flags.
pub fn apply_material(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let pages = Pages::of(memory);
    let entry_stack = cpu.esp;
    let material = cpu.ecx;
    let draw = pages.u32(memory, entry_stack.wrapping_add(4));
    let variant = pages.u32(memory, entry_stack.wrapping_add(8));
    apply_material_state(memory, pages, material, draw, variant);
    call(
        memory,
        cpu,
        pages,
        entry_stack.wrapping_sub(12),
        0x0106_2f20,
        material,
        &[draw],
        0x0106_589c,
    );
    cpu.esp = entry_stack.wrapping_add(12);
}

fn apply_material_state(memory: &mut dyn Mem, pages: Pages, material: u32, draw: u32, variant: u32) {
    let stamp = pages.u32(memory, 0x018d_2ce8).wrapping_add(1);
    let key = pages.u32(memory, material.wrapping_add(0x10));
    pages.set_u32(memory, material.wrapping_add(4), stamp);
    if pages.u32(memory, draw.wrapping_add(0x1cc)) != key
        || pages.u8(memory, draw.wrapping_add(0x169)) as u32 != variant
        || pages.u32(memory, draw.wrapping_add(0x1d0)) != 0x10fd_ca55
    {
        pages.set_u32(memory, draw.wrapping_add(0x1cc), key);
        dirty(memory, pages, draw, 3);
        pages.set_u8(memory, draw.wrapping_add(0x169), variant as u8);
        pages.set_u32(memory, draw.wrapping_add(0x1d0), 0x10fd_ca55);
        pages.set_u16(memory, draw.wrapping_add(0x158), (key & 0xfff) as u16);
        pages.set_u16(memory, draw.wrapping_add(0x15a), variant as u16);
    }
    let bank = pages.u32(memory, material.wrapping_add(0x28));
    let mode = pages.u8(memory, material.wrapping_add(0x14));
    if pages.u32(memory, draw.wrapping_add(0x11c)) != bank
        || pages.u8(memory, draw.wrapping_add(0x114)) != mode
    {
        pages.set_u32(memory, draw.wrapping_add(0x11c), bank);
        pages.set_u8(memory, draw.wrapping_add(0x114), mode);
        color_dirty(memory, pages, draw);
    }
    // COMISS/JBE sends unordered inputs through MULSS, too. CVTTSS2SI
    // supplies the original indefinite result for NaNs and overflow.
    let one = f32::from_bits(pages.u32(memory, 0x0140_6ce0));
    let scale = f32::from_bits(pages.u32(memory, 0x0161_c298));
    let mut rgba = 0;
    for lane in 0..4 {
        let component = f32::from_bits(pages.u32(memory, material.wrapping_add(0x40 + lane * 4)));
        let scaled = if component > one { scale } else { component * scale };
        rgba |= (x86::cvttss2si(scaled.to_bits()) & 255) << (lane * 8);
    }
    let slot = pages.u32(memory, material.wrapping_add(0x2c)) & 0xfff;
    let texture = pages.u32(memory, draw.wrapping_add(0x1f8 + slot * 8));
    if pages.u32(memory, draw.wrapping_add(0x118)) != texture
        || pages.u32(memory, draw.wrapping_add(0x110)) != rgba
    {
        pages.set_u32(memory, draw.wrapping_add(0x110), rgba);
        pages.set_u32(memory, draw.wrapping_add(0x118), texture);
        color_dirty(memory, pages, draw);
    }
    let value = pages.u32(memory, material.wrapping_add(0x24));
    if pages.u32(memory, draw.wrapping_add(0x120)) != value {
        pages.set_u32(memory, draw.wrapping_add(0x120), value);
        color_dirty(memory, pages, draw);
    }
    let flags = pages.u32(memory, material.wrapping_add(0x14));
    let old = pages.u32(memory, draw.wrapping_add(0x154));
    if (old >> 20) & 1 != (flags >> 8) & 1
        || (old >> 3) & 255 != (flags >> 9) & 15
        || (old >> 11) & 255 != (flags >> 14) & 255
        || (old >> 19) & 1 != (flags >> 13) & 1
    {
        let value = ((flags >> 8) & 1) << 20
            | ((flags >> 13) & 1) << 19
            | ((flags >> 9) & 15) << 3
            | ((flags >> 14) & 255) << 11
            | (old & 0xffe0_0007)
            | 1;
        pages.set_u32(memory, draw.wrapping_add(0x154), value);
    }
    if pages.u32(memory, draw.wrapping_add(0x1c30)) != 0 {
        save_pair(memory, pages, draw, 0x1c30, 0x347);
        pages.set_u32(memory, draw.wrapping_add(0x1c30), 0);
        if pages.u32(memory, draw.wrapping_add(0x1c34)) != 0 {
            pages.set_u32(memory, draw.wrapping_add(0x1c34), 0);
            dirty(memory, pages, draw, 2);
        }
        dirty(memory, pages, draw, 1);
    }
    let mut source = pages.u32(memory, material.wrapping_add(0x30));
    let mut i = 0;
    while i < pages.u32(memory, material.wrapping_add(0x18)) & 0x1ff {
        let slot = pages.u32(memory, source) >> 20;
        let first = pages.u32(memory, source.wrapping_add(4));
        pages.set_u32(memory, draw.wrapping_add(0x1f8 + slot * 8), first);
        let second = pages.u32(memory, source.wrapping_add(8));
        pages.set_u32(memory, draw.wrapping_add(0x1fc + slot * 8), second);
        source = source.wrapping_add(12);
        i += 1;
    }
    dirty(memory, pages, draw, 3);
}

// Translated from 0x01066C60 (DDDA.exe 2017-12-13 Steam), nDraw::Material::vf12.
/// Select a material pass, preserving the original virtual/direct callees.
/// Draw +0x16a selects the material family, +0x1d4 resolves an optional
/// resource binding, and +0x3730/+0x1d90 are saved state pairs. Material
/// vf09 fills a temporary RGBA value before the opacity rejection test.
pub fn select_material_pass(memory: &mut dyn Mem, cpu: &mut Cpu) {
    let entry_stack = cpu.esp;
    let pages = Pages::of(memory);
    let material = cpu.ecx;
    let result = select_pass(memory, cpu, pages, entry_stack, material);
    cpu.eax = result;
    cpu.esp = entry_stack.wrapping_add(12);
}

fn select_pass(memory: &mut dyn Mem, cpu: &mut Cpu, pages: Pages, entry_stack: u32, material: u32) -> u32 {
    let frame = (entry_stack.wrapping_sub(4) & !15).wrapping_sub(0x20);
    let vtable = pages.u32(memory, material);
    let get_color = pages.u32(memory, vtable.wrapping_add(0x24));
    let mut pass = (pages.u32(memory, material.wrapping_add(0x14)) >> 26) & 31;
    call(
        memory,
        cpu,
        pages,
        frame,
        get_color,
        material,
        &[frame.wrapping_add(0x10)],
        0x0106_6c89,
    );
    let threshold = f32::from_bits(pages.u32(memory, 0x0140_8e58));
    let alpha = f32::from_bits(pages.u32(memory, cpu.eax.wrapping_add(12)));
    if threshold > alpha {
        return 0;
    }
    let requested = pages.u32(memory, entry_stack.wrapping_add(8));
    let draw = pages.u32(memory, entry_stack.wrapping_add(4));
    if (requested as i32) < 24 {
        pass = requested;
    }
    let selected = if pages.u16(memory, draw.wrapping_add(0x16a)) & 0x104 != 0 {
        6
    } else {
        pass
    };
    call(
        memory,
        cpu,
        pages,
        frame,
        0x00ff_1090,
        draw,
        &[selected, 0],
        0x0106_6cc3,
    );
    let owner = pages.u32(memory, draw.wrapping_add(0x1d4));
    let mut binding = 0;
    if owner != 0 {
        let resource = pages.u32(memory, owner.wrapping_add(0xb8));
        if resource != 0 {
            let indirect = pages.u32(memory, resource.wrapping_add(0x44));
            binding = pages.u32(memory, indirect);
        }
    }
    let special = (pass == 11 || pass == 12) && binding != 0;
    let bank_offset = if special { 0x3540 } else { 0x3538 };
    let bank = pages.u32(memory, draw.wrapping_add(0x20));
    if pages.u32(memory, draw.wrapping_add(0x3730)) != pages.u32(memory, bank.wrapping_add(bank_offset)) {
        save_pair(memory, pages, draw, 0x3730, 0x6a7);
        let bank = pages.u32(memory, draw.wrapping_add(0x20));
        let value = pages.u32(memory, bank.wrapping_add(bank_offset));
        pages.set_u32(memory, draw.wrapping_add(0x3730), value);
        pages.set_u32(
            memory,
            draw.wrapping_add(0x3734),
            if special { 0x49d9_b6a8 } else { 0x6a7 },
        );
        dirty(memory, pages, draw, 3);
    }
    let texture = if special {
        pages.u32(memory, binding.wrapping_add(0x10))
    } else {
        0
    };
    if pages.u32(memory, draw.wrapping_add(0x1d90)) != texture {
        save_pair(memory, pages, draw, 0x1d90, 0x373);
        let mut key = 0;
        if texture != 0 {
            let stamp = pages.u32(memory, 0x018d_2ce8).wrapping_add(1);
            pages.set_u32(memory, texture.wrapping_add(4), stamp);
            pages.set_u32(memory, draw.wrapping_add(0x1d90), texture);
            key = pages.u32(memory, texture.wrapping_add(12)) | 0x0a82_0000;
        } else {
            pages.set_u32(memory, draw.wrapping_add(0x1d90), 0);
        }
        if pages.u32(memory, draw.wrapping_add(0x1d94)) != key {
            pages.set_u32(memory, draw.wrapping_add(0x1d94), key);
            dirty(memory, pages, draw, 2);
        }
        dirty(memory, pages, draw, 1);
    }
    let flags = pages.u16(memory, draw.wrapping_add(0x16a)) as u32;
    if flags & 0x813 != 0 {
        let high = pages.u32(memory, material.wrapping_add(0x14)) >> 31;
        let manager = pages.u32(memory, 0x018d_2c80);
        let table = pages.u32(memory, manager);
        let target = pages.u32(memory, table.wrapping_add(0x38));
        call(
            memory,
            cpu,
            pages,
            frame,
            target,
            manager,
            &[draw, high],
            0x0106_6fa7,
        );
        call(
            memory,
            cpu,
            pages,
            frame,
            0x0106_55a0,
            material,
            &[draw, 0],
            0x0106_6fb1,
        );
        if pages.u32(memory, material.wrapping_add(0x18)) >> 31 != 0 {
            let texture = pages.u32(memory, draw.wrapping_add(0xd80));
            let color = pages.u32(memory, 0x0162_1120);
            if pages.u32(memory, draw.wrapping_add(0x118)) != texture
                || pages.u32(memory, draw.wrapping_add(0x110)) != color
            {
                pages.set_u32(memory, draw.wrapping_add(0x110), color);
                pages.set_u32(memory, draw.wrapping_add(0x118), texture);
                color_dirty(memory, pages, draw);
            }
        }
        if pass == 16 {
            if nonzero_factor(memory, pages, draw) {
                call(
                    memory,
                    cpu,
                    pages,
                    frame,
                    0x00ff_1090,
                    draw,
                    &[12, 0],
                    0x0106_703c,
                );
            }
        } else if pass == 11 || pass == 12 {
            // JBE also returns immediately for unordered alpha.
            if below_one(memory, pages, draw) {
                let mut selected = 12;
                let mut resume = 0x0106_70e4;
                if !nonzero_factor(memory, pages, draw) {
                    selected = 16;
                    let texture = pages.u32(memory, draw.wrapping_add(0xd40));
                    let color = pages.u32(memory, 0x0162_113c);
                    if pages.u32(memory, draw.wrapping_add(0x118)) != texture
                        || pages.u32(memory, draw.wrapping_add(0x110)) != color
                    {
                        pages.set_u32(memory, draw.wrapping_add(0x110), color);
                        color_dirty(memory, pages, draw);
                        pages.set_u32(memory, draw.wrapping_add(0x118), texture);
                        resume = 0x0106_70c7;
                    }
                }
                call(
                    memory,
                    cpu,
                    pages,
                    frame,
                    0x00ff_1090,
                    draw,
                    &[selected, 0],
                    resume,
                );
            }
        }
        1
    } else {
        let route = if flags & 0x20 != 0 {
            Some((0x0106_6380, 0x0106_7100, [draw, pass, 0], 2))
        } else if flags & 0x1000 != 0 {
            Some((0x0106_5e40, 0x0106_7127, [draw, pass, 0], 2))
        } else if flags & 0x208 != 0 {
            Some((0x0106_58b0, 0x0106_7157, [draw, (!(flags >> 3)) & 1, pass], 3))
        } else if flags & 0x104 != 0 {
            if (pass == 11 || pass == 12) && below_one(memory, pages, draw) {
                pass = if nonzero_factor(memory, pages, draw) {
                    12
                } else {
                    16
                };
            } else if pass == 16 && nonzero_factor(memory, pages, draw) {
                pass = 12;
            }
            Some((0x0106_5b00, 0x0106_71dc, [draw, (!(flags >> 2)) & 1, pass], 3))
        } else {
            None
        };
        if let Some((target, resume, args, len)) = route {
            call(memory, cpu, pages, frame, target, material, &args[..len], resume);
            u32::from(cpu.eax as u8 != 0)
        } else {
            0
        }
    }
}

fn below_one(memory: &dyn Mem, pages: Pages, draw: u32) -> bool {
    f32::from_bits(pages.u32(memory, 0x0140_6ce0))
        > f32::from_bits(pages.u32(memory, draw.wrapping_add(0x17c)))
}

fn nonzero_factor(memory: &dyn Mem, pages: Pages, draw: u32) -> bool {
    let factor = pages.u32(memory, draw.wrapping_add(0x1330)) & !15;
    f32::from_bits(pages.u32(memory, factor.wrapping_add(8)))
        != f32::from_bits(pages.u32(memory, 0x0156_1738))
}
