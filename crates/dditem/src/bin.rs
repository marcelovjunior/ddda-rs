//! Little-endian cursor shared by the binary table readers.

use crate::Error;

pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.remaining() < n {
            return Err(Error::new(format!(
                "truncated: need {n} bytes at {:#x}, {} left",
                self.pos,
                self.remaining()
            )));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub(crate) fn i16(&mut self) -> Result<i16, Error> {
        Ok(self.u16()? as i16)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }

    pub(crate) fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// A NUL-terminated string. The loaders read at most `max` bytes into a
    /// fixed buffer (`0x00cfee30`); the file always carries the terminator.
    pub(crate) fn cstr(&mut self) -> Result<&'a [u8], Error> {
        let rest = &self.data[self.pos..];
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::new(format!("unterminated string at {:#x}", self.pos)))?;
        self.pos += n + 1;
        Ok(&rest[..n])
    }

    /// Magic and version header shared by the `etc\item` tables.
    pub(crate) fn header(&mut self, magic: &[u8; 4], version: u32) -> Result<u32, Error> {
        let m = self.bytes(4)?;
        if m != magic {
            return Err(Error::new(format!("bad magic {m:02x?}, want {magic:02x?}")));
        }
        let v = self.u32()?;
        if v != version {
            return Err(Error::new(format!("bad version {v:#x}, want {version:#x}")));
        }
        self.u32()
    }

    pub(crate) fn finish(&self) -> Result<(), Error> {
        if self.remaining() != 0 {
            return Err(Error::new(format!(
                "{} trailing bytes at {:#x}",
                self.remaining(),
                self.pos
            )));
        }
        Ok(())
    }
}
