//! Host-authoritative NPC lifetimes and actor state (co-op protocol design).
//! Native values come from S32's motion layer and S34's ordinary NPC actor;
//! clients load their own banks and never complete an action or death phase.
use ddhost::{Host, StoryWorld};
use ddid::{Reader, Result, Writer};
use ddstory::data::Data;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub command: u64,
    pub launch: u64,
    pub bank: u8,
    pub slot: u16,
    pub frame: f32,
    pub blend_frames: f32,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Combat {
    pub hp: f32,
    pub hp_max: f32,
    /// None is alive; 0..4 are Initialize, Motion, CorpseWait, Fade, Finished.
    pub death_phase: Option<u8>,
    pub alpha: f32,
    pub retired: bool,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcBody {
    pub id: u32,
    pub pos: [f32; 3],
    pub generation: u64,
    pub stage: u32,
    pub yaw: f32,
    pub scale: [f32; 3],
    pub gauge: bool,
    pub motion: Option<Motion>,
    pub combat: Option<Combat>,
}
impl NpcBody {
    /// Legacy schedule/parked NPC has no materialized actor state to invent.
    pub fn position(id: u32, pos: [f32; 3]) -> Self {
        Self {
            id,
            pos,
            generation: 0,
            stage: 0,
            yaw: 0.0,
            scale: [1.0; 3],
            gauge: false,
            motion: None,
            combat: None,
        }
    }
}
pub fn of<D: Data, W: StoryWorld>(host: &Host<D, W>) -> Vec<NpcBody> {
    let mut out = std::collections::BTreeMap::new();
    for &id in host.npcs.npcs.keys() {
        if let Some(pos) = host.npcs.position(id) {
            out.insert(id, NpcBody::position(id, pos));
        }
    }
    for (&id, &pos) in &host.npc_parked {
        out.entry(id).or_insert_with(|| NpcBody::position(id, pos));
    }
    for placement in host.story_npc_intels() {
        let id = placement.npc as u32;
        let Some(lifetime) = host.story_npc_lifetime(id) else {
            continue;
        };
        let mut npc = NpcBody::position(
            id,
            host.story_npc_position(id)
                .unwrap_or(placement.transform.position),
        );
        npc.generation = lifetime.generation;
        npc.stage = placement.stage;
        npc.yaw = placement.transform.angle[1];
        npc.scale = placement.transform.scale;
        npc.gauge = host.story_npc_gauge_visible(id) == Some(true);
        npc.motion = host.story_npc_motion(id).map(|actor| Motion {
            command: actor.token.command,
            launch: actor.token.launch,
            bank: (actor.motion >> 8) as u8,
            slot: actor.motion & 255,
            frame: actor.frame,
            blend_frames: actor.authored.blend_frames,
        });
        npc.combat = host.story_npc_combat_facts(lifetime).map(|facts| {
            use ddhost::ddenemyrt::npc_death::DeathPhase;
            let death = host.story_npc_death_state(lifetime);
            Combat {
                hp: facts.hp,
                hp_max: facts.max_hp,
                death_phase: death.map(|d| match d.phase {
                    DeathPhase::Initialize => 0,
                    DeathPhase::Motion => 1,
                    DeathPhase::CorpseWait => 2,
                    DeathPhase::Fade => 3,
                    DeathPhase::Finished => 4,
                }),
                alpha: death.map_or(1.0, |d| d.alpha),
                retired: host.story_npc_body_retired(id),
            }
        });
        // Use the actual combat actor's orientation/scale when one is bound.
        if let Some(actor) = host
            .story_npc_combat_owner(lifetime)
            .and_then(|owner| host.fight.as_ref()?.npcs.get(&owner))
        {
            npc.yaw = actor.yaw;
            npc.scale = actor.scale;
        }
        out.insert(id, npc);
    }
    out.into_values().collect()
}

// List membership carries creation/removal. Mask groups carry position,
// lifetime/pose, gauge, motion and combat; unknown masks are rejected.
const POSITION: u8 = 1;
const OWNER: u8 = 2;
const GAUGE: u8 = 4;
const MOTION: u8 = 8;
const COMBAT: u8 = 16;
const ALL: u8 = 31;
pub fn write(w: &mut Writer, npcs: &[NpcBody], base: &[NpcBody]) {
    w.len(npcs.len());
    for x in npcs {
        let b = base.iter().find(|b| b.id == x.id && b.generation == x.generation);
        let mask = b.map_or(ALL, |b| {
            if x == b {
                return 0;
            }
            u8::from(x.pos != b.pos) * POSITION
                | u8::from((x.generation, x.stage, x.yaw, x.scale) != (b.generation, b.stage, b.yaw, b.scale))
                    * OWNER
                | u8::from(x.gauge != b.gauge) * GAUGE
                | u8::from(x.motion != b.motion) * MOTION
                | u8::from(x.combat != b.combat) * COMBAT
        });
        w.u32(x.id).u8(mask);
        if mask & POSITION != 0 {
            for v in x.pos {
                w.f32(v);
            }
        }
        if mask & OWNER != 0 {
            w.u64(x.generation).u32(x.stage).f32(x.yaw);
            for v in x.scale {
                w.f32(v);
            }
        }
        if mask & GAUGE != 0 {
            w.bool(x.gauge);
        }
        if mask & MOTION != 0 {
            w.bool(x.motion.is_some());
            if let Some(m) = x.motion {
                w.u64(m.command)
                    .u64(m.launch)
                    .u8(m.bank)
                    .u16(m.slot)
                    .f32(m.frame)
                    .f32(m.blend_frames);
            }
        }
        if mask & COMBAT != 0 {
            w.bool(x.combat.is_some());
            if let Some(c) = x.combat {
                w.f32(c.hp)
                    .f32(c.hp_max)
                    .u8(c.death_phase.unwrap_or(255))
                    .f32(c.alpha)
                    .bool(c.retired);
            }
        }
    }
}
pub fn read(r: &mut Reader, base: &[NpcBody]) -> Result<Vec<NpcBody>> {
    let n = r.len(5)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u32()?;
        let mask = r.u8()?;
        if mask & !ALL != 0 {
            return Err(format!("NPC {id}: invalid delta mask {mask}"));
        }
        if out.iter().any(|x: &NpcBody| x.id == id) {
            return Err(format!("NPC {id}: duplicate identity"));
        }
        let mut x = match base.iter().find(|x| x.id == id) {
            Some(x) => *x,
            None if mask == ALL => NpcBody::position(id, [0.0; 3]),
            None => return Err(format!("NPC {id}: delta without a baseline")),
        };
        if mask & POSITION != 0 {
            x.pos = [r.f32()?, r.f32()?, r.f32()?];
        }
        if mask & OWNER != 0 {
            let generation = r.u64()?;
            if generation != x.generation && mask != ALL {
                return Err(format!("NPC {id}: partial replacement lifetime"));
            }
            x.generation = generation;
            x.stage = r.u32()?;
            x.yaw = r.f32()?;
            x.scale = [r.f32()?, r.f32()?, r.f32()?];
        }
        if mask & GAUGE != 0 {
            x.gauge = r.bool()?;
        }
        if mask & MOTION != 0 {
            x.motion = if r.bool()? {
                Some(Motion {
                    command: r.u64()?,
                    launch: r.u64()?,
                    bank: r.u8()?,
                    slot: r.u16()?,
                    frame: r.f32()?,
                    blend_frames: r.f32()?,
                })
            } else {
                None
            };
        }
        if mask & COMBAT != 0 {
            x.combat = if r.bool()? {
                let hp = r.f32()?;
                let hp_max = r.f32()?;
                let phase = r.u8()?;
                if phase > 4 && phase != 255 {
                    return Err(format!("NPC {id}: unknown death phase {phase}"));
                }
                Some(Combat {
                    hp,
                    hp_max,
                    death_phase: (phase != 255).then_some(phase),
                    alpha: r.f32()?,
                    retired: r.bool()?,
                })
            } else {
                None
            };
        }
        if !x.pos.iter().chain(&x.scale).all(|v| v.is_finite())
            || !x.yaw.is_finite()
            || x.motion.is_some_and(|m| {
                m.bank > 15 || m.slot > 255 || !m.frame.is_finite() || !m.blend_frames.is_finite()
            })
            || x.combat.is_some_and(|c| {
                !c.hp.is_finite()
                    || !c.hp_max.is_finite()
                    || c.hp_max < 0.0
                    || !c.alpha.is_finite()
                    || !(0.0..=1.0).contains(&c.alpha)
            })
        {
            return Err(format!("NPC {id}: invalid actor state"));
        }
        out.push(x);
    }
    Ok(out)
}
