//! Host ownership of authored door motion, requests and collision.
use std::{collections::BTreeMap, sync::Arc};
use dddoor::{collision, placement::{self, Id, Placement}, policy::{Availability, Observed, Policy}, runtime::{self, Door, Occupant}};
use crate::{Host, StoryWorld};
use ddstory::data::Data;

pub struct Entry {
    pub placement: Placement,
    pub matrix: [f32;16],
    pub policy: Policy,
    pub observed: Observed,
    pub motions: Arc<mtlmt::MotionList>,
    pub frame: f32,
    /// Changes on every motion switch, including a switch to the same slot.
    pub revision: u64,
}

pub struct Doors {
    pub stage: u32,
    pub states: BTreeMap<Id, Door>,
    pub entries: BTreeMap<Id, Entry>,
    pub collision: collision::Set,
    /// Explicit temporary presentation adapter: the original has richer
    /// focus/angle/action arbitration. Disabled unless the caller opts in.
    pub nearest_interaction_standin: bool,
    requests: Vec<(Id, Option<[f32;3]>)>,
    pub diagnostics: Vec<String>,
}

impl Doors {
    /// Caller resolves fresh/saved door persistence explicitly. Candidates
    /// whose policy remains unknown may be presented closed, but never
    /// accept an opening request on that basis.
    pub fn load(vfs: &ddvfs::Vfs, stage: u32, placements: Vec<Placement>,
        observed: impl Fn(&Placement) -> Observed) -> Result<Self,String> {
        let mut out = Self { stage, states: BTreeMap::new(), entries: BTreeMap::new(),
            collision: Default::default(), nearest_interaction_standin: false,
            requests: Vec::new(), diagnostics: Vec::new() };
        let mut collisions = BTreeMap::new();
        let mut clips = BTreeMap::new();
        for placement in placements {
            if placement.id.stage != stage { return Err("door placement stage mismatch".into()); }
            let id = placement.id;
            let result = (|| -> Result<(Door,Entry,collision::Leaf),String> {
                let matrix = upright_matrix(&placement)?;
                let policy = Policy::from_record(&placement.record)?;
                let state = observed(&placement);
                if policy.resolve(state) == Some(Availability::Open) {
                    return Err("permanently-open startup pose not implemented".into());
                }
                let paths = placement::resources(placement.object);
                let motions = if let Some(m) = clips.get(&placement.object) { Arc::clone(m) } else {
                    let bytes = vfs.read(&paths.motion, mtarc::type_hash("rMotionList"))?;
                    let m = Arc::new(mtlmt::parse(&bytes).map_err(|e|e.to_string())?);
                    for slot in 0..=6 {
                        if m.motions.get(slot).and_then(Option::as_ref).is_none() {
                            return Err(format!("missing authored door motion {slot}"));
                        }
                    }
                    clips.insert(placement.object,m.clone()); m
                };
                let door = Door::closed(id,placement.object,placement.transform.position,
                    [matrix[8],matrix[9],matrix[10]])?;
                let collision = collision::load(vfs,&paths.collision[0],matrix)?;
                let entry = Entry { placement: placement.clone(), matrix, policy, observed: state,
                    motions, frame:0.0, revision:0 };
                Ok((door,entry,collision))
            })();
            match result {
                Ok((door,entry,collision)) => {
                    if out.states.insert(id,door).is_some() { return Err("duplicate door identity".into()); }
                    out.entries.insert(id,entry); collisions.insert(id,collision);
                }
                Err(e) => out.diagnostics.push(format!("{id:?}: {e}")),
            }
        }
        out.collision = collision::Set(Arc::new(collisions));
        Ok(out)
    }

    /// Actor/AI/event adapter calls this when it actually issues message
    /// 0x1a. Player, pawn and NPC requests use the same host queue.
    pub fn request_open(&mut self,id:Id,actor:Option<[f32;3]>) { self.requests.push((id,actor)); }

    /// A bounded viewer stand-in for the original interaction focus.
    /// Uses the sourced uOmObj7010 broad query cylinder (ctor0x00c3e460),
    /// not automatic proximity opening. A rising interact edge is required.
    pub fn request_nearest(&mut self,position:[f32;3]) -> bool {
        if !self.nearest_interaction_standin { return false; }
        let selected = self.states.values().filter(|d| d.phase == runtime::Phase::Closed)
            .filter_map(|d| {
                let e = &self.entries[&d.id];
                if e.policy.resolve(e.observed) != Some(Availability::Unlocked) { return None; }
                let dx=position[0]-d.position[0]; let dz=position[2]-d.position[2];
                let distance=(dx*dx+dz*dz).sqrt();
                (distance<=400.0 && (position[1]-d.position[1]).abs()<=250.0).then_some((d.id,distance))
            }).min_by(|a,b|a.1.total_cmp(&b.1)).map(|(id,_)|id);
        if let Some(id)=selected { self.request_open(id,Some(position)); true } else { false }
    }

