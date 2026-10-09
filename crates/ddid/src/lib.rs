//! Identities every host-owned system shares (docs/adr/0002-game-host.md,
//! section 4), and the byte codec their save chunks use (section 6).
//!
//! - [`PlayerId`]: the seat in this session, 0 = the host's Arisen. It is
//!   the same type as `ddsim::PlayerId`.
//! - [`ProfileId`]: one Arisen across sessions and saves. Only the host
//!   maps seats to profiles. System crates key their state by seat and
//!   save it as [`Parts`]: one shared part plus one part per seat.
//!
//! The original has one Arisen and no such identities; these are design
//! decisions for co-op.

use std::collections::BTreeMap;

/// The seat of an Arisen in this session (0 = the host).
pub type PlayerId = u8;

/// Persistent identity of one Arisen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId(pub u64);

/// One system's saved state: the part the session shares and one part per
/// seated Arisen. The host files the shared part under the world and each
/// seat's part under that seat's profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parts {
    pub shared: Vec<u8>,
    pub players: BTreeMap<PlayerId, Vec<u8>>,
}

/// Little-endian writer for save chunks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn new() -> Self {
        Writer(Vec::new())
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.u16(v as u16)
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.u32(v as u32)
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// By bits: a load gives back the same float.
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }
    /// A length or count (u32).
    pub fn len(&mut self, n: usize) -> &mut Self {
        self.u32(u32::try_from(n).expect("chunk lengths fit u32"))
    }
    /// Length-prefixed bytes.
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.len(b.len());
        self.0.extend_from_slice(b);
        self
    }
    /// `Option<u16>` as a presence byte and the value.
    pub fn opt_u16(&mut self, v: Option<u16>) -> &mut Self {
        match v {
            Some(x) => self.u8(1).u16(x),
            None => self.u8(0),
        }
    }
    pub fn finish(self) -> Vec<u8> {
        self.0
    }
}

/// Reader matching [`Writer`]. Errors name what was being read.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

pub type Result<T> = std::result::Result<T, String>;

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Reader { b, at: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self
            .b
            .get(self.at..self.at.checked_add(n).ok_or("chunk length overflow")?)
            .ok_or_else(|| format!("chunk truncated at byte {}", self.at))?;
        self.at += n;
        Ok(s)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => Err(format!("bad bool {v} at byte {}", self.at - 1)),
        }
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    /// A count, refused when it could not fit in the rest of the chunk
    /// (each element takes at least `min_size` bytes).
    pub fn len(&mut self, min_size: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_size.max(1)) > self.b.len() - self.at {
            return Err(format!("count {n} too large at byte {}", self.at - 4));
        }
        Ok(n)
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.len(1)?;
        Ok(self.take(n)?.to_vec())
    }
    pub fn opt_u16(&mut self) -> Result<Option<u16>> {
        Ok(if self.bool()? { Some(self.u16()?) } else { None })
    }
    /// Checks a leading version byte.
    pub fn version(&mut self, what: &str, expected: u8) -> Result<()> {
        let v = self.u8()?;
        if v != expected {
            return Err(format!("{what} chunk version {v}, expected {expected}"));
        }
        Ok(())
    }
    /// Refuses trailing bytes.
    pub fn end(&self) -> Result<()> {
        if self.at != self.b.len() {
            return Err(format!("{} trailing bytes", self.b.len() - self.at));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut w = Writer::new();
        w.u8(1).bool(true).i8(-3).u16(7).i16(-9).u32(10).i32(-11).u64(1 << 40).f32(-0.0).len(3).bytes(b"xyz");
        w.opt_u16(Some(5)).opt_u16(None);
        let b = w.finish();
        let mut r = Reader::new(&b);
        assert_eq!(r.u8().unwrap(), 1);
        assert!(r.bool().unwrap());
        assert_eq!(r.i8().unwrap(), -3);
        assert_eq!(r.u16().unwrap(), 7);
        assert_eq!(r.i16().unwrap(), -9);
        assert_eq!(r.u32().unwrap(), 10);
        assert_eq!(r.i32().unwrap(), -11);
        assert_eq!(r.u64().unwrap(), 1 << 40);
        assert_eq!(r.f32().unwrap().to_bits(), (-0.0f32).to_bits());
        assert_eq!(r.len(1).unwrap(), 3);
        assert_eq!(r.bytes().unwrap(), b"xyz");
        assert_eq!(r.opt_u16().unwrap(), Some(5));
        assert_eq!(r.opt_u16().unwrap(), None);
        r.end().unwrap();
        assert!(Reader::new(&b[..3]).u32().is_err());
        assert!(Reader::new(&[0xff, 0xff, 0xff, 0x7f]).len(1).is_err());
    }
}
