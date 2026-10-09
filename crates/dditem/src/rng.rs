//! The game's seeded xorshift128 generator (an instance object, not the
//! global one), used by the Bitterblack curse roll.
//!
//! Translated from `0x00d006c0` (seed) and `0x00d00690` (next), DDDA.exe
//! 2017-12-13 Steam. Checked against an instruction-level emulation of the
//! two functions (test vectors in `tests.rs`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XorShift {
    pub state: [u32; 4],
}

/// One xorshift output step: `t = x ^ (x << 15)`, then
/// `w' = (((w >> 17) ^ t) >> 4) ^ w ^ t`.
fn step(x: u32, w: u32) -> u32 {
    let t = x ^ (x << 15);
    (((w >> 17) ^ t) >> 4) ^ w ^ t
}

impl XorShift {
    /// Seeding (`0x00d006c0`): the standard xorshift128 start words, then
    /// 100 chained steps whose `x` input comes from a mixer over
    /// `seed ^ 0xac9365`; the last four words become the state.
    pub fn new(seed: u32) -> Self {
        let mut a = seed ^ 0x00ac_9365;
        let mut w = 88_675_123u32;
        let mut chain = [0u32; 101];
        chain[0] = w;
        for slot in chain.iter_mut().skip(1) {
            let c = a & 3;
            a ^= (0x65ac_9365u32 as i32 >> c) as u32;
            let left = ((a << 1) ^ a) << 3;
            let right = ((a >> 1) ^ a) >> 3;
            a ^= left ^ right;
            w = step(a, w);
            *slot = w;
        }
        Self {
            state: [chain[97], chain[98], chain[99], chain[100]],
        }
    }

    /// Next value (`0x00d00690`); the new last word is returned.
    pub fn next_u32(&mut self) -> u32 {
        let [x, y, z, w] = self.state;
        let n = step(x, w);
        self.state = [y, z, w, n];
        n
    }
}
