//! nDraw state semantics. Guest addresses/handles remain opaque u32 values.
//! This is a call-level decoder, not a guest register/stack ABI replacement.

pub mod descriptor;

/// Recording representation: COM slot, arguments excluding `this`, and copied
/// pointee words (viewport, scissor, shader constants or declaration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub slot: u32,
    pub args: Vec<u32>,
    pub data: Vec<u32>,
}

pub trait Memory {
    fn read32(&self, address: u32) -> u32;
    fn write32(&mut self, address: u32, value: u32);
    fn read8(&self, address: u32) -> u32 {
        (self.read32(address & !3) >> ((address & 3) * 8)) & 255
    }
    fn read16(&self, address: u32) -> u32 {
        self.read8(address) | self.read8(address + 1) << 8
    }
}

fn call(out: &mut Vec<Call>, slot: u32, args: &[u32]) {
    out.push(Call {
        slot,
        args: args.to_vec(),
        data: vec![],
    });
}
fn data_call(out: &mut Vec<Call>, slot: u32, args: &[u32], data: Vec<u32>) {
    out.push(Call {
        slot,
        args: args.to_vec(),
        data,
    });
}

/// Word, shift, mask, D3D state. Order is original call order.
pub type Field = (usize, u32, u32, u32);
// Translated from 0x00DA7510 (DDDA.exe 2017-12-13 Steam).
pub const BLEND: &[Field] = &[
    (0, 0, 1, 27),
    (0, 11, 7, 171),
    (0, 24, 7, 209),
    (0, 1, 31, 19),
    (0, 6, 31, 20),
    (0, 14, 31, 207),
    (0, 19, 31, 208),
    (0, 27, 15, 168),
    (1, 0, 15, 190),
    (1, 4, 15, 191),
    (1, 8, 15, 192),
];
// Translated from 0x00DA77D0 (DDDA.exe 2017-12-13 Steam).
pub const DEPTH: &[Field] = &[
    (0, 0, 1, 7),
    (0, 1, 1, 14),
    (0, 2, 15, 23),
    (0, 6, 1, 52),
    (0, 8, 255, 58),
    (0, 16, 255, 59),
    (0, 24, 15, 53),
    (1, 0, 15, 54),
    (0, 28, 15, 55),
    (1, 4, 15, 56),
    (0, 7, 1, 185),
    (1, 8, 15, 186),
    (1, 16, 15, 187),
    (1, 12, 15, 188),
    (1, 20, 15, 189),
];
// Translated from 0x00DA7B30 (DDDA.exe 2017-12-13 Steam).
pub const RASTER: &[Field] = &[
    (0, 8, 255, 22),
    (0, 0, 255, 8),
    (0, 17, 1, 174),
    (0, 18, 1, 161),
    (0, 16, 1, 136),
    (0, 19, 1, 176),
    (2, 0, u32::MAX, 175),
    (1, 0, u32::MAX, 195),
];
// Translated from 0x00DA7250 (DDDA.exe 2017-12-13 Steam).
pub const SAMPLER: &[Field] = &[
    (0, 0, 7, 1),
    (0, 3, 7, 2),
    (0, 6, 7, 3),
    (1, 0, u32::MAX, 4),
    (0, 9, 15, 5),
    (0, 13, 15, 6),
    (0, 17, 15, 7),
    (0, 21, 31, 10),
    (0, 26, 15, 9),
    (2, 0, u32::MAX, 8),
];

