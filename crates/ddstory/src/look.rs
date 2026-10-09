//! What an event NPC actor wears: the models `uActorModelPl` builds for
//! unit type 2 (an NPC), read from the player's install. Only the model
//! choice is covered; body and face morphs (rBodyEdit, rFaceEdit), colours
//! and the part on/off masks are returned as data or not read.
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

use crate::data::Data;
use crate::Error;

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

/// The models of one NPC actor.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcLook {
    pub npc: u32,
    pub edit_path: String,
    pub edit: EditPawn,
    pub armor_set: i32,
    /// The base body: `model\pl\f\f_base\f000\f000` for sex 1, else the
    /// male one (0x0083fa60).
    pub base: String,
    /// (kind, path) of the armour set's models, then hair, then beard.
    pub models: Vec<(u32, String)>,
    pub parts_on: Vec<u16>,
    pub parts_off: Vec<u16>,
}

fn read<D: Data>(data: &D, path: &str, class: &str) -> Result<Vec<u8>, Error> {
    data.resource(path, class)?.ok_or_else(|| Error::Data(format!("{path} ({class}) missing")))
}

/// The look of NPC `npc` as event actor `unit_no` in event row `row`.
/// Per-sex tables: index 0 is `m_parts`, 1 `f_parts` (*assumed* from the
/// file names; the loader filling sGameSys +0xbe244 / +0xbe24c was not
/// traced).
pub fn npc_look<D: Data>(data: &D, npc: u32, unit_no: u32, row: u32) -> Result<NpcLook, Error> {
    let ledger_bytes = read(data, r"etc\item\npclist", "rNpcLedgerList")?;
    let ledger = ddnpc::Ledger::parse(&ledger_bytes).map_err(|e| Error::Data(format!("npclist: {e:?}")))?;
    let entry = ledger
        .entries
        .iter()
        .find(|n| n.id as i32 == npc as i32)
        .ok_or_else(|| Error::Data(format!("NPC {npc} not in the ledger")))?;
    let s = entry.stage;
    let edit_path = format!("edit\\st{s:03}\\n{npc:03}\\st{s:03}_n{npc:03}");
    let edit = EditPawn::parse(&read(data, &edit_path, "rEditPawn")?)?;
    if edit.weapons.iter().chain(&edit.parts).any(|&x| x != -1) {
        return Err(Error::Unimplemented(format!(
            "{edit_path}: weapons or equipment pieces (actor +0x1330.., +0x1358..; 0x00840180, 0x008407f0)"
        )));
    }
    let mut set = edit.armor_set as i32;
    let mut hair = edit.hair;
    apply_variant(npc, set_variant(unit_no, npc, row), &mut set, &mut hair);
    let female = edit.sex == 1;
    let tag = if female { "f" } else { "m" };
    let base = if female { r"model\pl\f\f_base\f000\f000" } else { r"model\pl\m\m_base\m000\m000" }.to_string();
    let mut models = vec![];
    let (mut parts_on, mut parts_off) = (vec![], vec![]);
    if set != -1 {
        let table = ArmorTableNpc::parse(&read(data, &format!("model\\pl\\parts\\{tag}_parts"), "rArmorTableNpc")?)?;
        let npc_models = ArmorModels::parse(&read(data, &format!("model\\pl\\parts\\{tag}_parts"), "rArmorModelNpc")?)?;
        let rec = table.get(set as u32).ok_or_else(|| Error::Data(format!("armour set {set:#x} missing")))?;
        // 0x00841960: the first two model ids that resolve.
        for &id in rec.models.iter().take(2) {
            if id as i32 >= 0 {
                if let Some((kind, p)) = npc_models.get(id) {
                    models.push((kind, p.to_string()));
                }
            }
        }
        parts_on = rec.on.clone();
        parts_off = rec.off.clone();
    }
    if hair != -1 || edit.beard != -1 {
        let table = ArmorTable::parse(&read(data, r"model\pl\parts\h_parts", "rArmorTable")?)?;
        let hm = ArmorModels::parse(&read(data, r"model\pl\parts\h_parts", "rArmorModel")?)?;
        for id in [hair, edit.beard] {
            if id == -1 {
                continue;
            }
            for &m in table.models(id as u32).unwrap_or(&[]) {
                if let Some((kind, p)) = hm.get(m) {
                    models.push((kind, p.to_string()));
                }
            }
        }
    }
    Ok(NpcLook { npc, edit_path, edit, armor_set: set, base, models, parts_on, parts_off })
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
}
