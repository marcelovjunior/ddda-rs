//! Local quick-use adapter. Reads the original save; never writes it.
//! Inventory selection is viewer policy, recovery rules live in headless
//! crates. LAN item requests are not yet part of the network protocol.

use bevy::prelude::Resource;
use ddeconomy::inventory::{Bag, Inventory, Owner, Slot};
use ddeconomy::item_use::{Recovery, UseError};
use ddhost::ddenemyrt::fight::Fight;
use dditem::itl::ItemList;
use ddsave::xml::{Document, Node};
use ddsim::{PlayerId, Sim};
use std::collections::BTreeMap;

// cStatus curse bit 0x200, tested by 0x00B81E30 before clamping stamina.
const CURSE_KIND: u32 = 9;

#[derive(Resource)]
pub struct QuickItems {
    pub inventory: Inventory,
    items: ItemList,
    factors: BTreeMap<PlayerId, f32>,
    owners: BTreeMap<PlayerId, Owner>,
    opening: Option<(ddeconomy::new_game::Phase, ddeconomy::new_game::Grants)>,
}

fn read_bag(node: Node<'_, '_>) -> Result<Bag, String> {
    let count: usize = node.get("mItemCount").ok_or("missing mItemCount")?;
    let array = node.child("mItem").ok_or("missing bag mItem")?;
    if count > 256 || array.count() != Some(256) || array.children().count() != 256 {
        return Err("invalid saved bag size".into());
    }
    let mut slots = Vec::new();
    for entry in array.children().take(count) {
        let item = ddsave::pawn::Item::read(entry).ok_or("invalid saved item")?;
        if item.num <= 0 || item.item_no < 0 {
            return Err("invalid occupied item slot".into());
        }
        slots.push(Slot {
            item: item.item_no,
            num: item.num,
            flags: item.flag,
        });
    }
    Ok(Bag { slots })
}

impl QuickItems {
    /// Save-free opening bags. Phase is an explicit story boundary: the
    /// challenge carries the editor's temporary items; NewlyArisen has
    /// the event-row-21 replacement. Neither phase starts with curatives.
    pub fn new_game(vfs: &ddvfs::Vfs, locals: &[PlayerId], phase: ddeconomy::new_game::Phase) -> Result<Self, String> {
        let items = ItemList::parse(&vfs.read(r"etc\item\itemList", mtarc::type_hash("rItemList"))?)
            .map_err(|e| e.to_string())?;
        let xfs = mtxfs::parse(&vfs.read(r"param\pl\other\plAbilityParam", mtarc::type_hash("rPlAbilityParam"))?)
            .map_err(|e| e.to_string())?;
        let nodes = xfs.walk();
        let row = nodes.get(1 + 0x4a).ok_or("missing recovery augment row")?;
        // 0x00444840 clears all six augment ids at the prologue boundary.
        let factor = match xfs.get(row, "mFVal1").and_then(|v| v.first()) {
            Some(mtxfs::Value::F(v)) if v.len() == 1 => v[0] as f32,
            _ => return Err("invalid unaugmented recovery factor".into()),
        };
        let mut inventory = Inventory::default();
        let mut owners = BTreeMap::new();
        let mut factors = BTreeMap::new();
        for &seat in locals {
            inventory.bags.insert(Owner::Arisen(seat), ddeconomy::new_game::bag(phase));
            owners.insert(seat, Owner::Arisen(seat));
            factors.insert(seat, factor);
        }
        Ok(Self { inventory, items, factors, owners, opening: Some((phase, Default::default())) })
    }

    /// Deliver event row 21 once (the host's actual PlayEvent request).
    pub fn heart_taken(&mut self) {
        let Some((phase, _)) = &mut self.opening else { return };
        if *phase == ddeconomy::new_game::Phase::NewlyArisen { return; }
        *phase = ddeconomy::new_game::Phase::NewlyArisen;
        for &seat in self.owners.keys() {
            ddeconomy::new_game::heart_taken(&mut self.inventory, &self.items, seat);
        }
    }

    /// Called when the real first vocation menu closes. The original has
    /// one Arisen; each local Arisen receives that first-choice equipment.
    pub fn first_choice(&mut self, vocation: ddhost::ddskill::vocation::Vocation) {
        let Some((_, grants)) = &mut self.opening else { return };
        let equipment = ddhost::ddprogress::change::starting_items(vocation);
        for &seat in self.owners.keys() {
            grants.first_choice(&mut self.inventory, &self.items, seat, &equipment);
        }
    }

