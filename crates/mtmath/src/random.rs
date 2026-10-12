//! `MtRandom`: the engine's pseudo-random generator, a 128-bit xorshift.
//!
//! The game keeps one global instance (at 0x018D017C) and draws from it
//! everywhere: 386 direct callers of [`Random::next`] in DDDA.exe.

/// The generator state (16 bytes: four words at +0x00, +0x04, +0x08,
/// +0x0C). `next` shifts the words down and appends a new one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Random {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub w: u32,
}

/// The four starting words written by [`Random::seed`] (Marsaglia's
/// xorshift128 defaults: 123456789, 362436069, 521288629, 88675123).
/// Embedded in 0x00D006C0.
pub const DEFAULT_STATE: [u32; 4] = [0x075B_CD15, 0x159A_55E5, 0x1F12_3BB5, 0x0549_1333];
/// XORed into the seed before scrambling. Embedded in 0x00D006C0.
pub const SEED_XOR: u32 = 0x00AC_9365;
/// The scramble constant. Embedded in 0x00D006C0.
pub const SCRAMBLE: u32 = 0x65AC_9365;
/// Rounds of the seeding loop; each round makes five steps. 0x00D006C0.
pub const SEED_ROUNDS: u32 = 20;

/// The step shared by [`Random::next`] and [`Random::seed`]: combines the
/// newest word `w` with `t = x ^ (x << 15)` of the oldest one.
#[inline(always)]
pub fn mix(t: u32, w: u32) -> u32 {
    (((w >> 17) ^ t) >> 4) ^ w ^ t
}

/// `t = x ^ (x << 15)`, the oldest word's part of [`mix`].
#[inline(always)]
pub fn spread(x: u32) -> u32 {
    x ^ (x << 15)
}

/// One step of the seed scrambler used by [`Random::seed`] in place of the
/// state's oldest word.
#[inline(always)]
pub fn scramble(s: u32) -> u32 {
    // the shift amount is s & 3, an arithmetic shift of a positive constant
    let s = s ^ (SCRAMBLE >> (s & 3));
    s ^ (((s << 1) ^ s) << 3) ^ (((s >> 1) ^ s) >> 3)
}

/// What [`Random::next`] leaves besides the new state: the returned word
/// and `t` (EDX in the original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drawn {
    pub value: u32,
    pub t: u32,
}

/// What [`Random::seed`] leaves in the original's scratch registers: the
/// last scrambled seed (EAX) and its `t` (ECX).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seeded {
    pub scrambled: u32,
    pub t: u32,
}

impl Random {
    /// The next 32-bit value. The state shifts down one word and the new
    /// word, which is also the result, becomes `w`.
    #[allow(clippy::should_implement_trait)] // the original's name
    // Translated from 0x00D00690 (DDDA.exe 2017-12-13 Steam), MtRandom::next.
    pub fn next(&mut self) -> Drawn {
        let t = spread(self.x);
        let value = mix(t, self.w);
        *self = Random { x: self.y, y: self.z, z: self.w, w: value };
        Drawn { value, t }
    }

    /// Reseeds the generator. Starting from [`DEFAULT_STATE`]'s `w`, it runs
    /// 100 steps of [`mix`], feeding each with the next value of the
    /// scrambled seed instead of the oldest word, and keeps the last four
    /// words. The starting `x`, `y`, `z` are overwritten without being used.
    // Translated from 0x00D006C0 (DDDA.exe 2017-12-13 Steam), MtRandom::seed.
    pub fn seed(&mut self, seed: u32) -> Seeded {
        let mut s = seed ^ SEED_XOR;
        let mut words = [DEFAULT_STATE[0], DEFAULT_STATE[1], DEFAULT_STATE[2], DEFAULT_STATE[3]];
        let mut t = 0;
        for _ in 0..SEED_ROUNDS * 5 {
            s = scramble(s);
            t = spread(s);
            let w = mix(t, words[3]);
            words = [words[1], words[2], words[3], w];
        }
        *self = Random { x: words[0], y: words[1], z: words[2], w: words[3] };
        Seeded { scrambled: s, t }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_shifts_the_state() {
        let mut r = Random { x: 1, y: 2, z: 3, w: 4 };
        let d = r.next();
        assert_eq!((r.x, r.y, r.z), (2, 3, 4));
        assert_eq!(r.w, d.value);
        assert_eq!(d.t, 1 ^ (1 << 15));
    }
}
