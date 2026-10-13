//! L172 recording-lane layout and a strict, relocation-aware stream oracle.
//!
//! This is diagnostic code, not an alternative command executor. Callers must
//! capture both executions from the same input state and freeze resource objects
//! while comparing. Shader payloads require an explicit decoder: comparing their
//! pointers alone would silently miss changed transient constants.

use ddnative_base::mem::{Mem, Pages};
use std::collections::BTreeMap;

/// Layout read from 0x00DB63E0 / 0x00FED200 / 0x00FED3B0 (Steam 2017-12-13).
pub const LANE_BYTES: u32 = 0x33c84;
pub const LANE_COUNT: usize = 8;
pub const SLOT_BASE: u32 = 0x1bdd0;
pub const SLOT_STRIDE: u32 = 0x20;

/// Only the eight recording-context pointer words, not whole scheduler slots.
/// The caller must use a private scheduler or exclude all scheduler consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneSlots(pub [u32; LANE_COUNT]);

impl LaneSlots {
    pub fn read(m: &dyn Mem, scheduler: u32) -> Self {
        let p = Pages::of(m);
        Self(std::array::from_fn(|i| {
            p.u32(m, scheduler.wrapping_add(SLOT_BASE + i as u32 * SLOT_STRIDE))
        }))
    }

    pub fn write(&self, m: &mut dyn Mem, scheduler: u32) {
        let p = Pages::of(m);
        for (i, value) in self.0.iter().enumerate() {
            p.set_u32(
                m,
                scheduler.wrapping_add(SLOT_BASE + i as u32 * SLOT_STRIDE),
                *value,
            );
        }
    }

    pub fn contiguous(base: u32) -> Self {
        Self(std::array::from_fn(|i| base.wrapping_add(i as u32 * LANE_BYTES)))
    }
}

