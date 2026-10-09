//! `etc\editPattern\editPattern` (rEditPatternData, XFS; loader
//! 0x00cb56a0): the character editor's palettes and the hair / beard
//! number tables. sGameSys +0xbe254 holds it; the save -> runtime edit
//! conversion 0x00780fa0 reads it at these runtime offsets:
//! - +0x60 `mPallet` (3 x cEditPallet, 0x400 bytes, colours from +0x10):
//!   pallet 0 hair, beard and eyebrow colours, pallet 1 skin, pallet 2
//!   eyes and makeup (index < 63, else colour 0);
//! - +0x12a0 `mHairId` (s8 x 64), +0x12e0 `mBeardId` (s8 x 48).

use crate::{Error, Source};

#[derive(Debug, Clone, PartialEq)]
pub struct EditPattern {
    /// RGBA by pallet (0 hair, 1 skin, 2 eyes and makeup).
    pub pallets: Vec<Vec<[f32; 4]>>,
    pub hair_id: Vec<i8>,
    pub beard_id: Vec<i8>,
}

impl EditPattern {
    pub fn load<S: Source + ?Sized>(src: &S) -> Result<EditPattern, Error> {
        let b = src
            .resource(r"etc\editPattern\editPattern", "rEditPatternData")?
            .ok_or_else(|| Error::Data("editPattern missing".into()))?;
        let x = mtxfs::parse(&b).map_err(|e| Error::Data(format!("editPattern: {e}")))?;
        let root = &x.root;
        let mut pallets = vec![];
        for v in x.get(root, "mPallet").unwrap_or(&[]) {
            let mtxfs::Value::Object(Some(o)) = v else { continue };
            let colors = x
                .get(o, "mColor")
                .unwrap_or(&[])
                .iter()
                .filter_map(|c| match c {
                    mtxfs::Value::F(f) if f.len() >= 4 => Some([f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32]),
                    _ => None,
                })
                .collect();
            pallets.push(colors);
        }
        let s8s = |name: &str| -> Vec<i8> {
            x.get(root, name)
                .unwrap_or(&[])
                .iter()
                .filter_map(|v| match v {
                    mtxfs::Value::S(n) => Some(*n as i8),
                    mtxfs::Value::U(n) => Some(*n as i8),
                    _ => None,
                })
                .collect()
        };
        let p = EditPattern { pallets, hair_id: s8s("mHairId"), beard_id: s8s("mBeardId") };
        if p.pallets.len() != 3 || p.hair_id.len() != 64 || p.beard_id.len() != 48 {
            return Err(Error::Data(format!(
                "editPattern: {} pallets, {} hair ids, {} beard ids",
                p.pallets.len(),
                p.hair_id.len(),
                p.beard_id.len()
            )));
        }
        Ok(p)
    }

    /// A pallet colour: index < 63, else colour 0 (0x00780fa0).
    pub fn color(&self, pallet: usize, index: u32) -> [f32; 4] {
        let p = &self.pallets[pallet];
        let i = if index < 0x3f { index as usize } else { 0 };
        p.get(i).copied().unwrap_or([1.0; 4])
    }

    /// Hair id `0x0f000000 | mHairId[n]` for n < 64 and a non-negative
    /// entry, else none (-1).
    pub fn hair(&self, n: u32) -> i32 {
        match self.hair_id.get(n as usize) {
            Some(&h) if n < 0x40 && h >= 0 => 0x0f00_0000 | h as i32,
            _ => -1,
        }
    }

    /// Beard id `0x10000000 | mBeardId[n]`, male characters only (n < 48,
    /// non-negative entry), else none.
    pub fn beard(&self, n: u32, sex: u8) -> i32 {
        match self.beard_id.get(n as usize) {
            Some(&b) if sex == 0 && n < 0x30 && b >= 0 => 0x1000_0000 | b as i32,
            _ => -1,
        }
    }
}
