//! Host-owned inventory actions. Bags live only in Economy.inventory and
//! are saved in ECON; every gameplay request names its authorized Arisen.
use crate::{Host, PlayerId, StoryWorld};
use ddeconomy::inventory::{Bag, Inventory, Owner, Slot};
use ddeconomy::item_use::{Recovery, UseError};
use dditem::itl::ItemList;
use ddstory::data::Data;

// cStatus curse bit 0x200, tested by 0x00B81E30 before clamping stamina.
const CURSE_KIND: u32 = 9;

#[derive(Clone, Debug)]
pub struct ItemData {
    pub items: ItemList,
    pub recovery_augment: [f32; 2],
}

impl ItemData {
    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let items = ItemList::parse(&vfs.read("etc/item/itemList", mtarc::type_hash("rItemList"))?)
            .map_err(|e| e.to_string())?;
        let xfs = mtxfs::parse(&vfs.read(
            "param/pl/other/plAbilityParam",
            mtarc::type_hash("rPlAbilityParam"),
        )?)
        .map_err(|e| e.to_string())?;
        let rows = xfs.walk();
        let row = rows.get(1 + 0x4a).ok_or("missing recovery augment row")?;
        let factor = |name| match xfs.get(row, name).and_then(|v| v.first()) {
            Some(mtxfs::Value::F(v)) if v.len() == 1 && v[0].is_finite() && v[0] > 0.0 => Ok(v[0] as f32),
            _ => Err(format!("invalid recovery augment {name}")),
        };
        Ok(Self {
            items,
            recovery_augment: [factor("mFVal0")?, factor("mFVal1")?],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipient {
    pub seat: PlayerId,
    pub owner: Owner,
    pub factor: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemRequest {
    /// Existing quick-use viewer policy: first useful supported restorative.
    QuickUse,
    /// Menu request revalidates the presentation's exact live slot.
    Use {
        owner: Owner,
        index: usize,
        expected: Slot,
    },
}

fn needed(effect: Recovery, seat: PlayerId, sim: &ddsim::Sim, fight: &ddenemyrt::fight::Fight) -> bool {
    let Some(actor) = fight.arisen.get(&seat) else {
        return false;
    };
    if actor.fighter.victim.dead() {
        return false;
    }
    let hp = match actor.fighter.victim {
        ddenemyrt::combat::Victim::Player { hp, hp_max, .. } => hp < hp_max && effect.hp > 0.0,
        _ => false,
    };
    let stamina = sim.player(seat).is_some_and(|p| {
        effect.stamina > 0.0
            && p.stamina
                < p.stamina_max
                    * if actor.fighter.statuses.has(CURSE_KIND) {
                        0.6
                    } else {
                        1.0
                    }
    });
    hp || stamina || actor.fighter.statuses.mask() & effect.cures != 0
}

/// Shared native immediate-recovery adapter. Only authorized headless callers
/// supply recipients. 0x0066CCD0 -> 0x00B77130 / 0x00B77780, Steam 2017-12-13.
fn apply(
    data: &ItemData,
    inventory: &mut Inventory,
    recipients: &[Recipient],
    owner: Owner,
    index: usize,
    sim: &mut ddsim::Sim,
    fight: &mut ddenemyrt::fight::Fight,
) -> Result<i16, String> {
    let user = recipients
        .iter()
        .find(|r| r.owner == owner)
        .ok_or("bag owner has no active actor")?
        .seat;
    if !fight.arisen.get(&user).is_some_and(|a| !a.fighter.victim.dead()) {
        return Err("item user is missing or dead".into());
    }
    let item = inventory
        .bag(owner)
        .and_then(|b| b.slots.get(index))
        .ok_or("item slot disappeared")?
        .item;
    let effect = inventory
        .recovery(&data.items, owner, index)
        .map_err(|e| format!("item use: {e:?}"))?;
    if !recipients
        .iter()
        .any(|r| (effect.whole_party || r.seat == user) && needed(effect, r.seat, sim, fight))
    {
        return Err("no recipient needs this supported restorative".into());
    }
    inventory
        .use_recovery(&data.items, owner, index, |effect| {
            for recipient in recipients.iter().filter(|r| effect.whole_party || r.seat == user) {
                let Some(actor) = fight.arisen.get(&recipient.seat) else {
                    continue;
                };
                if actor.fighter.victim.dead() {
                    continue;
                }
                let cursed = actor.fighter.statuses.has(CURSE_KIND);
                let effect = effect.for_recipient(recipient.factor, false);
                // Source 0x00B77780 applies stamina before cures.
                if let Some(p) = sim.player_mut(recipient.seat) {
                    p.recover_item_stamina(effect.stamina, cursed);
                }
                fight.recover_item(recipient.seat, effect.hp, effect.cures);
            }
            Ok::<_, UseError>(())
        })
        .map_err(|e| format!("item use: {e:?}"))?;
    Ok(item)
}

fn next(
    data: &ItemData,
    inventory: &Inventory,
    recipients: &[Recipient],
    user: PlayerId,
    sim: &ddsim::Sim,
    fight: &ddenemyrt::fight::Fight,
) -> Result<(Owner, usize), String> {
    let owner = recipients
        .iter()
        .find(|r| r.seat == user)
        .ok_or("no bag for seat")?
        .owner;
    let bag = inventory.bag(owner).ok_or("no bag")?;
    bag.slots
        .iter()
        .enumerate()
        .find_map(|(index, _)| {
            let effect = inventory.recovery(&data.items, owner, index).ok()?;
            recipients
                .iter()
                .any(|r| (effect.whole_party || r.seat == user) && needed(effect, r.seat, sim, fight))
                .then_some((owner, index))
        })
        .ok_or("no useful supported restorative in this bag".into())
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    /// Explicit new-session initialization, never called by save/load. Native
    /// opening contents/phase mutations are sourced in ddeconomy::new_game.
    pub fn new_game_items(&mut self, data: std::sync::Arc<ItemData>, phase: ddeconomy::new_game::Phase) {
        self.item_data = Some(data);
        self.economy.opening_phase = Some(phase);
        self.economy.opening_grants = Default::default();
        for seat in self.roster.ids() {
            self.economy
                .inventory
                .bags
                .insert(Owner::Arisen(seat), ddeconomy::new_game::bag(phase));
        }
    }

    /// Explicit co-op policy: a newly occupied seat starts from the current
    /// saved opening phase and selected vocation, with no past quest rewards.
    /// Existing bags are retained. The inherited late-join fight-kit and
    /// global story-vocation limitations are separate from inventory ownership.
    pub(crate) fn joined_items(&mut self, seat: PlayerId) {
        let (Some(phase), Some(data)) = (self.economy.opening_phase, self.item_data.as_ref()) else {
            return;
        };
        if self.economy.inventory.bag(Owner::Arisen(seat)).is_some() {
            return;
        }
        self.economy
            .inventory
            .bags
            .insert(Owner::Arisen(seat), ddeconomy::new_game::bag(phase));
        if phase != ddeconomy::new_game::Phase::BeforeCreation {
            if let Some(vocation) = self
                .story
                .as_ref()
                .and_then(|s| ddskill::vocation::Vocation::from_id(s.world.player_job))
            {
                self.economy.opening_grants.first_choice(
                    &mut self.economy.inventory,
                    &data.items,
                    seat,
                    &ddprogress::change::starting_items(vocation),
                );
            }
        }
    }

    pub fn character_created_items(&mut self) {
        if self.economy.opening_phase != Some(ddeconomy::new_game::Phase::BeforeCreation) {
            return;
        }
        self.economy.opening_phase = Some(ddeconomy::new_game::Phase::DragonChallenge);
        for seat in self.roster.ids() {
            self.economy.inventory.bags.insert(
                Owner::Arisen(seat),
                ddeconomy::new_game::bag(ddeconomy::new_game::Phase::DragonChallenge),
            );
        }
    }

    pub fn heart_taken_items(&mut self) {
        let Some(phase) = self.economy.opening_phase else {
            return;
        };
        if phase == ddeconomy::new_game::Phase::NewlyArisen {
            return;
        }
        let Some(data) = self.item_data.as_ref() else {
            return;
        };
        self.economy.opening_phase = Some(ddeconomy::new_game::Phase::NewlyArisen);
        for seat in self.roster.ids() {
            ddeconomy::new_game::heart_taken(&mut self.economy.inventory, &data.items, seat);
        }
    }

    pub(crate) fn first_choice_items(&mut self, vocation: ddskill::vocation::Vocation) {
        if self.economy.opening_phase.is_none() {
            return;
        }
        let Some(data) = self.item_data.as_ref() else {
            return;
        };
        let equipment = ddprogress::change::starting_items(vocation);
        for seat in self.roster.ids() {
            self.economy.opening_grants.first_choice(
                &mut self.economy.inventory,
                &data.items,
                seat,
                &equipment,
            );
        }
    }

    pub(crate) fn story_item_requests(&mut self, requests: &[ddstory::Request]) {
        if self.economy.opening_phase.is_none() {
            return;
        }
        let Some(data) = self.item_data.clone() else {
            return;
        };
        for request in requests {
            match request {
                ddstory::Request::Reward { quest, items, .. } => {
                    for seat in self.roster.ids() {
                        self.economy.opening_grants.reward(
                            &mut self.economy.inventory,
                            &data.items,
                            seat,
                            *quest,
                            items,
                        );
                    }
                }
                ddstory::Request::PlayEvent { row: 21, .. } => self.heart_taken_items(),
                _ => {}
            }
        }
    }

    pub fn item_recipients(&self) -> Vec<Recipient> {
        let Some(data) = self.item_data.as_ref() else {
            return Vec::new();
        };
        self.economy
            .inventory
            .bags
            .keys()
            .filter_map(|&owner| {
                let (seat, role) = match owner {
                    Owner::Arisen(p) if self.roster.seat(p).is_some() => {
                        (p, Some(ddprogress::Role::Arisen(p)))
                    }
                    Owner::Pawn(0) if self.economy.inventory.main_pawn_present => {
                        (4, Some(ddprogress::Role::MainPawn { owner: 0 }))
                    }
                    Owner::Pawn(slot @ 1..=2) if self.rift.party.hired[slot as usize - 1].is_some() => {
                        (Self::hired_pawn_seat(slot as usize - 1), None)
                    }
                    _ => return None,
                };
                let augmented = role
                    .and_then(|role| self.character_of(role))
                    .is_some_and(|c| c.augments.contains(&Some(0x4a)));
                // Hired pawn augment conversion remains absent from progression;
                // imported/local adapter below preserves existing save factors.
                Some(Recipient {
                    seat,
                    owner,
                    factor: data.recovery_augment[usize::from(!augmented)],
                })
            })
            .collect()
    }

    pub fn item_request(&mut self, player: PlayerId, request: ItemRequest) -> Result<i16, String> {
        if self.roster.seat(player).is_none() {
            return Err("unknown inventory requester".into());
        }
        if self.world.player_actions_blocked(player) {
            return Err("item requester actions are blocked by the world".into());
        }
        let data = self.item_data.clone().ok_or("item data unavailable")?;
        let recipients = self.item_recipients();
        let fight = self.fight.as_mut().ok_or("item use needs an active fight")?;
        let (owner, index) = match request {
            ItemRequest::QuickUse => next(
                &data,
                &self.economy.inventory,
                &recipients,
                player,
                &self.sim,
                fight,
            )?,
            ItemRequest::Use {
                owner,
                index,
                expected,
            } => {
                if !matches!(owner,Owner::Arisen(p) if p==player) && !matches!(owner, Owner::Pawn(_)) {
                    return Err("bag belongs to another player".into());
                }
                if self.economy.inventory.bag(owner).and_then(|b| b.slots.get(index)) != Some(&expected) {
                    return Err("selected item changed".into());
                }
                (owner, index)
            }
        };
        apply(
            &data,
            &mut self.economy.inventory,
            &recipients,
            owner,
            index,
            &mut self.sim,
            fight,
        )
    }
}

/// Legacy --play has no game Host. Keep its imported-save behavior in this
/// headless adapter; the opening always uses Host::item_request and ECON.
/// No new-game/reward mutation exists in the presentation-facing adapter.
pub struct ImportedItems {
    pub economy: ddeconomy::Economy,
    pub data: ItemData,
    pub recipients: Vec<Recipient>,
}

impl ImportedItems {
    pub fn load(vfs: &ddvfs::Vfs, xml: &str, locals: &[PlayerId]) -> Result<Self, String> {
        use ddsave::xml::{Document, Node};
        let data = ItemData::load(vfs)?;
        let document = Document::parse(xml).map_err(|e| e.to_string())?;
        let player = document
            .root()
            .path("mPlayerDataManual/mPlCmcEditAndParam")
            .ok_or("missing manual player data")?;
        let bag = |node: Node<'_, '_>| -> Result<Bag, String> {
            let count: usize = node.get("mItemCount").ok_or("missing mItemCount")?;
            let array = node.child("mItem").ok_or("missing bag mItem")?;
            if count > 256 || array.count() != Some(256) || array.children().count() != 256 {
                return Err("invalid saved bag size".into());
            }
            let mut slots = Vec::new();
            for row in array.children().take(count) {
                let item = ddsave::pawn::Item::read(row).ok_or("invalid saved item")?;
                if item.num <= 0 || item.item_no < 0 || data.items.get(item.item_no as u32).is_none() {
                    return Err("invalid occupied saved item".into());
                }
                slots.push(Slot {
                    item: item.item_no,
                    num: item.num,
                    flags: item.flag,
                });
            }
            Ok(Bag { slots })
        };
        let bags = player
            .child("mItem")
            .ok_or("missing saved bags")?
            .children()
            .map(bag)
            .collect::<Result<Vec<_>, _>>()?;
        if bags.len() != 4 {
            return Err("expected four saved bags".into());
        }
        let factor = |param: Node<'_, '_>| -> Result<f32, String> {
            let abilities: Vec<i8> = param.array("mAbility").ok_or("missing augment slots")?;
            Ok(data.recovery_augment[usize::from(!abilities.contains(&0x4a))])
        };
        let arisen_factor = factor(player.path("mPl/mParam").ok_or("missing Arisen params")?)?;
        let mut economy = ddeconomy::Economy::default();
        let mut recipients = Vec::new();
        for &seat in locals {
            economy
                .inventory
                .bags
                .insert(Owner::Arisen(seat), bags[0].clone());
            recipients.push(Recipient {
                seat,
                owner: Owner::Arisen(seat),
                factor: arisen_factor,
            });
        }
        if let Some(&seat) = locals.first() {
            if let Some(pawn) = player.child("mCmc").and_then(|a| a.children().next()) {
                // Same actor-seat mapping as the existing viewer's saved main
                // pawn, 4+Arisen seat; this is a presentation identity policy.
                let pawn_seat = 4 + seat;
                let pawn_factor = factor(pawn.child("mParam").ok_or("missing pawn params")?)?;
                recipients.push(Recipient {
                    seat: pawn_seat,
                    owner: Owner::Pawn(0),
                    factor: pawn_factor,
                });
                economy.inventory.bags.insert(Owner::Pawn(0), bags[1].clone());
                economy.inventory.main_pawn_present = true;
            }
        }
        Ok(Self {
            economy,
            data,
            recipients,
        })
    }
    pub fn use_next(
        &mut self,
        seat: PlayerId,
        sim: &mut ddsim::Sim,
        fight: &mut ddenemyrt::fight::Fight,
    ) -> Result<i16, String> {
        let (owner, index) = next(
            &self.data,
            &self.economy.inventory,
            &self.recipients,
            seat,
            sim,
            fight,
        )?;
        apply(
            &self.data,
            &mut self.economy.inventory,
            &self.recipients,
            owner,
            index,
            sim,
            fight,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Void;
    impl ddsim::Ground for Void {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            None
        }
    }
    #[test]
    fn installed_host_story_reward_and_first_choice_stay_once_after_checkpoint() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() {
            return;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let items = std::sync::Arc::new(ItemData::load(&vfs).unwrap());
        let story = ddstory::data::InstallData::open(&native, None).unwrap();
        let reward = ddstory::orders::reward(&story, 1).unwrap();
        let make = || {
            let mut h = Host::<crate::data::NoStoryData, _>::new(
                ddsim::Sim::new(Default::default()),
                None,
                ddstory::scripted::ScriptedHost::default(),
                Box::new(Void),
            );
            h.join(crate::ProfileId(1), crate::Control::Local, [0.0; 3], 0.0)
                .unwrap();
            h.join(crate::ProfileId(2), crate::Control::Local, [0.0; 3], 0.0)
                .unwrap();
            h.new_game_items(items.clone(), ddeconomy::new_game::Phase::NewlyArisen);
            h
        };
        let mut a = make();
        a.first_choice_items(ddskill::vocation::Vocation::Strider);
        a.story_item_requests(&reward);
        a.story_item_requests(&reward);
        for seat in [0, 1] {
            assert_eq!(a.economy.inventory.bag(Owner::Arisen(seat)).unwrap().count(43), 5);
        }
        let saved = a.save().unwrap();
        let expected = a.economy.clone();
        let mut b = make();
        b.load(&saved, crate::data::NoStoryData).unwrap();
        b.first_choice_items(ddskill::vocation::Vocation::Strider);
        b.story_item_requests(&reward);
        assert_eq!(
            b.economy, expected,
            "neither callback may recreate already delivered items on load"
        );
    }
    #[test]
    fn installed_late_join_gets_phase_and_current_kit_without_past_rewards() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() {
            return;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let data = std::sync::Arc::new(ItemData::load(&vfs).unwrap());
        let story_data = ddstory::data::InstallData::open(&native, None).unwrap();
        let reward = ddstory::orders::reward(&story_data, 1).unwrap();
        let mut story = ddstory::Story::new_game(crate::data::NoStoryData);
        story.world.player_job = ddskill::vocation::Vocation::Strider.id();
        let mut h = Host::new(
            ddsim::Sim::new(Default::default()),
            Some(story),
            ddstory::scripted::ScriptedHost::default(),
            Box::new(Void),
        );
        h.join(crate::ProfileId(1), crate::Control::Local, [0.0; 3], 0.0)
            .unwrap();
        h.new_game_items(data.clone(), ddeconomy::new_game::Phase::NewlyArisen);
        h.first_choice_items(ddskill::vocation::Vocation::Strider);
        h.story_item_requests(&reward);
        let original = h.economy.inventory.bag(Owner::Arisen(0)).unwrap().clone();
        assert_eq!(
            h.join(crate::ProfileId(2), crate::Control::Remote, [0.0; 3], 0.0),
            Some(1)
        );
        let late = h.economy.inventory.bag(Owner::Arisen(1)).unwrap();
        assert_eq!(late.count(43), 0);
        assert_eq!(late.count(0x4a1), 1);
        for item in ddprogress::change::starting_items(ddskill::vocation::Vocation::Strider) {
            assert_eq!(late.count(item as i16), 1);
        }
        assert_eq!(h.economy.inventory.bag(Owner::Arisen(0)).unwrap(), &original);
        let before = h.economy.clone();
        h.joined_items(1);
        assert_eq!(h.economy, before);
    }
    #[test]
    fn installed_legacy_import_checks_occupied_entries_and_keeps_local_bags_distinct() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() {
            return;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let entry = r#"<class type="sItemManager::cITEM_PARAM_DATA"><s16 name="data.mNum" value="2"/><s16 name="data.mItemNo" value="43"/><u32 name="data.mFlag" value="1"/></class>"#;
        let bag = format!(
            r#"<class><u32 name="mItemCount" value="1"/><array name="mItem" count="256">{}</array></class>"#,
            entry.repeat(256)
        );
        let xml = format!(
            r#"<class><class name="mPlayerDataManual"><class name="mPlCmcEditAndParam"><array name="mItem" count="4">{}</array><class name="mPl"><class name="mParam"><array name="mAbility" count="6">{}</array></class></class></class></class></class>"#,
            bag.repeat(4),
            r#"<s8 value="-1"/>"#.repeat(6)
        );
        let mut imported = ImportedItems::load(&vfs, &xml, &[0, 1]).unwrap();
        let original = imported.economy.inventory.bag(Owner::Arisen(1)).unwrap().clone();
        assert_eq!(
            original.slots,
            vec![Slot {
                item: 43,
                num: 2,
                flags: 1
            }]
        );
        imported.economy.inventory.bag_mut(Owner::Arisen(0)).slots.clear();
        assert_eq!(
            imported.economy.inventory.bag(Owner::Arisen(1)).unwrap(),
            &original
        );
        assert!(imported
            .recipients
            .iter()
            .all(|r| r.factor.is_finite() && r.factor > 0.0));
        for malformed in [
            xml.replace(
                r#"name="mItemCount" value="1""#,
                r#"name="mItemCount" value="257""#,
            ),
            xml.replace(r#"name="data.mNum" value="2""#, r#"name="data.mNum" value="0""#),
            xml.replace(
                r#"name="data.mItemNo" value="43""#,
                r#"name="data.mItemNo" value="-1""#,
            ),
        ] {
            assert!(ImportedItems::load(&vfs, &malformed, &[0, 1]).is_err());
        }
    }
}