/// Diagnostic bounds, not original game limits. Exceeding one is a refusal.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub records: usize,
    pub bytes: usize,
    pub depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            records: 100_000,
            bytes: 64 * 1024 * 1024,
            depth: 128,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    Limit(&'static str),
    AddressOverflow,
    NullPointer,
    Cycle(u32),
    UnsupportedKind(u32),
    Shader(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BindingKind {
    Targets,
    Fixed,
    Shader,
    Streams,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub kind: BindingKind,
    /// First-use identity preserves pointer reuse (executor batching/cache tests).
    pub identity: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Exact header: no guessed masks for lane counters or sort metadata.
    pub header: u32,
    /// Scalar words with relocated command/binding pointers removed.
    pub words: Vec<u32>,
    pub bindings: Vec<Binding>,
    pub children: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: u32,
    pub record: Record,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamSnapshot {
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    /// Root entry index followed by nested/associated entry indices.
    pub path: Vec<usize>,
    pub field: &'static str,
}

impl StreamSnapshot {
    /// Report a bounded diagnostic rather than dumping shader or game bytes.
    pub fn first_difference(&self, other: &Self) -> Option<Difference> {
        fn entries(a: &[Entry], b: &[Entry], path: &mut Vec<usize>) -> Option<Difference> {
            if a.len() != b.len() {
                return Some(Difference {
                    path: path.clone(),
                    field: "entry count",
                });
            }
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                path.push(i);
                let field = if a.key != b.key {
                    Some("sort key")
                } else if a.record.header != b.record.header {
                    Some("record header")
                } else if a.record.words != b.record.words {
                    Some("record payload")
                } else if a.record.bindings != b.record.bindings {
                    Some("bindings")
                } else {
                    None
                };
                if let Some(field) = field {
                    return Some(Difference {
                        path: path.clone(),
                        field,
                    });
                }
                if let Some(diff) = entries(&a.record.children, &b.record.children, path) {
                    return Some(diff);
                }
                path.pop();
            }
            None
        }
        entries(&self.entries, &other.entries, &mut Vec::new())
    }
}

/// Capture the recording context's root array (+0x44) and count (+0x48),
/// as consumed/updated by 0x00FED3B0. Resources must remain frozen.
pub fn capture_context(m: &dyn Mem, context: u32, limits: Limits) -> Result<StreamSnapshot, CaptureError> {
    let p = Pages::of(m);
    let entries = p.u32(m, Capture::address(context, 0x44)?);
    let count = p.u32(m, Capture::address(context, 0x48)?);
    capture_stream(m, entries, count, limits, &mut |m, shader| {
        capture_shader(m, shader, limits.bytes)
    })
}

/// Freeze the shader input consumed by 0x00DAA8B0. Metadata identity and
/// resource IDs remain exact; transient constant addresses are replaced with
/// their tag and consumed bytes. Unused indexed values remain exact (a stricter
/// comparison than rendering equivalence). Counts/ranges follow that consumer,
/// not guessed allocation sizes. Derived temporary overreads are refused.
pub fn capture_shader(m: &dyn Mem, binding: u32, max_bytes: usize) -> Result<Vec<u8>, CaptureError> {
    struct Reader<'a> {
        m: &'a dyn Mem,
        p: Pages,
        out: Vec<u8>,
        max: usize,
    }
    impl Reader<'_> {
        fn word(&self, base: u32, offset: u32) -> Result<u32, CaptureError> {
            if base == 0 {
                return Err(CaptureError::NullPointer);
            }
            let a = Capture::address(base, offset)?;
            Capture::address(a, 3)?;
            Ok(self.p.u32(self.m, a))
        }
        fn append(&mut self, b: &[u8]) -> Result<(), CaptureError> {
            if b.len() > self.max.saturating_sub(self.out.len()) {
                return Err(CaptureError::Limit("shader bytes"));
            }
            self.out.extend_from_slice(b);
            Ok(())
        }
        fn value(&mut self, v: u32) -> Result<(), CaptureError> {
            self.append(&v.to_le_bytes())
        }
        fn block(&mut self, base: u32, size: u32) -> Result<(), CaptureError> {
            self.value(size)?;
            if size == 0 {
                return Ok(());
            }
            if base == 0 {
                return Err(CaptureError::NullPointer);
            }
            Capture::address(base, size - 1)?;
            if size as usize > self.max.saturating_sub(self.out.len()) {
                return Err(CaptureError::Limit("shader bytes"));
            }
            let start = self.out.len();
            self.out.resize(start + size as usize, 0);
            self.m.read_bytes(base, &mut self.out[start..]);
            Ok(())
        }
    }
    let mut r = Reader {
        m,
        p: Pages::of(m),
        out: Vec::new(),
        max: max_bytes,
    };
    let metadata = r.word(binding, 8)?;
    // Header words (including metadata identity) and the metadata's fixed fields.
    r.block(binding, 12)?;
    r.block(metadata, 0x24)?;
    let value_count = r.word(metadata, 0x20)?;
    if value_count > 1024 {
        return Err(CaptureError::Shader(
            "indexed value count exceeds descriptor encoding".into(),
        ));
    }
    let mut values = Vec::new();
    for i in 0..value_count {
        values.push(r.word(binding, 12 + i * 4)?);
    }
    let mut usage = vec![0u8; values.len()]; // 1: constant pointer, 2: resource identity
    let mut cursor = r.word(metadata, 4)?;
    for stage in 0..2 {
        let summary = r.word(metadata, 8 + stage * 4)?;
        let counts = [
            (summary >> 8) & 15,
            (summary >> 12) & 127,
            (summary >> 19) & 15,
            (summary >> 23) & 31,
            summary >> 28,
        ];
        let total: u32 = counts.iter().sum();
        if total != summary & 255 {
            return Err(CaptureError::Shader(
                "descriptor section length differs from decoded counts".into(),
            ));
        }
        for (section, count) in counts.into_iter().enumerate() {
            for _ in 0..count {
                let d = r.word(cursor, 0)?;
                cursor = Capture::address(cursor, 4)?;
                r.value(section as u32)?;
                r.value(d)?;
                let index = ((d >> 1) & if section == 4 { 127 } else { 31 }) as usize;
                if section == 0 {
                    for index in [((d >> 12) & 1023) as usize, (d >> 22) as usize] {
                        let value = *values
                            .get(index)
                            .ok_or_else(|| CaptureError::Shader("sampler index outside values".into()))?;
                        usage[index] |= 2;
                        r.value(value)?;
                    }
                } else {
                    let value = *values
                        .get(index)
                        .ok_or_else(|| CaptureError::Shader("constant index outside values".into()))?;
                    if section == 4 {
                        let kind = (d >> 8) & 255;
                        if ![3, 7].contains(&kind) || d >> 24 > 1 {
                            return Err(CaptureError::Shader(
                                "unsupported derived constant temporary".into(),
                            ));
                        }
                        usage[index] |= 2;
                        r.value(value)?;
                        // Texture dimensions are mutable; capture their packed words.
                        r.value(r.word(value, 0x20)?)?;
                        r.value(r.word(value, 0x24)?)?;
                    } else {
                        usage[index] |= 1;
                        r.value(value & 15)?;
                        let mut count = d >> 24;
                        if section == 1 && value & 15 != 0 {
                            count = count.min((value & 15) * 16);
                        }
                        let source = Capture::address(value & !15, ((d >> 6) & 1023) * 4)?;
                        // BOOL uploads count scalar words; integer/float count vec4s.
                        r.block(source, count * if section == 3 { 4 } else { 16 })?;
                    }
                }
            }
        }
    }
    for (value, usage) in values.into_iter().zip(usage) {
        if usage == 3 {
            return Err(CaptureError::Shader(
                "value used as both resource and constant pointer".into(),
            ));
        }
        r.value(usage as u32)?;
        r.value(if usage == 1 { value & 15 } else { value })?;
    }
    Ok(r.out)
}