/// Decode a packed payload, including the original old-payload comparisons.
/// `stage` is Some(D3D sampler index) only for SAMPLER.
pub fn packed(
    fields: &[Field],
    new: &[u32],
    old: Option<&[u32]>,
    stage: Option<u32>,
    filtering: u32,
) -> Vec<Call> {
    let mut out = vec![];
    for &(word, shift, mask, state) in fields {
        let mut value = (new[word] >> shift) & mask;
        if stage.is_some() && old.is_some() && (new[0] >> 13 & 15) == 3 {
            if filtering == 1 && (5..=7).contains(&state) {
                value = 2;
            }
            if (2..=5).contains(&filtering) && state == 10 {
                value = 1 << (filtering - 1);
            }
        }
        if old.is_none_or(|p| (p[word] >> shift & mask) != value) {
            match stage {
                Some(stage) => call(&mut out, 69, &[stage, state, value]),
                None => call(&mut out, 57, &[state, value]),
            }
        }
    }
    out
}

fn apply_payload(
    m: &impl Memory,
    out: &mut Vec<Call>,
    fields: &[Field],
    new: u32,
    old: u32,
    stage: Option<u32>,
    filtering: u32,
) {
    let n: Vec<_> = (0..3).map(|i| m.read32(new + i * 4)).collect();
    let o: Vec<_> = (0..3).map(|i| m.read32(old + i * 4)).collect();
    out.extend(packed(
        fields,
        &n,
        (old != 0).then_some(o.as_slice()),
        stage,
        filtering,
    ));
}

// Translated from 0x00DAA5F0 (DDDA.exe 2017-12-13 Steam).
pub fn fixed(m: &mut impl Memory, ctx: u32, binding: u32) -> Vec<Call> {
    let mut out = vec![];
    let previous = m.read32(ctx + 12);
    if previous == binding {
        return out;
    }
    let old = m.read32(ctx + 0x18);
    let new = m.read32(binding + 8);
    if old != new {
        if previous == 0 || old == 0 {
            apply_payload(m, &mut out, BLEND, new + 0x20, 0, None, 0);
        } else if m.read32(old + 12) != m.read32(new + 12) {
            apply_payload(m, &mut out, BLEND, new + 0x20, old + 0x20, None, 0);
        }
        if previous == 0 || old == 0 || m.read32(binding) != m.read32(previous) {
            let rgba = m.read32(binding);
            call(
                &mut out,
                57,
                &[193, (rgba & 0xff00ff00) | (rgba & 255) << 16 | (rgba >> 16 & 255)],
            );
        }
        m.write32(ctx + 0x18, new);
    }
    let old = m.read32(ctx + 0x20);
    let new = m.read32(binding + 12);
    let reference = m.read8(binding + 4);
    if old != new || (previous != 0 && m.read8(previous + 4) != reference) {
        if previous == 0 || old == 0 {
            apply_payload(m, &mut out, DEPTH, new + 0x18, 0, None, 0);
        } else if m.read32(old + 12) != m.read32(new + 12) {
            apply_payload(m, &mut out, DEPTH, new + 0x18, old + 0x18, None, 0);
        }
        if previous == 0 || old == 0 || m.read8(previous + 4) != reference {
            call(&mut out, 57, &[57, reference]);
        }
        m.write32(ctx + 0x20, new);
    }
    let old = m.read32(ctx + 0x1c);
    let new = m.read32(binding + 16);
    let flip = m.read32(binding + 4) >> 8 & 1;
    let old_flip = if previous == 0 {
        0
    } else {
        m.read32(previous + 4) >> 8 & 1
    };
    if old != new || (previous != 0 && flip != old_flip) {
        if previous == 0 || old == 0 {
            apply_payload(m, &mut out, RASTER, new + 0x20 + 12 * flip, 0, None, 0);
        } else if flip != old_flip || m.read32(old + 12) != m.read32(new + 12) {
            apply_payload(
                m,
                &mut out,
                RASTER,
                new + 0x20 + 12 * flip,
                old + 0x20 + 12 * old_flip,
                None,
                0,
            );
        }
        m.write32(ctx + 0x1c, new);
    }
    m.write32(ctx + 12, binding);
    out
}

