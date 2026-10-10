//! Thin presentation for the headless, session-owned event camera.
//! Original sources: cCamImpAnim 0x007b77e0, camera transitions
//! 0x007e0960/0x007ddef0/0x007e3c70 (DDDA.exe 2017-12-13 Steam).

use bevy::prelude::*;
use mtcamera::quest::{Blend, Pose, ease};
use crate::{opening::OpeningRes, play::{FollowCam, PlayState}, stage::UNIT};

#[derive(Default)]
pub struct View {
    generation: Option<u64>,
    entry: Option<Pose>,
    last: Option<Pose>,
    end: Option<Blend>,
    return_frames: f32,
    projection: Option<Projection>,
}

pub fn install(world: &mut ddhost::opening::OpeningWorld, vfs:&ddvfs::Vfs, stage:u32)->Result<(),String> {
    world.cameras.get_or_insert_with(Default::default).load(vfs,stage)
}

pub fn show(
    time:Res<Time>,
    pause:Res<crate::pause::PauseState>,
    state:Option<Res<PlayState>>,
    opening:Option<Res<OpeningRes>>,
    mut camera:Query<(&mut Transform,&mut Projection),With<FollowCam>>,
    bodies:Query<&Transform,Without<FollowCam>>,
    mut view:Local<View>,
) {
    let (Some(state),Some(opening))=(state,opening) else{return};
    let guard=opening.0.lock().unwrap();
    let Some(cameras)=&guard.host.world.cameras else{return};
    let Ok((mut transform,mut projection))=camera.single_mut() else{return};
    let Some(body)=state.entities.get(&state.follow).and_then(|e|bodies.get(*e).ok()) else{return};
    // Exactly the current follow adapter's target, using the same shown
    // (interpolated) body as play::show, not an invented unit-length ray.
    let target=body.translation/UNIT+state.origin+Vec3::Y*crate::play::CAMERA_TARGET_HEIGHT;
    let gameplay=read_pose(&transform,&projection,state.origin,target);
    let Some(gameplay)=gameplay else{return};
    if let Some(active)=&cameras.active {
        if view.generation!=Some(active.generation) {
            view.generation=Some(active.generation);
            view.entry=Some(gameplay);
            view.projection=Some(projection.clone());
            view.return_frames=0.0;
            info!("opening authored camera: stage{} id{} generation{} duration{}s",
                active.stage,active.camera_id,active.generation,active.camera.duration());
        }
        let (mut pose,_)=active.camera.sample(active.seconds);
        let blend=&active.camera.start;
        if blend.frames>0.0 && active.seconds*ddsim::MOTION_FPS<blend.frames {
            let weight=ease(&blend.curve,(active.seconds*ddsim::MOTION_FPS/blend.frames).clamp(0.0,1.0));
            if let Some(entry)=view.entry {
                pose=entry.interpolate(pose,weight,blend.kind).unwrap_or(pose);
            }
        }
        view.last=Some(pose);view.end=Some(active.camera.end.clone());
        apply(pose,state.origin,&mut transform,&mut projection);
    } else if view.generation.take().is_some() || view.end.is_some() {
        if !pause.paused {view.return_frames+=time.delta_secs()*ddsim::MOTION_FPS;}
        if let Some(saved)=&view.projection { *projection=saved.clone(); }
        let gameplay=read_pose(&transform,&projection,state.origin,target).unwrap_or(gameplay);
        let blend=view.end.as_ref().unwrap();
        let weight=if blend.frames>0.0 {ease(&blend.curve,(view.return_frames/blend.frames).clamp(0.0,1.0))} else {1.0};
        if weight<1.0 {
            let pose=view.last.unwrap().interpolate(gameplay,weight,blend.kind).unwrap_or(gameplay);
            apply(pose,state.origin,&mut transform,&mut projection);
        } else {
            view.end=None;view.entry=None;view.last=None;view.projection=None;
            info!("opening authored camera: return transition complete");
        }
    }
}

fn read_pose(t:&Transform,projection:&Projection,origin:Vec3,target:Vec3)->Option<Pose> {
    let Projection::Perspective(p)=projection else{return None};
    let eye=t.translation/UNIT+origin;
    Some(Pose {eye:eye.to_array(),target:target.to_array(),roll_degrees:0.0,fov_degrees:p.fov.to_degrees()})
}

fn apply(pose:Pose,origin:Vec3,t:&mut Transform,projection:&mut Projection) {
    let eye=(Vec3::from(pose.eye)-origin)*UNIT;
    let target=(Vec3::from(pose.target)-origin)*UNIT;
    *t=Transform::from_translation(eye).looking_at(target,Vec3::Y);
    t.rotation*=Quat::from_rotation_z(-pose.roll_degrees.to_radians());
    if let Projection::Perspective(p)=projection {p.fov=pose.fov_degrees.to_radians();}
}
