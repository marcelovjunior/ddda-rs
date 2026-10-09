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
//! - hair `0x0f000000 | mHairNo`, beard `0x10000000 | mBeardNo`: the
//!   `h_parts` armour-table ids are 0x0f0000NN (hair) and 0x100000NN
//!   (beard), and NPC edits use the same form (st200 n001: hair 104 ->
//!   0x0f000068);
//! - the beard only for male characters (*guessed*).

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

fn character<S: Source + ?Sized>(
    t: &Tables,
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
    for &it in &eq {
        let p = item_piece(items, it);
        if p != -1 && k < 8 {
            pieces[k] = p;
            k += 1;
        }
    }
    let hair = field(&f, "mHairNo").map_or(-1, |h| (0x0f00_0000 | h) as i32);
    // *Guessed*: only male characters wear the beard model. Female records
    // carry a beard number too (23 in the maintainer's save); the
    // record -> runtime conversion that would decide it is not traced.
    let beard = match field(&f, "mBeardNo") {
        Some(b) if sex == 0 => (0x1000_0000 | b) as i32,
        _ => -1,
    };
    let look = dressed_look(t, unit, sex, &Outfit { pieces, hair, beard }, None);
    Some(SavedLook { who, look, items: eq, edit: f })
}

/// The Arisen and the party pawns of a save slot (`mPlayerDataManual` or
/// `mPlayerDataBase`).
pub fn save_looks<S: Source + ?Sized>(src: &S, save_xml: &str, slot: &str) -> Result<Vec<SavedLook>, Error> {
    let t = Tables::load(src)?;
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
                out.extend(character(&t, &items, format!("pawn{i}"), Unit::Pawn, e, p, src));
            }
        }
    }
    Ok(out)
}
