//! Looks of the save's Arisen and pawns (`cSAVE_DATA_EDIT` and the
//! equipped items of `cSAVE_DATA_PARAM`).
//!
//! The runtime edit of a character (the struct `uActorModelPl` copies with
//! 0x00843ed0: sex [3], pieces [9..0x10], hair [0x11], beard [0x12],
//! colours [0x14..]) is filled from these records by code this crate has
//! not traced. The links used here are *inferred* and marked:
//! - equipment piece of an item: item record +0x40 (`rItemList`); it holds
//!   an `m_parts` armour-table id (category 1-7 << 24 | number) for 640
//!   items of the shipped list;
//! - equipment slots 0 and 1 are the weapons, 2-11 the armour (the
//!   save's slots hold weapon ids in 0/1 and armour ids after).
//!
//! Hair, beard and colours are traced (0x00780fa0, the save -> runtime
//! edit conversion): hair `0x0f000000 | mHairId[mHairNo]`, beard
//! `0x10000000 | mBeardId[mBeardNo]` for male characters only, colours
//! from the editPattern pallets (`crate::pattern`).

use crate::pattern::EditPattern;
use crate::{dressed_look, Error, Look, Outfit, Source, Tables, Unit};
use ddsave::xml::Node;

/// One character of the save.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedLook {
    /// "arisen" or "pawnN" (party order of `mCmc`).
    pub who: String,
    pub look: Look,
    /// Equipped item numbers (`mEquipItem`, -1 empty).
    pub items: Vec<i16>,
    /// The raw edit record (`cSAVE_DATA_EDIT`) fields, by name, for morphs
    /// and colours (not applied: the edit -> runtime conversion is not
    /// traced).
    pub edit: Vec<(String, u32)>,
}

fn edit_fields(n: Node<'_, '_>) -> Vec<(String, u32)> {
    n.children()
        .filter(|c| c.tag() != "array")
        .filter_map(|c| Some((c.name()?.into_owned(), c.parse::<u32>()?)))
        .collect()
}

fn field(f: &[(String, u32)], name: &str) -> Option<u32> {
    f.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
}

/// The item record's armour piece (+0x40), or -1 (*inferred*, see the
/// module note). Pieces outside the armour categories 1-7 are dropped.
pub fn item_piece(items: &[[u8; 0x80]], item: i16) -> i32 {
    if item < 0 {
        return -1;
    }
    let Some(r) = items.get(item as usize) else { return -1 };
    let v = u32::from_le_bytes(r[0x40..0x44].try_into().unwrap());
    if (1..=7).contains(&(v >> 24)) {
        v as i32
    } else {
        -1
    }
}

