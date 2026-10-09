//! Gold and Rift Crystals of one Arisen (`cPlayerInfo.mGold` +0x348 and
//! +0x34c of the player record in sGameSys).

use dditem::rules::{Currency, CURRENCY_MAX};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wallet {
    pub gold: i32,
    pub rift_crystals: i32,
}

impl Wallet {
    pub fn balance(&self, c: Currency) -> i32 {
        match c {
            Currency::Gold => self.gold,
            Currency::RiftCrystals => self.rift_crystals,
        }
    }

    /// Add, capped at 99,999,999 (gold `0x004431e0`, RC `0x00443310`).
    pub fn add(&mut self, c: Currency, amount: u32) {
        let v = match c {
            Currency::Gold => &mut self.gold,
            Currency::RiftCrystals => &mut self.rift_crystals,
        };
        *v = (*v as i64 + amount as i64).min(CURRENCY_MAX as i64) as i32;
    }

    /// Spend, floored at 0 (gold `0x00443280`, RC `0x004433f0`). The game
    /// checks affordability before; this does not.
    pub fn spend(&mut self, c: Currency, amount: u32) {
        let v = match c {
            Currency::Gold => &mut self.gold,
            Currency::RiftCrystals => &mut self.rift_crystals,
        };
        *v = (*v as i64 - amount as i64).max(0) as i32;
    }
}
