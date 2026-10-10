//! Host-owned authored quest camera clock. The renderer only consumes poses.
//! Native order 0x005c4600 observes current parameter identity; cCamImpAnim
//! 0x007b77e0 marks it ended when 0x007ba2d0 exhausts its sequence.
//! Co-op policy: the stage-scoped camera belongs to the session. Every
//! occupied seat receives its authored input locks. Local presentation uses
//! the same shot; remote camera snapshot replication remains unimplemented.
//! This extends the original's single stage/camera owner to N seats.

use mtcamera::quest::{Animation, List};
use std::collections::BTreeMap;

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
    stage_end_pending: bool,
}

impl Cameras {
    pub fn install(&mut self, stage: u32, list: List) {
        self.lists.insert(stage, list);
    }
    pub fn load(&mut self, vfs: &ddvfs::Vfs, stage: u32) -> Result<(), String> {
        self.enter_stage(stage);
        if self.lists.contains_key(&stage) {
            return Ok(());
        }
        let path = format!("scr/st{stage:03}/etc/st{stage:03}");
        let bytes = vfs.read(&path, mtarc::type_hash("rCameraParamList"))?;
        let list = List::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
        self.install(stage, list);
        Ok(())
    }
    /// 0x005c4600 ends an observed camera when the stage/current camera
    /// disappears; 0x007dfd40 compares identity with the CURRENT stage's
    /// parameter list. A previous-stage camera cannot remain its owner.
    pub fn enter_stage(&mut self, stage: u32) {
        if self.active.as_ref().is_some_and(|active| active.stage != stage) {
            self.active = None;
            self.stage_end_pending = true;
            self.error = None;
        }
    }
    pub fn request(&mut self, stage: u32, camera_id: u32, simple: bool) -> Result<(), String> {
        // Simple stage-4 fades and actor-relative cameras require their own
        // adapters. Missing/unsupported resources never report completion.
        if simple {
            return Err("simple-event camera playback is not implemented".into());
        }
        let camera = self
            .lists
            .get(&stage)
            .and_then(|l| l.animations.get(camera_id as usize))
            .and_then(Option::as_ref)
            .ok_or_else(|| format!("stage {stage} camera {camera_id} is not installed Anim"))?;
        if !camera.supported_world_camera() || camera.option_flags & 1 == 0 {
            return Err(format!(
                "stage {stage} camera {camera_id}: unsupported targeting, blend, input domains or sequence end"
            ));
        }
        if self.active.is_some() {
            return Err("overlapping camera owners need per-owner identity completion".into());
        }
        self.generation += 1;
        self.stage_end_pending = false;
        self.active = Some(Active {
            stage,
            camera_id,
            generation: self.generation,
            seconds: 0.0,
            camera: camera.clone(),
        });
        self.error = None;
        Ok(())
    }
    /// Advance the actual resource clock. Exact final key time is active;
    /// 0x007ba2d0 returns false strictly after the final authored key.
    pub fn step(&mut self, seconds: f32) -> bool {
        if std::mem::take(&mut self.stage_end_pending) {
            return true;
        }
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        active.seconds += seconds;
        if active.camera.sample(active.seconds).1 {
            return false;
        }
        self.active = None;
        true
    }
    pub fn blocks_actions(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|a| a.camera.blocks_actions(a.seconds * ddsim::MOTION_FPS))
    }
}