    /// The host report supplies the installed reward's item pairs at the
    /// authored clear row, including q1's first restoratives after ev10.
    pub fn rewards(&mut self, requests: &[ddstory::Request]) {
        let Some((_, grants)) = &mut self.opening else { return };
        for request in requests {
            if let ddstory::Request::Reward { quest, items, .. } = request {
                for &seat in self.owners.keys() {
                    grants.reward(&mut self.inventory, &self.items, seat, *quest, items);
                }
            }
        }
    }

    /// The original save has four bags in cSAVE_DATA_ITEM order. Each
    /// local Arisen imports its own copy of bag 0, matching the viewer's
    /// existing same-save character setup. Only the first local Arisen's
    /// main pawn is bound here, to avoid duplicating the shared pawn bag.
    pub fn load(vfs: &ddvfs::Vfs, xml: &str, locals: &[PlayerId]) -> Result<Self, String> {
        let doc = Document::parse(xml).map_err(|e| e.to_string())?;
        let player = doc
            .root()
            .path("mPlayerDataManual/mPlCmcEditAndParam")
            .ok_or("missing manual player data")?;
        let bags = player.child("mItem").ok_or("missing saved bags")?;
        let bags: Vec<_> = bags.children().map(read_bag).collect::<Result<_, _>>()?;
        if bags.len() != 4 {
            return Err("expected four saved character bags".into());
        }
        let bytes = vfs.read(r"etc\item\itemList", mtarc::type_hash("rItemList"))?;
        let items = ItemList::parse(&bytes).map_err(|e| e.to_string())?;
        let bytes = vfs.read(
            r"param\pl\other\plAbilityParam",
            mtarc::type_hash("rPlAbilityParam"),
        )?;
        let xfs = mtxfs::parse(&bytes).map_err(|e| e.to_string())?;
        let nodes = xfs.walk();
        let row = nodes.get(1 + 0x4a).ok_or("missing recovery augment row")?;
        let factor = |param: Node<'_, '_>| -> Result<f32, String> {
            let abilities: Vec<i8> = param.array("mAbility").ok_or("missing augment slots")?;
            let field = if abilities.contains(&0x4a) {
                "mFVal0"
            } else {
                "mFVal1"
            };
            match xfs.get(row, field).and_then(|v| v.first()) {
                Some(mtxfs::Value::F(v)) if v.len() == 1 => Ok(v[0] as f32),
                _ => Err("invalid recovery augment factor".into()),
            }
        };
        let arisen_factor = factor(player.path("mPl/mParam").ok_or("missing Arisen params")?)?;
        let mut inventory = Inventory::default();
        let mut owners = BTreeMap::new();
        let mut factors = BTreeMap::new();
        for &seat in locals {
            owners.insert(seat, Owner::Arisen(seat));
            factors.insert(seat, arisen_factor);
            inventory.bags.insert(Owner::Arisen(seat), bags[0].clone());
        }
        if let Some(&seat) = locals.first() {
            if let Some(pawn) = player.child("mCmc").and_then(|a| a.children().next()) {
                let pawn_seat = crate::play::pawn_seat(seat);
                factors.insert(
                    pawn_seat,
                    factor(pawn.child("mParam").ok_or("missing pawn params")?)?,
                );
                owners.insert(pawn_seat, Owner::Pawn(0));
                inventory.bags.insert(Owner::Pawn(0), bags[1].clone());
            }
        }
        Ok(Self {
            inventory,
            items,
            factors,
            owners,
            opening: None,
        })
    }

    fn needed(&self, effect: Recovery, seat: PlayerId, sim: &Sim, fight: &Fight) -> bool {
        let Some(actor) = fight.arisen.get(&seat) else {
            return false;
        };
        if actor.fighter.victim.dead() {
            return false;
        }
        let hp = match actor.fighter.victim {
            ddhost::ddenemyrt::combat::Victim::Player { hp, hp_max, .. } => hp < hp_max && effect.hp > 0.0,
            _ => false,
        };
        let stamina = sim.players.iter().find(|p| p.id == seat).is_some_and(|p| {
            let cursed = actor.fighter.statuses.has(CURSE_KIND);
            effect.stamina > 0.0 && p.stamina < p.stamina_max * if cursed { 0.6 } else { 1.0 }
        });
        hp || stamina || actor.fighter.statuses.mask() & effect.cures != 0
    }