// Translated from 0x00DAA7C0 (DDDA.exe 2017-12-13 Steam).
pub fn streams(m: &mut impl Memory, ctx: u32, binding: u32) -> Vec<Call> {
    let mut out = vec![];
    if m.read32(ctx + 0x14) == binding {
        return out;
    }
    let ib = m.read32(binding + 0x28);
    let handle = if ib == 0 { 0 } else { m.read32(ib + 0x20) };
    if m.read32(ctx + 0x30) != handle {
        m.write32(ctx + 0x30, handle);
        call(&mut out, 104, &[handle]);
    }
    let mut extent = 0;
    for i in 0..4 {
        let vb = m.read32(binding + i * 4);
        let (handle, offset, stride) = if vb == 0 {
            (0, 0, 0)
        } else {
            let offset = m.read32(binding + 0x14 + i * 4);
            let stride = m.read8(binding + 0x10 + i);
            extent = extent.max(m.read32(vb + 0x14).wrapping_sub(offset) / stride);
            (m.read32(vb + 0x20), offset, stride)
        };
        if m.read32(ctx + 0x34 + i * 4) != handle
            || m.read32(ctx + 0x44 + i * 4) != stride
            || m.read32(ctx + 0x54 + i * 4) != offset
        {
            m.write32(ctx + 0x34 + i * 4, handle);
            m.write32(ctx + 0x44 + i * 4, stride);
            m.write32(ctx + 0x54 + i * 4, offset);
            call(&mut out, 100, &[i, handle, offset, stride]);
        }
    }
    m.write32(ctx + 0xe20, extent);
    m.write32(ctx + 0x14, binding);
    out
}

fn rect(m: &impl Memory, addr: u32) -> [i32; 4] {
    std::array::from_fn(|i| m.read32(addr + i as u32 * 4) as i32)
}
fn normalize(r: &mut [i32; 4]) {
    if r[2] < r[0] {
        r[0] = 0;
        r[2] = 0;
    }
    if r[3] < r[1] {
        r[1] = 0;
        r[3] = 0;
    }
}
fn store_rect(m: &mut impl Memory, addr: u32, r: [i32; 4]) {
    for (i, x) in r.into_iter().enumerate() {
        m.write32(addr + i as u32 * 4, x as u32);
    }
}

// Translated from 0x00DA6FE0 (DDDA.exe 2017-12-13 Steam).
pub fn targets(m: &mut impl Memory, ctx: u32, binding: u32, invalid_viewport: [u32; 4]) -> Vec<Call> {
    let mut out = vec![];
    if m.read32(ctx + 8) == binding {
        return out;
    }
    for i in 0..5 {
        let view = m.read32(binding + i * 4);
        if m.read32(ctx + 0x84 + i * 4) != view {
            m.write32(ctx + 0x84 + i * 4, view);
            let surface = if view == 0 { 0 } else { m.read32(view + 0x14) };
            if i == 4 {
                call(&mut out, 39, &[surface]);
            } else {
                call(&mut out, 37, &[i, surface]);
            }
            for (j, v) in invalid_viewport.into_iter().enumerate() {
                m.write32(ctx + 0x64 + j as u32 * 4, v);
            }
        }
    }
    let alternate = (m.read32(binding + 0x14) >> 9 & 1) * 16;
    let mut viewport = rect(m, binding + 0x18 + alternate);
    viewport[0] = viewport[0].max(0);
    viewport[1] = viewport[1].max(0);
    viewport[2] = viewport[2].min(m.read16(binding + 0x58) as i32);
    viewport[3] = viewport[3].min(m.read16(binding + 0x5a) as i32);
    normalize(&mut viewport);
    if viewport != rect(m, ctx + 0x64) {
        store_rect(m, ctx + 0x64, viewport);
        data_call(
            &mut out,
            47,
            &[],
            vec![
                viewport[0] as u32,
                viewport[1] as u32,
                (viewport[2] - viewport[0]) as u32,
                (viewport[3] - viewport[1]) as u32,
                0,
                1f32.to_bits(),
            ],
        );
    }
    let mut scissor = rect(m, binding + 0x38 + alternate);
    scissor[0] = scissor[0].max(viewport[0]);
    scissor[1] = scissor[1].max(viewport[1]);
    scissor[2] = scissor[2].min(viewport[2]);
    scissor[3] = scissor[3].min(viewport[3]);
    normalize(&mut scissor);
    if scissor != rect(m, ctx + 0x74) {
        store_rect(m, ctx + 0x74, scissor);
        data_call(&mut out, 75, &[], scissor.map(|x| x as u32).to_vec());
    }
    m.write32(ctx + 8, binding);
    out
}