/// A look from an edit record (field name -> value, the 41 u8 fields of
/// `cSAVE_DATA_EDIT`) and the 12 equipment slots (item numbers).
pub fn edit_look<S: Source + ?Sized>(
    src: &S,
    t: &Tables,
    pat: &EditPattern,
    items: &[[u8; 0x80]],
    unit: Unit,
    f: &[(String, u32)],
    eq: &[i16],
) -> Option<Look> {
    let sex = field(f, "mGender")? as u8;
    let mut pieces = [-1i32; 8];
    let mut k = 0;
    for &it in eq.iter().skip(2) {
        let p = item_piece(items, it);
        if p != -1 && k < 8 {
            pieces[k] = p;
            k += 1;
        }
    }
    let raw = |slot: usize| {
        eq.get(slot).filter(|&&i| i >= 0).and_then(|&i| items.get(i as usize)).map_or(-1, |r| {
            u32::from_le_bytes(r[0x40..0x44].try_into().unwrap()) as i32
        })
    };
    // Translated from 0x00780fa0 (DDDA.exe 2017-12-13 Steam).
    let hair = pat.hair(field(f, "mHairNo").unwrap_or(0xff));
    let beard = pat.beard(field(f, "mBeardNo").unwrap_or(0xff), sex);
    let col = |pallet: usize, name: &str| pat.color(pallet, field(f, name).unwrap_or(0xff));
    let h = col(0, "mColorHair");
    let skin = col(1, "mColorBody");
    let mut look = dressed_look(
        t,
        unit,
        sex,
        &Outfit { pieces, hair, beard },
        Some([h[0] * 0.5 + 0.2, h[1] * 0.5 + 0.2, h[2] * 0.5 + 0.2]),
    );
    look.skin_color = Some([skin[0], skin[1], skin[2]]);
    let brow = field(f, "mFaceEyebrow").unwrap_or(0);
    if let Some((scale, texture)) = pat.eyebrow(brow) {
        let offset = crate::eyebrow::save_offset(brow, field(f, "mOfsXFaceEyebrow").unwrap_or(2), field(f, "mOfsYFaceEyebrow").unwrap_or(2));
        look.eyebrow = Some(crate::eyebrow::Eyebrow::from_edit(col(0, "mColorEyebrow"), offset, scale, texture));
    }
    look.weapons = crate::weapon::weapons(raw(0), raw(1));
    // Preset numbers (0x00780fa0): the field, plus 100 for a female
    // unless mHumanBodyAll is 200 or more.
    let base = if sex == 1 && field(f, "mHumanBodyAll").unwrap_or(0) < 200 { 100 } else { 0 };
    // The body morph from the human presets (0x00780fa0 -> 0x00cbc750).
    // The nine floats come from a skeleton measurement (0x00cbbf00) that
    // is not translated: neutral (1.0) here.
    let mut human = crate::morph::HumanEdit::neutral();
    for (part, (name, field_name)) in crate::morph::PRESET_PARTS.iter().enumerate() {
        let n = field(f, field_name).unwrap_or(0) + base;
        let path = format!("edit\\parts\\{name}\\{name}_{n:03}");
        let preset = src
            .resource(&path, "rHumanPartsEdit")
            .ok()
            .flatten()
            .and_then(|b| crate::morph::HumanParts::parse(&b).ok());
        human.apply_preset(part, preset.as_ref());
    }
    look.body = Some(crate::morph::body_morph(&human, &|_| true));
    look.body_shape = Some(crate::body::Edit::from_save(&|name| field(f, name)).ok()?);
    // The face morph from the face presets (0x00780fa0 -> 0x00cb9280) and
    // the slider parameters.
    let mut face = crate::face::FaceEdit::neutral();
    for (part, (name, field_name)) in crate::face::FACE_PRESET_PARTS.iter().enumerate() {
        let n = field(f, field_name).unwrap_or(0) + base;
        let path = format!("edit\\parts\\{name}\\{name}_{n:03}");
        let preset = src
            .resource(&path, "rFacePartsEdit")
            .ok()
            .flatten()
            .and_then(|b| crate::face::FaceParts::parse(&b).ok());
        face.apply_preset(part, preset.as_ref());
    }
    let params = crate::face::FaceParams::from_save(&|n| field(f, n).map(|v| v as u8));
    look.face = crate::face::face_morph(&face, &params, &|_| true);
    Some(look)
}

#[allow(clippy::too_many_arguments)]
fn character<S: Source + ?Sized>(
    src: &S,
    t: &Tables,
    pat: &EditPattern,
    items: &[[u8; 0x80]],
    who: String,
    unit: Unit,
    edit: Node<'_, '_>,
    param: Node<'_, '_>,
) -> Option<SavedLook> {
    let f = edit_fields(edit);
    let eq: Vec<i16> = param
        .child("mEquipItem")?
        .children()
        .map(|c| c.get::<i16>("data.mItemNo").unwrap_or(-1))
        .collect();
    let look = edit_look(src, t, pat, items, unit, &f, &eq)?;
    Some(SavedLook { who, look, items: eq, edit: f })
}

fn item_records<S: Source + ?Sized>(src: &S) -> Result<Vec<[u8; 0x80]>, Error> {
    let itl = src
        .resource(r"etc\item\itemList", "rItemList")?
        .ok_or_else(|| Error::Data("itemList missing".into()))?;
    let list = dditem::itl::ItemList::parse(&itl).map_err(|e| Error::Data(format!("itemList: {e:?}")))?;
    Ok(list.records.iter().map(|r| r.raw).collect())
}

