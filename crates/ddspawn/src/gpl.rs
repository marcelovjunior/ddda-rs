//! `rLayoutGroupParamList` (`.gpl`, `scr\st<S>\etc\st<S>_{e,n,p,t}`): a
//! stage's placement groups (enemies, NPCs, objects, sensor targets).
//!
//! Read sequence (byte-exact on all 199 files of the install):
//! - `rLayoutGroupParamList::vf10` 0x00CC6140: `"gpl\0"`, version 0x9E,
//!   the slot table (`u32 n` + n u32; slot N holds a group when bit 31 is
//!   set, and group N's placements are the layouts `st<S>_<A>m<B>n_<t>N`),
//!   a second u32 array, a u32, the group count, the groups in slot order;
//! - `cGroupParam::vf09` 0x00CC4690 for each group: two words, the unit
//!   kinds (`cUnitData` 0x00CC5920: name, belong flag), the cells
//!   (`cID` 0x00CC59E0), 41 scalars (below), three area lists
//!   (0x00CC6330, `cLifeArea` 0x00CC58C0 via 0x00CC63C0) and a tail;
//! - area shapes (`AreaHitShape::vf05` 0x00744960) and their `nZone`
//!   shape loaders (types 1 area 0x00FBEB90, 2 sphere 0x00FBADD0, 3
//!   cylinder 0x00FBA310, 6 cone 0x00FBA710, 8 AABB 0x00FBB010, 9 OBB
//!   0x00FB9270).
//!
//! Names: `mUnitKindList`, `mLayoutIDArray`, `mAppearBgn` / `mAppearEnd`
//! and `mRspnType` follow Riftstone's GPL notes (MIT); fields without a
//! known name keep their `cGroupParam` offset.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let s = self
            .b
            .get(self.at..self.at + n)
            .ok_or_else(|| Error(format!("gpl: truncated at {:#x}", self.at)))?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, Error> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn s32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], Error> {
        let mut o = [0.0; N];
        for v in &mut o {
            *v = self.f32()?;
        }
        Ok(o)
    }
    fn cstr(&mut self) -> Result<String, Error> {
        let start = self.at;
        while self.u8()? != 0 {}
        Ok(String::from_utf8_lossy(&self.b[start..self.at - 1]).into_owned())
    }
    /// Counts are read unsigned; a count beyond what is left is refused.
    fn count(&mut self, min_size: usize) -> Result<usize, Error> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_size) > self.b.len() - self.at {
            return Err(Error(format!("gpl: count {n} at {:#x} too large", self.at - 4)));
        }
        Ok(n)
    }
}

/// `cGroupParam::cUnitData`: an enemy kind of the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// e.g. `em0100`.
    pub name: String,
    pub belong: bool,
}

/// `cGroupParam::cID`: a map cell the group has placements in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Low 10 bits: the stage; the rest: the owning group.
    pub stage: u32,
    pub group: u32,
    /// The cell `<m>m<n>n` (the file stores n first).
    pub n: u16,
    pub m: u16,
}

/// An area shape (`nZone::ShapeInfo*`).
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    None,
    /// Type 1 `ShapeInfoArea`: height, base y, 4 vertices, a reference
    /// point, two more fields.
    Area {
        height: f32,
        base_y: f32,
        vertices: [[f32; 4]; 4],
        point: [f32; 4],
        flag: u8,
        word: u32,
    },
    /// Type 2 `ShapeInfoSphere`.
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
    /// Type 3 `ShapeInfoCylinder`: two end points and a radius.
    Cylinder {
        p0: [f32; 3],
        p1: [f32; 3],
        radius: f32,
    },
    /// Types 6 (cone), 8 (AABB), 9 (OBB): kept as read (none in the install).
    Other {
        kind: i32,
        words: Vec<u32>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    pub name: String,
    pub pos: [f32; 3],
    pub flags: [u8; 2],
    /// Shape header: f32 (1.0 in the data) and u8.
    pub scale: f32,
    pub byte: u8,
    pub shape: Shape,
}