/// A result is equal only after both complete captures succeed. Never compare
/// partially collected streams after an error. Guest memory faults propagate.
/// `shader` must freeze all metadata, indexed values and pointed-to constant
/// bytes, preserving tagged-pointer semantics. An unsupported source must Err.
/// Resource identities within target/fixed/stream blocks remain exact guest IDs;
/// their objects must be unchanged between the two executions.
///
/// Record layouts: 0x00DAB480 and 0x011F0BE0. Nested array relocation:
/// 0x00FED3B0. No original code or data is embedded here.
pub fn capture_stream(
    m: &dyn Mem,
    entries: u32,
    count: u32,
    limits: Limits,
    shader: &mut dyn FnMut(&dyn Mem, u32) -> Result<Vec<u8>, CaptureError>,
) -> Result<StreamSnapshot, CaptureError> {
    let mut capture = Capture {
        m,
        p: Pages::of(m),
        limits,
        shader,
        records: 0,
        bytes: 0,
        path: Vec::new(),
        identities: BTreeMap::new(),
    };
    Ok(StreamSnapshot {
        entries: capture.entries(entries, count, 0)?,
    })
}

struct Capture<'a> {
    m: &'a dyn Mem,
    p: Pages,
    limits: Limits,
    shader: &'a mut dyn FnMut(&dyn Mem, u32) -> Result<Vec<u8>, CaptureError>,
    records: usize,
    bytes: usize,
    path: Vec<u32>,
    identities: BTreeMap<(BindingKind, u32), usize>,
}

