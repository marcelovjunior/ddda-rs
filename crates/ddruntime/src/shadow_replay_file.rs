//! Private, bounded capture format for offline L172 replay. Contains player
//! game data; never check these files into the repository or redistribute them.
use super::*;
use std::io::{self, Read, Write};

const MAGIC: &[u8; 8] = b"DDSH172\x04";
const CPU_WORDS: usize = 84;

pub(super) struct Fixture {
    pub input: Arc<Snapshot>,
    pub incoming: Cpu,
    pub outgoing: Cpu,
    pub mxcsr: u32,
    pub result_mxcsr: u32,
    pub context: u32,
    pub scheduler: u32,
    pub stack: Range<u32>,
    pub writes: BTreeMap<u32, u8>,
    /// Incomplete captures are diagnostic artifacts, never accepted proofs.
    pub rejection: Option<String>,
    /// Exact host shader objects referenced by the captured stream (private data).
    pub shaders: BTreeMap<u32, Vec<u32>>,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid L172 capture")
}
fn word(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn put(out: &mut impl Write, value: u32) -> io::Result<()> {
    out.write_all(&value.to_le_bytes())
}
fn write_cpu(out: &mut impl Write, cpu: &Cpu, mxcsr: u32) -> io::Result<()> {
    let mut words = crate::shadow_fee320::snapshot(cpu);
    assert_eq!(words.len(), CPU_WORDS);
    words[50] = mxcsr;
    for value in words {
        put(out, value)?;
    }
    Ok(())
}
fn read_cpu(input: &mut impl Read) -> io::Result<(Cpu, u32)> {
    let mut words = [0; CPU_WORDS];
    for value in &mut words {
        *value = word(input)?;
    }
    if words[40] > 13
        || words[45] > 1
        || words[46] > 1
        || (words[49] | words[50]) & !0xffff != 0
        || words[51..57].iter().any(|v| *v > 0xffff)
    {
        return Err(invalid());
    }
    let mut cpu = Cpu {
        eax: words[0],
        ecx: words[1],
        edx: words[2],
        ebx: words[3],
        esp: words[4],
        ebp: words[5],
        esi: words[6],
        edi: words[7],
        fs_base: words[47],
        sys_flags: words[48],
        ..Cpu::default()
    };
    for (value, source) in cpu.xmm.iter_mut().flatten().zip(&words[8..40]) {
        *value = *source;
    }
    cpu.fl = x86::Flags {
        kind: words[40] as u8,
        a: words[41],
        b: words[42],
        r: words[43],
        bits: words[44],
        cf_in: words[45] != 0,
    };
    cpu.df = words[46] != 0;
    cpu.mxcsr.0 = words[49];
    cpu.seg.es = words[51] as u16;
    cpu.seg.cs = words[52] as u16;
    cpu.seg.ss = words[53] as u16;
    cpu.seg.ds = words[54] as u16;
    cpu.seg.fs = words[55] as u16;
    cpu.seg.gs = words[56] as u16;
    for (bytes, value) in cpu.st.state.chunks_exact_mut(4).zip(&words[57..]) {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    Ok((cpu, words[50]))
}

impl Fixture {
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(MAGIC)?;
        for value in [self.context, self.scheduler, self.stack.start, self.stack.end] {
            put(out, value)?;
        }
        write_cpu(out, &self.incoming, self.mxcsr)?;
        write_cpu(out, &self.outgoing, self.result_mxcsr)?;
        let rejection = self.rejection.as_deref().unwrap_or("").as_bytes();
        if rejection.len() > 4096 {
            return Err(invalid());
        }
        put(out, rejection.len() as u32)?;
        out.write_all(rejection)?;
        self.input.write_to(out)?;
        put(out, self.writes.len() as u32)?;
        for (&address, &value) in &self.writes {
            put(out, address)?;
            out.write_all(&[value])?;
        }
        put(out, self.shaders.len() as u32)?;
        let mut total = 0usize;
        if self.shaders.len() > 4096 {
            return Err(invalid());
        }
        for (&handle, tokens) in &self.shaders {
            total = total.checked_add(tokens.len()).ok_or_else(invalid)?;
            if handle == 0 || tokens.is_empty() || total > 1 << 20 {
                return Err(invalid());
            }
            put(out, handle)?;
            put(out, tokens.len() as u32)?;
            for &token in tokens {
                put(out, token)?;
            }
        }
        Ok(())
    }
    pub fn read(input: &mut impl Read) -> io::Result<Self> {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC && &magic != b"DDSH172\x03" {
            return Err(invalid());
        }
        let (context, scheduler, low, high) = (word(input)?, word(input)?, word(input)?, word(input)?);
        let (incoming, mxcsr) = read_cpu(input)?;
        let (outgoing, result_mxcsr) = read_cpu(input)?;
        let rejection_len = word(input)? as usize;
        if rejection_len > 4096 {
            return Err(invalid());
        }
        let mut rejection = vec![0; rejection_len];
        input.read_exact(&mut rejection)?;
        let rejection = if rejection.is_empty() {
            None
        } else {
            Some(String::from_utf8(rejection).map_err(|_| invalid())?)
        };
        if low >= incoming.esp || incoming.esp >= high || outgoing.esp > high || high - low > 8 << 20 {
            return Err(invalid());
        }
        let snapshot = Arc::new(Snapshot::read_from(input)?);
        let count = word(input)? as usize;
        if count > lanes::MAX_TRACKED_BYTES {
            return Err(invalid());
        }
        let mut writes = BTreeMap::new();
        for _ in 0..count {
            let address = word(input)?;
            let mut value = [0];
            input.read_exact(&mut value)?;
            if snapshot.probe(address, 1, true).is_err() || writes.insert(address, value[0]).is_some() {
                return Err(invalid());
            }
        }
        let mut shaders = BTreeMap::new();
        if &magic == MAGIC {
            let count = word(input)?;
            if count > 4096 {
                return Err(invalid());
            }
            let mut total = 0usize;
            for _ in 0..count {
                let handle = word(input)?;
                let len = word(input)? as usize;
                total = total.checked_add(len).ok_or_else(invalid)?;
                if handle == 0 || len == 0 || total > 1 << 20 {
                    return Err(invalid());
                }
                let tokens = (0..len).map(|_| word(input)).collect::<io::Result<Vec<_>>>()?;
                if shaders.insert(handle, tokens).is_some() {
                    return Err(invalid());
                }
            }
        }
        let mut trailing = [0];
        if input.read(&mut trailing)? != 0 {
            return Err(invalid());
        }
        Ok(Self {
            input: snapshot,
            incoming,
            outgoing,
            mxcsr,
            result_mxcsr,
            context,
            scheduler,
            stack: low..high,
            writes,
            rejection,
            shaders,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_corruption_rejection() {
        ddnative_host::install();
        let mut memory = crate::memory::GuestMemory::new();
        memory.map(0x1000, 4096, 3).unwrap();
        let mut snapshot = Snapshot::default();
        snapshot.capture_page(&memory, 0x1000, 3);
        let incoming = Cpu {
            esp: 0x1800,
            ecx: 0x12345678,
            ..Cpu::default()
        };
        let outgoing = Cpu {
            esp: 0x180c,
            eax: 0x87654321,
            ..incoming.clone()
        };
        let fixture = Fixture {
            input: Arc::new(snapshot),
            incoming,
            outgoing,
            mxcsr: 0x1f80,
            result_mxcsr: 0x3f80,
            context: 0x1000,
            scheduler: 0x1100,
            stack: 0x1000..0x2000,
            writes: BTreeMap::from([(0x1100, 5)]),
            rejection: Some("synthetic diagnostic capture only".into()),
            shaders: BTreeMap::from([(0x1234, vec![0xfffe0300, 0xffff])]),
        };
        let mut bytes = Vec::new();
        fixture.write(&mut bytes).unwrap();
        let decoded = Fixture::read(&mut &bytes[..]).unwrap();
        assert_eq!(decoded.incoming.ecx, fixture.incoming.ecx);
        assert_eq!(decoded.outgoing.eax, fixture.outgoing.eax);
        assert_eq!(decoded.result_mxcsr, 0x3f80);
        assert_ne!(decoded.result_mxcsr, decoded.outgoing.mxcsr.0);
        assert_eq!(decoded.writes, fixture.writes);
        assert_eq!(decoded.rejection, fixture.rejection);
        assert_eq!(decoded.shaders, fixture.shaders);
        assert_eq!(decoded.input.u32(0x1100), 0);
        let mut again = Vec::new();
        decoded.write(&mut again).unwrap();
        assert_eq!(bytes, again);
        // Legacy captures have no shader bank; they remain strict byte proofs.
        let bank_start = bytes.len() - 20;
        let mut legacy = bytes[..bank_start].to_vec();
        legacy[7] = 3;
        assert!(Fixture::read(&mut &legacy[..]).unwrap().shaders.is_empty());
        let mut oversized = bytes.clone();
        oversized[bank_start..bank_start + 4].copy_from_slice(&4097u32.to_le_bytes());
        assert!(Fixture::read(&mut &oversized[..]).is_err());
        let mut duplicate = bytes.clone();
        duplicate[bank_start..bank_start + 4].copy_from_slice(&2u32.to_le_bytes());
        duplicate.extend_from_slice(&bytes[bank_start + 4..]);
        assert!(Fixture::read(&mut &duplicate[..]).is_err());
        let mut oversized = bytes.clone();
        oversized[bank_start + 8..bank_start + 12].copy_from_slice(&((1u32 << 20) + 1).to_le_bytes());
        assert!(Fixture::read(&mut &oversized[..]).is_err());
        assert!(Fixture::read(&mut &bytes[..bytes.len() - 1]).is_err());
        bytes[0] ^= 1;
        assert!(Fixture::read(&mut &bytes[..]).is_err());
        bytes[0] ^= 1;
        bytes.push(0);
        assert!(Fixture::read(&mut &bytes[..]).is_err());
    }
}
