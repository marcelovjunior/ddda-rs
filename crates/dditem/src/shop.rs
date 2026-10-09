//! `rShopList` (`etc\shop\n%03dShopList`, XFS): one NPC's shop.
//!
//! `uNpc::vf32` (`0x00bace50`) loads the list for its NPC number; NPCs 784
//! and 156 load the Dark Arisen copies `dl1\etc\shop\n784ShopList` and
//! `dl1\etc\shop\n156ShopList` instead. Property names and offsets from
//! `rShopList::vf03` (`0x00cdd610`), `cLineupData::vf03` (`0x00cdcf20`)
//! and `cShopListJudgment::vf03` (`0x00cc8280`). The exe groups them as
//! "ショップオープン条件" (shop open conditions) and "アイテムオープン条件"
//! (item open conditions), each with "AND条件" and "OR条件" lists.

use crate::Error;
use mtxfs::{Object, Value, Xfs};

pub const JUDGMENTS: usize = 10;
pub const DISCOUNT_SLOTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopList {
    pub shop_type: u32,
    /// `mDiscountOkItem0..7`: owning one of these items selects the
    /// matching `discount_rate` (`0x00cde010`). -1 = unused.
    pub discount_ok_item: [i32; DISCOUNT_SLOTS],
    /// `mDiscountRate0..7`, percent.
    pub discount_rate: [i32; DISCOUNT_SLOTS],
    /// `mDefaultRate`, percent.
    pub default_rate: i32,
    /// `mDiscontGSF` (sic): a game-state flag number, `0xffffffff` = none.
    pub discount_gsf: u32,
    /// `mDiscountGSFRate`, percent, used while `discount_gsf` is set.
    pub discount_gsf_rate: i32,
    pub open_and: [Judgment; JUDGMENTS],
    pub open_or: [Judgment; JUDGMENTS],
    pub lineup: Vec<Lineup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Judgment {
    pub command: i32,
    pub params: [i32; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lineup {
    pub item: u32,
    /// `mItemNum` (s8 property): stock count, -1 = unlimited.
    pub num: i32,
    /// `mItemRearrival` (u8 property).
    pub rearrival: u32,
    pub open_and: [Judgment; JUDGMENTS],
    pub open_or: [Judgment; JUDGMENTS],
}

fn int(v: &Value) -> Option<i64> {
    match v {
        Value::U(u) => Some(*u as i64),
        Value::S(s) => Some(*s),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

fn field_int(x: &Xfs, o: &Object, name: &str) -> Result<i64, Error> {
    x.get(o, name)
        .and_then(|v| v.first())
        .and_then(int)
        .ok_or_else(|| Error::new(format!("missing integer {name}")))
}

fn children<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<Vec<&'a Object>, Error> {
    let vals = x
        .get(o, name)
        .ok_or_else(|| Error::new(format!("missing {name}")))?;
    let mut out = Vec::new();
    for v in vals {
        match v {
            Value::Object(Some(c)) => out.push(c.as_ref()),
            Value::Object(None) => {}
            _ => return Err(Error::new(format!("{name} is not an object list"))),
        }
    }
    Ok(out)
}

fn judgments(x: &Xfs, o: &Object, name: &str) -> Result<[Judgment; JUDGMENTS], Error> {
    let objs = children(x, o, name)?;
    if objs.len() != JUDGMENTS {
        return Err(Error::new(format!("{name}: {} judgments", objs.len())));
    }
    let mut out = [Judgment::default(); JUDGMENTS];
    for (j, ob) in out.iter_mut().zip(objs) {
        j.command = field_int(x, ob, "mCommand")? as i32;
        for (k, p) in j.params.iter_mut().enumerate() {
            *p = field_int(x, ob, &format!("mParam0{k}"))? as i32;
        }
    }
    Ok(out)
}

impl ShopList {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let x = mtxfs::parse(data).map_err(|e| Error::new(format!("xfs: {e:?}")))?;
        Self::from_xfs(&x)
    }

    pub fn from_xfs(x: &Xfs) -> Result<Self, Error> {
        let r = &x.root;
        if x.class_of(r).name() != Some("rShopList") {
            return Err(Error::new("root is not rShopList"));
        }
        let mut discount_ok_item = [0; DISCOUNT_SLOTS];
        let mut discount_rate = [0; DISCOUNT_SLOTS];
        for i in 0..DISCOUNT_SLOTS {
            discount_ok_item[i] = field_int(x, r, &format!("mDiscountOkItem{i}"))? as i32;
            discount_rate[i] = field_int(x, r, &format!("mDiscountRate{i}"))? as i32;
        }
        let mut lineup = Vec::new();
        for arr in children(x, r, "mShopLineup")? {
            for l in children(x, arr, "mpArray")? {
                lineup.push(Lineup {
                    item: field_int(x, l, "mItemNo")? as u32,
                    num: field_int(x, l, "mItemNum")? as i32,
                    rearrival: field_int(x, l, "mItemRearrival")? as u32,
                    open_and: judgments(x, l, "mLineeupJAnd")?,
                    open_or: judgments(x, l, "mLineeupJOr")?,
                });
            }
        }
        Ok(Self {
            shop_type: field_int(x, r, "mShopType")? as u32,
            discount_ok_item,
            discount_rate,
            default_rate: field_int(x, r, "mDefaultRate")? as i32,
            discount_gsf: field_int(x, r, "mDiscontGSF")? as u32,
            discount_gsf_rate: field_int(x, r, "mDiscountGSFRate")? as i32,
            open_and: judgments(x, r, "mShopJAnd")?,
            open_or: judgments(x, r, "mShopJOr")?,
            lineup,
        })
    }
}
