//! Received NPC pose inputs for the existing installed gauge renderer.
use ddhost::ddenemyrt::{body::MotionId, npc_body::NpcBody};
use std::sync::Arc;

pub(crate) struct GaugeActor {
    pub owner: u32,
    pub npc: u32,
    pub body: Arc<NpcBody>,
    pub motion: MotionId,
    pub frame: f32,
    pub position: [f32; 3],
    pub yaw: f32,
    pub scale: [f32; 3],
    pub hp: f32,
    pub hp_max: f32,
}

#[derive(Default)]
pub(crate) struct Bodies(std::collections::HashMap<u32, Arc<NpcBody>>);
impl Bodies {
    pub fn load(
        &mut self,
        npcs: &[ddnet::npc::NpcBody],
        vfs: &ddvfs::Vfs,
        exe: &ddexe::Exe<'_>,
    ) -> Result<(), String> {
        self.0
            .retain(|id, _| npcs.iter().any(|n| n.id == *id && n.combat.is_some()));
        for n in npcs.iter().filter(|n| n.combat.is_some()) {
            if !self.0.contains_key(&n.id) {
                // Same generic opening type/variant adapter as the host, not
                // a synthetic skeleton or locally simulated combat owner.
                self.0
                    .insert(n.id, Arc::new(NpcBody::load(vfs, exe, n.id, n.id, 0, 0, 0)?));
            }
        }
        Ok(())
    }
    pub fn gauges(&self, npcs: &std::collections::HashMap<u32, ddnet::npc::NpcBody>) -> Vec<GaugeActor> {
        npcs.values()
            .filter_map(|n| {
                let combat = n.combat?;
                let motion = n.motion?;
                if !n.gauge || combat.hp <= 0.0 || combat.retired {
                    return None;
                }
                Some(GaugeActor {
                    owner: n.id,
                    npc: n.id,
                    body: self.0.get(&n.id)?.clone(),
                    motion: MotionId::new(motion.bank, motion.slot),
                    frame: motion.frame,
                    position: n.pos,
                    yaw: n.yaw,
                    scale: n.scale,
                    hp: combat.hp,
                    hp_max: combat.hp_max,
                })
            })
            .collect()
    }
}
