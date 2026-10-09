//! The executable's event table: which cutscenes exist, where their
//! timelines live and which NPCs act in them.
//!
//! The table is data inside DDDA.exe (`.data`, VA 0x01822d60, 98 rows of
//! 16 bytes, per-hundred-stage index at 0x01822d34). Its bytes are the same
//! in the SteamStub-packed exe of the install and in an unpacked copy, so
//! it is read from the player's own `DDDA.exe` rather than copied here.
//!
//! Row layout (users 0x00435a10, 0x00515130, 0x00516140, 0x0050be20):
//!
//! ```text
//! +0x0 u16 stage   +0x2 u16 event number
//! +0x4 u32 VA of the path "stNNN\evNN\stNNNevNN" (loaded as event\<path>, 0x004362f0)
//! +0x8 u16 passed to [0x018fa4ec] vf+0x4c when the event starts (0 or 3; meaning untraced)
//! +0xa u16 0, 1 or 2 (meaning untraced)
//! +0xc u32 VA of a u16 list of NPC ids ending in 0xffff, or 0
//! ```
//!
//! An event is addressed by its row index. `Event` orders give
//! (stage, event number); 0x00435a10 finds the row and then swaps some
//! rows for variants (see [`EventTable::resolve`]).

/// Build this table was measured on (METHODOLOGY.md): PE timestamp.
pub const PE_TIMESTAMP: u32 = 1_513_180_209;
pub const TABLE_VA: u32 = 0x0182_2d60;
pub const INDEX_VA: u32 = 0x0182_2d34;
pub const ROWS: usize = 98;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub stage: u16,
    pub event: u16,
    pub path: String,
    pub start_arg: u16,
    pub field_a: u16,
    pub npcs: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTable {
    pub rows: Vec<EventRow>,
    /// First row of each hundred of stage numbers (index `stage / 100`),
    /// as stored at 0x01822d34; row range is `[first[i], first[i+1])`.
    pub first: Vec<u32>,
}

/// Minimal PE view: virtual-address reads of DDDA.exe.
pub struct Pe<'a> {
    b: &'a [u8],
    base: u32,
    sections: Vec<(u32, u32, u32, u32)>, // va, vsize, raw size, raw offset
    timestamp: u32,
}

fn rd32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn rd16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

impl<'a> Pe<'a> {
    pub fn parse(b: &'a [u8]) -> Option<Pe<'a>> {
        let pe = rd32(b, 0x3c)? as usize;
        if b.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let n = rd16(b, pe + 6)? as usize;
        let timestamp = rd32(b, pe + 8)?;
        let opt = rd16(b, pe + 20)? as usize;
        let base = rd32(b, pe + 24 + 28)?;
        let mut sections = vec![];
        for i in 0..n {
            let s = pe + 24 + opt + 40 * i;
            sections.push((rd32(b, s + 12)?, rd32(b, s + 8)?, rd32(b, s + 16)?, rd32(b, s + 20)?));
        }
        Some(Pe { b, base, sections, timestamp })
    }
    /// PE header timestamp (identifies the build).
    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }
    /// `len` bytes at virtual address `va`, if mapped from the file.
    pub fn bytes(&self, va: u32, len: usize) -> Option<&'a [u8]> {
        let o = self.off(va)?;
        self.b.get(o..o + len)
    }
    fn off(&self, va: u32) -> Option<usize> {
        let rva = va.checked_sub(self.base)?;
        self.sections.iter().find_map(|&(sva, vs, rs, ro)| {
            (rva >= sva && rva < sva + vs.min(rs)).then(|| (ro + rva - sva) as usize)
        })
    }
    fn u16(&self, va: u32) -> Option<u16> {
        rd16(self.b, self.off(va)?)
    }
    fn u32(&self, va: u32) -> Option<u32> {
        rd32(self.b, self.off(va)?)
    }
    fn cstr(&self, va: u32) -> Option<String> {
        let o = self.off(va)?;
        let s = self.b.get(o..)?;
        let n = s.iter().position(|&c| c == 0)?;
        Some(s[..n].iter().map(|&c| c as char).collect())
    }
}

