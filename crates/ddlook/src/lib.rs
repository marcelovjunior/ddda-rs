//! Headless character looks: which models make up a character, which of
//! their parts show, and the edit records that colour and shape them. The
//! renderer (ddview) builds one skeleton from this. See docs/CHARACTERS.md.
//!
//! NPC and event actors follow the actor builder `uActorModelPl::vf05`
//! 0x0083e010:
//!
//! The chain, all DDDA.exe 2017-12-13 Steam:
//! 1. `uActorModelPl::vf05` 0x0083e010: the NPC's home stage from the NPC
//!    ledger (lookup 0x004769c0, field +6), then
//!    `edit\stSSS\nNNN\stSSS_nNNN` (rEditPawn) applied by 0x008442d0.
//! 2. Variant of the armour set: 0x00ba6dd0 (special unit numbers) and
//!    0x00ba6950 (per NPC).
//! 3. Base body by sex (0x0083fa60), the NPC's armour set (0x00841960:
//!    rArmorTableNpc -> rArmorModelNpc), hair and beard (0x008407f0:
//!    rArmorTable / rArmorModel of `h_parts`).

pub mod save;

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A path, record kind or field this crate does not translate.
    Unimplemented(String),
    Data(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unimplemented(s) => write!(f, "unimplemented: {s}"),
            Error::Data(s) => write!(f, "data: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// Where resources come from (the player's install).
pub trait Source {
    /// A resource by path and class name; `None` when absent.
    fn resource(&self, path: &str, class: &str) -> Result<Option<Vec<u8>>, Error>;
}

impl Source for ddvfs::Vfs {
    fn resource(&self, path: &str, class: &str) -> Result<Option<Vec<u8>>, Error> {
        let t = mtarc::type_hash(class);
        if self.locate(path, t).is_none_or(|l| l.is_empty()) {
            return Ok(None);
        }
        self.read(path, t).map(Some).map_err(|e| Error::Data(format!("{path}: {e}")))
    }
}

struct Rd<'a> {
    b: &'a [u8],
    o: usize,
    what: &'static str,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let s = self.b.get(self.o..self.o + n).ok_or_else(|| Error::Data(format!("{}: short at {:#x}", self.what, self.o)))?;
        self.o += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }
    fn i16(&mut self) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    /// NUL-terminated string (stream helper 0x00cfee30).
    fn cstr(&mut self) -> Result<String, Error> {
        let rest = self.b.get(self.o..).unwrap_or(&[]);
        let n = rest.iter().position(|&c| c == 0).ok_or_else(|| Error::Data(format!("{}: unterminated string", self.what)))?;
        let s = rest[..n].iter().map(|&c| c as char).collect();
        self.o += n + 1;
        Ok(s)
    }
    fn magic(&mut self, m: &[u8; 4], version: u32) -> Result<(), Error> {
        if self.take(4)? != m || self.u32()? != version {
            return Err(Error::Data(format!("{}: bad magic or version", self.what)));
        }
        Ok(())
    }
}

/// rEditPawn ("edp", version 0x20). Field names are the runtime offsets
/// they land at in the loaded resource (+0x60...) and in the actor.
#[derive(Debug, Clone, PartialEq)]
pub struct EditPawn {
    /// +0x60 -> actor +0x1324: 1 female, else male (0x0083fa60).
    pub sex: i16,
    /// +0x64 -> actor +0x139c: NPC armour set (rArmorTableNpc id), -1 none.
    pub armor_set: i16,
    pub f68: i16,
    pub f6c: i16,
    pub f70: f32,
    pub f74: f32,
    pub human_edit: String,
    pub body_edit: String,
    pub face_edit: String,
    /// +0x84..+0x8c -> actor +0x1330..+0x1338: weapons (uWeaponPl), -1 none.
    pub weapons: [i32; 3],
    /// +0x90..+0xac -> actor +0x1358..+0x1374: equipment pieces, -1 none.
    pub parts: [i32; 8],
    /// +0xb0 -> actor +0x1378: hair (h_parts rArmorTable id).
    pub hair: i32,
    /// +0xb4 -> actor +0x137c: beard (same table).
    pub beard: i32,
    /// +0xc0..+0x114: colours and scales (22 floats).
    pub floats: [f32; 22],
    pub f118: [u32; 3],
    pub f124: bool,
    pub f125: bool,
    pub f128: u16,
}

