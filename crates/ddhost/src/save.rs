//! Save composition across crates.
//!
//! The original writes one save for one Arisen (`sSave`, cSAVE_DATA_*).
//! The host writes its own container (design decision, ADR 0002): a list
//! of chunks, each owned either by the shared world or by one Arisen's
//! [`ProfileId`]. Each crate serialises its own state into a chunk; the
//! host only frames them. Chunks the host does not know are kept as they
//! are, so a save survives a build that lacks a newer crate's chunk type.
//!
//! Layout ("DDHS", version 1, little endian):
//! `magic[4] version:u32 count:u32` then per chunk
//! `tag[4] owner:u8 (0 shared, 1 profile) [profile:u64] len:u32 bytes`.

pub use ddid::ProfileId;

pub const MAGIC: &[u8; 4] = b"DDHS";
pub const VERSION: u32 = 1;

/// The story world (`ddstory::World::save`, "DDST").
pub const TAG_WORLD: [u8; 4] = *b"WRLD";
/// One Arisen's place in the world: stage-frame position and facing.
pub const TAG_PLACE: [u8; 4] = *b"PLCE";
/// The host's own state: frame count, seats with profiles, hirers.
pub const TAG_HOST: [u8; 4] = *b"HOST";
/// `ddeconomy::Economy::save` parts.
pub const TAG_ECON: [u8; 4] = *b"ECON";
/// `ddprogress::Progression::save` parts.
pub const TAG_PROG: [u8; 4] = *b"PROG";
/// `ddrift::Rift::save` (shared only).
pub const TAG_RIFT: [u8; 4] = *b"RIFT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Shared,
    Profile(ProfileId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub tag: [u8; 4],
    pub owner: Owner,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveFile {
    pub chunks: Vec<Chunk>,
}

impl SaveFile {
    /// Adds or replaces the chunk with this tag and owner.
    pub fn put(&mut self, tag: [u8; 4], owner: Owner, bytes: Vec<u8>) {
        match self.chunks.iter_mut().find(|c| c.tag == tag && c.owner == owner) {
            Some(c) => c.bytes = bytes,
            None => self.chunks.push(Chunk { tag, owner, bytes }),
        }
    }

    pub fn get(&self, tag: [u8; 4], owner: Owner) -> Option<&[u8]> {
        self.chunks.iter().find(|c| c.tag == tag && c.owner == owner).map(|c| c.bytes.as_slice())
    }

    /// The chunks of one Arisen: what a guest takes home (design decision:
    /// the host saves everyone, and each player can keep their own part).
    pub fn profile_part(&self, p: ProfileId) -> SaveFile {
        SaveFile { chunks: self.chunks.iter().filter(|c| c.owner == Owner::Profile(p)).cloned().collect() }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        for c in &self.chunks {
            b.extend_from_slice(&c.tag);
            match c.owner {
                Owner::Shared => b.push(0),
                Owner::Profile(p) => {
                    b.push(1);
                    b.extend_from_slice(&p.0.to_le_bytes());
                }
            }
            b.extend_from_slice(&(c.bytes.len() as u32).to_le_bytes());
            b.extend_from_slice(&c.bytes);
        }
        b
    }

    pub fn from_bytes(b: &[u8]) -> Result<SaveFile, String> {
        let mut r = Reader { b, at: 0 };
        if r.take(4)? != MAGIC {
            return Err("not a DDHS save".into());
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(format!("DDHS version {version}, expected {VERSION}"));
        }
        let n = r.u32()?;
        let mut chunks = Vec::new();
        for _ in 0..n {
            let tag: [u8; 4] = r.take(4)?.try_into().unwrap();
            let owner = match r.take(1)?[0] {
                0 => Owner::Shared,
                1 => Owner::Profile(ProfileId(u64::from_le_bytes(r.take(8)?.try_into().unwrap()))),
                o => return Err(format!("chunk owner kind {o}")),
            };
            let len = r.u32()? as usize;
            chunks.push(Chunk { tag, owner, bytes: r.take(len)?.to_vec() });
        }
        if r.at != b.len() {
            return Err(format!("{} trailing bytes", b.len() - r.at));
        }
        Ok(SaveFile { chunks })
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("save truncated")?;
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// A [`TAG_PLACE`] chunk: position (cm, stage frame) and yaw.
pub fn place_bytes(pos: [f32; 3], yaw: f32) -> Vec<u8> {
    pos.iter().chain([yaw].iter()).flat_map(|v| v.to_le_bytes()).collect()
}

pub fn place_from(b: &[u8]) -> Option<([f32; 3], f32)> {
    if b.len() != 16 {
        return None;
    }
    let f = |i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    Some(([f(0), f(1), f(2)], f(3)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_unknown_chunks() {
        let mut s = SaveFile::default();
        s.put(TAG_WORLD, Owner::Shared, vec![1, 2, 3]);
        s.put(TAG_PLACE, Owner::Profile(ProfileId(9)), place_bytes([1.0, 2.0, 3.0], 0.5));
        s.put(*b"ECON", Owner::Profile(ProfileId(9)), vec![7; 10]);
        s.put(TAG_WORLD, Owner::Shared, vec![4]);
        let b = s.to_bytes();
        let t = SaveFile::from_bytes(&b).unwrap();
        assert_eq!(t, s);
        assert_eq!(t.get(TAG_WORLD, Owner::Shared), Some(&[4u8][..]));
        assert_eq!(place_from(t.get(TAG_PLACE, Owner::Profile(ProfileId(9))).unwrap()), Some(([1.0, 2.0, 3.0], 0.5)));
        assert_eq!(t.profile_part(ProfileId(9)).chunks.len(), 2);
        assert!(SaveFile::from_bytes(&b[..b.len() - 1]).is_err());
    }
}
