//! Presentation of the authoritative opening dragon; no simulation here.
use super::{OpeningStage, Session};
use crate::{
    character::Playback,
    play::{FollowCam, PlayState},
    stage::UNIT,
};
use bevy::prelude::*;

pub(super) struct DragonView {
    owner: u32,
    entity: Entity,
    shown: Option<ddhost::ddenemyrt::body::MotionId>,
}

pub(super) fn sync(view: &mut OpeningStage, st: &PlayState, s: &mut Session) {
    let dragon = s.host.fight.as_ref().and_then(|f| f.dragon.as_ref());
    if let Some(old) = &s.dragon_view {
        if dragon.is_none_or(|d| d.actor.fighter.owner != old.owner) {
            view.commands.entity(old.entity).despawn();
            s.dragon_view = None;
        }
    }
    let Some(dragon) = dragon else {
        return;
    };
    if s.dragon_view.is_some() {
        return;
    }
    let Some(streamer) = view.streamer.as_mut() else {
        return;
    };
    let model = dragon
        .data
        .ai
        .resources
        .iter()
        .find(|r| r.class == "rModel" && r.flags & 1 != 0);
    let bank = dragon.data.body.banks.first().and_then(Option::as_ref);
    let (Some(model), Some(bank), Some(motion)) = (model, bank, dragon.actor.motion()) else {
        return;
    };
    match crate::character::spawn(
        &mut view.commands,
        &mut streamer.loader,
        &mut view.meshes,
        &mut view.materials,
        &mut view.images,
        &mut view.bindposes,
        &model.path,
        &bank.path,
        motion.slot as usize,
        (Vec3::from(dragon.actor.pos) - st.origin) * UNIT,
        Some(dragon.actor.frame),
    ) {
        Ok(entity) => {
            info!(
                "opening dragon body owner{} motion{:?} pos{:?}",
                dragon.actor.fighter.owner, motion, dragon.actor.pos
            );
            s.dragon_view = Some(DragonView {
                owner: dragon.actor.fighter.owner,
                entity,
                shown: Some(motion),
            });
        }
        Err(error) => s.failure = Some(format!("dragon body: {error}")),
    }
}

pub(super) fn show(
    st: &PlayState,
    s: &mut Session,
    roots: &mut Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
) {
    let (Some(view), Some(dragon)) = (
        &mut s.dragon_view,
        s.host.fight.as_ref().and_then(|f| f.dragon.as_ref()),
    ) else {
        return;
    };
    let Ok((mut transform, mut playback)) = roots.get_mut(view.entity) else {
        return;
    };
    transform.translation = (Vec3::from(dragon.actor.pos) - st.origin) * UNIT;
    transform.rotation = Quat::from_array(dragon.actor.rotation);
    transform.scale = Vec3::from(dragon.scale);
    if let Some(motion) = dragon.actor.motion() {
        if view.shown != Some(motion) {
            if let Some(motion_data) = dragon.data.body.motion(motion) {
                // All four entrance bindings use five-frame interpolation
                // (A6BA90/A6BF00/A60B70/A6BD80).
                playback.change_motion(motion_data.clone(), 5.0);
            }
            view.shown = Some(motion);
        }
        playback.hold_frame = Some(dragon.actor.frame);
    }
}
