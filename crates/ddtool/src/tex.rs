//! `tex-scan` and `tex-dds`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use mttex::{D3dFormat, Shape};

use crate::scan::{for_each_resource, read_one};

pub const RTEXTURE: u32 = 0x241F_5DEB;

/// Parses every rTexture in the tree; any failure fails the scan.
pub fn tex_scan(dir: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let stats = Mutex::new(BTreeMap::<(u8, String, String), (u64, u64)>::new());
    let walk = for_each_resource(dir, RTEXTURE, |_, _, bytes| {
        let t = mttex::parse(bytes).map_err(|e| e.to_string())?;
        let h = &t.header;
        let key = (h.format_code, format!("{:?}", h.format), format!("{:?}", h.shape));
        let mut s = stats.lock().unwrap();
        let v = s.entry(key).or_default();
        v.0 += 1;
        v.1 += bytes.len() as u64;
        Ok(())
    })?;
    println!("archives:  {}", walk.archives);
    println!(
        "textures:  {} parsed, {} failed",
        walk.visited - walk.failures.len(),
        walk.failures.len()
    );
    println!("elapsed:   {:.1} s\n", started.elapsed().as_secs_f64());
    println!("code  d3d format        shape    count        MB");
    for ((code, fmt, shape), (n, bytes)) in stats.into_inner().unwrap() {
        println!(
            "{code:#04x}  {fmt:<16} {shape:<7} {n:>6} {:>9.1}",
            bytes as f64 / 1e6
        );
    }
    for f in walk.failures.iter().take(20) {
        println!("  FAIL {f}");
    }
    if walk.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} textures failed", walk.failures.len()))
    }
}

/// Writes one texture as a DDS file (for viewing in external tools).
pub fn tex_dds(arc: &Path, entry: &str, out: &Path) -> Result<(), String> {
    let bytes = read_one(arc, entry, RTEXTURE)?;
    let t = mttex::parse(&bytes).map_err(|e| e.to_string())?;
    let dds = to_dds(&t)?;
    std::fs::write(out, dds).map_err(|e| format!("{}: {e}", out.display()))?;
    let h = &t.header;
    println!(
        "{}x{} {:?} code {:#04x} {:?}, {} mips, {} faces, depth {}",
        h.width, h.height, h.format, h.format_code, h.shape, h.mip_count, h.faces, h.depth
    );
    Ok(())
}

/// Builds a legacy (DX9) DDS file: header + surfaces in TEX order, which
/// is also DDS order (face-major, largest mip first).
pub fn to_dds(t: &mttex::Texture) -> Result<Vec<u8>, String> {
    let h = &t.header;
    // DDS_PIXELFORMAT: (flags, fourcc, bits, r, g, b, a)
    let pf: (u32, u32, u32, u32, u32, u32, u32) = match h.format {
        D3dFormat::Dxt1 => (0x4, u32::from_le_bytes(*b"DXT1"), 0, 0, 0, 0, 0),
        D3dFormat::Dxt3 => (0x4, u32::from_le_bytes(*b"DXT3"), 0, 0, 0, 0, 0),
        D3dFormat::Dxt5 => (0x4, u32::from_le_bytes(*b"DXT5"), 0, 0, 0, 0, 0),
        D3dFormat::A8R8G8B8 => (0x41, 0, 32, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000),
        D3dFormat::X8R8G8B8 => (0x40, 0, 32, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0),
        D3dFormat::A8B8G8R8 => (0x41, 0, 32, 0x0000_00FF, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000),
        f => return Err(format!("DDS export of {f:?} not implemented")),
    };
    let mut caps = 0x1000u32; // DDSCAPS_TEXTURE
    let mut caps2 = 0u32;
    let mut flags = 0x1 | 0x2 | 0x4 | 0x1000; // CAPS | HEIGHT | WIDTH | PIXELFORMAT
    if h.mip_count > 1 {
        flags |= 0x20000; // MIPMAPCOUNT
        caps |= 0x8 | 0x40_0000; // COMPLEX | MIPMAP
    }
    match h.shape {
        Shape::Cube => {
            caps |= 0x8;
            caps2 |= 0x200 | 0xFC00; // CUBEMAP | all six faces
        }
        Shape::Volume => {
            flags |= 0x80_0000; // DEPTH
            caps |= 0x8;
            caps2 |= 0x20_0000; // VOLUME
        }
        Shape::Flat => {}
    }
    let mut out = Vec::new();
    let mut put = |v: u32| out.extend(v.to_le_bytes());
    put(u32::from_le_bytes(*b"DDS "));
    put(124);
    put(flags);
    put(h.height);
    put(h.width);
    put(t
        .surfaces
        .first()
        .map_or(0, |s| s.data.len() as u32 / s.depth.max(1)));
    put(if h.shape == Shape::Volume { h.depth } else { 0 });
    put(h.mip_count);
    for _ in 0..11 {
        put(0);
    }
    put(32);
    put(pf.0);
    put(pf.1);
    put(pf.2);
    put(pf.3);
    put(pf.4);
    put(pf.5);
    put(pf.6);
    put(caps);
    put(caps2);
    put(0);
    put(0);
    put(0);
    for s in &t.surfaces {
        out.extend_from_slice(s.data);
    }
    Ok(out)
}
