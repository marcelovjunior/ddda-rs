//! A fast hasher for maps keyed by guest addresses and small integer
//! tuples (L78): the D3D9 objects and device states are looked up several
//! times per draw, and std's SipHash cost more than the lookup itself. The
//! keys come from the game, not from an adversary, so a multiply-rotate
//! hash (rustc's FxHash) is enough; maps keyed by it behave exactly like
//! std's apart from speed and iteration order.
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().unwrap()));
        }
        for &b in chunks.remainder() {
            self.add(b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, v: u8) {
        self.add(v as u64);
    }
    #[inline]
    fn write_u16(&mut self, v: u16) {
        self.add(v as u64);
    }
    #[inline]
    fn write_u32(&mut self, v: u32) {
        self.add(v as u64);
    }
    #[inline]
    fn write_u64(&mut self, v: u64) {
        self.add(v);
    }
    #[inline]
    fn write_usize(&mut self, v: usize) {
        self.add(v as u64);
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;
pub type FxMap<K, V> = HashMap<K, V, FxBuild>;
pub type FxSet<K> = HashSet<K, FxBuild>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    fn hash<T: Hash>(v: T) -> u64 {
        FxBuild::default().hash_one(v)
    }

    /// Both halves of a tuple key count (a hasher that kept only the last
    /// word would put every (sampler, type) with one type in one bucket).
    #[test]
    fn tuple_keys_differ_in_either_half() {
        assert_ne!(hash((1u32, 2u32)), hash((2u32, 2u32)));
        assert_ne!(hash((1u32, 2u32)), hash((1u32, 3u32)));
        let distinct: FxSet<u64> = (0..21u32).flat_map(|s| (0..14u32).map(move |t| hash((s, t)))).collect();
        assert_eq!(distinct.len(), 21 * 14);
    }

    #[test]
    fn a_map_finds_what_it_holds() {
        let mut m: FxMap<u32, u32> = FxMap::default();
        for k in 0..10_000u32 {
            m.insert(k.wrapping_mul(0x9E37_79B9) & !0xF, k);
        }
        for k in 0..10_000u32 {
            assert_eq!(m.get(&(k.wrapping_mul(0x9E37_79B9) & !0xF)), Some(&k));
        }
    }
}
