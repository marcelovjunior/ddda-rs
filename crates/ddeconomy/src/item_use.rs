//! Consumable recovery from installed ITL records, with explicit bag ownership.
//!
//! Translated from 0x00B77130, 0x00B77780 and 0x00856050 (DDDA.exe
//! 2017-12-13 Steam). This slice rejects harmful/buff/special items instead
//! of silently applying only their recovery portion. Animation and throwing
//! are separate from the instant inventory recovery path.

use crate::inventory::{Inventory, Owner, Slot};
use dditem::itl::{ItemList, ItemRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseError { NoSuchSlot, Storage, Unsupported }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recovery {
    pub hp: f32,
    pub stamina: f32,
    /// cStatus kind bits; possession is applied only to pawns.
    pub cures: u64,
    /// Item +0x38 bits 12..14 == 4 (0x00B77130).
    pub whole_party: bool,
}

impl Recovery {
    /// 0x00B77780 uses the recipient's augment 0x4a multiplier for both
    /// amounts. The caller reads mFVal0/1 from plAbilityParam, according
    /// to whether that recipient equips the augment (0x00783340).
    /// Medicant sharing multiplies by 0.25 and does not pass status effects.
    pub fn for_recipient(self, augment_factor: f32, shared: bool) -> Self {
        let share = if shared { 0.25 } else { 1.0 };
        Self { hp: (self.hp * augment_factor) * share,
            stamina: (self.stamina * augment_factor) * share,
            cures: if shared { 0 } else { self.cures }, ..self }
    }

    pub fn from_record(item: &ItemRecord, slot: Slot) -> Result<Self, UseError> {
        let raw = &item.raw;
        let hp = i16::from_le_bytes([raw[0x70], raw[0x71]]) as f32;
        let stamina = i16::from_le_bytes([raw[0x72], raw[0x73]]) as f32;
        // Only unenhanced consumables. Equipment has level-scaled status
        // values (0x0045B620); special-use items need their own dispatch.
        if item.kind() >= 7 || slot.level() != 0 || hp < 0.0 || stamina < 0.0
            || matches!(slot.item, 0x62 | 0x74) || raw[0x68..0x6f].iter().any(|&v| v != 0)
        { return Err(UseError::Unsupported); }
        let mut cures = 0;
        // Calls/arguments in 0x00856050; getters 0x0045A8D0..0x0045B370.
        // Oil and drenched are +2a/+29 (the old survey reversed them).
        for (offset, kind) in [(0x1d,10), (0x1e,11), (0x25,0), (0x26,1),
            (0x27,2), (0x28,4), (0x2a,5), (0x29,6), (0x2b,7), (0x2c,3),
            (0x2d,8), (0x2e,9), (0x2f,12), (0x30,14), (0x32,15), (0x31,16), (0x33,17)] {
            let value = raw[offset] as i8;
            if value >= 2 { return Err(UseError::Unsupported); }
            if value == 1 { cures |= 1u64 << kind; }
        }
        if hp == 0.0 && stamina == 0.0 && cures == 0 { return Err(UseError::Unsupported); }
        let packed = u32::from_le_bytes(raw[0x38..0x3c].try_into().unwrap());
        Ok(Self { hp, stamina, cures, whole_party: (packed >> 12) & 7 == 4 })
    }
}

impl Inventory {
    pub fn recovery(&self, items: &ItemList, owner: Owner, index: usize) -> Result<Recovery, UseError> {
        if matches!(owner, Owner::Storage(_)) { return Err(UseError::Storage); }
        let slot = *self.bag(owner).and_then(|b| b.slots.get(index)).filter(|s| s.num > 0)
            .ok_or(UseError::NoSuchSlot)?;
        let item = items.get(slot.item as u32).ok_or(UseError::Unsupported)?;
        Recovery::from_record(item, slot)
    }

    /// Apply to the host's selected recipients, then consume exactly one
    /// copy (0x00B77130). A failed application leaves the bag untouched.
    /// Authorization of a player's request to use a pawn bag is host policy.
    pub fn use_recovery<R>(&mut self, items: &ItemList, owner: Owner, index: usize,
        apply: impl FnOnce(Recovery) -> Result<R, UseError>) -> Result<R, UseError> {
        let effect = self.recovery(items, owner, index)?;
        let result = apply(effect)?;
        let slot = self.bag(owner).unwrap().slots[index];
        let item = items.get(slot.item as u32).unwrap();
        // Item flag 13 and Eternal Ferrystone are not consumed by B77130.
        if item.flags() & (1 << 13) == 0 && slot.item != 0x6bc { self.remove(owner, index, 1); }
        Ok(result)
    }
}
