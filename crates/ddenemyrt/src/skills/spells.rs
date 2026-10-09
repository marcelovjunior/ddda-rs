//! Ingle's speed shell and Levin's lightning shells. Collision and damage
//! rows are loaded from shl_param_07 and the staff collision resources.
//! Source paths: B49C50/B49DF0 (spawn), C12F70/C12D60 (speed and impact),
//! BF7A20/BF7CD0 (lightning), BD8B90/BD95B0/C13290 (collision phases).
use crate::body::{load, model_matrix, CollisionSet};
use ddcombat::events::Activation;
use ddcombat::volumes::{attack_volumes, AttackVolume, Mat4, Pose};
use ddskill::magic::Magic;

#[derive(Debug, Clone)]
pub struct Param {
    pub seq: [i32; 3],
    pub thresholds: [f32; 2],
    pub lifetime: Option<f32>,
    pub scale: f32,
    pub speed: Option<crate::shell::ShellParam>,
    pub gravity: f32,
    pub gravity_accel: f32,
    pub gravity_max: f32,
    pub gravity_distance: f32,
}

#[derive(Debug, Clone)]
pub struct SpellData {
    pub shell: Param,
    pub impact: Option<Param>,
    pub collision: CollisionSet,
    pub copies: u32,
    pub delay: f32,
    pub origin_offset: [f32; 3],
    pub origin_joint: i32,
    pub target_offset: [f32; 3],
    pub lightning: bool,
}

