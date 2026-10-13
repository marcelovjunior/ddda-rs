//! CPU-only storage used by the backend worker's draw path.
use d3d9cmd::Constants;

/// Hash a composite immutable key once, retaining full equality checks even
/// when hashes collide. The hash is process-local, never serialized.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Hashed<K> { pub key: K, hash: u64 }

impl<K: std::hash::Hash> Hashed<K> {
    pub fn new(key: K, mut hasher: impl std::hash::Hasher) -> Self {
        key.hash(&mut hasher);
        Self { key, hash: hasher.finish() }
    }
}

impl<K> std::hash::Hash for Hashed<K> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { state.write_u64(self.hash); }
}

/// A bounded memo with full equality inside identity-hash buckets. The old
/// single-entry slots thrashed even when all derived objects already existed.
/// 8192 entries accommodate the measured ~5700-draw scene (NATIVE_RENDERER.md)
/// while bounding retained snapshots; a full memo simply derives again.
pub(crate) struct DrawMemo<T> {
    entries: crate::cache::FxMap<u64, Vec<T>>,
    len: usize,
    limit: usize,
}

impl<T> Default for DrawMemo<T> {
    fn default() -> Self { Self { entries: Default::default(), len: 0, limit: 8192 } }
}

impl<T> DrawMemo<T> {
    pub fn get(&self, hash: u64, matches: impl Fn(&T) -> bool) -> Option<&T> {
        self.entries.get(&hash)?.iter().find(|value| matches(value))
    }
    pub fn insert(&mut self, hash: u64, value: T) {
        if self.len == self.limit { self.clear(); }
        self.entries.entry(hash).or_default().push(value);
        self.len += 1;
    }
    pub fn clear(&mut self) { self.entries.clear(); self.len = 0; }
}

/// Bit-preserving little-endian copy of complete register ranges.
pub(crate) fn copy_registers(dst: &mut [u8], registers: &[[u32; 4]]) {
    assert_eq!(dst.len(), registers.len() * 16);
    #[cfg(target_endian = "little")]
    {
        // u32 arrays have no padding, and the input remains borrowed for the copy.
        let bytes = unsafe { std::slice::from_raw_parts(registers.as_ptr().cast::<u8>(), dst.len()) };
        dst.copy_from_slice(bytes);
    }
    #[cfg(target_endian = "big")]
    for (out, word) in dst.chunks_exact_mut(4).zip(registers.iter().flatten()) {
        out.copy_from_slice(&word.to_le_bytes());
    }
}

pub(crate) fn pack_constants(dst: &mut [u8], c: &Constants, floats: usize, extra: &[[u32; 4]]) {
    assert_eq!(dst.len(), 16 * (floats + 32 + extra.len()));
    let used = c.f.len().min(floats);
    copy_registers(&mut dst[..used * 16], &c.f[..used]);
    dst[used * 16..floats * 16].fill(0);
    let (ints, tail) = dst[floats * 16..].split_at_mut(256);
    #[cfg(target_endian = "little")]
    {
        // i32 is four bytes with no padding; raw signed bits are the GPU ABI.
        let bytes = unsafe { std::slice::from_raw_parts(c.i.as_ptr().cast::<u8>(), 256) };
        ints.copy_from_slice(bytes);
    }
    #[cfg(target_endian = "big")]
    for (out, word) in ints.chunks_exact_mut(4).zip(c.i.iter().flatten()) {
        out.copy_from_slice(&word.to_le_bytes());
    }
    // Boolean registers occupy one vec4 each. Build all padding at once;
    // these are raw u32 values, not normalized Rust bools.
    let mut bools = [[0u32; 4]; 16];
    for (out, &value) in bools.iter_mut().zip(&c.b) { out[0] = value; }
    copy_registers(&mut tail[..256], &bools);
    copy_registers(&mut tail[256..], extra);
}

#[derive(Clone, Copy, Default)]
struct Word { epoch: u64, bits: u64 }
type Page = Box<[Word]>;
type Directory = Box<[Option<Page>]>;

/// Exact membership over all u32 guest IDs (addresses, not dense handles).
/// Two sparse radix levels lead to generation-stamped 64-bit words. Hot
/// inserts do no hashing or allocation; clear advances one epoch, retaining
/// pages for the next submission. Only the first use of a new page allocates.
pub(crate) struct ResourceUses {
    directories: Box<[Option<Directory>]>,
    epoch: u64,
}

impl Default for ResourceUses {
    fn default() -> Self {
        Self { directories: (0..256).map(|_| None).collect(), epoch: 1 }
    }
}

