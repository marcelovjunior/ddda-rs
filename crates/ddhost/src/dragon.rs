//! Admission and frame ownership of the authored Cassardis dragon.
use crate::{Host, HostError, StoryWorld};
use ddenemyrt::dragon::{DragonData, OpeningDragon};
use ddstory::data::Data;
use std::sync::Arc;

pub struct OpeningDragonData {
    pub data: Arc<DragonData>,
    pub graph: Arc<mtfsm::Graph>,
    pub placement: mtlot::Placement,
    pub group: ddspawn::gpl::Group,
}

impl OpeningDragonData {
    /// Bulk placement, model/motions/stats, and FSM remain install reads.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let groups = crate::spawns::group_list(vfs, 200)?;
        let group = groups
            .groups
            .into_iter()
            .find(|g| g.slot == 0)
            .ok_or("stage200 dragon group0 absent")?;
        let lot = mtlot::parse(&vfs.read("scr/st200/etc/st200_00m00n_e00", mtarc::type_hash("rLayout"))?)
            .map_err(|e| e.to_string())?;
        let record = lot
            .iter()
            .find(|r| r.kind == 32 && r.get("mSeqButtleSts") == Some(&mtlot::Value::Int(4)))
            .ok_or("stage200 sequence4 dragon absent")?;
        let placement = record.placement().ok_or("dragon transform absent")?;
        let path = match record.get("mFsmFilePath") {
            Some(mtlot::Value::Str(path)) => String::from_utf8(path.clone()).map_err(|e| e.to_string())?,
            _ => return Err("dragon placement FSM absent".into()),
        };
        let graph = Arc::new(
            mtfsm::Graph::parse(&vfs.read(&path, mtarc::type_hash("rAIFSM"))?).map_err(|e| e.to_string())?,
        );
        let scale = placement.scale.iter().sum::<f32>() / 3.0;
        Ok(Self {
            data: Arc::new(DragonData::opening(vfs, exe, scale)?),
            graph,
            placement,
            group,
        })
    }
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    pub fn set_opening_dragon_data(&mut self, data: OpeningDragonData) {
        self.opening_dragon_data = Some(data);
    }

    /// Reevaluate admission each host frame, including LOT changes after a
    /// cell has loaded. S31 replaces the scene's scripted shortcut; S29 can
    /// already honor the authored group's live gate when the scene runs.
    pub(crate) fn sync_opening_dragon(&mut self) -> Result<(), HostError> {
        let (Some(data), Some(story), Some(fight)) =
            (&self.opening_dragon_data, &self.story, &mut self.fight)
        else {
            return Ok(());
        };
        let world = &story.world;
        if story.stage.stage_no != 200 {
            fight.dragon = None;
            return Ok(());
        }
        let flag = |n: u16| world.lot(200, n as usize);
        let inputs = ddspawn::World {
            scene: world.sce_no as i32,
            hour: world.hour,
            flag: &flag,
        };
        if fight.dragon.is_some() {
            if ddspawn::should_despawn(&data.group, &inputs) {
                fight.dragon = None;
            }
        } else if ddspawn::gate(&data.group, &inputs) == ddspawn::Gate::Spawn {
            let owner = fight.allocate_enemy_owner();
            fight.dragon = Some(
                OpeningDragon::new(
                    data.data.clone(),
                    data.graph.clone(),
                    owner,
                    data.placement.position,
                    data.placement.angle[1],
                    data.placement.scale,
                )
                .map_err(HostError::Npc)?,
            );
        }
        Ok(())
    }

    pub(crate) fn step_opening_dragon_a(&mut self, step: f32) -> Result<(), HostError> {
        let (Some(story), Some(fight)) = (&mut self.story, &mut self.fight) else {
            return Ok(());
        };
        let Some(dragon) = &mut fight.dragon else {
            return Ok(());
        };
        let world = &mut story.world;
        dragon
            .pass_a(
                step,
                self.roster
                    .ids()
                    .filter_map(|id| self.sim.player(id).map(|p| p.position))
                    .collect(),
                world.sce_no,
                0,
                world.area_stay,
            )
            .map_err(HostError::Npc)?;
        for effect in &dragon.effects {
            match *effect {
                ddenemyrt::dragon::DragonEffect::Gsf { no, value } => world.gsf.set(no as usize, value),
                ddenemyrt::dragon::DragonEffect::EscapeFly { .. } => {
                    return Err(HostError::Npc("dragon fall recovery action60 remains unimplemented".into()));
                }
                _ => {}
            }
        }
        for request in &dragon.world_requests {
            match *request {
                ddenemyrt::dragon::ActorRequest::Gsf { no, value } => world.gsf.set(no as usize, value),
                ddenemyrt::dragon::ActorRequest::QuestFlag { quest, bit, value } => {
                    world.set_quest_bit(quest as usize, bit as usize, value)
                }
                ddenemyrt::dragon::ActorRequest::LayoutFlag { stage, no, value } => {
                    world.set_lot(stage, no as usize, value)
                }
                _ => {
                    return Err(HostError::Npc(format!(
                        "dragon world request {request:?} unimplemented"
                    )))
                }
            }
        }
        Ok(())
    }

    pub(crate) fn step_opening_dragon_c(&mut self, step: f32) -> Result<(), HostError> {
        let Some(fight) = &mut self.fight else {
            return Ok(());
        };
        if let Some(dragon) = &mut fight.dragon {
            dragon.pass_c_with_terrain(step, &*self.ground).map_err(HostError::Npc)?;
            fight
                .positions
                .insert(dragon.actor.fighter.owner, dragon.actor.pos);
        }
        Ok(())
    }
}
