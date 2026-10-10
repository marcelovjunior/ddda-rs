//! Snapshot projection only: the host owns admission, clock and completion.
use ddid::{Reader, Result, Writer};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuestCamera {
    pub stage: u32,
    pub camera_id: u32,
    pub generation: u64,
    pub seconds: f32,
    pub input_locked: bool,
}
impl QuestCamera {
    pub fn of(active: &ddhost::quest_camera::Active, input_locked: bool) -> Self {
        Self {
            stage: active.stage,
            camera_id: active.camera_id,
            generation: active.generation,
            seconds: active.seconds,
            input_locked,
        }
    }
}
pub fn write(w: &mut Writer, camera: Option<QuestCamera>) {
    // None is sent explicitly even in deltas: losing the end packet cannot
    // strand a guest in a locally completed camera or leave input locked.
    w.bool(camera.is_some());
    if let Some(c) = camera {
        w.u32(c.stage)
            .u32(c.camera_id)
            .u64(c.generation)
            .f32(c.seconds)
            .bool(c.input_locked);
    }
}
pub fn read(r: &mut Reader) -> Result<Option<QuestCamera>> {
    if !r.bool()? {
        return Ok(None);
    }
    let c = QuestCamera {
        stage: r.u32()?,
        camera_id: r.u32()?,
        generation: r.u64()?,
        seconds: r.f32()?,
        input_locked: r.bool()?,
    };
    if c.generation == 0 || !c.seconds.is_finite() || c.seconds < 0.0 {
        return Err("invalid quest camera clock/identity".into());
    }
    Ok(Some(c))
}