impl EditPawn {
    // Translated from rEditPawn::vf10 0x00cb5ab0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<EditPawn, Error> {
        let mut r = Rd { b, o: 0, what: "rEditPawn" };
        r.magic(b"edp\0", 0x20)?;
        let sex = r.i16()?;
        let armor_set = r.i16()?;
        let f68 = r.i16()?;
        let f6c = r.i16()?;
        let f70 = r.f32()?;
        let f74 = r.f32()?;
        let human_edit = r.cstr()?;
        let body_edit = r.cstr()?;
        let face_edit = r.cstr()?;
        let weapons = [r.i32()?, r.i32()?, r.i32()?];
        let mut parts = [0; 8];
        for p in &mut parts {
            *p = r.i32()?;
        }
        let hair = r.i32()?;
        let beard = r.i32()?;
        let mut floats = [0.0; 22];
        for f in &mut floats {
            *f = r.f32()?;
        }
        let f118 = [r.u32()?, r.u32()?, r.u32()?];
        let f124 = r.u8()? == 1;
        let f125 = r.u8()? == 1;
        let f128 = r.u16()?;
        Ok(EditPawn {
            sex,
            armor_set,
            f68,
            f6c,
            f70,
            f74,
            human_edit,
            body_edit,
            face_edit,
            weapons,
            parts,
            hair,
            beard,
            floats,
            f118,
            f124,
            f125,
            f128,
        })
    }
}

/// rArmorModel ("amr", 0x10) / rArmorModelNpc ("nmr", 0x10): model paths
/// by id. `kind` 0 and 1 go to two different model slot lists of the actor
/// (0x00843820 / 0x008436c0).
#[derive(Debug, Clone, PartialEq)]
pub struct ArmorModels {
    pub entries: Vec<(u32, u32, String)>,
}

impl ArmorModels {
    // Translated from rArmorModel::vf10 0x00caab30 and rArmorModelNpc::vf10
    // 0x00cab1c0 (DDDA.exe 2017-12-13 Steam). The blend list that follows
    // in `amr` files is not read.
    pub fn parse(b: &[u8]) -> Result<ArmorModels, Error> {
        let npc = b.starts_with(b"nmr\0");
        let mut r = Rd { b, o: 0, what: "rArmorModel" };
        r.magic(if npc { b"nmr\0" } else { b"amr\0" }, 0x10)?;
        let n = r.u32()?;
        if !npc {
            r.u32()?; // blend count
        }
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let id = r.u32()?;
            let kind = r.u32()?;
            entries.push((id, kind, r.cstr()?));
        }
        Ok(ArmorModels { entries })
    }

    /// 0x00cab3e0 / 0x00caae80: the entry with this id.
    pub fn get(&self, id: u32) -> Option<(u32, &str)> {
        self.entries.iter().find(|e| e.0 == id).map(|e| (e.1, e.2.as_str()))
    }
}

/// One record of rArmorTable ("atr", 1): an id and its model ids.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmorTable {
    pub entries: Vec<(u32, u32, u32, Vec<u32>)>,
}

impl ArmorTable {
    // Translated from rArmorTable::vf10 0x00cabbc0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<ArmorTable, Error> {
        let mut r = Rd { b, o: 0, what: "rArmorTable" };
        r.magic(b"atr\0", 1)?;
        let n = r.u32()?;
        let _size = r.u32()?;
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let id = r.u32()?;
            let a = r.u32()?;
            let c = r.u32()?;
            let k = r.u32()?;
            let mut m = Vec::with_capacity(k as usize);
            for _ in 0..k {
                m.push(r.u32()?);
            }
            entries.push((id, a, c, m));
        }
        Ok(ArmorTable { entries })
    }

    pub fn models(&self, id: u32) -> Option<&[u32]> {
        self.entries.iter().find(|e| e.0 == id).map(|e| e.3.as_slice())
    }
}

