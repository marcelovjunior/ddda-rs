//! Enemy bodies rebuilt from the host's resource identity and this
//! machine's install. No combat or AI runs on the guest.
use crate::{
    character::Playback,
    play::FollowCam,
    stage::{Loader, UNIT},
};
use bevy::{mesh::skinning::SkinnedMeshInverseBindposes, prelude::*};
use ddnet::mirror::{Enemy, EnemyFamily, EnemyIdentity};
use std::collections::{BTreeMap, BTreeSet};

struct View {
    identity: EnemyIdentity,
    entity: Entity,
    banks: BTreeMap<u8, mtlmt::MotionList>,
    shown: (u8, u16),
}

#[derive(Default)]
pub(crate) struct Views {
    bodies: BTreeMap<u32, View>,
    unknown: BTreeSet<u16>,
    failed: BTreeMap<u32, EnemyIdentity>,
    removed: Vec<Entity>,
}

impl Views {
    /// The authoritative list also determines removal. A replacement
    /// model at an existing owner replaces its body rather than skinning
    /// the old skeleton with the new actor's motion.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sync(
        &mut self,
        enemies: &[Enemy],
        origin: Vec3,
        commands: &mut Commands,
        loader: &mut Loader,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    ) {
        let stale: Vec<_> = self
            .bodies
            .iter()
            .filter(|(id, v)| !enemies.iter().any(|e| e.id == **id && e.identity == v.identity))
            .map(|(&id, _)| id)
            .collect();
        for id in stale {
            let v = self.bodies.remove(&id).unwrap();
            commands.entity(v.entity).despawn();
            self.removed.push(v.entity);
            self.failed.remove(&id);
        }
        self.failed
            .retain(|id, identity| enemies.iter().any(|e| e.id == *id && e.identity == *identity));
        for e in enemies {
            if let EnemyFamily::Unknown(n) = e.identity.family {
                if self.unknown.insert(n) {
                    warn!("net: unsupported enemy family {n}, owner {}", e.id);
                }
                continue;
            }
            if self.bodies.contains_key(&e.id) || self.failed.contains_key(&e.id) {
                continue;
            }
            let result = (|| -> Result<View, String> {
                let mut banks = BTreeMap::new();
                for (bank, path) in &e.identity.banks {
                    let bytes = loader.vfs.read(path, crate::character::RMOTIONLIST)?;
                    banks.insert(
                        *bank,
                        mtlmt::parse(&bytes).map_err(|err| format!("{path}: {err}"))?,
                    );
                }
                let path = e
                    .identity
                    .banks
                    .iter()
                    .find(|(bank, _)| *bank == e.bank)
                    .map(|(_, path)| path)
                    .ok_or_else(|| format!("missing motion bank {}", e.bank))?;
                let entity = crate::character::spawn(
                    commands,
                    loader,
                    meshes,
                    materials,
                    images,
                    bindposes,
                    &e.identity.model,
                    path,
                    usize::from(e.slot),
                    (Vec3::from(e.pos) - origin) * UNIT,
                    Some(e.frame),
                )?;
                Ok(View {
                    identity: e.identity.clone(),
                    entity,
                    banks,
                    shown: (e.bank, e.slot),
                })
            })();
            match result {
                Ok(v) => {
                    info!(
                        "net: enemy owner {} family {:?} body {}",
                        e.id, e.identity.family, e.identity.model
                    );
                    self.bodies.insert(e.id, v);
                }
                Err(error) => {
                    warn!("net: enemy owner {} body: {error}", e.id);
                    self.failed.insert(e.id, e.identity.clone());
                }
            }
        }
    }

    pub(crate) fn show(
        &mut self,
        enemies: &[Enemy],
        origin: Vec3,
        hidden: bool,
        roots: &mut Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
        visible: &mut std::collections::HashMap<Entity, bool>,
    ) {
        for old in self.removed.drain(..) {
            visible.remove(&old);
        }
        for e in enemies {
            let Some(v) = self.bodies.get_mut(&e.id) else {
                continue;
            };
            visible.insert(v.entity, !hidden);
            let Ok((mut transform, mut playback)) = roots.get_mut(v.entity) else {
                continue;
            };
            transform.translation = (Vec3::from(e.pos) - origin) * UNIT;
            transform.rotation = Quat::from_array(e.rotation.quaternion());
            transform.scale = Vec3::from(e.scale) * UNIT;
            let motion = v
                .banks
                .get(&e.bank)
                .and_then(|b| b.motions.get(usize::from(e.slot)))
                .and_then(Option::as_ref);
            if v.shown != (e.bank, e.slot) {
                if let Some(motion) = motion {
                    // Presentation choice: no host blend state is replicated.
                    playback.change_motion(motion.clone(), 0.0);
                }
                v.shown = (e.bank, e.slot);
            }
            playback.hold_frame = Some(motion.map_or(e.frame, |m| super::display_frame(m, e.frame)));
        }
    }
}
