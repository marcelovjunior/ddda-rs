//! Target contents as backends read them back (D3D9's layout and bits,
//! rows tightly packed): decoded to channel values, compared, and written
//! as PNG images for people to look at.
use std::io;
use std::path::Path;

/// Bytes per pixel of the target formats read back (D3DFORMAT numbers).
pub fn bytes_per_pixel(format: u32) -> Option<usize> {
    Some(match format {
        21 | 22 | 31 | 32 | 33 | 34 | 112 | 114 => 4,
        23 | 24 | 25 | 26 | 111 | 81 => 2,
        36 | 113 | 115 => 8,
        116 => 16,
        28 | 50 => 1,
        _ => return None,
    })
}

fn half(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as i32, ((h >> 10) & 31) as i32, (h & 1023) as f32);
    let v = if e == 0 {
        m * 2f32.powi(-24)
    } else if e == 31 {
        if m == 0.0 { f32::INFINITY } else { f32::NAN }
    } else {
        (1.0 + m / 1024.0) * 2f32.powi(e - 15)
    };
    if s == 1 { -v } else { v }
}

/// A pixel's RGBA as numbers: unorm formats in their integer levels
/// (0..255, 0..1023, 0..65535), float formats as values; channels a format
/// lacks read 0 (alpha of X8 formats reads 255).
pub fn decode(format: u32, px: &[u8]) -> [f32; 4] {
    let u16le = |k: usize| u16::from_le_bytes([px[2 * k], px[2 * k + 1]]);
    let f32le = |k: usize| f32::from_le_bytes(px[4 * k..4 * k + 4].try_into().unwrap());
    match format {
        21 => [px[2] as f32, px[1] as f32, px[0] as f32, px[3] as f32],
        22 => [px[2] as f32, px[1] as f32, px[0] as f32, 255.0],
        32 => [px[0] as f32, px[1] as f32, px[2] as f32, px[3] as f32],
        33 => [px[0] as f32, px[1] as f32, px[2] as f32, 255.0],
        113 => [half(u16le(0)), half(u16le(1)), half(u16le(2)), half(u16le(3))],
        111 => [half(u16le(0)), 0.0, 0.0, 0.0],
        112 => [half(u16le(0)), half(u16le(1)), 0.0, 0.0],
        114 => [f32le(0), 0.0, 0.0, 0.0],
        115 => [f32le(0), f32le(1), 0.0, 0.0],
        116 => [f32le(0), f32le(1), f32le(2), f32le(3)],
        34 => [u16le(0) as f32, u16le(1) as f32, 0.0, 0.0],
        36 => [u16le(0) as f32, u16le(1) as f32, u16le(2) as f32, u16le(3) as f32],
        81 => [u16le(0) as f32, 0.0, 0.0, 0.0],
        31 => {
            let v = u32::from_le_bytes(px[..4].try_into().unwrap());
            [(v & 1023) as f32, ((v >> 10) & 1023) as f32, ((v >> 20) & 1023) as f32, (v >> 30) as f32]
        }
        23 => {
            let v = u16le(0);
            [((v >> 11) & 31) as f32, ((v >> 5) & 63) as f32, (v & 31) as f32, 0.0]
        }
        28 => [0.0, 0.0, 0.0, px[0] as f32],
        50 => [px[0] as f32, 0.0, 0.0, 0.0],
        _ => [0.0; 4],
    }
}

/// The full-scale value of a format's channels (unorm levels), or 1.0 for
/// float formats: what [`decode`] values are divided by to show them.
pub fn full_scale(format: u32) -> f32 {
    match format {
        21 | 22 | 32 | 33 | 28 | 50 => 255.0,
        31 => 1023.0,
        34 | 36 | 81 => 65535.0,
        23 => 31.0,
        _ => 1.0,
    }
}

/// Two read-backs of one target compared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Difference {
    pub pixels: usize,
    /// pixels whose bits differ beyond the tolerance
    pub differing: usize,
    /// the largest channel difference (decoded units)
    pub max: f32,
    /// the first differing pixel (x, y) and both values
    pub first: Option<(u32, u32, [f32; 4], [f32; 4])>,
}