impl SpellData {
    pub fn load(vfs: &ddvfs::Vfs, magic: &Magic) -> Result<Self, String> {
        let bytes = load::read(vfs, crate::shell::SHELL_LIST, "rShlParamList")?;
        let list = ddenemy::ShellParamList::parse(&bytes)?;
        let level = &magic.levels[0];
        let spec = &level.shells[0];
        let group = list
            .groups
            .get(spec.group as usize)
            .and_then(Option::as_ref)
            .ok_or("missing spell group")?;
        let read = |index: usize| -> Result<(Param, Option<usize>), String> {
            let common = group
                .shells
                .get(index)
                .and_then(Option::as_ref)
                .ok_or("missing spell shell")?;
            let object = list
                .source
                .walk()
                .into_iter()
                .find(|o| o.number == common.object_number)
                .ok_or("missing shell object")?;
            let int = |name: &str| -> Result<i32, String> {
                match list.source.get(object, name).and_then(|v| v.first()) {
                    Some(mtxfs::Value::S(v)) => Ok(*v as i32),
                    _ => Err(format!("missing {name}")),
                }
            };
            let float = |name: &str| -> Result<f32, String> {
                match list.source.get(object, name).and_then(|v| v.first()) {
                    Some(mtxfs::Value::F(v)) => Ok(v[0] as f32),
                    _ => Err(format!("missing {name}")),
                }
            };
            let speed = if common.class_hash == mtarc::type_hash("cShlParamSpeed") {
                Some(crate::shell::ShellParam {
                    speed: float("spd")?,
                    speed_max: float("spdMax")?,
                    accel: float("spdAccel")?,
                    die_dist: float("dieDist")?,
                    seq: 0,
                })
            } else {
                None
            };
            let lifetime = if common.is_time_die {
                Some(common.die_frame)
            } else if common.class_hash == mtarc::type_hash("cShlParamLightningThunder") {
                Some(float("efctTime")?)
            } else {
                None
            };
            let child = int("hitShlNo").ok().filter(|n| *n >= 0).map(|n| n as usize);
            Ok((
                Param {
                    seq: [int("seq_0")?, int("seq_1")?, int("seq_2")?],
                    thresholds: [float("colTimer_0")?, float("colTimer_1")?],
                    lifetime,
                    scale: common.scale,
                    speed,
                    gravity: float("gravity").unwrap_or(0.),
                    gravity_accel: float("gravityAccel").unwrap_or(0.),
                    gravity_max: float("gravityMax").unwrap_or(0.),
                    gravity_distance: float("gravityDist").unwrap_or(f32::INFINITY),
                },
                child,
            ))
        };
        let (shell, child) = read(spec.index as usize)?;
        let impact = child.map(|i| read(i).map(|v| v.0)).transpose()?;
        let collision_path = std::str::from_utf8(&list.collision_path).map_err(|e| e.to_string())?;
        // uPlayer vf198 B5BA20 binds the staff shell list and m0007_00
        // attack resource, as for WandShot. Shells retain their own sequences.
        let collision = load::collision(vfs, collision_path, Some(r"collision\pl\m0007_00"))?;
        Ok(Self {
            lightning: shell.speed.is_none(),
            shell,
            impact,
            collision,
            copies: if level.multi_set_type == 0 {
                1
            } else {
                level.lock_on_count
            },
            delay: level.multi_shell_wait,
            origin_offset: magic.chant_offset,
            origin_joint: magic.chant_joint,
            target_offset: spec.offset,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Spell {
    pub owner: u32,
    pub shooter: u32,
    pub skill: u32,
    pub pos: [f32; 3],
    pub direction: [f32; 3],
    pub age: f32,
    pub delay: f32,
    pub distance: f32,
    pub speed: f32,
    pub gravity: f32,
    pub impact: bool,
}

impl Spell {
    fn param<'a>(&self, data: &'a SpellData) -> &'a Param {
        if self.impact {
            data.impact.as_ref().unwrap()
        } else {
            &data.shell
        }
    }
    // Translated from C12F70 (speed/gravity) and BF7CD0 (lightning age),
    // DDDA.exe 2017-12-13 Steam. Wall ray impacts are handled by the caller.
    pub fn step(&mut self, data: &SpellData, dt: f32) -> bool {
        if self.delay > 0. {
            self.delay -= dt;
            return true;
        }
        let p = self.param(data);
        self.age += dt;
        if p.lifetime.is_some_and(|life| self.age >= life) {
            return false;
        }
        if let Some(v) = p.speed {
            let delta = self.speed * dt;
            for k in 0..3 {
                self.pos[k] += self.direction[k] * delta;
            }
            if self.distance >= p.gravity_distance {
                self.pos[1] += self.gravity * dt;
                self.gravity = (self.gravity + p.gravity_accel * dt).max(p.gravity_max);
            }
            self.distance += delta;
            let speed = self.speed + v.accel * dt;
            self.speed = if v.accel < 0. {
                speed.max(v.speed_max)
            } else {
                speed.min(v.speed_max)
            };
            if self.distance >= v.die_dist {
                return false;
            }
        }
        true
    }
    pub fn volumes(&self, data: &SpellData) -> Vec<AttackVolume> {
        if self.delay > 0. {
            return vec![];
        }
        let p = self.param(data);
        // C13290 uses distance for speed shells, BD95B0 uses age for others.
        let progress = if p.speed.is_some() {
            self.distance
        } else {
            self.age
        };
        let phase = if progress < p.thresholds[0] {
            0
        } else if progress < p.thresholds[1] {
            1
        } else {
            2
        };
        let Ok(seq) = u32::try_from(p.seq[phase]) else {
            return vec![];
        };
        attack_volumes(
            &data.collision.body,
            data.collision.attacks.as_ref(),
            &[Activation {
                resource_id: data.collision.body.resource_id,
                seq,
            }],
            &At(model_matrix(self.pos, 0.)),
            p.scale,
            self.owner,
        )
    }
}
struct At(Mat4);
impl Pose for At {
    fn model_matrix(&self) -> Mat4 {
        self.0
    }
    fn joint_matrix(&self, _: i32) -> Option<Mat4> {
        None
    }
}

#[derive(Debug, Default)]
pub struct SpellWorld {
    pub shells: Vec<Spell>,
    seed: u32,
}

impl SpellWorld {
    /// Deterministic co-op RNG adapter; DDDA's RNG algorithm is unresolved.
    fn jitter(&mut self) -> f32 {
        if self.seed == 0 {
            self.seed = 0x12345678;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        // BF7A20: independent x/z uniform jitter in [-45,45).
        (self.seed as f64 / 4294967296.0) as f32 * 90. - 45.
    }
    pub fn fire(
        &mut self,
        data: &SpellData,
        skill: u32,
        shooter: u32,
        pose: &crate::body::Posed,
        target: Option<[f32; 3]>,
        next_owner: &mut u32,
    ) {
        let origin_matrix = pose.joint_matrix(data.origin_joint).unwrap_or(pose.model);
        let origin = ddcombat::volumes::transform_point(&origin_matrix, data.origin_offset);
        let aim =
            target.unwrap_or_else(|| ddcombat::volumes::transform_point(&pose.model, data.target_offset));
        for n in 0..data.copies {
            let mut pos = if data.lightning { aim } else { origin };
            if data.lightning {
                pos[0] += self.jitter();
                pos[2] += self.jitter();
            }
            let delta = [aim[0] - origin[0], aim[1] - origin[1], aim[2] - origin[2]];
            let len = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            let direction = if len > f32::EPSILON {
                delta.map(|v| v / len)
            } else {
                [pose.model[2][0], pose.model[2][1], pose.model[2][2]]
            };
            self.shells.push(Spell {
                owner: *next_owner,
                shooter,
                skill,
                pos,
                direction,
                age: 0.,
                delay: n as f32 * data.delay,
                distance: 0.,
                speed: data.shell.speed.map_or(0., |v| v.speed),
                gravity: data.shell.gravity,
                impact: false,
            });
            *next_owner += 1;
        }
    }
    pub fn step(&mut self, skills: &super::SkillSet, dt: f32) {
        self.shells.retain_mut(|s| {
            skills
                .0
                .get(&s.skill)
                .and_then(|d| d.spell.as_ref())
                .is_some_and(|d| s.step(d, dt))
        });
    }
    /// C12D60: an Ingle hit ends the speed shell and creates hitShlNo.
    pub fn hit(&mut self, owners: &[u32], skills: &super::SkillSet, next_owner: &mut u32) {
        let mut children = Vec::new();
        self.shells.retain(|s| {
            let Some(d) = skills.0.get(&s.skill).and_then(|d| d.spell.as_ref()) else {
                return false;
            };
            if !s.impact && d.shell.speed.is_some() && owners.contains(&s.owner) {
                if d.impact.is_some() {
                    let mut child = s.clone();
                    child.owner = *next_owner;
                    *next_owner += 1;
                    child.impact = true;
                    child.age = 0.;
                    child.distance = 0.;
                    child.delay = 0.;
                    children.push(child);
                }
                false
            } else {
                true
            }
        });
        self.shells.extend(children);
    }
}
