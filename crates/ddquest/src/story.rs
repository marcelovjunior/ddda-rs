//! Story state: the saved flag banks quest scripts read and write.
//!
//! Layout from the executable (sGameSys singleton at *0x018fa4bc,
//! `cSAVE_DATA_STAGE` at sGameSys +0xac6e0, field names from MtDTI):
//!
//! | Bank | Where | Size | Evidence |
//! |---|---|---|---|
//! | SceNo (scenario number) | sGameSys +0x30 | u32 | cFSMOrder `SceNo` getter 0x005baf00 |
//! | GSF (`mGsfFlag`) | cSAVE_DATA_STAGE +0x4 | 0x600 bits | getter 0x005bad40, setter 0x00401280 |
//! | Quest flags (`mQstFlag`) | cSAVE_DATA_STAGE +0xc4 | 96 bits per quest | 0x004369d0, `MyQuestFlag` count 0x005b72f0 |
//!
//! Bits are numbered from the most significant bit of each 32-bit word:
//! bit `i` is `word[i >> 5] & (0x8000_0000 >> (i & 31))` (0x005bad40,
//! 0x00401280). The per-script FreeFlag bits are not saved here; they are
//! LSB-first bytes inside the running script (0x005bada0).

/// Number of global story flags (bounds check in 0x005bad40 / 0x005cc030).
pub const GSF_COUNT: usize = 0x600;
/// Bits per quest in `mQstFlag` (`MyQuestFlag` count, 0x005b72f0).
pub const QUEST_BITS: usize = 96;
/// Quests addressable in `mQstFlag` (bound in 0x004369d0).
pub const QUEST_COUNT: usize = 128;

/// A bit set numbered MSB-first within 32-bit words, as the game stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsbBits {
    words: Vec<u32>,
    len: usize,
}

impl MsbBits {
    pub fn new(len: usize) -> Self {
        Self { words: vec![0; len.div_ceil(32)], len }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Out-of-range reads are false (0x005bad40 returns 0 past 0x600).
    // Translated from 0x005bad40 (DDDA.exe 2017-12-13 Steam).
    pub fn get(&self, i: usize) -> bool {
        i < self.len && self.words[i >> 5] & (0x8000_0000 >> (i & 31)) != 0
    }
    /// Out-of-range writes are ignored (0x005cc030 checks the bound first).
    // Translated from 0x00401280 / 0x004012e0 (DDDA.exe 2017-12-13 Steam).
    pub fn set(&mut self, i: usize, v: bool) {
        if i >= self.len {
            return;
        }
        let m = 0x8000_0000u32 >> (i & 31);
        if v {
            self.words[i >> 5] |= m;
        } else {
            self.words[i >> 5] &= !m;
        }
    }
    pub fn words(&self) -> &[u32] {
        &self.words
    }
    /// Rebuild from stored words (e.g. a save); extra words are ignored,
    /// missing ones are zero.
    pub fn from_words(len: usize, words: &[u32]) -> Self {
        let mut b = Self::new(len);
        for (d, s) in b.words.iter_mut().zip(words) {
            *d = *s;
        }
        b
    }
}

/// The story-progress banks of one save. Shared by all players of a
/// session: the host owns it (see docs/COOP_DESIGN.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryFlags {
    /// Scenario number: the coarse story clock. Quest scripts set it with
    /// events (`quest\q0001_b00` sets 1200 with event st200 ev10); NPC
    /// layout selection compares it with 3600 and 7800 (0x00bbdaa0).
    pub sce_no: u32,
    pub gsf: MsbBits,
    quests: Vec<MsbBits>,
}

impl Default for StoryFlags {
    fn default() -> Self {
        Self {
            sce_no: 0,
            gsf: MsbBits::new(GSF_COUNT),
            quests: (0..QUEST_COUNT).map(|_| MsbBits::new(QUEST_BITS)).collect(),
        }
    }
}

impl StoryFlags {
    pub fn quest(&self, q: usize) -> Option<&MsbBits> {
        self.quests.get(q)
    }
    pub fn quest_mut(&mut self, q: usize) -> Option<&mut MsbBits> {
        self.quests.get_mut(q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msb_first_numbering() {
        let mut b = MsbBits::new(GSF_COUNT);
        b.set(0, true);
        b.set(33, true);
        assert_eq!(b.words()[0], 0x8000_0000);
        assert_eq!(b.words()[1], 0x4000_0000);
        assert!(b.get(33) && !b.get(32));
        b.set(GSF_COUNT, true);
        assert!(!b.get(GSF_COUNT));
    }

    #[test]
    fn quest_bit_94_is_mask_2_of_the_third_word() {
        // 0x004369d0 tests byte mask 1/2/4 of the third word of a quest's
        // 12 bytes, i.e. bits 95/94/93 in MSB-first numbering.
        let mut q = MsbBits::new(QUEST_BITS);
        q.set(94, true);
        assert_eq!(q.words()[2], 2);
        q.set(95, true);
        q.set(93, true);
        assert_eq!(q.words()[2], 7);
    }
}
