//! Host-owned authored quest camera clock. The renderer only consumes poses.
//! Native order 0x005c4600 observes current parameter identity; cCamImpAnim
//! 0x007b77e0 marks it ended when 0x007ba2d0 exhausts its sequence.
//! Co-op policy: the stage-scoped camera belongs to the session. Every
//! occupied seat sees the same shot and receives its authored input locks.
//! This extends the original's single stage/camera owner to N seats.

use std::collections::BTreeMap;
use mtcamera::quest::{Animation, List};

#[derive(Clone, Debug)]
pub struct Active {
    pub stage: u32,
    pub camera_id: u32,
    pub generation: u64,
    pub seconds: f32,
    pub camera: Animation,
}

#[derive(Default)]
pub struct Cameras {
    lists: BTreeMap<u32, List>,
    pub active: Option<Active>,
    pub error: Option<String>,
    generation: u64,
}

impl Cameras {
    pub fn install(&mut self, stage: u32, list: List) { self.lists.insert(stage,list); }
    pub fn load(&mut self, vfs: &ddvfs::Vfs, stage: u32) -> Result<(),String> {
        if self.lists.contains_key(&stage) { return Ok(()); }
        let path=format!("scr/st{stage:03}/etc/st{stage:03}");
        let bytes=vfs.read(&path,mtarc::type_hash("rCameraParamList"))?;
        let list=List::parse(&bytes).map_err(|e|format!("{path}: {e}"))?;
        self.install(stage,list);
        Ok(())
    }
    pub fn request(&mut self,stage:u32,camera_id:u32,simple:bool)->Result<(),String> {
        // Simple stage-4 fades and actor-relative cameras require their own
        // adapters. Missing/unsupported resources never report completion.
        if simple {return Err("simple-event camera playback is not implemented".into());}
        let camera=self.lists.get(&stage).and_then(|l|l.animations.get(camera_id as usize))
            .and_then(Option::as_ref).ok_or_else(||format!("stage {stage} camera {camera_id} is not installed Anim"))?;
        if !camera.supported_world_camera() || camera.option_flags&1==0 {
            return Err(format!("stage {stage} camera {camera_id}: unsupported targeting/sequence end"));
        }
        self.generation+=1;
        self.active=Some(Active {stage,camera_id,generation:self.generation,seconds:0.0,camera:camera.clone()});
        self.error=None;
        Ok(())
    }
    /// Advance the actual resource clock. Exact final key time is active;
    /// 0x007ba2d0 returns false strictly after the final authored key.
    pub fn step(&mut self, seconds:f32)->bool {
        let Some(active)=self.active.as_mut() else { return false };
        active.seconds+=seconds;
        if active.camera.sample(active.seconds).1 {return false;}
        self.active=None;
        true
    }
    pub fn blocks_actions(&self)->bool {
        self.active.as_ref().is_some_and(|a|a.camera.blocks_actions(a.seconds*ddsim::MOTION_FPS))
    }
}
