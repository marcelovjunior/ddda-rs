//! Enemy bodies rebuilt from the host's resource identity and this
//! machine's install. No combat or AI runs on the guest.
use crate::{
    character::Playback,
    play::FollowCam,
    stage::{Loader, UNIT},
};
use bevy::{mesh::skinning::SkinnedMeshInverseBindposes, prelude::*};
use ddnet::mirror::{Enemy, EnemyFamily, EnemyIdentity};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

struct View {
    identity: EnemyIdentity,
    entity: Entity,
    banks: Arc<BTreeMap<u8, mtlmt::MotionList>>,
    shown: (u8, u16),
}

#[derive(Default)]
pub(crate) struct Views {
    bodies: BTreeMap<u32, View>,
    unknown: BTreeSet<u16>,
    failed: BTreeMap<u32, (EnemyIdentity, u8, u16)>,
    removed: Vec<Entity>,
    banks: BTreeMap<Vec<(u8, String)>, Arc<BTreeMap<u8, mtlmt::MotionList>>>,
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
        self.retire(enemies, commands);
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
                let banks = match self.banks.get(&e.identity.banks) {
                    Some(banks) => banks.clone(),
                    None => {
                        let mut banks = BTreeMap::new();
                        for (bank, path) in &e.identity.banks {
                            let bytes = loader.vfs.read(path, crate::character::RMOTIONLIST)?;
                            banks.insert(
                                *bank,
                                mtlmt::parse(&bytes).map_err(|err| format!("{path}: {err}"))?,
                            );
                        }
                        let banks = Arc::new(banks);
                        self.banks.insert(e.identity.banks.clone(), banks.clone());
                        banks
                    }
                };
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
                commands.entity(entity).insert(transform(e, origin));
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
                    self.failed.insert(e.id, (e.identity.clone(), e.bank, e.slot));
                }
            }
        }
    }

    fn retire(&mut self, enemies: &[Enemy], commands: &mut Commands) {
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
        self.failed.retain(|id, (identity, bank, slot)| {
            enemies
                .iter()
                .any(|e| e.id == *id && e.identity == *identity && e.bank == *bank && e.slot == *slot)
        });
        self.banks
            .retain(|key, _| enemies.iter().any(|e| &e.identity.banks == key));
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
            *transform = self::transform(e, origin);
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

/// Character roots hold the cm-to-m conversion; model joints stay in cm.
fn transform(e: &Enemy, origin: Vec3) -> Transform {
    Transform::from_translation((Vec3::from(e.pos) - origin) * UNIT)
        .with_rotation(Quat::from_array(e.rotation.quaternion()))
        .with_scale(Vec3::from(e.scale) * UNIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::SystemState;
    use ddnet::mirror::EnemyRotation;

    fn playback() -> Playback {
        Playback {
            motion: mtlmt::Motion {
                frame_count: 10,
                loop_frame: -1,
                end_position: [0.0; 4],
                end_rotation: [0.0, 0.0, 0.0, 1.0],
                flags: 0,
                tracks: vec![],
            },
            joints: Default::default(),
            hold_frame: None,
            event_frame: None,
            blend_request: None,
            blend: None,
            morph: None,
            drawn: false,
            upper: None,
        }
    }
    fn enemy(id: u32, family: EnemyFamily, pos: [f32; 3]) -> Enemy {
        Enemy {
            id,
            identity: EnemyIdentity {
                family,
                model: "fixture".into(),
                banks: vec![],
            },
            pos,
            rotation: EnemyRotation::Yaw(0.0),
            scale: [1.0; 3],
            action: 0,
            bank: 0,
            slot: 0,
            frame: 3.0,
            hp: 25.0,
            hp_max: 100.0,
            dead: false,
        }
    }

    #[test]
    fn guest_pose_is_keyed_by_owner_and_preserves_quaternion_and_cm_scale() {
        let mut world = World::new();
        let goblin_entity = world.spawn((Transform::default(), playback())).id();
        let dragon_entity = world.spawn((Transform::default(), playback())).id();
        let goblin = enemy(100, EnemyFamily::Goblin, [200.0, 300.0, 400.0]);
        let mut dragon = enemy(101, EnemyFamily::Dragon, [500.0, 600.0, 700.0]);
        let rotation = Quat::from_euler(EulerRot::XYZ, 0.3, 0.8, -0.4);
        dragon.rotation = EnemyRotation::Quaternion(rotation.to_array());
        dragon.scale = [1.0, 2.0, 3.0];
        let mut views = Views::default();
        for (e, entity) in [(&goblin, goblin_entity), (&dragon, dragon_entity)] {
            views.bodies.insert(
                e.id,
                View {
                    identity: e.identity.clone(),
                    entity,
                    banks: Arc::new(BTreeMap::new()),
                    shown: (0, 0),
                },
            );
        }
        let mut query: SystemState<Query<(&mut Transform, &mut Playback), Without<FollowCam>>> =
            SystemState::new(&mut world);
        let mut visible = std::collections::HashMap::new();
        // Reverse the snapshot order: ownership cannot depend on the old
        // viewer's zip with preallocated goblin entities.
        views.show(
            &[dragon.clone(), goblin],
            Vec3::new(100.0, 0.0, 100.0),
            false,
            &mut query.get_mut(&mut world).unwrap(),
            &mut visible,
        );
        let transform = world.get::<Transform>(dragon_entity).unwrap();
        assert_eq!(transform.translation, Vec3::new(400.0, 600.0, 600.0) * UNIT);
        assert_eq!(transform.scale, Vec3::new(1.0, 2.0, 3.0) * UNIT);
        assert_eq!(transform.rotation, rotation);
        assert_eq!(
            world.get::<Transform>(goblin_entity).unwrap().translation,
            Vec3::new(100.0, 300.0, 300.0) * UNIT
        );
        assert_eq!(
            world.get::<Playback>(dragon_entity).unwrap().hold_frame,
            Some(3.0)
        );
        assert_eq!(visible[&dragon_entity], true);
        views.show(
            &[dragon],
            Vec3::ZERO,
            true,
            &mut query.get_mut(&mut world).unwrap(),
            &mut visible,
        );
        assert_eq!(
            visible[&dragon_entity], false,
            "cutscene hides the guest enemy body"
        );
    }
    #[test]
    fn removed_or_replaced_identity_despawns_its_body_and_children() {
        let mut world = World::new();
        let old = world.spawn_empty().id();
        let child = world.spawn(ChildOf(old)).id();
        let retained = world.spawn_empty().id();
        let a = enemy(100, EnemyFamily::Goblin, [0.0; 3]);
        let b = enemy(101, EnemyFamily::Dragon, [0.0; 3]);
        let mut views = Views::default();
        for (e, entity) in [(&a, old), (&b, retained)] {
            views.bodies.insert(
                e.id,
                View {
                    identity: e.identity.clone(),
                    entity,
                    banks: Arc::new(BTreeMap::new()),
                    shown: (0, 0),
                },
            );
        }
        let mut replacement = a;
        replacement.identity.model = "replacement".into();
        let mut state: SystemState<Commands> = SystemState::new(&mut world);
        views.retire(&[b, replacement], &mut state.get_mut(&mut world).unwrap());
        state.apply(&mut world);
        assert!(world.get_entity(old).is_err());
        assert!(world.get_entity(child).is_err());
        assert!(world.get_entity(retained).is_ok());
        assert!(!views.bodies.contains_key(&100));
        assert!(views.bodies.contains_key(&101));
        views.retire(&[], &mut state.get_mut(&mut world).unwrap());
        state.apply(&mut world);
        assert!(world.get_entity(retained).is_err());
        assert!(views.bodies.is_empty());
    }
}