impl Capture<'_> {
    fn address(base: u32, offset: u32) -> Result<u32, CaptureError> {
        base.checked_add(offset).ok_or(CaptureError::AddressOverflow)
    }

    fn word(&self, base: u32, offset: u32) -> Result<u32, CaptureError> {
        let address = Self::address(base, offset)?;
        Self::address(address, 3)?;
        Ok(self.p.u32(self.m, address))
    }

    fn charge(&mut self, bytes: usize) -> Result<(), CaptureError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(CaptureError::Limit("bytes"))?;
        if self.bytes > self.limits.bytes {
            return Err(CaptureError::Limit("bytes"));
        }
        Ok(())
    }

    fn block(&mut self, address: u32, size: u32) -> Result<Vec<u8>, CaptureError> {
        if address == 0 {
            return Err(CaptureError::NullPointer);
        }
        Self::address(address, size.saturating_sub(1))?;
        self.charge(size as usize)?;
        let mut bytes = vec![0; size as usize];
        self.m.read_bytes(address, &mut bytes);
        Ok(bytes)
    }

    fn binding(&mut self, kind: BindingKind, address: u32) -> Result<Binding, CaptureError> {
        if address == 0 {
            return Err(CaptureError::NullPointer);
        }
        let next = self.identities.len();
        let identity = *self.identities.entry((kind, address)).or_insert(next);
        let bytes = match kind {
            BindingKind::Targets => self.block(address, 0x5c)?,
            BindingKind::Fixed => self.block(address, 0x14)?,
            BindingKind::Streams => self.block(address, 0x30)?,
            BindingKind::Shader => {
                let bytes = (self.shader)(self.m, address)?;
                self.charge(bytes.len())?;
                bytes
            }
        };
        Ok(Binding {
            kind,
            identity,
            bytes,
        })
    }

    fn entries(&mut self, base: u32, count: u32, depth: usize) -> Result<Vec<Entry>, CaptureError> {
        if depth > self.limits.depth {
            return Err(CaptureError::Limit("depth"));
        }
        if count as usize > self.limits.records.saturating_sub(self.records) {
            return Err(CaptureError::Limit("records"));
        }
        if count != 0 && base == 0 {
            return Err(CaptureError::NullPointer);
        }
        let size = count.checked_mul(8).ok_or(CaptureError::AddressOverflow)?;
        if size != 0 {
            Self::address(base, size - 1)?;
        }
        let mut result = Vec::new();
        for i in 0..count {
            let key = self.word(base, i * 8)?;
            let address = self.word(base, i * 8 + 4)?;
            result.push(Entry {
                key,
                record: self.record(address, depth)?,
            });
        }
        Ok(result)
    }

    fn record(&mut self, address: u32, depth: usize) -> Result<Record, CaptureError> {
        if address == 0 {
            return Err(CaptureError::NullPointer);
        }
        if depth > self.limits.depth {
            return Err(CaptureError::Limit("depth"));
        }
        if self.path.contains(&address) {
            return Err(CaptureError::Cycle(address));
        }
        self.records += 1;
        if self.records > self.limits.records {
            return Err(CaptureError::Limit("records"));
        }
        self.charge(8)?;
        self.path.push(address);
        let header = self.word(address, 0)?;
        let kind = (header >> 16) & 15;
        let mut out = Record {
            header,
            words: Vec::new(),
            bindings: Vec::new(),
            children: Vec::new(),
        };
        match kind {
            0..=3 => {
                for (offset, binding) in [
                    (4, BindingKind::Targets),
                    (8, BindingKind::Fixed),
                    (12, BindingKind::Shader),
                    (16, BindingKind::Streams),
                ] {
                    out.bindings
                        .push(self.binding(binding, self.word(address, offset)?)?);
                }
                let end = [0x1c, 0x20, 0x24, 0x28][kind as usize];
                for offset in (0x14..end).step_by(4) {
                    out.words.push(self.word(address, offset)?);
                }
            }
            6 => {
                // +4 is preserved metadata, only +8's entry-array address moves.
                out.words.push(self.word(address, 4)?);
                let base = self.word(address, 8)?;
                let count = self.word(address, 12)?;
                out.children = self.entries(base, count, depth + 1)?;
            }
            7 => {
                out.words.push(self.word(address, 4)?);
                out.words.push(self.word(address, 8)?);
            }
            4 | 5 => {
                let end = if kind == 4 { 0x20 } else { 0x28 };
                for offset in (4..end).step_by(4) {
                    out.words.push(self.word(address, offset)?);
                }
                let associated = self.word(address, end)?;
                out.children.push(Entry {
                    key: 0,
                    record: self.record(associated, depth + 1)?,
                });
            }
            // No known payload size for the executor's default-skip cases.
            // Refuse rather than silently reporting a complete match.
            _ => return Err(CaptureError::UnsupportedKind(kind)),
        }
        self.charge(out.words.len() * 4)?;
        self.path.pop();
        Ok(out)
    }
}