/// A stand-in Arisen when there is no save: the first built-in pawn of
/// `etc\editPattern\romPawnInfo` of that sex, with its edit record and
/// equipment. A viewer choice, not a game rule (the original's player
/// actor copies the real player, 0x00843ed0).
pub fn template_look<S: Source + ?Sized>(src: &S, sex: u8) -> Result<Look, Error> {
    let t = Tables::load(src)?;
    let pat = EditPattern::load(src)?;
    let items = item_records(src)?;
    let b = src
        .resource(r"etc\editPattern\romPawnInfo", "rRomPawnInfo")?
        .ok_or_else(|| Error::Data("romPawnInfo missing".into()))?;
    let info = ddpawn::RomPawnInfo::parse(&b).map_err(|e| Error::Data(format!("romPawnInfo: {e}")))?;
    for c in &info.records {
        let f: Vec<(String, u32)> = ddpawn::cmc::EDIT_U8_FIELDS
            .iter()
            .zip(c.edit.fields.iter())
            .map(|(n, &v)| (n.to_string(), v as u32))
            .collect();
        if field(&f, "mGender") != Some(sex as u32) {
            continue;
        }
        let eq: Vec<i16> = c.param.equip.iter().map(|e| e.item_no).collect();
        if let Some(l) = edit_look(src, &t, &pat, &items, Unit::Player, &f, &eq) {
            return Ok(l);
        }
    }
    Err(Error::Data(format!("no built-in pawn of sex {sex}")))
}

/// A look for edit fields (`(field, value)`, `EDIT_U8_FIELDS` names)
/// wearing the equipment of the first built-in pawn of the same sex
/// (`romPawnInfo`). For a freshly edited Arisen: the original's starting
/// equipment is not traced, so this is a *viewer choice*.
pub fn edit_look_dressed<S: Source + ?Sized>(src: &S, fields: &[(String, u32)]) -> Result<Look, Error> {
    let t = Tables::load(src)?;
    let pat = EditPattern::load(src)?;
    let items = item_records(src)?;
    let sex = field(fields, "mGender").unwrap_or(0);
    let b = src
        .resource(r"etc\editPattern\romPawnInfo", "rRomPawnInfo")?
        .ok_or_else(|| Error::Data("romPawnInfo missing".into()))?;
    let info = ddpawn::RomPawnInfo::parse(&b).map_err(|e| Error::Data(format!("romPawnInfo: {e}")))?;
    let gender = ddpawn::cmc::EDIT_U8_FIELDS.iter().position(|f| *f == "mGender").unwrap_or(0);
    let eq: Vec<i16> = info
        .records
        .iter()
        .find(|c| u32::from(c.edit.fields[gender]) == sex)
        .map(|c| c.param.equip.iter().map(|e| e.item_no).collect())
        .unwrap_or_default();
    edit_look(src, &t, &pat, &items, Unit::Player, fields, &eq).ok_or_else(|| Error::Data("no look for these edit values".into()))
}

/// The Arisen and the party pawns of a save slot (`mPlayerDataManual` or
/// `mPlayerDataBase`).
pub fn save_looks<S: Source + ?Sized>(src: &S, save_xml: &str, slot: &str) -> Result<Vec<SavedLook>, Error> {
    let t = Tables::load(src)?;
    let pat = EditPattern::load(src)?;
    let items = item_records(src)?;
    let doc = ddsave::xml::Document::parse(save_xml).map_err(|e| Error::Data(format!("save xml: {e:?}")))?;
    let root = doc.root();
    let ep = root
        .child(slot)
        .and_then(|p| p.child("mPlCmcEditAndParam"))
        .ok_or_else(|| Error::Data(format!("{slot}: no mPlCmcEditAndParam")))?;
    let mut out = vec![];
    let pl = ep.child("mPl").ok_or_else(|| Error::Data("no mPl".into()))?;
    out.extend(character(
        src,
        &t,
        &pat,
        &items,
        "arisen".into(),
        Unit::Player,
        pl.child("mEdit").ok_or_else(|| Error::Data("no mPl/mEdit".into()))?,
        pl.child("mParam").ok_or_else(|| Error::Data("no mPl/mParam".into()))?,
    ));
    if let Some(cmc) = ep.child("mCmc") {
        for (i, c) in cmc.children().enumerate() {
            if let (Some(e), Some(p)) = (c.child("mEdit"), c.child("mParam")) {
                out.extend(character(src, &t, &pat, &items, format!("pawn{i}"), Unit::Pawn, e, p));
            }
        }
    }
    Ok(out)
}