impl EventTable {
    /// Read the table from DDDA.exe (packed or unpacked). Refuses other
    /// builds (PE timestamp) and implausible rows.
    pub fn from_exe(exe: &[u8]) -> Result<EventTable, String> {
        let pe = Pe::parse(exe).ok_or("not a PE image")?;
        if pe.timestamp != PE_TIMESTAMP {
            return Err(format!("PE timestamp {} is not the supported build", pe.timestamp));
        }
        let mut rows = Vec::with_capacity(ROWS);
        for i in 0..ROWS as u32 {
            let r = TABLE_VA + 16 * i;
            let bad = || format!("row {i} unreadable");
            let stage = pe.u16(r).ok_or_else(bad)?;
            let event = pe.u16(r + 2).ok_or_else(bad)?;
            let path = pe.cstr(pe.u32(r + 4).ok_or_else(bad)?).ok_or_else(bad)?;
            let expect = format!("st{stage:03}\\ev{event:02}\\st{stage:03}ev{event:02}");
            if path != expect {
                return Err(format!("row {i}: path {path:?}, expected {expect:?}"));
            }
            let mut npcs = vec![];
            let list = pe.u32(r + 12).ok_or_else(bad)?;
            if list != 0 {
                let mut a = list;
                loop {
                    let id = pe.u16(a).ok_or_else(bad)?;
                    if id == 0xffff {
                        break;
                    }
                    npcs.push(id);
                    a += 2;
                    if npcs.len() > 64 {
                        return Err(format!("row {i}: NPC list unterminated"));
                    }
                }
            }
            rows.push(EventRow {
                stage,
                event,
                path,
                start_arg: pe.u16(r + 8).ok_or_else(bad)?,
                field_a: pe.u16(r + 10).ok_or_else(bad)?,
                npcs,
            });
        }
        let first = (0..10).map(|i| pe.u32(INDEX_VA + 4 * i)).collect::<Option<Vec<_>>>().ok_or("index unreadable")?;
        Ok(EventTable { rows, first })
    }

    /// Row of (stage, event) before variant selection, searched only in the
    /// stage's hundred like 0x00435a10.
    // Translated from 0x00435a10 (DDDA.exe 2017-12-13 Steam), lookup part.
    pub fn find(&self, stage: u16, event: u16) -> Option<usize> {
        let h = (stage / 100) as usize;
        let (lo, hi) = (*self.first.get(h)? as usize, *self.first.get(h + 1)? as usize);
        (lo..hi.min(self.rows.len())).find(|&i| self.rows[i].stage == stage && self.rows[i].event == event)
    }

    /// The row actually played. 0x00435a10 swaps rows 7..12 and 13..18
    /// (stage 100 events 40..45 and 50..55) by a value derived from
    /// sGameSys +0xb3360 through 0x00477530 (`variant_b3360`): when it is
    /// 1 the first set plays, otherwise the second; rows 0x1a..0x1f
    /// depend on 0x00476d00 and are not modelled (returns the row itself).
    /// Which player property these are is untraced.
    pub fn resolve(&self, row: usize, variant_b3360: Option<i32>) -> usize {
        match (row, variant_b3360) {
            (7..=12, Some(v)) | (13..=18, Some(v)) => {
                let base = if row >= 13 { row - 6 } else { row };
                if v == 1 {
                    base
                } else {
                    base + 6
                }
            }
            _ => row,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_rows_swap_by_six() {
        let t = EventTable { rows: vec![], first: vec![] };
        assert_eq!(t.resolve(7, Some(1)), 7);
        assert_eq!(t.resolve(7, Some(0)), 13);
        assert_eq!(t.resolve(13, Some(1)), 7);
        assert_eq!(t.resolve(18, Some(2)), 18);
        assert_eq!(t.resolve(22, Some(0)), 22);
        assert_eq!(t.resolve(9, None), 9);
    }
}
