//! State hashing for replay and desync checks.
//!
//! FNV-1a, 64 bit, over a canonical text of the state: the `Debug` form of
//! each value. Rust prints floats in their shortest round-trip form, so two
//! states print alike exactly when every float has the same bits (apart
//! from NaN payloads), private fields included. Slow, but the hash runs in
//! tests and desync checks, not every frame.

use std::fmt::Debug;

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateHasher(u64);

impl Default for StateHasher {
    fn default() -> Self {
        StateHasher(OFFSET)
    }
}

impl StateHasher {
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        for &x in b {
            self.0 ^= x as u64;
            self.0 = self.0.wrapping_mul(PRIME);
        }
        // A separator, so ("ab", "c") and ("a", "bc") differ.
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(PRIME);
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    pub fn debug(&mut self, v: &impl Debug) -> &mut Self {
        self.bytes(format!("{v:?}").as_bytes())
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_by_bits() {
        let h = |v: f32| StateHasher::default().debug(&v).finish();
        assert_ne!(h(0.1), h(f32::from_bits(0.1f32.to_bits() + 1)));
        assert_ne!(h(0.0), h(-0.0));
        assert_eq!(h(0.25), h(0.25));
        let a = StateHasher::default().bytes(b"ab").bytes(b"c").finish();
        let b = StateHasher::default().bytes(b"a").bytes(b"bc").finish();
        assert_ne!(a, b);
    }
}
