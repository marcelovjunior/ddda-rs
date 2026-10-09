//! Think-table records (`cThinkMgrAct`, 0xa0 bytes) read from `DDDA.exe`.
//!
//! The 31 built-in tables are an array of record pointers at
//! [`TABLES_VA`] in `.data`; uEm vf 0x2f8 (uEm0100: 0x008eaf00) returns
//! `TABLES_VA[index]` for the FSM's CtrlThinkTbl process (0x005a9240).
//! Records are walked in place by the interpreter, so a table is just the
//! address of its first record; jumps and calls name other record addresses
//! (+0x34). The bulk records are read at run time from the player's exe
//! (`ddexe`), never copied into the repo.

use crate::Error;
use std::collections::{HashMap, VecDeque};

/// `.data` array of the built-in table pointers (indices 0..=30; 31 and up
/// point into `.rdata` and are not tables).
pub const TABLES_VA: u32 = 0x017e_bf78;
/// Number of built-in tables.
pub const TABLE_COUNT: u32 = 31;
/// Record size (`cThinkMgrAct`, DTI 0x01996760, allocated with size 0xa0 by
/// 0x008b6bf0).
pub const RECORD_SIZE: u32 = 0xa0;
/// "No limit" in the range fields.
pub const FLT_MAX_BITS: u32 = 0x7f7f_ffff;

/// One record, as the 40 little-endian words of its 0xa0 bytes.
///
/// Word +0x00 is the kind: 1 is a chain record (a condition, then a
/// command), 0 a row. The interpreter steps 0xa0 past kinds 0 and 1 and
/// stays put on anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Act {
    pub va: u32,
    pub w: [u32; 40],
}

impl Act {
    pub fn word(&self, off: u32) -> u32 {
        self.w[(off / 4) as usize]
    }
    pub fn int(&self, off: u32) -> i32 {
        self.word(off) as i32
    }
    pub fn float(&self, off: u32) -> f32 {
        f32::from_bits(self.word(off))
    }
    /// +0x00: 1 chain record, 0 row.
    pub fn kind(&self) -> u32 {
        self.w[0]
    }
    /// The interpreter's step past this record: 0xa0 for kinds 0 and 1,
    /// else 0 (0x008b853c and every caller of 0x008b7e80).
    pub fn step(&self) -> u32 {
        if self.kind() < 2 { RECORD_SIZE } else { 0 }
    }
    /// Chain record: +0x08 code. Row: +0x08 flags.
    pub fn code(&self) -> u32 {
        self.w[2]
    }
    pub fn flags(&self) -> u32 {
        self.w[2]
    }
    /// Chain record: +0x0c; nonzero makes the condition true and the
    /// command a no-op (0x008b9500, 0x008ba2a0). Row: +0x0c second flags.
    pub fn param(&self) -> u32 {
        self.w[3]
    }
    /// Row +0x30: the result (an action id, or -1 none, -2 return, -6 table
    /// switch).
    pub fn action(&self) -> i32 {
        self.w[12] as i32
    }
    /// +0x34: a record address (jump/call target, or the table for chain
    /// commands 1 and 2).
    pub fn table(&self) -> u32 {
        self.w[13]
    }
    /// Chain record +0x38 (`a`) and +0x40 (`b`): the arguments of its
    /// condition and command.
    pub fn a(&self) -> u32 {
        self.w[14]
    }
    pub fn b(&self) -> u32 {
        self.w[16]
    }
    /// Row check/commit triple `i` (0..4) at +0x38 + 12*i: (op, a, b).
    pub fn triple(&self, i: usize) -> (u32, u32, u32) {
        let k = 14 + 3 * i;
        (self.w[k], self.w[k + 1], self.w[k + 2])
    }
    /// +0x68 as one 64-bit value: low dword sets (low 16) / clears (high 16)
    /// bits of the think's +0x1250 mask; high dword requires / forbids them.
    pub fn mask1250(&self) -> u64 {
        u64::from(self.w[26]) | u64::from(self.w[27]) << 32
    }
    /// +0x70: the same for the +0x124c mask.
    pub fn mask124c(&self) -> u64 {
        u64::from(self.w[28]) | u64::from(self.w[29]) << 32
    }
    /// +0x78: four 16-bit byte rules for the think bytes +0x60..+0x63.
    pub fn byte_rules(&self) -> [u32; 4] {
        [
            self.w[30] & 0xffff,
            self.w[30] >> 16,
            self.w[31] & 0xffff,
            self.w[31] >> 16,
        ]
    }
}

/// Every record reachable from the selected built-in tables (and the records they
/// name), keyed by address.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    roots: Vec<u32>,
    recs: HashMap<u32, Act>,
}

impl Tables {
    /// Reads the tables from the supported exe. Each table is walked from
    /// its first record to its end row (row flag bit 0); an end row with
    /// flag 0x80000000 can be stepped over (0x008b8528), so the walk goes on
    /// after it. Record addresses at +0x34 inside `.data` are followed.
    // Translated from 0x008b81a0 record walk (DDDA.exe 2017-12-13 Steam).
    pub fn read(exe: &ddexe::Exe<'_>) -> Result<Self, Error> {
        let de = |e: ddexe::Error| Error::Data(e.0);
        let roots = exe.u32s(TABLES_VA, TABLE_COUNT as usize).map_err(de)?;
        // Also the one-record table uEm0100 loads when grabbed (0x00aad940
        // writes it to +0xe9c): 0x017ec010.
        Self::read_roots(exe, roots, &[0x017e_c010])
    }

    /// Reads a family's root tables with the shared record walker. `extra`
    /// contains tables installed directly by the unit rather than exposed
    /// through its indexed table getter; these do not change root indices.
    pub fn read_roots(exe: &ddexe::Exe<'_>, roots: Vec<u32>, extra: &[u32]) -> Result<Self, Error> {
        let de = |e: ddexe::Error| Error::Data(e.0);
        let mut t = Tables {
            roots: roots.clone(),
            recs: HashMap::new(),
        };
        let mut todo: VecDeque<u32> = roots.into_iter().collect();
        todo.extend(extra.iter().copied());
        while let Some(start) = todo.pop_front() {
            let mut va = start;
            while !t.recs.contains_key(&va) {
                let b = exe.bytes(va, RECORD_SIZE as usize).map_err(de)?;
                let mut w = [0u32; 40];
                for (i, c) in b.as_chunks::<4>().0.iter().enumerate() {
                    w[i] = u32::from_le_bytes(*c);
                }
                let act = Act { va, w };
                let next = act.table();
                if next != 0 && exe.section_of(next) == Some(".data") {
                    todo.push_back(next);
                }
                let stop = act.kind() >= 2
                    || (act.kind() == 0 && act.flags() & 1 != 0 && act.flags() & 0x8000_0000 == 0);
                t.recs.insert(va, act);
                if stop {
                    break;
                }
                va += RECORD_SIZE;
            }
        }
        Ok(t)
    }

    /// Builds tables from records given directly (tests).
    pub fn from_records(roots: Vec<u32>, recs: impl IntoIterator<Item = Act>) -> Self {
        Tables {
            roots,
            recs: recs.into_iter().map(|a| (a.va, a)).collect(),
        }
    }

    /// The first record of built-in table `index` (`TABLES_VA[index]`).
    pub fn root(&self, index: u32) -> Option<u32> {
        self.roots.get(index as usize).copied()
    }

    pub fn get(&self, va: u32) -> Result<&Act, Error> {
        self.recs
            .get(&va)
            .ok_or_else(|| Error::Data(format!("no think record at {va:#010x}")))
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
}
