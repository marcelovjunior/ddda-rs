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
    capture_queued: bool,
    q5: bool,
}

pub fn install(world: &mut ddhost::opening::OpeningWorld, vfs:&ddvfs::Vfs, stage:u32)->Result<(),String> {
    world.cameras.get_or_insert_with(Default::default).load(vfs,stage)
}

pub fn show(
    mut commands:Commands,
    time:Res<Time>,
    pause:Res<crate::pause::PauseState>,
    state:Option<Res<PlayState>>,
    opening:Option<Res<OpeningRes>>,
    mut camera:Query<(&mut Transform,&mut Projection),With<FollowCam>>,
    bodies:Query<&Transform,Without<FollowCam>>,
    mut view:Local<View>,
    mut exit:MessageWriter<bevy::app::AppExit>,
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
        if active.stage==100 && active.camera_id==12 && active.seconds>=active.camera.duration()*0.5 && !view.capture_queued {
            if let Some(path)=std::env::var_os("DDVIEW_EVENT_CAMERA_CAPTURE") {
                commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                    .observe(bevy::render::view::screenshot::save_to_disk(std::path::PathBuf::from(path)));
                view.capture_queued=true;
                info!("opening authored camera12: midpoint screenshot requested from real playback");
            }
        }
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
    let q5=guard.host.story.as_ref().is_some_and(|s|s.world.quest_bit(2,5));
    if q5 && !view.q5 {
        info!("opening authored camera12: story observed q2:5 after resource exhaustion");
    }
    view.q5=q5;
    // Optional bounded evidence run: stop only after actual camera capture,
    // authored q5 and the return blend. No story/combat fact is injected.
    if q5 && view.capture_queued && view.end.is_none() && cameras.active.is_none()
        && std::env::var_os("DDVIEW_EVENT_CAMERA_CAPTURE").is_some() {
        exit.write(bevy::app::AppExit::Success);
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