    pub fn step(&mut self,delta:f32,actors:&[Occupant],hour:u32) {
        for entry in self.entries.values_mut() { entry.observed.hour=hour; }
        for (id,actor) in std::mem::take(&mut self.requests) {
            let Some(door)=self.states.get(&id) else { continue };
            let Some(entry)=self.entries.get(&id) else { continue };
            if entry.policy.resolve(entry.observed)!=Some(Availability::Unlocked) { continue; }
            let pair=runtime::find_pair(door,self.states.values());
            let change=self.states.get_mut(&id).unwrap().request_open(actor,true);
            if change.motion.is_some() {
                self.apply(id,change);
                if let Some(pair)=pair {
                    let change=self.states.get_mut(&pair).unwrap().request_open(actor,true);
                    self.apply(pair,change);
                    self.states.get_mut(&id).unwrap().pair=Some(pair);
                    self.states.get_mut(&pair).unwrap().pair=Some(id);
                }
            }
        }
        let ids:Vec<_>=self.states.keys().copied().collect();
        for id in ids {
            let door=&self.states[&id]; let entry=&self.entries[&id];
            let last=entry.motions.motions[door.motion].as_ref().unwrap().frame_count.saturating_sub(1) as f32;
            // Existing headless animation convention: clamp a one-shot at
            // its last authored frame. Original0x01057100 reads model flags;
            // full motion flag generation remains the animation subsystem.
            let ended=entry.frame>=last;
            let busy=runtime::occupied(door.position,actors);
            let change=self.states.get_mut(&id).unwrap().step(ended,busy);
            self.apply(id,change);
            let door=&self.states[&id]; let entry=self.entries.get_mut(&id).unwrap();
            let last=entry.motions.motions[door.motion].as_ref().unwrap().frame_count.saturating_sub(1) as f32;
            entry.frame=(entry.frame+delta).min(last);
        }
        runtime::finish_frame(&mut self.states);
    }

    fn apply(&mut self,id:Id,change:runtime::Change) {
        if change.motion.is_some() { let e=self.entries.get_mut(&id).unwrap(); e.frame=0.0; e.revision+=1; }
        if let Some(enabled)=change.collision { self.collision.enable(id,enabled); }
        // Authored sound requests are exposed by core Change; host audio
        // event replication is not wired yet.
    }
    pub fn digest(&self) -> u64 {
        let mut hash=crate::hash::StateHasher::default();
        hash.debug(&(self.stage,self.nearest_interaction_standin,&self.requests));
        for (id,door) in &self.states {
            let entry=&self.entries[id];
            hash.debug(&(door,entry.frame,entry.revision,entry.policy,entry.observed));
        }
        hash.finish()
    }
}

/// The opening's door placements are yaw-only, so Euler order cannot
/// alter their result. Refuse tilted doors until mOrder's six rotation
/// conventions are connected; never silently choose a different order.
pub fn upright_matrix(p:&Placement)->Result<[f32;16],String> {
    if p.transform.angle[0]!=0.0 || p.transform.angle[2]!=0.0 { return Err("tilted door needs mOrder evaluation".into()); }
    let (s,c)=p.transform.angle[1].sin_cos(); let scale=p.transform.scale; let pos=p.transform.position;
    Ok([c*scale[0],0.0,-s*scale[0],0.0, 0.0,scale[1],0.0,0.0,
        s*scale[2],0.0,c*scale[2],0.0, pos[0],pos[1],pos[2],1.0])
}

impl<D:Data,W:StoryWorld> Host<D,W> {
    /// Replace the base ground without losing the active door layer.
    pub fn set_ground_with_doors(&mut self,base:Box<dyn ddsim::Ground+Send>) {
        self.ground=match &self.doors { Some(doors)=>doors.collision.wrap(base),None=>base };
    }
    /// New stage/explicit loaded-cell materialization. Calling this does
    /// not infer persistent state: the loader's observations are retained.
    pub fn install_doors(&mut self,doors:Doors,base:Box<dyn ddsim::Ground+Send>) {
        self.ground=doors.collision.wrap(base); self.doors=Some(doors);
    }
    pub(crate) fn step_doors(&mut self,presses:&[crate::PlayerId]) {
        let mut actors:Vec<_>=self.sim.players.iter().map(|p|Occupant {
            position:p.position,kind:runtime::OccupantKind::Player }).collect();
        for n in self.story_npc_intels() {
            if let Some(position) = self.story_npc_position(n.npc as u32) {
                actors.push(Occupant{position,kind:runtime::OccupantKind::Npc});
            }
        }
        let Some(doors)=&mut self.doors else{return};
        for &player in presses {
            if let Some(p)=self.sim.player(player) { doors.request_nearest(p.position); }
        }
        let hour=self.story.as_ref().map_or(12,|s|s.world.hour);
        doors.step(self.clock.delta,&actors,hour);
    }
}


