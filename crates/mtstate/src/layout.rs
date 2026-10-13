//! InputLayout's synthesized D3DVERTEXELEMENT9 array, not its source semantic
//! records. Shader declaration assembly assigns semantics again across streams.

use crate::{data_call, Call, Memory};

// Embedded type widths 0x01441BD8 and semantic pairs 0x01441B38/3C.
const WIDTH: [usize; 15] = [0, 4, 2, 2, 2, 2, 2, 1, 1, 1, 1, 4, 4, 1, 4];
const USAGE: [u32; 16] = [0, 3, 6, 7, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5];
const INDEX: [u32; 16] = [0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

#[derive(Debug, PartialEq, Eq)]
pub enum LayoutError {
    Type(u32),
    Extent(usize),
    Elements(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    /// Eight-byte declaration elements including D3DDECL_END.
    pub elements: Vec<[u32; 2]>,
    pub stride_20: u32,
    /// Source bit31 at the first byte of the final four-byte scan group.
    /// Stored by 0x0112ADE6; unused by the declaration generator.
    pub last_flag_24: u8,
}

fn semantic(i: usize) -> Result<u32, LayoutError> {
    if i >= 16 {
        return Err(LayoutError::Elements(i + 1));
    }
    Ok(USAGE[i] << 16 | INDEX[i] << 24)
}

// Translated from 0x01129750 (DDDA.exe 2017-12-13 Steam).
fn emit(
    out: &mut Vec<[u32; 2]>,
    kind: usize,
    mut components: usize,
    mut offset: u32,
) -> Result<(), LayoutError> {
    while components > 0 {
        let (format, used, bytes) = match kind {
            1 => {
                let n = components.min(4);
                (n as u32 - 1, n, 4 * n as u32)
            }
            2..=6 => {
                let n = if components < 3 { 2 } else { 4 };
                let format = match kind {
                    2 => {
                        if n == 2 {
                            15
                        } else {
                            16
                        }
                    }
                    3 | 4 => {
                        if n == 2 {
                            6
                        } else {
                            7
                        }
                    }
                    _ => {
                        if n == 2 {
                            9
                        } else {
                            10
                        }
                    }
                };
                (format, n, n as u32 * 2)
            }
            7 | 8 => (5, 4, 4),
            9 | 10 | 13 => (8, 4, 4),
            11 | 12 | 14 => (8, 1, 4),
            _ => return Err(LayoutError::Type(kind as u32)),
        };
        out.push([(offset & 65535) << 16, format | semantic(out.len())?]);
        offset = offset.wrapping_add(bytes);
        components = components.saturating_sub(used);
    }
    Ok(())
}

// Translated from 0x0112AAB0 (DDDA.exe 2017-12-13 Steam).
// Allocation, prefix/hash and memcpy are outside this semantic result.
pub fn from_descriptors(descriptors: &[[u32; 2]]) -> Result<Layout, LayoutError> {
    let mut kinds = [0usize; 256];
    let mut flags = [0u8; 256];
    let mut extent = 0;
    for d in descriptors {
        let kind = (d[1] >> 6 & 31) as usize;
        let width = *WIDTH.get(kind).ok_or(LayoutError::Type(kind as u32))?;
        let length = (d[1] >> 11 & 127) as usize * width;
        let offset = (d[1] >> 22 & 511) as usize;
        let end = offset + length;
        if end > 256 {
            return Err(LayoutError::Extent(end));
        }
        kinds[offset..end].fill(kind);
        flags[offset..end].fill((d[1] >> 31) as u8);
        extent = extent.max(end);
    }
    let mut out = vec![];
    let (mut kind, mut count, mut start) = (0, 0, 0u32);
    for offset in (0..extent).step_by(4) {
        let next = *kinds[offset..offset + 4].iter().find(|k| **k != 0).unwrap_or(&0);
        let compatible = if WIDTH[kind] * count < 4 {
            WIDTH[kind] == WIDTH[next]
        } else {
            kind == next
        };
        if next != 0 && compatible {
            count += 4 / WIDTH[next];
        } else {
            if kind != 0 && count != 0 {
                emit(&mut out, kind, count, start)?;
                start = offset as u32;
            }
            if next == 0 {
                kind = 0;
                count = 0;
                start = offset as u32 + 4;
            } else {
                kind = next;
                count = 4 / WIDTH[next];
            }
        }
    }
    if kind != 0 && count != 0 {
        emit(&mut out, kind, count, start)?;
    }
    out.push([255, 17]);
    Ok(Layout {
        elements: out,
        stride_20: ((extent + 3) & !3) as u32,
        last_flag_24: if extent == 0 { 0 } else { flags[(extent - 1) & !3] },
    })
}

// Translated from 0x00E0FCC0 (DDDA.exe 2017-12-13 Steam), successful
// CreateVertexDeclaration path. `layouts` is the resolved per-stream layout
// pointer (sShader2+0x3C+(source_id&0xFFF)*8), not a COM declaration.
pub fn declaration(m: &impl Memory, layouts: [u32; 4]) -> Result<Call, LayoutError> {
    let mut data = vec![];
    for (stream, layout) in layouts.into_iter().enumerate() {
        if layout == 0 {
            continue;
        }
        let pointer = m.read32(layout + 0x10);
        for i in 0..m.read32(layout + 0x14) {
            let n = data.len() / 2;
            data.push(m.read32(pointer + i * 8) & 0xffff0000 | stream as u32);
            data.push(m.read32(pointer + i * 8 + 4) & 0xffff | semantic(n)?);
        }
    }
    data.extend([255, 17]);
    let mut calls = vec![];
    data_call(&mut calls, 86, &[], data);
    Ok(calls.pop().unwrap())
}
