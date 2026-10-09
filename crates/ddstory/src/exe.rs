//! Story tables that exist only as code in DDDA.exe, decoded from the
//! player's exe at run time (never copied into this repository).
//!
//! They sit in `.text`; in the Steam exe of 2017-12-13 those bytes are
//! the same in the installed (SteamStub-wrapped) file and in an unpacked
//! copy (measured at 0x00401000, 0x00444c20, 0x005b7a60). Without them the
//! routines that need them fail with [`crate::Error::Unimplemented`].

use ddquest::events::{Pe, PE_TIMESTAMP};

/// The world set-up at the end of the prologue, 0x00444c20 (called by
/// 0x00444b20): GSF bits set, layout flags set (stage, flag), and NPC
/// death bits cleared (revived).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrologueEnd {
    pub gsf: Vec<u32>,
    pub lots: Vec<(u32, u32)>,
    pub revive: Vec<u32>,
}

const FUNC: u32 = 0x0044_4c20;
const END: u32 = 0x0044_5aa8;

impl PrologueEnd {
    /// Decode the `mov dword ptr [esp+disp], imm32` table set-up of
    /// 0x00444c20: GSF list at esp+0x46c (27), (stage index, flag) pairs at
    /// esp+0x14 (0x110, low half stage index into 0x01530348, high half
    /// flag), revive list at esp+0x454 (6). `None` for other builds or
    /// when the decoded values are implausible.
    // Decoded from 0x00444c20 (DDDA.exe 2017-12-13 Steam).
    pub fn from_exe(exe: &[u8]) -> Option<PrologueEnd> {
        let pe = Pe::parse(exe)?;
        if pe.timestamp() != PE_TIMESTAMP {
            return None;
        }
        let code = pe.bytes(FUNC, (END - FUNC) as usize)?;
        let mut movs: Vec<(u32, u32)> = vec![];
        let mut i = 0;
        while i + 8 <= code.len() {
            // C7 44 24 d8 imm32 / C7 84 24 d32 imm32
            if code[i] == 0xc7 && code[i + 1] == 0x44 && code[i + 2] == 0x24 {
                let d = code[i + 3] as u32;
                let v = u32::from_le_bytes(code[i + 4..i + 8].try_into().ok()?);
                movs.push((d, v));
                i += 8;
                continue;
            }
            if code[i] == 0xc7 && code[i + 1] == 0x84 && code[i + 2] == 0x24 && i + 11 <= code.len() {
                let d = u32::from_le_bytes(code[i + 3..i + 7].try_into().ok()?);
                let v = u32::from_le_bytes(code[i + 7..i + 11].try_into().ok()?);
                movs.push((d, v));
                i += 11;
                continue;
            }
            i += 1;
        }
        let pick = |base: u32, n: u32| -> Option<Vec<u32>> {
            (0..n)
                .map(|k| movs.iter().find(|(d, _)| *d == base + 4 * k).map(|(_, v)| *v))
                .collect()
        };
        let gsf = pick(0x46c, 27)?;
        let raw = pick(0x14, 0x110)?;
        let revive = pick(0x454, 6)?;
        let mut lots = vec![];
        for v in raw {
            let idx = (v & 0xffff) as usize;
            let stage = *crate::world::STAGE_INDEX.get(idx)? as u32;
            lots.push((stage, v >> 16));
        }
        if gsf.iter().any(|&g| g >= 0x600) || revive.iter().any(|&n| n >= 900) {
            return None;
        }
        Some(PrologueEnd { gsf, lots, revive })
    }
}