/// The 41 scalar reads of `cGroupParam::vf09`, as read (raw u32/s32), with
/// accessors for the fields the spawner tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scalars(pub [u32; 41]);

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The slot (group number N of the layout names).
    pub slot: u32,
    /// +0x18 (s32) and +0x1C.
    pub head: (i32, u32),
    /// `mUnitKindList`.
    pub units: Vec<Unit>,
    /// `mLayoutIDArray`.
    pub cells: Vec<Cell>,
    pub s: Scalars,
    pub areas1: Vec<Area>,
    pub life: Vec<Vec<Area>>,
    pub areas2: Vec<Area>,
    /// +0x138, +0x13C, +0x130, +0x134.
    pub tail: (u8, u32, u8, u32),
    // Derived fields used by the spawner (cGroupParam offsets):
    /// +0x20 bit 0, +0x24, +0x28: `mAppearBgn` / `mAppearEnd`, the SceNo
    /// window [begin, end).
    pub scene_window: Option<(i32, i32)>,
    /// +0x20 bit 1 / bit 3 and +0x2C: the two layout flags (low / high u16).
    pub layout_flag: Option<u16>,
    pub layout_flag2: Option<u16>,
    /// The raw +0x2C word.
    pub layout_flags: u32,
    /// +0x34 bit 0 and +0x38: the hour window [begin, end) on a 24-hour
    /// ring (low / high u16).
    pub hours: Option<(u16, u16)>,
    /// +0x34 bit 2: the group never spawns.
    pub disabled: bool,
    /// +0x58: `mRspnType` (bits 0..2) and the rest of the respawn word
    /// (days in bits 3..10, clamped to 7 unless the type is 6).
    pub respawn: u32,
    /// +0x54 bits 0..2: despawn when the layout flag / SceNo window / hour
    /// window stops holding (`cLayoutSetCharaBase::vf06` 0x0075F860).
    pub despawn: u32,
    /// Conditions the spawn test reads that are not ported: +0x20 bits 4
    /// and 5, +0x140, +0x12C bit 17, +0x34 bit 7.
    pub other_conditions: Vec<&'static str>,
}

fn area(r: &mut Rd) -> Result<Area, Error> {
    let name = r.cstr()?;
    let pos = r.f32s::<3>()?;
    let flags = [r.u8()?, r.u8()?];
    let kind = r.s32()?;
    if kind == 0 {
        return Ok(Area {
            name,
            pos,
            flags,
            scale: 0.0,
            byte: 0,
            shape: Shape::None,
        });
    }
    if !matches!(kind, 1 | 2 | 3 | 6 | 8 | 9) {
        return Err(Error(format!("gpl: shape type {kind} at {:#x}", r.at - 4)));
    }
    let scale = r.f32()?;
    let byte = r.u8()?;
    let shape = match kind {
        1 => {
            let height = r.f32()?;
            let base_y = r.f32()?;
            let v = r.f32s::<16>()?;
            let point = r.f32s::<4>()?;
            let flag = r.u8()?;
            let word = r.u32()?;
            let mut vertices = [[0.0; 4]; 4];
            for (i, vx) in vertices.iter_mut().enumerate() {
                vx.copy_from_slice(&v[i * 4..i * 4 + 4]);
            }
            Shape::Area {
                height,
                base_y,
                vertices,
                point,
                flag,
                word,
            }
        }
        2 => {
            let s = r.f32s::<4>()?;
            Shape::Sphere {
                center: [s[0], s[1], s[2]],
                radius: s[3],
            }
        }
        3 => {
            let s = r.f32s::<12>()?;
            Shape::Cylinder {
                p0: [s[0], s[1], s[2]],
                p1: [s[4], s[5], s[6]],
                radius: s[8],
            }
        }
        k => {
            let n = match k {
                6 => 7,
                8 => 8,
                _ => 22,
            };
            let mut words = Vec::with_capacity(n);
            for _ in 0..n {
                words.push(r.u32()?);
            }
            if k == 9 {
                words.push(u32::from(r.u8()?));
            }
            Shape::Other { kind: k, words }
        }
    };
    Ok(Area {
        name,
        pos,
        flags,
        scale,
        byte,
        shape,
    })
}

fn areas(r: &mut Rd) -> Result<Vec<Area>, Error> {
    let n = r.count(19)?;
    (0..n).map(|_| area(r)).collect()
}