/// Compare two read-backs of a `width`-wide target of `format`: a pixel
/// differs when its bits differ and (with a tolerance above 0) a decoded
/// channel differs by more than `tolerance`. NaN equals NaN.
pub fn compare(format: u32, width: u32, a: &[u8], b: &[u8], tolerance: f32) -> Difference {
    let Some(bpp) = bytes_per_pixel(format) else {
        return Difference { pixels: 0, differing: usize::from(a != b), max: f32::INFINITY, first: None };
    };
    let mut d = Difference { pixels: a.len().min(b.len()) / bpp, ..Default::default() };
    if a.len() != b.len() {
        d.differing = d.pixels.max(1);
        d.max = f32::INFINITY;
    }
    // the X byte of X8R8G8B8 and X8B8G8R8 is unused (the D3D9 driver
    // reads it back as 0 whatever was drawn; L41 render_to_texture)
    let used = if matches!(format, 22 | 33) { 3 } else { bpp };
    for (k, (pa, pb)) in a.chunks_exact(bpp).zip(b.chunks_exact(bpp)).enumerate() {
        if pa[..used] == pb[..used] {
            continue;
        }
        let (va, vb) = (decode(format, pa), decode(format, pb));
        let diff = va.iter().zip(&vb).map(|(x, y)| if x.is_nan() && y.is_nan() { 0.0 } else if x == y { 0.0 } else { (x - y).abs() }).fold(0.0f32, |m, v| if v.is_nan() { f32::INFINITY } else { m.max(v) });
        if tolerance > 0.0 && diff <= tolerance {
            continue;
        }
        d.differing += 1;
        d.max = d.max.max(if diff == 0.0 { f32::MIN_POSITIVE } else { diff });
        if d.first.is_none() {
            d.first = Some(((k as u32) % width.max(1), (k as u32) / width.max(1), va, vb));
        }
    }
    d
}

/// A read-back as RGBA8 to look at: unorm channels scaled, float channels
/// clamped to [0, 1]; single-channel formats as grey; alpha opaque.
pub fn to_rgba8(format: u32, raw: &[u8]) -> Vec<u8> {
    let Some(bpp) = bytes_per_pixel(format) else { return Vec::new() };
    let scale = full_scale(format);
    let grey = matches!(format, 111 | 114 | 81 | 50);
    raw.chunks_exact(bpp).flat_map(|px| {
        let v = decode(format, px);
        let c = |x: f32| ((x / scale).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        if grey { [c(v[0]), c(v[0]), c(v[0]), 255] } else { [c(v[0]), c(v[1]), c(v[2]), 255] }
    }).collect()
}

/// Where two read-backs differ, to look at: black where equal, red where
/// they differ, brighter for larger differences (relative to the
/// format's full scale; any difference is at least a dark red).
pub fn diff_rgba8(format: u32, a: &[u8], b: &[u8]) -> Vec<u8> {
    let Some(bpp) = bytes_per_pixel(format) else { return Vec::new() };
    let scale = full_scale(format);
    let used = if matches!(format, 22 | 33) { 3 } else { bpp };
    a.chunks_exact(bpp).zip(b.chunks_exact(bpp)).flat_map(|(pa, pb)| {
        if pa[..used] == pb[..used] {
            return [0, 0, 0, 255];
        }
        let (va, vb) = (decode(format, pa), decode(format, pb));
        let m = va.iter().zip(&vb).map(|(x, y)| (x - y).abs() / scale).fold(0.0f32, |m, v| if v.is_nan() { 1.0 } else { m.max(v) });
        [(96.0 + 159.0 * (m * 8.0).min(1.0)) as u8, 0, 0, 255]
    }).collect()
}

/// Write RGBA8 pixels as a PNG file.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut e = png::Encoder::new(file, width, height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let mut w = e.write_header().map_err(io::Error::other)?;
    w.write_image_data(rgba).map_err(io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differences_are_counted_beyond_the_tolerance() {
        let a = [10u8, 20, 30, 255, 1, 2, 3, 4];
        let b = [10u8, 21, 30, 255, 1, 2, 3, 4];
        let d = compare(21, 2, &a, &b, 0.0);
        assert_eq!((d.pixels, d.differing, d.max), (2, 1, 1.0));
        assert_eq!(d.first.map(|f| (f.0, f.1)), Some((0, 0)));
        assert_eq!(compare(21, 2, &a, &b, 1.0).differing, 0);
        let nan = f32::NAN.to_le_bytes();
        let mut other = nan;
        other[0] ^= 1;
        assert_eq!(compare(114, 1, &nan, &other, 0.5).differing, 0);
        // the X byte is not compared
        assert_eq!(compare(22, 1, &[1, 2, 3, 0], &[1, 2, 3, 255], 0.0).differing, 0);
    }

    #[test]
    fn half_floats_decode() {
        assert_eq!(half(0x3C00), 1.0);
        assert_eq!(half(0xC000), -2.0);
        assert_eq!(half(0x7C00), f32::INFINITY);
        assert_eq!(half(0x0001), 2f32.powi(-24));
    }
}
