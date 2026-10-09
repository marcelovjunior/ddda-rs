//! Shared story-pawn records, recruited independently of the Rift menus.
use crate::{Host, HostError, StoryWorld};
use ddstory::data::Data;

impl<D: Data, W: StoryWorld> Host<D, W> {
    /// Recruit a story-stock record into a shared hired slot (1 or 2).
    /// Co-op reserves the second hired slot for the additional Arisen.
    /// Repeated delivery returns false without resetting hire bookkeeping.
    ///
    /// Record selection/copy: 0x00440190 -> 0x0043e5b0 / 0x00447390
    /// (DDDA.exe 2017-12-13 Steam). This does not yet instantiate a body
    /// or apply the equipment-derived active-stat conversion.
    pub fn recruit_story_pawn(&mut self, stock: u32, slot: u32) -> Result<bool, HostError> {
        let index = match slot {
            1 | 2 => slot as usize - 1,
            _ => {
                return Err(HostError::Rift(format!(
                    "story pawn hired slot {slot} outside 1..=2"
                )))
            }
        };
        if stock >= 20 {
            return Err(HostError::Rift(format!("story pawn stock {stock} outside 0..20")));
        }
        let rom_id = 0x2000_0000 | stock;
        if self.rift.party.hired[index]
            .as_ref()
            .is_some_and(|p| p.pawn.rom_id == rom_id)
        {
            return Ok(false);
        }
        if self.rift.party.reserved[index] || self.rift.party.hired[index].is_some() {
            return Err(HostError::Rift(format!(
                "story pawn slot {slot} is occupied or reserved"
            )));
        }
        let owner = self
            .roster
            .ids()
            .next()
            .ok_or(HostError::NoData("story pawn owner seat"))?;
        let story = self.story.as_ref().ok_or(HostError::NoData("story"))?;
        // Rook's name is overridden by ledger 504 regardless of language.
        // Other stock uses the session language (English without RiftData).
        let language = self.data.rift.as_ref().map_or(1, |d| d.language);
        let pawn = ddstory::prologue::story_stock(&story.data, stock as usize, language)?;
        let mut bag = ddeconomy::inventory::Bag::default();
        // Translated from 0x00440190 (0x0044020b..0x004402a2): the
        // Rook event clears pawn bag 2, adds/equips the five fixed items
        // through 0x0045ece0, then adds one item 64 through 0x0045e680.
        // The stock loader already equips exactly those source-backed
        // items; reuse that record instead of another copied item list.
        if stock == 14 && slot == 1 {
            for equip in pawn.param.equip.iter().filter(|e| e.num > 0 && e.item_no >= 0) {
                bag.slots.push(ddeconomy::inventory::Slot {
                    item: equip.item_no,
                    num: equip.num,
                    flags: equip.flag,
                });
            }
            bag.slots.push(ddeconomy::inventory::Slot {
                item: 64,
                num: 1,
                flags: ddeconomy::inventory::FLAG_VALID,
            });
        }
        let hire_time = self.play_time();
        self.rift.party.hired[index] = Some(ddrift::PartyPawn {
            online: ddrift::PawnRef::rom(rom_id),
            pawn,
            hire_time,
            quests_cleared: 0,
        });
        self.hired_by[index] = Some(owner);
        if stock == 14 && slot == 1 {
            *self
                .economy
                .inventory
                .bag_mut(ddeconomy::inventory::Owner::Pawn(slot as u8)) = bag;
        }
        self.refresh_party();
        Ok(true)
    }
}