/// Errors mark input outside the defined descriptor contract. In particular,
/// unknown derived constants read uninitialized original stack storage.
#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    DerivedConstant { kind: u32, count: u32 },
    Comparison(u32),
}

// Translated from 0x00DAA8B0 (DDDA.exe 2017-12-13 Steam).
// D3D comparison conversion table 0x01445A70 has eight entries (1..8).
pub fn shader(
    m: &mut impl Memory,
    ctx: u32,
    binding: u32,
    srgb: bool,
    filtering: u32,
) -> Result<Vec<Call>, DecodeError> {
    let mut out = vec![];
    let previous = m.read32(ctx + 0x10);
    if previous == binding {
        return Ok(out);
    }
    let metadata = m.read32(binding + 8);
    let declaration = m.read32(metadata + 0x18);
    if m.read32(ctx + 0x2c) != declaration {
        m.write32(ctx + 0x2c, declaration);
        call(&mut out, 87, &[declaration]);
    }
    let flags = m.read32(binding);
    let enabled = u32::from(flags & 0x180000 == 0x180000);
    let cached = m.read32(ctx);
    if (cached >> 25 & 1) != enabled {
        m.write32(ctx, (cached & !0x2000000) | enabled << 25);
        call(&mut out, 57, &[15, enabled]);
    }
    if enabled != 0 {
        let cached = m.read32(ctx);
        if ((flags & 0x7f8) ^ (cached >> 6)) & 0x7fff8 != 0 {
            let comparison = flags >> 3 & 255;
            if comparison >= 8 {
                return Err(DecodeError::Comparison(comparison));
            }
            m.write32(ctx, (flags & 0x7f8) << 6 | cached & 0xfe0001ff);
            call(&mut out, 57, &[25, comparison + 1]);
        }
        let cached = m.read32(ctx);
        if (flags >> 10 ^ cached) & 0x1fe != 0 {
            m.write32(ctx, cached ^ ((flags >> 10 ^ cached) & 0x1fe));
            call(&mut out, 57, &[24, flags >> 11 & 255]);
        }
    }
    let vs = m.read32(metadata + 0x10);
    let ps = m.read32(metadata + 0x14);
    let old_vs = if m.read32(ctx + 0x24) == vs {
        previous.wrapping_add(12)
    } else {
        m.write32(ctx + 0x24, vs);
        call(&mut out, 92, &[vs]);
        0
    };
    let mut old_ps = if m.read32(ctx + 0x28) == ps {
        previous.wrapping_add(12)
    } else {
        m.write32(ctx + 0x28, ps);
        call(&mut out, 107, &[ps]);
        0
    };
    if old_vs == 0 {
        old_ps = 0;
    }
    let descriptors = m.read32(metadata + 4);
    let mut derived = [0; 4];
    for pixel in [false, true] {
        let summary = m.read32(metadata + if pixel { 12 } else { 8 });
        let mut desc = descriptors
            + if pixel {
                (m.read32(metadata + 8) & 255) * 4
            } else {
                0
            };
        let old_values = if pixel { old_ps } else { old_vs };
        let mut used = [false; 4];
        for _ in 0..(summary >> 8 & 15) {
            let d = m.read32(desc);
            desc += 4;
            let index = d & 4095;
            let sampler = m.read32(binding + 12 + (d >> 12 & 1023) * 4);
            let texture = m.read32(binding + 12 + (d >> 22) * 4);
            let cache = ctx + if pixel { 0xd8 } else { 0x98 } + index * 4;
            let stage = index + if pixel { 0 } else { 257 };
            let old = m.read32(cache);
            if sampler != old {
                if old == 0 || m.read32(old + 12) != m.read32(sampler + 12) {
                    apply_payload(
                        m,
                        &mut out,
                        SAMPLER,
                        sampler + 0x30,
                        if old == 0 { 0 } else { old + 0x30 },
                        Some(stage),
                        filtering,
                    );
                }
                m.write32(cache, sampler);
            }
            if m.read32(cache + 0x80) != texture {
                m.write32(cache + 0x80, texture);
                call(&mut out, 65, &[stage, m.read32(texture + 0x34)]);
            }
            if pixel && srgb {
                let value = u32::from(
                    m.read32(texture + 0x20) & 0x40000000 != 0 && m.read32(texture + 0x2c) & 0x80000 == 0,
                );
                call(&mut out, 69, &[stage, 11, value]);
                let mask = 1u32 << (index & 31);
                let bits = m.read32(ctx + 0x198);
                m.write32(ctx + 0x198, if value != 0 { bits | mask } else { bits & !mask });
            }
            if !pixel {
                used[index as usize] = m.read32(cache + 0x80) != 0;
            }
        }
        if !pixel {
            for index in 0..4 {
                if !used[index as usize] && m.read32(ctx + 0x98 + index * 4) != 0 {
                    m.write32(ctx + 0x118 + index * 4, 0);
                    call(&mut out, 65, &[index + 257, 0]);
                }
            }
        }
        for (section, count) in [
            (0, summary >> 12 & 127),
            (1, summary >> 19 & 15),
            (2, summary >> 23 & 31),
            (3, summary >> 28),
        ] {
            for _ in 0..count {
                let d = m.read32(desc);
                desc += 4;
                let value_index = d >> 1 & if section == 3 { 127 } else { 31 };
                let value = m.read32(binding + 12 + value_index * 4);
                if old_values != 0 && m.read32(old_values + value_index * 4) == value {
                    continue;
                }
                let register = d >> 16 & 255;
                let mut count = d >> 24;
                let slot = if pixel { 109 } else { 94 }
                    + match section {
                        1 => 2,
                        2 => 4,
                        _ => 0,
                    };
                if section == 3 {
                    let kind = d >> 8 & 255;
                    if !matches!(kind, 3 | 7) || count != 1 {
                        return Err(DecodeError::DerivedConstant { kind, count });
                    }
                    let dimensions = [
                        m.read16(value + 0x20),
                        m.read16(value + 0x22) & 0x3fff,
                        m.read16(value + 0x26) & 0x7ff,
                    ];
                    for (i, n) in dimensions.into_iter().enumerate() {
                        derived[i] = if kind == 3 {
                            (1.0f32 / (n as f32)).to_bits()
                        } else {
                            (n as f32).to_bits()
                        };
                    }
                    derived[3] = 0;
                    data_call(&mut out, slot, &[register, count], derived.to_vec());
                } else {
                    let pointer = (value & !15) + (d >> 6 & 1023) * 4;
                    if section == 0 && value & 15 != 0 {
                        count = count.min((value & 15) * 16);
                    }
                    if section == 0 && d & 1 != 0 {
                        for i in 0..count {
                            let converted = (0..4)
                                .map(|j| (m.read32(pointer + i * 16 + j * 4) as i32 as f32).to_bits())
                                .collect();
                            data_call(&mut out, slot, &[register + i, 1], converted);
                        }
                    } else {
                        let data = (0..count * if section == 2 { 1 } else { 4 })
                            .map(|i| m.read32(pointer + i * 4))
                            .collect();
                        data_call(&mut out, slot, &[register, count], data);
                    }
                }
            }
        }
    }
    m.write32(ctx + 0x10, binding);
    Ok(out)
}
