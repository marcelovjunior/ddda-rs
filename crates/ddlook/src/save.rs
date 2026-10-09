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

#[allow(clippy::too_many_arguments)]
fn character<S: Source + ?Sized>(
    t: &Tables,
    pat: &EditPattern,
    items: &[[u8; 0x80]],
    who: String,
    unit: Unit,
    edit: Node<'_, '_>,
    param: Node<'_, '_>,
    _src: &S,
) -> Option<SavedLook> {
    let f = edit_fields(edit);
    let sex = field(&f, "mGender")? as u8;
    let eq: Vec<i16> = param
        .child("mEquipItem")?
        .children()
        .map(|c| c.get::<i16>("data.mItemNo").unwrap_or(-1))
        .collect();
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
    let hair = pat.hair(field(&f, "mHairNo").unwrap_or(0xff));
    let beard = pat.beard(field(&f, "mBeardNo").unwrap_or(0xff), sex);
    let col = |pallet: usize, name: &str| pat.color(pallet, field(&f, name).unwrap_or(0xff));
    let h = col(0, "mColorHair");
    let b = col(0, "mColorBeard");
    let skin = col(1, "mColorBody");
    let mut look = dressed_look(
        t,
        unit,
        sex,
        &Outfit { pieces, hair, beard },
        Some([h[0] * 0.5 + 0.2, h[1] * 0.5 + 0.2, h[2] * 0.5 + 0.2]),
    );
    look.beard_color = Some([b[0] * 0.5 + 0.2, b[1] * 0.5 + 0.2, b[2] * 0.5 + 0.2]);
    look.skin_color = Some([skin[0], skin[1], skin[2]]);
    look.weapons = crate::weapon::weapons(raw(0), raw(1));
    Some(SavedLook { who, look, items: eq, edit: f })
}

/// The Arisen and the party pawns of a save slot (`mPlayerDataManual` or
/// `mPlayerDataBase`).
pub fn save_looks<S: Source + ?Sized>(src: &S, save_xml: &str, slot: &str) -> Result<Vec<SavedLook>, Error> {
    let t = Tables::load(src)?;
    let pat = EditPattern::load(src)?;
    let itl = src
        .resource(r"etc\item\itemList", "rItemList")?
        .ok_or_else(|| Error::Data("itemList missing".into()))?;
    let list = dditem::itl::ItemList::parse(&itl).map_err(|e| Error::Data(format!("itemList: {e:?}")))?;
    let items: Vec<[u8; 0x80]> = list.records.iter().map(|r| r.raw).collect();
    let doc = ddsave::xml::Document::parse(save_xml).map_err(|e| Error::Data(format!("save xml: {e:?}")))?;
    let root = doc.root();
    let ep = root
        .child(slot)
        .and_then(|p| p.child("mPlCmcEditAndParam"))
        .ok_or_else(|| Error::Data(format!("{slot}: no mPlCmcEditAndParam")))?;
    let mut out = vec![];
    let pl = ep.child("mPl").ok_or_else(|| Error::Data("no mPl".into()))?;
    out.extend(character(
        &t,
        &pat,
        &items,
        "arisen".into(),
        Unit::Player,
        pl.child("mEdit").ok_or_else(|| Error::Data("no mPl/mEdit".into()))?,
        pl.child("mParam").ok_or_else(|| Error::Data("no mPl/mParam".into()))?,
        src,
    ));
    if let Some(cmc) = ep.child("mCmc") {
        for (i, c) in cmc.children().enumerate() {
            if let (Some(e), Some(p)) = (c.child("mEdit"), c.child("mParam")) {
                out.extend(character(&t, &pat, &items, format!("pawn{i}"), Unit::Pawn, e, p, src));
            }
        }
    }
    Ok(out)
}