    /// Viewer policy: first useful supported restorative in this seat's
    /// bag. A missing/dead character or fully recovered party spends none.
    /// Host calls this before its next frame; it must not run on a client.
    pub fn use_next(&mut self, seat: PlayerId, sim: &mut Sim, fight: &mut Fight) -> Result<i16, String> {
        if !fight.arisen.get(&seat).is_some_and(|a| !a.fighter.victim.dead()) {
            return Err("item user is missing or dead".into());
        }
        let owner = *self.owners.get(&seat).ok_or("no bag for seat")?;
        let bag = self.inventory.bag(owner).ok_or("no bag")?;
        let (index, item, effect) = bag
            .slots
            .iter()
            .enumerate()
            .find_map(|(i, slot)| {
                let effect = self.inventory.recovery(&self.items, owner, i).ok()?;
                let useful = if effect.whole_party {
                    self.owners.keys().any(|&id| self.needed(effect, id, sim, fight))
                } else {
                    self.needed(effect, seat, sim, fight)
                };
                useful.then_some((i, slot.item, effect))
            })
            .ok_or("no useful supported restorative in this bag")?;
        let recipients: Vec<_> = self
            .owners
            .iter()
            .filter(|(id, _)| effect.whole_party || **id == seat)
            .map(|(&id, _)| (id, self.factors[&id]))
            .collect();
        self.inventory
            .use_recovery(&self.items, owner, index, |effect| {
                for (id, factor) in recipients {
                    let Some(actor) = fight.arisen.get(&id) else {
                        continue;
                    };
                    if actor.fighter.victim.dead() {
                        continue;
                    }
                    let effect = effect.for_recipient(factor, false);
                    // B77780 applies stamina before cures (curse still caps it).
                    let cursed = actor.fighter.statuses.has(CURSE_KIND);
                    if let Some(player) = sim.players.iter_mut().find(|p| p.id == id) {
                        player.recover_item_stamina(effect.stamina, cursed);
                    }
                    fight.recover_item(id, effect.hp, effect.cures);
                }
                Ok::<_, UseError>(())
            })
            .map_err(|e| format!("item use: {e:?}"))?;
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_quick_use_waits_for_authored_reward_and_heals_only_its_seat() {
        let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
        if !native.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let exe = ddexe::ExeFile::next_to(&native).unwrap();
        let data = ddhost::ddenemyrt::fight::FightData::load(&vfs, &exe.image().unwrap()).unwrap();
        let mut fight = Fight::new(std::sync::Arc::new(data));
        let mut sim = Sim::new(Default::default());
        for seat in [0, 1] { fight.add_arisen(seat); sim.add_player(seat, [0.0; 3], 0.0); }
        let mut quick = QuickItems::new_game(&vfs, &[0, 1], ddeconomy::new_game::Phase::DragonChallenge).unwrap();
        quick.heart_taken();
        quick.heart_taken();
        quick.first_choice(ddhost::ddskill::vocation::Vocation::Strider);
        quick.first_choice(ddhost::ddskill::vocation::Vocation::Strider);
        if let ddhost::ddenemyrt::combat::Victim::Player { hp, hp_max, .. } = &mut fight.arisen.get_mut(&0).unwrap().fighter.victim {
            *hp = 20.0; *hp_max = 1000.0;
        }
        assert!(quick.use_next(0, &mut sim, &mut fight).is_err());
        let story = ddstory::data::InstallData::open(&native, None).unwrap();
        let reward = ddstory::orders::reward(&story, 1).unwrap();
        quick.rewards(&reward);
        quick.rewards(&reward);
        let other_hp = fight.arisen[&1].fighter.victim.hp();
        let other_bag = quick.inventory.bag(Owner::Arisen(1)).unwrap().clone();
        assert_eq!(quick.inventory.bag(Owner::Arisen(0)).unwrap().count(43), 5);
        assert_eq!(quick.use_next(0, &mut sim, &mut fight).unwrap(), 43);
        assert!(fight.arisen[&0].fighter.victim.hp() > 20.0);
        assert_eq!(fight.arisen[&1].fighter.victim.hp(), other_hp);
        assert_eq!(quick.inventory.bag(Owner::Arisen(0)).unwrap().count(43), 4);
        assert_eq!(quick.inventory.bag(Owner::Arisen(1)).unwrap(), &other_bag);
    }

    #[test]
    fn saved_bag_count_and_invalid_entries_are_checked() {
        let entries = "<class type=\"sItemManager::cITEM_PARAM_DATA\"><s16 name=\"data.mNum\" value=\"2\"/><s16 name=\"data.mItemNo\" value=\"43\"/><u32 name=\"data.mFlag\" value=\"1\"/></class>".repeat(256);
        let xml = format!("<class><u32 name=\"mItemCount\" value=\"1\"/><array name=\"mItem\" count=\"256\">{entries}</array></class>");
        let doc = Document::parse(&xml).unwrap();
        assert_eq!(
            read_bag(doc.root()).unwrap().slots,
            vec![Slot {
                item: 43,
                num: 2,
                flags: 1
            }]
        );
        let malformed = xml.replace("value=\"1\"/><array", "value=\"257\"/><array");
        let doc = Document::parse(&malformed).unwrap();
        assert!(read_bag(doc.root()).is_err());
    }

    #[test]
    fn saved_bags_import_into_distinct_local_seats() {
        let Some(path) = std::env::var_os("DDDA_TEST_SAVE") else {
            return;
        };
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let save = ddsave::SaveFile::parse(&std::fs::read(path).unwrap()).unwrap();
        let mut items = QuickItems::load(&vfs, &save.xml, &[0, 1]).unwrap();
        let first = items.inventory.bag(Owner::Arisen(0)).unwrap().clone();
        assert_eq!(&first, items.inventory.bag(Owner::Arisen(1)).unwrap());
        items.inventory.bag_mut(Owner::Arisen(0)).slots.clear();
        assert_eq!(&first, items.inventory.bag(Owner::Arisen(1)).unwrap());
        assert!(items.factors.values().all(|f| f.is_finite() && *f > 0.0));
    }

    #[test]
    fn quick_use_consumes_one_and_applies_installed_potion_stamina_and_poison_cure() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
        if !native.is_dir() {
            return;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let exe = ddexe::ExeFile::next_to(&native).unwrap();
        let data = ddhost::ddenemyrt::fight::FightData::load(&vfs, &exe.image().unwrap()).unwrap();
        let mut fight = Fight::new(std::sync::Arc::new(data));
        fight.add_arisen(0);
        fight.add_arisen(1);
        let mut sim = Sim::new(Default::default());
        sim.add_player(0, [0.0; 3], 0.0);
        let table = ItemList::parse(
            &vfs.read(r"etc\item\itemList", mtarc::type_hash("rItemList"))
                .unwrap(),
        )
        .unwrap();
        let mut inventory = Inventory::default();
        inventory.bag_mut(Owner::Arisen(0)).slots = [99, 45, 48]
            .map(|item| Slot {
                item,
                num: 1,
                flags: 1,
            })
            .to_vec();
        let mut quick = QuickItems {
            inventory,
            items: table,
            factors: [(0, 1.0)].into(),
            owners: [(0, Owner::Arisen(0))].into(),
            opening: None,
        };
        if let ddhost::ddenemyrt::combat::Victim::Player { hp, hp_max, .. } =
            &mut fight.arisen.get_mut(&0).unwrap().fighter.victim
        {
            *hp = 100.0;
            *hp_max = 2000.0;
        }
        let other_hp = fight.arisen[&1].fighter.victim.hp();
        assert_eq!(quick.use_next(0, &mut sim, &mut fight).unwrap(), 99);
        assert_eq!(fight.arisen[&0].fighter.victim.hp(), 1600.0);
        assert_eq!(fight.arisen[&1].fighter.victim.hp(), other_hp);
        sim.players[0].stamina = 0.0;
        assert_eq!(quick.use_next(0, &mut sim, &mut fight).unwrap(), 45);
        assert_eq!(sim.players[0].stamina, 250.0);
        fight
            .arisen
            .get_mut(&0)
            .unwrap()
            .fighter
            .statuses
            .add(0, &fight.data.tables.status_player);
        assert_eq!(quick.use_next(0, &mut sim, &mut fight).unwrap(), 48);
        assert!(!fight.arisen[&0].fighter.statuses.has(0));
        assert!(quick.inventory.bag(Owner::Arisen(0)).unwrap().slots.is_empty());
        assert!(quick.use_next(0, &mut sim, &mut fight).is_err());
    }
}
