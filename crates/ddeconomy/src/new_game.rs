//! The opening's Arisen bags, independent of presentation and save files.
//! Co-op policy gives each Arisen the original's bag-0 grants separately.

use crate::inventory::{Bag, BuyBack, Inventory, Owner, PlayerId, Slot, FLAG_IN_USE, FLAG_VALID};
use dditem::itl::ItemList;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    DragonChallenge,
    NewlyArisen,
}

/// Small item override lists embedded in code, not copied resource data.
/// Translated from 0x0077fd80 (editor success, 0x0052b2d0) and
/// 0x0050c290 event row 21 (DDDA.exe 2017-12-13 Steam). The prologue
/// bag is erased at 0x00444b02 -> sItemManager::vf10 0x0045da90.
pub fn bag(phase: Phase) -> Bag {
    let items: &[i16] = match phase {
        Phase::DragonChallenge => &[0x12d, 0x4a0],
        Phase::NewlyArisen => &[0x4a1],
    };
    Bag {
        slots: items.iter().map(|&item| Slot { item, num: 1, flags: FLAG_VALID | FLAG_IN_USE }).collect(),
    }
}

/// Event row 21's inventory mutation before the heart-taking movie.
/// Translated from 0x0050c290 case 0x15 (DDDA.exe 2017-12-13 Steam):
/// unequip, remove one of each temporary item, add/equip 0x4a1.
pub fn heart_taken(inventory: &mut Inventory, items: &ItemList, seat: PlayerId) {
    let owner = Owner::Arisen(seat);
    for slot in &mut inventory.bag_mut(owner).slots {
        slot.flags &= if slot.flags & 4 == 0 { 0xffff_f7fd } else { 0xffff_fffb };
    }
    inventory.remove_item(owner, 0x12d, 1);
    inventory.remove_item(owner, 0x4a0, 1);
    inventory.add(items, owner, seat, 0x4a1, 1, 0, &mut BuyBack::default());
    if let Some(slot) = inventory.bag_mut(owner).slots.iter_mut().find(|s| s.item == 0x4a1) {
        slot.flags |= FLAG_IN_USE;
    }
}

/// Delivery bookkeeping for one opening session. The story disables fired
/// QCT rows; keeping delivery ids also protects against a repeated adapter
/// notification. This is host state, shared by all local Arisen seats.
#[derive(Debug, Default)]
pub struct Grants {
    first_choices: BTreeSet<PlayerId>,
    rewards: BTreeSet<(PlayerId, u32)>,
}

impl Grants {
    /// The first vocation choice clears equipment and gives/equips the
    /// supplied ids from ddprogress::change::starting_items.
    /// Translated from 0x00673740 -> 0x00459650 / 0x0045e680 /
    /// 0x00459100 (DDDA.exe 2017-12-13 Steam).
    pub fn first_choice(&mut self, inventory: &mut Inventory, items: &ItemList, seat: PlayerId, equipment: &[u16]) {
        if !self.first_choices.insert(seat) { return; }
        let owner = Owner::Arisen(seat);
        for slot in &mut inventory.bag_mut(owner).slots {
            // 0x00459650 handles the two equipped copies separately.
            slot.flags &= if slot.flags & 4 == 0 { 0xffff_f7fd } else { 0xffff_fffb };
        }
        let mut buyback = BuyBack::default();
        for &id in equipment {
            inventory.add(items, owner, seat, id as i16, 1, 0, &mut buyback);
            if let Some(slot) = inventory.bag_mut(owner).slots.iter_mut().find(|s| s.item == id as i16) {
                slot.flags |= FLAG_IN_USE;
            }
        }
    }

    /// Give runtime-decoded reward pairs only when the story actually
    /// emits Request::Reward. Translated from 0x00790539 ->
    /// 0x0045e680 (DDDA.exe 2017-12-13 Steam). No resource item ids live here.
    pub fn reward(&mut self, inventory: &mut Inventory, items: &ItemList, seat: PlayerId, quest: u32, grants: &[(i32, i32)]) {
        if !self.rewards.insert((seat, quest)) { return; }
        let mut buyback = BuyBack::default();
        for &(item, count) in grants {
            if let (Ok(item), Ok(count)) = (i16::try_from(item), u32::try_from(count)) {
                inventory.add(items, Owner::Arisen(seat), seat, item, count, 0, &mut buyback);
            }
        }
    }
}
