//! Descriptor (+0x10) to packed D3D9 state. Table domains are checked: an
//! out-of-domain engine enum would read unrelated original image storage.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEnum(pub &'static str, pub u32);

// Embedded conversion constants at 0x01445AD0, 0x01445A70,
// 0x01445B48, 0x01445A20 and 0x01445A2C (2017-12-13 Steam).
const FACTOR: [u32; 13] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 14, 15];
fn table(name: &'static str, values: &[u32], index: u32) -> Result<u32, InvalidEnum> {
    values
        .get(index as usize)
        .copied()
        .ok_or(InvalidEnum(name, index))
}
fn compare(index: u32) -> Result<u32, InvalidEnum> {
    table("comparison/stencil", &[1, 2, 3, 4, 5, 6, 7, 8], index)
}

// Translated from 0x0117CB70 (DDDA.exe 2017-12-13 Steam).
pub fn blend(d: [u32; 4]) -> Result<[u32; 2], InvalidEnum> {
    let factor = |i| table("blend factor", &FACTOR, i);
    let op = |i| table("blend operation", &[1, 2, 3, 4, 5], i);
    Ok([
        ((d[0] >> 1) & 1)
            | factor(d[0] >> 9 & 255)? << 1
            | factor(d[0] >> 17 & 255)? << 6
            | op(d[0] >> 25)? << 11
            | factor(d[1] & 255)? << 14
            | factor(d[1] >> 8 & 255)? << 19
            | op(d[1] >> 16 & 127)? << 24
            | (d[2] & 15) << 27,
        (d[2] >> 8 & 15) | (d[2] >> 16 & 15) << 4 | (d[2] >> 24 & 15) << 8,
    ])
}

// Translated from 0x0117C8E0 (DDDA.exe 2017-12-13 Steam).
pub fn depth(d: [u32; 2]) -> Result<[u32; 2], InvalidEnum> {
    let a = d[0];
    let b = d[1];
    let two_sided = ((b >> 16 ^ b) & 0xf0) != 0
        || ((a >> 15 ^ b) & 0xf000) != 0
        || ((a >> 15 ^ b) & 0xf00) != 0
        || ((b >> 16 ^ b) & 15) != 0;
    Ok([
        (a & 0x43)
            | compare(a >> 2 & 15)? << 2
            | (two_sided as u32) << 7
            | (a >> 7 & 255) << 8
            | (a >> 15 & 255) << 16
            | compare(a >> 23 & 15)? << 24
            | compare(b & 15)? << 28,
        compare(a >> 27 & 15)?
            | compare(b >> 4 & 15)? << 4
            | compare(b >> 8 & 15)? << 8
            | compare(b >> 16 & 15)? << 12
            | compare(b >> 12 & 15)? << 16
            | compare(b >> 20 & 15)? << 20,
    ])
}

// Translated from 0x0117C6E0 (DDDA.exe 2017-12-13 Steam).
pub fn raster(d: [u32; 4]) -> Result<[[u32; 3]; 2], InvalidEnum> {
    let fill = table("fill", &[2, 3], d[0] & 7)?;
    let cull = table("cull", &[1, 2, 3], d[0] >> 3 & 7)?;
    let flip = |c| match c {
        2 => 3,
        3 => 2,
        _ => c,
    };
    let cull = if d[0] & 64 != 0 { flip(cull) } else { cull };
    // Scalar SSE multiplier at 0x01445A34, used by 0x0117C70E.
    // This is one float ULP above 2^-24; do not replace with a power of two.
    let bias = (d[1] as i32 as f32 * f32::from_bits(0x33800001)).to_bits();
    let flags = (d[0] << 9) & 0xf0000;
    Ok([
        [fill | cull << 8 | flags, bias, d[3]],
        [fill | flip(cull) << 8 | flags, bias, d[3]],
    ])
}

fn cvttss2si(x: f32) -> u32 {
    if !(-2147483648.0..2147483648.0).contains(&x) {
        0x80000000
    } else {
        (x.trunc() as i32) as u32
    }
}

// Translated from 0x0117CD70 (DDDA.exe 2017-12-13 Steam).
pub fn sampler(d: [u32; 8]) -> Result<[u32; 3], InvalidEnum> {
    let address = |i| table("address mode", &[1, 2, 3, 4, 5], i);
    let mut flags = address(d[0] >> 8 & 7)?
        | address(d[0] >> 11 & 7)? << 3
        | address(d[0] >> 14 & 7)? << 6
        | (d[0] << 4 & 0x3e00000);
    // FISTP uses a temporary x87 control word with rounding toward zero.
    let level = f32::from_bits(d[2]) as f64;
    let integer = if !(-9223372036854775808.0..9223372036854775808.0).contains(&level) {
        i64::MIN
    } else {
        level.trunc() as i64
    };
    flags |= (integer as u32 & 15) << 26;
    let filter = d[0] & 255;
    if filter <= 17 {
        // 0..8 and 9..17 produce identical D3D9 filters, including the
        // comparison-filter family (no D3D9 comparison sampler setting).
        let f = filter % 9;
        let mag = if f == 8 { 2 } else { 1 + (f >> 1 & 1) };
        let min = if f == 8 { 3 } else { 1 + (f >> 2 & 1) };
        let mip = if f == 8 { 2 } else { 1 + (f & 1) };
        flags |= mag << 9 | min << 13 | mip << 17;
    }
    let channel = |i| cvttss2si(f32::from_bits(d[i]) * 255.0) & 255;
    Ok([
        flags,
        channel(7) << 24 | channel(4) << 16 | channel(5) << 8 | channel(6),
        d[1],
    ])
}
