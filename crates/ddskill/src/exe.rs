//! Read-only access to the player's installed `DDDA.exe`.
//!
//! The skill catalogue's bulk tables (skill records, name tables, action
//! tables, motion lists) are data inside the executable. Following the
//! constants policy in `AGENTS.md` they are read here at run time from the
//! player's own exe, never copied into the repo. Only the supported build
//! (PE timestamp, `METHODOLOGY.md`) is accepted, because every address is
//! specific to it. The sections used (`.text`, `.rdata`, `.data`) have the
//! same bytes in the installed Steam exe and the unpacked analysis copy
//! (checked for every table this crate reads).

use crate::Error;
use std::collections::HashMap;

/// PE timestamp of DDDA.exe 2017-12-13 Steam (`METHODOLOGY.md`).
pub const PE_TIMESTAMP: u32 = 1_513_180_209;

/// `MtDTI` constructor. Every reflected class registers itself with
/// `push name; mov ecx, dti; call 0x00cf5780` (e.g. 0x0136ddf0 for
/// `cPlActWpnDaggerCstmHyakuretsu`), which is how [`Exe::dti_names`] names
/// the class objects that the action tables point to.
pub const DTI_CONSTRUCTOR_VA: u32 = 0x00cf_5780;

#[derive(Debug, Clone, Copy)]
struct Section {
    name: [u8; 8],
    va: u32,
    vsize: u32,
    raw_size: u32,
    raw_offset: u32,
}

/// A parsed PE image of the supported build.
pub struct Exe<'a> {
    bytes: &'a [u8],
    base: u32,
    sections: Vec<Section>,
}

fn rd16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn rd32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