/// One record of rArmorTableNpc ("NTR", 0).
#[derive(Debug, Clone, PartialEq)]
pub struct NpcSet {
    pub id: u32,
    pub f1c: u32,
    /// rArmorModelNpc ids (-1 none).
    pub models: Vec<u32>,
    /// Colour binaries: two ints and RGBA.
    pub colors: Vec<(u32, u32, [f32; 4])>,
    /// Part ids switched on / off (0x00bc14a0, applied by 0x00841e20).
    pub on: Vec<u16>,
    pub off: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArmorTableNpc {
    pub sets: Vec<NpcSet>,
}

impl ArmorTableNpc {
    // Translated from rArmorTableNpc::vf10 0x00cac350 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<ArmorTableNpc, Error> {
        let mut r = Rd { b, o: 0, what: "rArmorTableNpc" };
        r.magic(b"NTR\0", 0)?;
        let _size = r.u32()?;
        let n = r.u32()?;
        let mut sets = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let id = r.u32()?;
            let f1c = r.u32()?;
            let k = r.u32()?;
            let mut models = Vec::with_capacity(k as usize);
            for _ in 0..k {
                models.push(r.u32()?);
            }
            let k = r.u32()?;
            let mut colors = Vec::with_capacity(k as usize);
            for _ in 0..k {
                colors.push((r.u32()?, r.u32()?, [r.f32()?, r.f32()?, r.f32()?, r.f32()?]));
            }
            let k = r.u32()?;
            let mut on = Vec::with_capacity(k as usize);
            for _ in 0..k {
                on.push(r.u16()?);
            }
            let k = r.u32()?;
            let mut off = Vec::with_capacity(k as usize);
            for _ in 0..k {
                off.push(r.u16()?);
            }
            sets.push(NpcSet { id, f1c, models, colors, on, off });
        }
        Ok(ArmorTableNpc { sets })
    }

    /// 0x00cac960: the set with this id.
    pub fn get(&self, id: u32) -> Option<&NpcSet> {
        self.sets.iter().find(|s| s.id == id)
    }
}

/// The armour-set variant flag of an actor (+0x13a0), set by 0x00ba6dd0
/// only for the special unit numbers 999, 0x3e3 and 0x3e4; 0 otherwise.
// Translated from 0x00ba6dd0 (DDDA.exe 2017-12-13 Steam).
pub fn set_variant(unit_no: u32, npc: u32, event_row: u32) -> u32 {
    if unit_no == 999 || unit_no == 0x3e4 {
        match (npc, event_row) {
            (0x66, 0x1c) => 2,
            (0x68, 0x1f) => 4,
            _ => 0,
        }
    } else if unit_no == 0x3e3 {
        match event_row {
            7..=0x12 | 0x40 | 0x41 | 0x4e..=0x51 => match npc {
                2 | 0x66 => 1,
                0x68 => 4,
                _ => 0,
            },
            _ => 0,
        }
    } else {
        0
    }
}

/// The armour set and hair after the per-NPC variants (0x00ba6950).
// Translated from 0x00ba6950 (DDDA.exe 2017-12-13 Steam).
pub fn apply_variant(npc: u32, variant: u32, set: &mut i32, hair: &mut i32) {
    match (npc, *set) {
        (2, 0x89) if variant != 0 => *set = 0x6d,
        (0x65, 0x84) if variant != 0 => *set = 0x85,
        (0x66, 0x84) if variant == 1 => (*set, *hair) = (0x85, 0x0f00_006f),
        (0x66, 0x84) if variant == 2 => (*set, *hair) = (0x86, 0x0f00_006c),
        (0x68, 0x8d) if variant == 1 => (*set, *hair) = (0xa9, 0x0f00_0070),
        (0x68, 0x8d) if variant == 2 => (*set, *hair) = (0xaa, 0x0f00_0071),
        (0x68, 0x8d) if variant == 3 => (*set, *hair) = (0x8d, 0x0f00_0070),
        (0x68, 0x8d) if variant == 4 => (*set, *hair) = (0xca, 0x0f00_0071),
        (0x75, 0x8b) if variant != 0 => *set = 0xa8,
        _ => {}
    }
}

/// rArmorPartsOff ("aor", 0x10): per armour model (id % 1000) the body
/// parts it hides: `(condition, part)`; a negative condition always hides,
/// otherwise only when a model with that id % 1000 is also worn.
#[derive(Debug, Clone, PartialEq)]
pub struct PartsOff {
    pub records: Vec<(u32, Vec<(i16, u16)>)>,
}

