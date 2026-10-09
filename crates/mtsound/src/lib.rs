//! Read-only SPAC v12, SREQ v19 and STRQ v27 memory images.
//! No playback or interpretation of untraced mixing/loop metadata.
use std::ops::Range;
fn span(b: &[u8], o: usize, n: usize) -> Result<&[u8], String> {
    b.get(o..o.checked_add(n).ok_or("size overflow")?)
        .ok_or_else(|| format!("out of bounds: {o:#x}+{n:#x}/{}", b.len()))
}
fn u32at(b: &[u8], o: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(span(b, o, 4)?.try_into().unwrap()))
}
fn count(b: &[u8], o: usize) -> Result<usize, String> {
    Ok(u32at(b, o)? as usize)
}
fn table(b: &[u8], o: usize, n: usize, s: usize) -> Result<&[u8], String> {
    span(b, o, n.checked_mul(s).ok_or("table overflow")?)
}
fn text(b: &[u8], o: usize) -> Result<&str, String> {
    let tail = span(b, o, b.len().checked_sub(o).ok_or("string offset")?)?;
    let n = tail.iter().position(|&v| v == 0).ok_or("unterminated string")?;
    std::str::from_utf8(&tail[..n]).map_err(|e| e.to_string())
}
fn header(b: &[u8], magic: &[u8; 4], version: u32, n: usize) -> Result<(), String> {
    span(b, 0, n)?;
    if &b[..4] != magic || u32at(b, 4)? != version {
        return Err("unsupported sound magic/version".into());
    }
    Ok(())
}
fn index(value: u32, n: usize) -> Result<Option<usize>, String> {
    if value == u32::MAX {
        Ok(None)
    } else if (value as usize) < n {
        Ok(Some(value as usize))
    } else {
        Err(format!("index {value} outside {n}"))
    }
}
#[derive(Debug)]
pub struct Wave<'a> {
    pub header: &'a [u8],
    pub samples: &'a [u8],
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    data_header_offset: usize,
}
impl Wave<'_> {
    /// Reconstruct a conventional RIFF/WAVE with the split data inserted after its chunk header.
    pub fn riff(&self) -> Vec<u8> {
        let p = self.data_header_offset + 8;
        let mut out = Vec::with_capacity(self.header.len() + self.samples.len());
        out.extend_from_slice(&self.header[..p]);
        out.extend_from_slice(self.samples);
        out.extend_from_slice(&self.header[p..]);
        out
    }
}
#[derive(Debug)]
pub struct Package<'a> {
    pub waves: Vec<Option<Wave<'a>>>,
    pub programs: &'a [u8],
    pub wave_metadata: &'a [u8],
}
impl<'a> Package<'a> {
    /// Translated from 0x00e21600 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &'a [u8]) -> Result<Self, String> {
        header(b, b"SPAC", 12, 32)?;
        let nw = count(b, 8)?;
        let np = count(b, 12)?;
        let nm = count(b, 16)?;
        let po = count(b, 20)?;
        let mo = count(b, 24)?;
        let so = count(b, 28)?;
        let programs = table(b, po, np, 20)?;
        let wave_metadata = table(b, mo, nm, 44)?;
        if nw > b.len() / 4 {
            return Err("wave count".into());
        }
        let mut waves = Vec::new();
        let mut h = 32;
        let mut s = so;
        for _ in 0..nw {
            if u32at(b, h)? == u32::MAX {
                waves.push(None);
                h += 4;
                continue;
            }
            if span(b, h, 4)? != b"RIFF" || span(b, h + 8, 8)? != b"WAVEfmt " {
                return Err("split RIFF header".into());
            }
            let fmt_len = count(b, h + 16)?;
            if fmt_len < 16 {
                return Err("short WAVE fmt".into());
            }
            let d = h + 20 + fmt_len;
            if span(b, d, 4)? != b"data" {
                return Err("missing split data chunk".into());
            }
            let size = count(b, d + 4)?;
            let full = count(b, h + 4)?.checked_add(8).ok_or("RIFF size")?;
            let head = full.checked_sub(size).ok_or("split RIFF size")?;
            if head < d - h + 8 {
                return Err("RIFF header shorter than data header".into());
            }
            let bytes = span(b, h, head)?;
            let samples = span(b, s, size)?;
            let f = span(b, h + 20, 16)?;
            waves.push(Some(Wave {
                header: bytes,
                samples,
                format_tag: u16::from_le_bytes(f[0..2].try_into().unwrap()),
                channels: u16::from_le_bytes(f[2..4].try_into().unwrap()),
                sample_rate: u32at(f, 4)?,
                bits_per_sample: u16::from_le_bytes(f[14..16].try_into().unwrap()),
                data_header_offset: d - h,
            }));
            h += head;
            s += size;
        }
        if h > po || s > b.len() {
            return Err("overlapping package sections".into());
        }
        for m in wave_metadata.chunks_exact(44) {
            index(u32at(m, 40)?, nw)?;
        }
        Ok(Self {
            waves,
            programs,
            wave_metadata,
        })
    }
}
#[derive(Debug)]
pub struct AuxTables<'a> {
    pub sections: &'a [u8],
    pub settings: &'a [u8],
    pub speakers: &'a [u8],
    pub values: &'a [u8],
}
fn aux(b: &[u8], counts: usize, offsets: usize) -> Result<AuxTables<'_>, String> {
    let mut ts = Vec::new();
    let sizes = [12, 28, 16, 8];
    for (i, s) in sizes.iter().enumerate() {
        ts.push(table(
            b,
            count(b, offsets + i * 4)?,
            count(b, counts + i * 4)?,
            *s,
        )?);
    }
    for (i, field) in [(0, 8), (1, 24), (2, 12)] {
        for r in ts[i].chunks_exact(sizes[i]) {
            index(u32at(r, field)?, ts[i + 1].len() / sizes[i + 1])?;
        }
    }
    Ok(AuxTables {
        sections: ts[0],
        settings: ts[1],
        speakers: ts[2],
        values: ts[3],
    })
}
#[derive(Debug)]
pub struct Request<'a> {
    pub raw: &'a [u8],
    pub source: Option<&'a str>,
    pub section: Option<usize>,
}
#[derive(Debug)]
pub struct Requests<'a> {
    pub requests: Vec<Request<'a>>,
    pub random: Option<&'a str>,
    pub auxiliary: AuxTables<'a>,
}
impl<'a> Requests<'a> {
    /// Translated from 0x00f735b0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &'a [u8]) -> Result<Self, String> {
        header(b, b"SREQ", 19, 52)?;
        let rows = table(b, 52, count(b, 8)?, 144)?;
        let auxiliary = aux(b, 12, 36)?;
        let mut requests = Vec::new();
        for r in rows.chunks_exact(144) {
            let source_id = u32at(r, 128)?;
            let source = if source_id == u32::MAX {
                None
            } else {
                let entry = count(b, 28)?
                    .checked_add(
                        (source_id as usize)
                            .checked_mul(4)
                            .ok_or("package index overflow")?,
                    )
                    .ok_or("package offset overflow")?;
                Some(text(b, count(b, entry)?)?)
            };
            requests.push(Request {
                raw: r,
                source,
                section: index(u32at(r, 132)?, auxiliary.sections.len() / 12)?,
            });
        }
        let random = if u32at(b, 32)? == u32::MAX {
            None
        } else {
            Some(text(b, count(b, 32)?)?)
        };
        Ok(Self {
            requests,
            random,
            auxiliary,
        })
    }
}
#[derive(Debug)]
pub struct StreamSource<'a> {
    pub path: &'a str,
    pub metadata: &'a [u8],
}
#[derive(Debug)]
pub struct StreamRequest<'a> {
    pub raw: &'a [u8],
    pub source: Option<usize>,
    pub section: Option<usize>,
}
#[derive(Debug)]
pub struct StreamRequests<'a> {
    pub sources: Vec<StreamSource<'a>>,
    pub requests: Vec<StreamRequest<'a>>,
    pub random: Option<&'a str>,
    pub auxiliary: AuxTables<'a>,
}
impl<'a> StreamRequests<'a> {
    /// Translated from 0x00dc0ef0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &'a [u8]) -> Result<Self, String> {
        header(b, b"STRQ", 27, 60)?;
        let src = table(b, count(b, 32)?, count(b, 8)?, 24)?;
        let rows = table(b, count(b, 36)?, count(b, 12)?, 156)?;
        let auxiliary = aux(b, 16, 44)?;
        let mut sources = Vec::new();
        for s in src.chunks_exact(24) {
            sources.push(StreamSource {
                path: text(b, count(s, 0)?)?,
                metadata: &s[4..],
            });
        }
        let mut requests = Vec::new();
        for r in rows.chunks_exact(156) {
            requests.push(StreamRequest {
                raw: r,
                source: index(u32at(r, 140)?, sources.len())?,
                section: index(u32at(r, 144)?, auxiliary.sections.len() / 12)?,
            });
        }
        let random = if u32at(b, 40)? == u32::MAX {
            None
        } else {
            Some(text(b, count(b, 40)?)?)
        };
        Ok(Self {
            sources,
            requests,
            random,
            auxiliary,
        })
    }
}
/// Safe relative loose-file location; existence/case matching belongs to the caller.
/// Prefix handling translated from 0x00dbffe0 (DDDA.exe 2017-12-13 Steam).
pub fn stream_path(source: &str) -> Result<String, String> {
    let path = source.replace('\\', "/");
    if path
        .split('/')
        .any(|p| p == ".." || p.is_empty() || p.contains(':'))
    {
        return Err("unsafe stream path".into());
    }
    if path.to_ascii_lowercase().starts_with("sound/stream/") {
        Ok(format!("{path}.sngw"))
    } else {
        Ok(format!("sound/stream/{path}.sngw"))
    }
}
#[derive(Debug, PartialEq)]
pub struct VorbisInfo {
    pub channels: u8,
    pub sample_rate: u32,
    pub pages: usize,
}
/// Validate Ogg framing, CRCs, serial/sequence and the Vorbis identification packet.
/// This checks the container, not audio decoding.
pub fn validate_vorbis(b: &[u8]) -> Result<VorbisInfo, String> {
    let mut pos = 0;
    let mut pages = 0;
    let mut serial = None;
    let mut first = Vec::new();
    let mut first_done = false;
    while pos < b.len() {
        let h = span(b, pos, 27)?;
        if &h[..4] != b"OggS" || h[4] != 0 {
            return Err("Ogg header".into());
        }
        let n = h[26] as usize;
        let lace = span(b, pos + 27, n)?;
        let size = lace.iter().map(|&x| x as usize).sum::<usize>();
        let page = span(b, pos, 27 + n + size)?;
        let sid = u32at(h, 14)?;
        if serial.get_or_insert(sid) != &sid || u32at(h, 18)? != pages as u32 {
            return Err("Ogg serial/sequence".into());
        }
        let mut crc = 0u32;
        for (i, &x) in page.iter().enumerate() {
            crc ^= (if (22..26).contains(&i) { 0 } else { x as u32 }) << 24;
            for _ in 0..8 {
                crc = if crc & 0x80000000 != 0 {
                    (crc << 1) ^ 0x04c11db7
                } else {
                    crc << 1
                };
            }
        }
        if crc != u32at(h, 22)? {
            return Err("Ogg CRC".into());
        }
        if !first_done {
            let mut p = 27 + n;
            for &l in lace {
                first.extend_from_slice(&page[p..p + l as usize]);
                p += l as usize;
                if l < 255 {
                    first_done = true;
                    break;
                }
            }
        }
        pos += page.len();
        pages += 1;
    }
    if !first_done
        || first.len() < 30
        || &first[..7] != b"\x01vorbis"
        || u32at(&first, 7)? != 0
        || first[11] == 0
        || u32at(&first, 12)? == 0
        || first[29] & 1 == 0
    {
        return Err("Vorbis identification".into());
    }
    Ok(VorbisInfo {
        channels: first[11],
        sample_rate: u32at(&first, 12)?,
        pages,
    })
}
/// Validate a reconstructed RIFF container and return each chunk's bounded payload range.
pub fn riff_chunks(b: &[u8]) -> Result<Vec<([u8; 4], Range<usize>)>, String> {
    if span(b, 0, 4)? != b"RIFF" || span(b, 8, 4)? != b"WAVE" || count(b, 4)?.checked_add(8) != Some(b.len())
    {
        return Err("RIFF length/magic".into());
    }
    let mut p = 12;
    let mut out = Vec::new();
    while p < b.len() {
        let tag = span(b, p, 4)?.try_into().unwrap();
        let n = count(b, p + 4)?;
        span(b, p + 8, n)?;
        out.push((tag, p + 8..p + 8 + n));
        p += 8 + n + (n & 1);
    }
    if p != b.len() {
        return Err("RIFF padding".into());
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn put(b: &mut [u8], p: usize, v: u32) {
        b[p..p + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn requests() -> Vec<u8> {
        let mut b = vec![0; 204];
        b[..4].copy_from_slice(b"SREQ");
        put(&mut b, 4, 19);
        put(&mut b, 8, 1);
        put(&mut b, 28, 196);
        put(&mut b, 32, u32::MAX);
        for p in [36, 40, 44, 48] {
            put(&mut b, p, 204)
        }
        put(&mut b, 52 + 128, 0);
        put(&mut b, 52 + 132, u32::MAX);
        put(&mut b, 196, 200);
        b[200..204].copy_from_slice(b"abc\0");
        b
    }
    #[test]
    fn request_links_and_bounds() {
        let b = requests();
        assert_eq!(Requests::parse(&b).unwrap().requests[0].source, Some("abc"));
        for end in 0..b.len() {
            assert!(Requests::parse(&b[..end]).is_err(), "{end}");
        }
        let mut bad = b.clone();
        put(&mut bad, 52 + 128, u32::MAX - 1);
        assert!(Requests::parse(&bad).is_err());
        put(&mut bad, 52 + 128, 0);
        put(&mut bad, 52 + 132, 0);
        assert!(Requests::parse(&bad).is_err());
    }
    #[test]
    fn stream_links_and_bounds() {
        let mut b = vec![0; 244];
        b[..4].copy_from_slice(b"STRQ");
        put(&mut b, 4, 27);
        put(&mut b, 8, 1);
        put(&mut b, 12, 1);
        put(&mut b, 32, 60);
        put(&mut b, 36, 84);
        put(&mut b, 40, u32::MAX);
        for p in [44, 48, 52, 56] {
            put(&mut b, p, 244)
        }
        put(&mut b, 60, 240);
        put(&mut b, 84 + 140, 0);
        put(&mut b, 84 + 144, u32::MAX);
        b[240..].copy_from_slice(b"abc\0");
        let s = StreamRequests::parse(&b).unwrap();
        assert_eq!(s.sources[s.requests[0].source.unwrap()].path, "abc");
        put(&mut b, 84 + 140, 1);
        assert!(StreamRequests::parse(&b).is_err());
        put(&mut b, 84 + 140, 0);
        put(&mut b, 32, u32::MAX);
        assert!(StreamRequests::parse(&b).is_err());
    }
    #[test]
    fn split_riff_and_null_slot() {
        let mut b = vec![0; 82];
        b[..4].copy_from_slice(b"SPAC");
        put(&mut b, 4, 12);
        put(&mut b, 8, 2);
        for p in [20, 24, 28] {
            put(&mut b, p, 80)
        }
        put(&mut b, 32, u32::MAX);
        b[36..40].copy_from_slice(b"RIFF");
        put(&mut b, 40, 38);
        b[44..52].copy_from_slice(b"WAVEfmt ");
        put(&mut b, 52, 16);
        b[56] = 1;
        b[58] = 1;
        put(&mut b, 60, 22050);
        b[70] = 8;
        b[72..76].copy_from_slice(b"data");
        put(&mut b, 76, 2);
        b[80..].copy_from_slice(&[12, 34]);
        let r = Package::parse(&b).unwrap();
        assert!(r.waves[0].is_none());
        let w = r.waves[1].as_ref().unwrap();
        assert_eq!(w.samples, [12, 34]);
        assert_eq!(riff_chunks(&w.riff()).unwrap().len(), 2);
        put(&mut b, 76, 100);
        assert!(Package::parse(&b).is_err());
    }
    #[test]
    fn safe_stream_paths() {
        assert_eq!(
            stream_path("bgm\\wave\\Bridge").unwrap(),
            "sound/stream/bgm/wave/Bridge.sngw"
        );
        assert_eq!(stream_path("sound\\stream\\x").unwrap(), "sound/stream/x.sngw");
        for p in ["../x", "C:/x", "/x", "x//y"] {
            assert!(stream_path(p).is_err());
        }
    }
    #[test]
    fn ogg_crc_and_identification() {
        let mut b = vec![0; 58];
        b[..4].copy_from_slice(b"OggS");
        b[5] = 6;
        b[26] = 1;
        b[27] = 30;
        b[28..35].copy_from_slice(b"\x01vorbis");
        b[39] = 2;
        put(&mut b, 40, 44100);
        b[57] = 1;
        let mut crc = 0u32;
        for &x in &b {
            crc ^= (x as u32) << 24;
            for _ in 0..8 {
                crc = if crc & 0x80000000 != 0 {
                    (crc << 1) ^ 0x04c11db7
                } else {
                    crc << 1
                }
            }
        }
        put(&mut b, 22, crc);
        assert_eq!(
            validate_vorbis(&b).unwrap(),
            VorbisInfo {
                channels: 2,
                sample_rate: 44100,
                pages: 1
            }
        );
        b[40] ^= 1;
        assert!(validate_vorbis(&b).is_err());
    }
}