impl<'a> Exe<'a> {
    /// Parses the PE headers and refuses any build but the supported one.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let bad = || Error("not a PE image".into());
        let pe = rd32(bytes, 0x3c).ok_or_else(bad)? as usize;
        if bytes.get(pe..pe + 4) != Some(&b"PE\0\0"[..]) {
            return Err(bad());
        }
        let timestamp = rd32(bytes, pe + 8).ok_or_else(bad)?;
        if timestamp != PE_TIMESTAMP {
            return Err(Error(format!(
                "DDDA.exe PE timestamp {timestamp} is not the supported build {PE_TIMESTAMP}"
            )));
        }
        let count = rd16(bytes, pe + 6).ok_or_else(bad)? as usize;
        let optional = rd16(bytes, pe + 20).ok_or_else(bad)? as usize;
        let base = rd32(bytes, pe + 24 + 28).ok_or_else(bad)?;
        let mut sections = Vec::with_capacity(count);
        for i in 0..count {
            let s = pe + 24 + optional + 40 * i;
            sections.push(Section {
                name: bytes.get(s..s + 8).ok_or_else(bad)?.try_into().unwrap(),
                vsize: rd32(bytes, s + 8).ok_or_else(bad)?,
                va: rd32(bytes, s + 12).ok_or_else(bad)?,
                raw_size: rd32(bytes, s + 16).ok_or_else(bad)?,
                raw_offset: rd32(bytes, s + 20).ok_or_else(bad)?,
            });
        }
        Ok(Self { bytes, base, sections })
    }

    /// File offset of a virtual address that has file bytes.
    fn offset(&self, va: u32) -> Option<usize> {
        let rva = va.checked_sub(self.base)?;
        self.sections.iter().find_map(|s| {
            (rva >= s.va && rva < s.va + s.vsize.min(s.raw_size))
                .then(|| (s.raw_offset + rva - s.va) as usize)
        })
    }

    /// `len` bytes at `va`.
    pub fn bytes(&self, va: u32, len: usize) -> Result<&'a [u8], Error> {
        let o = self.offset(va).ok_or_else(|| Error(format!("{va:#010x} has no file bytes")))?;
        self.bytes
            .get(o..o + len)
            .ok_or_else(|| Error(format!("{va:#010x}+{len:#x} past the end of the file")))
    }

    pub fn u8(&self, va: u32) -> Result<u8, Error> {
        Ok(self.bytes(va, 1)?[0])
    }
    pub fn u16(&self, va: u32) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.bytes(va, 2)?.try_into().unwrap()))
    }
    pub fn i16(&self, va: u32) -> Result<i16, Error> {
        self.u16(va).map(|v| v as i16)
    }
    pub fn u32(&self, va: u32) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(va, 4)?.try_into().unwrap()))
    }
    pub fn i32(&self, va: u32) -> Result<i32, Error> {
        self.u32(va).map(|v| v as i32)
    }

    /// NUL-terminated ASCII string at `va`.
    pub fn cstr(&self, va: u32) -> Result<String, Error> {
        let o = self.offset(va).ok_or_else(|| Error(format!("{va:#010x} has no file bytes")))?;
        let rest = &self.bytes[o..];
        let n = rest
            .iter()
            .take(1024)
            .position(|&c| c == 0)
            .ok_or_else(|| Error(format!("unterminated string at {va:#010x}")))?;
        let s = &rest[..n];
        if !s.iter().all(|c| c.is_ascii() && !c.is_ascii_control()) {
            return Err(Error(format!("non-ASCII string at {va:#010x}")));
        }
        Ok(s.iter().map(|&c| c as char).collect())
    }

    /// Class names of the reflected classes, keyed by the address of their
    /// `MtDTI` object, found by scanning `.text` for the constructor calls
    /// (`68 <name> B9 <dti> E8 <rel32 to DTI_CONSTRUCTOR_VA>`).
    pub fn dti_names(&self) -> HashMap<u32, String> {
        let mut out = HashMap::new();
        for s in self.sections.iter().filter(|s| s.name.starts_with(b".text")) {
            let start = s.raw_offset as usize;
            let end = (s.raw_offset + s.raw_size.min(s.vsize)) as usize;
            let Some(text) = self.bytes.get(start..end) else { continue };
            let va0 = self.base + s.va;
            for i in 0..text.len().saturating_sub(15) {
                if text[i] != 0x68 || text[i + 5] != 0xb9 || text[i + 10] != 0xe8 {
                    continue;
                }
                let rel = i32::from_le_bytes(text[i + 11..i + 15].try_into().unwrap());
                let next = va0 + i as u32 + 15;
                if next.wrapping_add(rel as u32) != DTI_CONSTRUCTOR_VA {
                    continue;
                }
                let name_va = u32::from_le_bytes(text[i + 1..i + 5].try_into().unwrap());
                let dti = u32::from_le_bytes(text[i + 6..i + 10].try_into().unwrap());
                if let Ok(name) = self.cstr(name_va) {
                    if !name.is_empty() {
                        out.insert(dti, name);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE image: one `.text` section at VA 0x00401000 (file 0x200)
    /// and one `.rdata` at 0x00402000 (file 0x400).
    fn image(timestamp: u32, text: &[u8], rdata: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 0x600];
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        b[0x88..0x8c].copy_from_slice(&timestamp.to_le_bytes());
        b[0x94..0x96].copy_from_slice(&0xe0u16.to_le_bytes());
        b[0x98 + 28..0x98 + 32].copy_from_slice(&0x0040_0000u32.to_le_bytes());
        for (i, (name, va, off)) in [(b".text\0\0\0", 0x1000u32, 0x200u32), (b".rdata\0\0", 0x2000, 0x400)]
            .iter()
            .enumerate()
        {
            let s = 0x98 + 0xe0 + 40 * i;
            b[s..s + 8].copy_from_slice(*name);
            b[s + 8..s + 12].copy_from_slice(&0x200u32.to_le_bytes());
            b[s + 12..s + 16].copy_from_slice(&va.to_le_bytes());
            b[s + 16..s + 20].copy_from_slice(&0x200u32.to_le_bytes());
            b[s + 20..s + 24].copy_from_slice(&off.to_le_bytes());
        }
        b[0x200..0x200 + text.len()].copy_from_slice(text);
        b[0x400..0x400 + rdata.len()].copy_from_slice(rdata);
        b
    }

    #[test]
    fn refuses_other_builds() {
        assert!(Exe::parse(b"MZ").is_err());
        assert!(Exe::parse(&image(1, &[], &[])).is_err());
        assert!(Exe::parse(&image(PE_TIMESTAMP, &[], &[])).is_ok());
    }

    #[test]
    fn reads_and_names() {
        // push 0x00402000 ("cFoo"); mov ecx, 0x01234568; call DTI_CONSTRUCTOR_VA
        let mut text = vec![0x68, 0x00, 0x20, 0x40, 0x00, 0xb9, 0x68, 0x45, 0x23, 0x01, 0xe8];
        let next = 0x0040_1000u32 + 15;
        text.extend_from_slice(&DTI_CONSTRUCTOR_VA.wrapping_sub(next).to_le_bytes());
        // The same pattern calling elsewhere is ignored.
        text.extend_from_slice(&[0x68, 0x00, 0x20, 0x40, 0x00, 0xb9, 0x00, 0, 0, 0x02, 0xe8, 0, 0, 0, 0]);
        let b = image(PE_TIMESTAMP, &text, b"cFoo\0\x34\x12");
        let exe = Exe::parse(&b).unwrap();
        assert_eq!(exe.cstr(0x0040_2000).unwrap(), "cFoo");
        assert_eq!(exe.u16(0x0040_2005).unwrap(), 0x1234);
        assert!(exe.u32(0x0050_0000).is_err());
        let names = exe.dti_names();
        assert_eq!(names.len(), 1);
        assert_eq!(names[&0x0123_4568], "cFoo");
    }
}