impl PartsOff {
    // Layout read from the file and the consumer 0x00bc1060 (record +4 id,
    // entry +4 s16 condition, +6 u16 part). The loader (rArmorPartsOff
    // vf10) was not decompiled; every file parses to its exact length.
    pub fn parse(b: &[u8]) -> Result<PartsOff, Error> {
        let mut r = Rd { b, o: 0, what: "rArmorPartsOff" };
        r.magic(b"aor\0", 0x10)?;
        let n = r.u32()?;
        let _size = r.u32()?;
        let mut records = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let id = r.u32()?;
            let k = r.u32()?;
            let mut e = Vec::with_capacity(k as usize);
            for _ in 0..k {
                let c = r.i16()?;
                e.push((c, r.u16()?));
            }
            records.push((id, e));
        }
        if r.o != b.len() {
            return Err(Error::Data(format!("rArmorPartsOff: {} trailing bytes", b.len() - r.o)));
        }
        Ok(PartsOff { records })
    }

    pub fn get(&self, id: u32) -> Option<&[(i16, u16)]> {
        self.records.iter().find(|r| r.0 == id).map(|r| r.1.as_slice())
    }
}

/// The part visibility mask of a character (actor +0x110, 512 bits): a
/// mesh draws when the bit of its part group (`mtmod::Mesh::group`) is
/// set. Body parts are 0-199 (base body 0-31, hair 50, beard 66); outfit
/// parts are 200 and up (e.g. 300, 301).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartMask(pub [u32; 16]);

impl Default for PartMask {
    /// Every part on (*assumed* model default; the builders below only
    /// clear and set ranges).
    fn default() -> Self {
        PartMask([u32::MAX; 16])
    }
}

impl PartMask {
    pub fn visible(&self, part: u16) -> bool {
        let p = part as usize;
        p < 512 && self.0[p >> 5] >> (p & 31) & 1 != 0
    }
    pub fn set(&mut self, part: u16, on: bool) {
        let p = part as usize;
        if p < 512 {
            if on {
                self.0[p >> 5] |= 1 << (p & 31);
            } else {
                self.0[p >> 5] &= !(1 << (p & 31));
            }
        }
    }
}

/// Who a character is, for the rules that differ (actor +0x1290).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Player,
    Pawn,
    Npc,
}

/// One model of a look.
#[derive(Debug, Clone, PartialEq)]
pub struct LookPart {
    /// rArmorModel kind: 0 and 1 are the actor's two model slot lists
    /// (0x00843820 / 0x008436c0).
    pub kind: u32,
    pub path: String,
    /// Hair and beard take the edit's hair colour (0x008407f0 ->
    /// 0x008438f0).
    pub hair: bool,
}

/// Everything a renderer needs to build one character on one skeleton.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// 0 male, 1 female.
    pub sex: u8,
    /// The base body; its skeleton is the character's skeleton.
    pub base: String,
    pub parts: Vec<LookPart>,
    pub mask: PartMask,
    /// Hair and beard colour: edit floats 8-10 x 0.5 + 0.2 (actor
    /// +0x13d0..+0x13d8, 0x008407f0), when the edit record has them.
    pub hair_color: Option<[f32; 3]>,
    /// Edit-record resources (rHumanEdit, rBodyEdit, rFaceEdit) for
    /// morphs; not applied by this crate.
    pub morphs: Option<[String; 3]>,
}

fn read<S: Source + ?Sized>(data: &S, path: &str, class: &str) -> Result<Vec<u8>, Error> {
    data.resource(path, class)?.ok_or_else(|| Error::Data(format!("{path} ({class}) missing")))
}

fn base_body(sex: u8) -> String {
    // Translated from 0x0083fa60 (DDDA.exe 2017-12-13 Steam).
    if sex == 1 { r"model\pl\f\f_base\f000\f000" } else { r"model\pl\m\m_base\m000\m000" }.to_string()
}


/// The armour tables, read once.
pub struct Tables {
    /// `m_parts` rArmorTable (sGameSys +0xbe238) and `h_parts` (+0xbe23c).
    pub table: ArmorTable,
    pub table_h: ArmorTable,
    /// rArmorModel by sex index: m, f, h (sGameSys +0xbe22c[sex]).
    pub models: [ArmorModels; 3],
    /// `m_parts` rArmorPartsOff (sGameSys +0xbe240).
    pub parts_off: PartsOff,
    /// rArmorTableNpc and rArmorModelNpc for m and f (+0xbe24c / +0xbe244).
    pub npc_sets: [ArmorTableNpc; 2],
    pub npc_models: [ArmorModels; 2],
}