impl ResourceUses {
    pub fn clear(&mut self) {
        if self.epoch == u64::MAX {
            for directory in self.directories.iter_mut().flatten() {
                for page in directory.iter_mut().flatten() { page.fill(Word::default()); }
            }
            self.epoch = 1;
        } else { self.epoch += 1; }
    }

    pub fn contains(&self, resource: &u32) -> bool {
        let Some(directory) = &self.directories[(resource >> 24) as usize] else { return false };
        let Some(page) = &directory[((resource >> 16) & 255) as usize] else { return false };
        let word = &page[((resource >> 6) & 1023) as usize];
        word.epoch == self.epoch && word.bits & (1 << (resource & 63)) != 0
    }

    fn insert(&mut self, resource: u32) {
        let directory = self.directories[(resource >> 24) as usize]
            .get_or_insert_with(|| (0..256).map(|_| None).collect());
        let page = directory[((resource >> 16) & 255) as usize]
            .get_or_insert_with(|| vec![Word::default(); 1024].into_boxed_slice());
        let word = &mut page[((resource >> 6) & 1023) as usize];
        if word.epoch != self.epoch { *word = Word { epoch: self.epoch, bits: 0 }; }
        word.bits |= 1 << (resource & 63);
    }

    pub fn extend(&mut self, resources: impl IntoIterator<Item = u32>) {
        for resource in resources { self.insert(resource); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_hash_keeps_full_key_equality() {
        let mut map = std::collections::HashMap::new();
        // Force an identical cached hash; different keys must remain distinct.
        map.insert(Hashed { key: vec![1, 2], hash: 7 }, 12);
        map.insert(Hashed { key: vec![1, 3], hash: 7 }, 13);
        assert_eq!(map[&Hashed { key: vec![1, 2], hash: 7 }], 12);
        assert_eq!(map[&Hashed { key: vec![1, 3], hash: 7 }], 13);
    }

    #[test]
    fn draw_memo_collision_and_eviction_never_return_a_different_state() {
        let mut memo = DrawMemo { limit: 3, ..Default::default() };
        memo.insert(7, 11);
        memo.insert(7, 12);
        memo.insert(8, 13);
        assert_eq!(memo.get(7, |v| *v == 11), Some(&11));
        assert_eq!(memo.get(7, |v| *v == 12), Some(&12));
        assert_eq!(memo.get(7, |v| *v == 13), None);
        memo.insert(7, 14);
        assert_eq!(memo.get(7, |v| *v == 11), None);
        assert_eq!(memo.get(7, |v| *v == 14), Some(&14));
        memo.clear();
        assert_eq!(memo.get(7, |_| true), None);
    }

    #[test]
    fn packed_constants_match_scalar_abi_at_every_prefix() {
        let c = Constants {
            f: (0..256).map(|i| [i, !i, 0x8000_0000, 0x7fc0_1234]).collect(),
            i: std::array::from_fn(|i| [i as i32, -(i as i32), i32::MIN, i32::MAX]),
            b: std::array::from_fn(|i| (i as u32).wrapping_mul(0x1111_1111)),
        };
        for floats in [0, 1, 17, 224, 256, 260] {
            for extras in [0, 1, 5, 6] {
                let extra = vec![[u32::MAX, 0, 0x7f80_0000, 0x8000_0000]; extras];
                let mut actual = vec![0xcc; 16 * (floats + 32 + extras)];
                pack_constants(&mut actual, &c, floats, &extra);
                let expected: Vec<u8> = (0..floats).map(|i| c.f.get(i).copied().unwrap_or([0; 4]))
                    .chain(c.i.map(|v| v.map(|w| w as u32)))
                    .chain(c.b.map(|b| [b, 0, 0, 0])).chain(extra)
                    .flatten().flat_map(u32::to_le_bytes).collect();
                assert_eq!(actual, expected, "{floats} floats / {extras} extra");
            }
        }
    }

    #[test]
    fn resource_membership_matches_set_through_clear_and_epoch_wrap() {
        let mut uses = ResourceUses::default();
        let ids = [0, 1, 63, 64, 65535, 65536, 0x1234_5678, 0x1234_5679, 0xffff_ffff];
        let mut reference = std::collections::HashSet::new();
        for epoch in [1, 2, u64::MAX, 1] {
            uses.epoch = epoch;
            for id in ids {
                uses.extend([id, id]);
                reference.insert(id);
                for check in ids.into_iter().chain(ids.map(|x| x.wrapping_add(1))) {
                    assert_eq!(uses.contains(&check), reference.contains(&check));
                }
            }
            uses.clear();
            reference.clear();
            for id in ids { assert!(!uses.contains(&id)); }
        }
    }
}