// Translated from 0x00CC4690 (DDDA.exe 2017-12-13 Steam).
fn group(r: &mut Rd, slot: u32) -> Result<Group, Error> {
    let head = (r.s32()?, r.u32()?);
    let n = r.count(2)?;
    let mut units = Vec::with_capacity(n);
    for _ in 0..n {
        units.push(Unit {
            name: r.cstr()?,
            belong: r.u8()? != 0,
        });
    }
    let n = r.count(16)?;
    let mut cells = Vec::with_capacity(n);
    for _ in 0..n {
        let lo = r.u32()?;
        let hi = r.u32()?;
        cells.push(Cell {
            stage: lo & 0x3ff,
            group: hi,
            n: r.u32()? as u16,
            m: r.u32()? as u16,
        });
    }
    let mut s = [0u32; 41];
    let mut k = 0;
    let mut next = |r: &mut Rd| -> Result<u32, Error> {
        let v = r.u32()?;
        s[k] = v;
        k += 1;
        Ok(v)
    };
    for _ in 0..23 {
        next(r)?;
    }
    let areas1 = areas(r)?;
    for _ in 0..15 {
        next(r)?;
    }
    let outer = r.count(4)?;
    let mut life = Vec::with_capacity(outer);
    for _ in 0..outer {
        life.push(areas(r)?);
    }
    next(r)?;
    let areas2 = areas(r)?;
    let tail = (r.u8()?, r.u32()?, r.u8()?, r.u32()?);
    // The scalar order of 0x00CC4690 (index: field):
    //  0 +0x20 b4   1 +0x12C b0-4  2 +0x20 b5   3 +0x20 b6   4 +0x20 b7
    //  5 +0x12C b12-16   6 +0x12C b17   7 +0x12C b18-22   8..11 +0x140..+0x143
    // 12 +0x20 b0  13 +0x24  14 +0x28  15 +0x20 b1  16 +0x20 b3  17 +0x2C
    // 18 +0x34 b0  19 +0x38  20 +0x34 b6  21 +0x30  22 +0x34 b1
    // (areas1) 23..26 +0x34 b2..b5  27 +0x50  28 +0x34 b7  29 +0x34 b8-16
    // 30..32 +0x54 b0..b2  33 +0x58 b0-2  34 +0x58 b3-10  35 b11-12
    // 36 b13-14  37 b15  (life areas)  38 +0x70  (areas2)
    let on = |i: usize| s[i] != 0;
    let mut respawn = (s[33] & 7)
        | ((s[34] & 0xff) << 3)
        | ((s[35] & 3) << 11)
        | ((s[36] & 3) << 13)
        | (u32::from(on(37)) << 15);
    if respawn & 7 != 6 && (respawn & 0x7f8) > 0x38 {
        respawn = (respawn & !0x7c0) | 0x38;
    }
    let mut other = vec![];
    if on(0) {
        other.push("+0x20 bit 4 (0x004A9040)");
    }
    if on(2) {
        other.push("+0x20 bit 5 (0x004A9080)");
    }
    if on(8) {
        other.push("+0x140 (sSetManager +0x14B7C)");
    }
    if on(6) {
        other.push("+0x12C bit 17 (sSetManager +0x14B70 table)");
    }
    if on(28) {
        other.push("+0x34 bit 7 (0x004A52B0)");
    }
    Ok(Group {
        slot,
        head,
        units,
        cells,
        scene_window: on(12).then_some((s[13] as i32, s[14] as i32)),
        layout_flag: on(15).then_some(s[17] as u16),
        layout_flag2: on(16).then_some((s[17] >> 16) as u16),
        layout_flags: s[17],
        hours: on(18).then_some((s[19] as u16, (s[19] >> 16) as u16)),
        disabled: on(23),
        respawn,
        despawn: u32::from(on(30)) | u32::from(on(31)) << 1 | u32::from(on(32)) << 2,
        other_conditions: other,
        s: Scalars(s),
        areas1,
        life,
        areas2,
        tail,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupList {
    pub version: u32,
    pub slots: Vec<u32>,
    pub extra: Vec<u32>,
    pub dlc: u32,
    pub groups: Vec<Group>,
}

impl GroupList {
    /// Parses a whole file; the bytes must be consumed exactly.
    // Translated from 0x00CC6140 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<GroupList, Error> {
        let mut r = Rd { b, at: 0 };
        if r.take(4)? != b"gpl\0" {
            return Err(Error("gpl: bad magic".into()));
        }
        let version = r.u32()?;
        if version != 0x9e {
            return Err(Error(format!("gpl: version {version:#x}")));
        }
        let n = r.count(4)?;
        let slots = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
        let n = r.count(4)?;
        let extra = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
        let dlc = r.u32()?;
        let count = r.count(16)?;
        let used: Vec<u32> = slots
            .iter()
            .enumerate()
            .filter(|(_, v)| *v & 0x8000_0000 != 0)
            .map(|(i, _)| i as u32)
            .take(0x127)
            .collect();
        let mut groups = Vec::with_capacity(count);
        for i in 0..count {
            let slot = *used
                .get(i)
                .ok_or_else(|| Error(format!("gpl: group {i} has no slot")))?;
            groups.push(group(&mut r, slot)?);
        }
        if r.at != b.len() {
            return Err(Error(format!("gpl: {} bytes left", b.len() - r.at)));
        }
        Ok(GroupList {
            version,
            slots,
            extra,
            dlc,
            groups,
        })
    }

    pub fn group(&self, slot: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.slot == slot)
    }
}