impl Tables {
    /// The per-sex slots are *assumed* to hold `m_parts`, `f_parts`,
    /// `h_parts` in that order (the loader filling them was not traced;
    /// the base body rule 0x0083fa60 uses 1 for female).
    pub fn load<S: Source + ?Sized>(src: &S) -> Result<Tables, Error> {
        let p = |t: &str| format!("model\\pl\\parts\\{t}_parts");
        Ok(Tables {
            table: ArmorTable::parse(&read(src, &p("m"), "rArmorTable")?)?,
            table_h: ArmorTable::parse(&read(src, &p("h"), "rArmorTable")?)?,
            models: [
                ArmorModels::parse(&read(src, &p("m"), "rArmorModel")?)?,
                ArmorModels::parse(&read(src, &p("f"), "rArmorModel")?)?,
                ArmorModels::parse(&read(src, &p("h"), "rArmorModel")?)?,
            ],
            parts_off: PartsOff::parse(&read(src, &p("m"), "rArmorPartsOff")?)?,
            npc_sets: [
                ArmorTableNpc::parse(&read(src, &p("m"), "rArmorTableNpc")?)?,
                ArmorTableNpc::parse(&read(src, &p("f"), "rArmorTableNpc")?)?,
            ],
            npc_models: [
                ArmorModels::parse(&read(src, &p("m"), "rArmorModelNpc")?)?,
                ArmorModels::parse(&read(src, &p("f"), "rArmorModelNpc")?)?,
            ],
        })
    }

    /// The model ids of an equipment piece or hair id (category << 24 |
    /// number): `m_parts` table for sexes 0/1, `h_parts` for 2 (0x00bc0fd0,
    /// 0x00bc1060).
    fn piece_models(&self, sex: u8, id: i32) -> &[u32] {
        let t = if sex == 2 { &self.table_h } else { &self.table };
        t.models(id as u32).unwrap_or(&[])
    }

    /// Resolve a piece to its model paths (0x008407f0: kind 0 / 1 lists).
    fn resolve(&self, sex_models: u8, sex_table: u8, id: i32, hair: bool, out: &mut Vec<LookPart>) {
        if id == -1 {
            return;
        }
        for &m in self.piece_models(sex_table, id) {
            if let Some((kind, p)) = self.models[sex_models as usize].get(m) {
                // One shipped record names an empty path: no model.
                if !p.is_empty() {
                    out.push(LookPart { kind, path: p.to_string(), hair });
                }
            }
        }
    }
}

/// What a dressed (non-NPC-set) character wears: actor fields +0x1358
/// (one piece), +0x135c (two), +0x1364 (five), hair +0x1378, beard
/// +0x137c, each an armour-table id (category << 24 | number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outfit {
    pub pieces: [i32; 8],
    pub hair: i32,
    pub beard: i32,
}

/// A dressed character (`uActorModelPl` non-NPC branch, also the player
/// and pawns): base body, equipment, hair and beard, and the part mask.
// Translated from 0x008407f0 (piece, hair and beard models) and
// 0x008449b0 with 0x00bc1060 (mask) (DDDA.exe 2017-12-13 Steam).
pub fn dressed_look(t: &Tables, unit: Unit, sex: u8, outfit: &Outfit, hair_color: Option<[f32; 3]>) -> Look {
    let mut parts = vec![];
    for &id in &outfit.pieces {
        t.resolve(sex, sex, id, false, &mut parts);
    }
    t.resolve(2, 2, outfit.hair, true, &mut parts);
    t.resolve(2, 2, outfit.beard, true, &mut parts);
    // 0x008449b0: parts 0-199 on; the player hides part 17, a pawn part 16
    // (NPC ids pick by 0x00ba7150, not translated).
    let mut mask = PartMask::default();
    for p in 0..200 {
        mask.set(p, true);
    }
    match unit {
        Unit::Player => mask.set(17, false),
        Unit::Pawn => mask.set(16, false),
        Unit::Npc => {}
    }
    // 0x00bc1060 for every piece: the parts-off records of its models
    // (id % 1000); a non-negative condition needs that model worn too.
    let worn: Vec<u32> = outfit.pieces.iter().filter(|&&p| p != -1).flat_map(|&p| t.piece_models(sex, p).iter().copied()).collect();
    for &id in outfit.pieces.iter().filter(|&&p| p != -1) {
        for &m in t.piece_models(sex, id) {
            for &(cond, part) in t.parts_off.get(m % 1000).unwrap_or(&[]) {
                if cond < 0 || worn.iter().any(|&w| (w % 1000) as i16 == cond) {
                    mask.set(part, false);
                }
            }
        }
    }
    Look { sex, base: base_body(sex), parts, mask, hair_color, morphs: None }
}

/// The NPC as an event actor: `uActorModelPl::vf05` 0x0083e010 branch 2.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcLook {
    pub npc: u32,
    pub edit_path: String,
    pub edit: EditPawn,
    pub armor_set: i32,
    pub look: Look,
}

/// The look of NPC `npc` as event actor `unit_no` in event row `row`.
pub fn npc_look<S: Source + ?Sized>(src: &S, npc: u32, unit_no: u32, row: u32) -> Result<NpcLook, Error> {
    let t = Tables::load(src)?;
    npc_look_with(src, &t, npc, unit_no, row)
}

pub fn npc_look_with<S: Source + ?Sized>(src: &S, t: &Tables, npc: u32, unit_no: u32, row: u32) -> Result<NpcLook, Error> {
    let ledger_bytes = read(src, r"etc\item\npclist", "rNpcLedgerList")?;
    let ledger = ddnpc::Ledger::parse(&ledger_bytes).map_err(|e| Error::Data(format!("npclist: {e:?}")))?;
    let entry = ledger
        .entries
        .iter()
        .find(|n| n.id as i32 == npc as i32)
        .ok_or_else(|| Error::Data(format!("NPC {npc} not in the ledger")))?;
    let s = entry.stage;
    let edit_path = format!("edit\\st{s:03}\\n{npc:03}\\st{s:03}_n{npc:03}");
    let edit = EditPawn::parse(&read(src, &edit_path, "rEditPawn")?)?;
    if edit.weapons.iter().any(|&x| x != -1) {
        return Err(Error::Unimplemented(format!("{edit_path}: weapons (actor +0x1330.., 0x00840180)")));
    }
    let mut set = edit.armor_set as i32;
    let mut hair = edit.hair;
    apply_variant(npc, set_variant(unit_no, npc, row), &mut set, &mut hair);
    let sex = (edit.sex == 1) as u8;
    let f = &edit.floats;
    let hair_color = Some([f[8] * 0.5 + 0.2, f[9] * 0.5 + 0.2, f[10] * 0.5 + 0.2]);
    let morphs = Some([edit.human_edit.clone(), edit.body_edit.clone(), edit.face_edit.clone()]);
    let outfit = Outfit {
        pieces: [edit.parts[0], edit.parts[1], edit.parts[2], edit.parts[3], edit.parts[4], edit.parts[5], edit.parts[6], edit.parts[7]],
        hair,
        beard: edit.beard,
    };
    let mut look = if set == -1 {
        dressed_look(t, Unit::Npc, sex, &outfit, hair_color)
    } else {
        // Translated from 0x00841960 (set models) and 0x00841e20 (mask)
        // (DDDA.exe 2017-12-13 Steam). Pieces are still resolved by
        // 0x008407f0.
        let mut look = dressed_look(t, Unit::Npc, sex, &outfit, hair_color);
        let rec = t.npc_sets[sex as usize].get(set as u32).ok_or_else(|| Error::Data(format!("armour set {set:#x} missing")))?;
        let mut parts = vec![];
        for &id in rec.models.iter().take(2) {
            if id as i32 >= 0 {
                if let Some((kind, p)) = t.npc_models[sex as usize].get(id) {
                    parts.push(LookPart { kind, path: p.to_string(), hair: false });
                }
            }
        }
        parts.append(&mut look.parts);
        look.parts = parts;
        let mut mask = PartMask::default();
        for p in 200..512 {
            mask.set(p, false);
        }
        for &p in &rec.on {
            mask.set(p, true);
        }
        for &p in &rec.off {
            mask.set(p, false);
        }
        look.mask = mask;
        look
    };
    look.morphs = morphs;
    Ok(NpcLook { npc, edit_path, edit, armor_set: set, look })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants() {
        assert_eq!(set_variant(1, 1, 22), 0);
        assert_eq!(set_variant(0x3e3, 2, 7), 1);
        let (mut s, mut h) = (0x89, 5);
        apply_variant(2, 0, &mut s, &mut h);
        assert_eq!(s, 0x89);
        apply_variant(2, 1, &mut s, &mut h);
        assert_eq!(s, 0x6d);
    }

    #[test]
    fn mask_bits() {
        let mut m = PartMask::default();
        assert!(m.visible(300));
        m.set(300, false);
        assert!(!m.visible(300) && m.visible(301));
        assert!(!m.visible(600));
    }
}
