//! Basic shortbow arrows, separate from the staff's homing shell.
//!
//! Parameters and collision rows are read from the player's installation.
//! Callers supply the posed bow-joint origin and resolved camera aim target
//! (`0x00B1D010` / `0x00B1D640`), never a guessed offset from the feet.
//! The action scheduler, accuracy cone and world collision belong to the
//! caller; this module models ordinary, unenchanted arrow flight only.

use crate::body::{CollisionSet, load, model_matrix};
use ddcombat::damage::AttackRow;
use ddcombat::events::Activation;
use ddcombat::volumes::{AttackVolume, Mat4, Pose, Volume, attack_volumes};

/// Secondary category 10 in uPlayerBase::vf198, 0x00B5BA20.
pub const PARAM_PATH: &str = r"param\shl_param_06";
/// Source record selected by 0x00B16DC0; the basic action is row zero.
pub const BOW_PARAM_PATH: &str = r"param\bow_act_param_06";

/// Runtime ordered basic shoot-controller projection. Charging and authored
/// angle/spread paths are rejected until their separate source translation.
pub fn load_shot_param(vfs: &ddvfs::Vfs) -> Result<ddsim::BowShotParam, String> {
    use mtxfs::Value;
    let bytes = load::read(vfs, BOW_PARAM_PATH, "rBowActParamList")?;
    let x = mtxfs::parse(&bytes).map_err(|e| e.to_string())?;
    let object = |o: &mtxfs::Object, name: &str| match x.get(o, name).and_then(|v| v.first()) {
        Some(Value::Object(Some(o))) => Ok((**o).clone()),
        _ => Err(format!("bow has no {name}")),
    };
    let rows = object(&x.root, "mParamList")?;
    let basic = object(&rows, "mpArray")?;
    let f = |o: &mtxfs::Object, name: &str| match x.get(o, name).and_then(|v| v.first()) {
        Some(Value::F(v)) if v.first().is_some_and(|v| v.is_finite()) => Ok(v[0] as f32),
        _ => Err(format!("bow has no finite {name}")),
    };
    let u = |o: &mtxfs::Object, name: &str| match x.get(o, name).and_then(|v| v.first()) {
        Some(Value::U(v)) => u32::try_from(*v).map_err(|_| format!("bad bow {name}")),
        Some(Value::S(v)) => u32::try_from(*v).map_err(|_| format!("negative bow {name}")),
        _ => Err(format!("bow has no {name}")),
    };
    for name in ["maxChargeLv", "usableLv", "shootType", "shootDirType"] {
        if u(&basic, name)? != 0 {
            return Err(format!("unsupported basic bow {name}"));
        }
    }
    if !matches!(
        x.get(&basic, "isShotRetNml").and_then(|v| v.first()),
        Some(Value::Bool(false))
    ) {
        return Err("unsupported basic bow return-normal shot".into());
    }
    let shoot_list = object(&basic, "shootCtrlList")?;
    let mut shoots = Vec::new();
    for row in x
        .get(&shoot_list, "mpArray")
        .ok_or("bow has no shoot-controller array")?
    {
        let Value::Object(Some(row)) = row else {
            return Err("null bow shoot row".into());
        };
        let angle = [f(row, "degX")?, f(row, "degY")?];
        let spread = [f(row, "degRndX")?, f(row, "degRndY")?];
        if angle != [0.0; 2] || spread != [0.0; 2] {
            return Err("unsupported authored bow angle/spread".into());
        }
        shoots.push(ddsim::attack::BowShotRow {
            wait_frame: f(row, "waitFrame")?,
            charge_level: u(row, "chargeLv")?,
            group: u(row, "shlGroup")?,
            index: u(row, "shlNo")?,
            angle,
            spread,
            shell_wait: f(row, "shlWait")?,
        });
    }
    let adjust = load::read(vfs, r"param\pl\other\m_pl000_00", "rAdjustParam")?;
    // Source vf26 0x00B1C2C0 requests adjust row 0x1BC; ajp header +12.
    if adjust.get(..4) != Some(b"ajp\0".as_slice()) {
        return Err("bad bow adjust file".into());
    }
    let count = adjust.get(8..12).ok_or("short bow adjust header")?;
    if u32::from_le_bytes(count.try_into().unwrap()) <= 0x1BC {
        return Err("missing bow stamina adjust".into());
    }
    let stamina = adjust
        .get(12 + 4 * 0x1BC..12 + 4 * 0x1BD)
        .ok_or("short bow stamina adjust")?;
    let byte = |name| u8::try_from(u(&basic, name)?).map_err(|_| format!("bad bow byte {name}"));
    Ok(ddsim::BowShotParam {
        upper: byte("motWaitShot_Up")?,
        lower: byte("motWaitShot_Low")?,
        reload_speed: f(&basic, "reloadSpeed")?,
        shot_wait: f(&basic, "shotWait")?,
        charge_begin_frame: f(&basic, "chargeBgnFrameShot")?,
        stamina: f(&basic, "stamina")?,
        stamina_attack_rate: f(&basic, "staminaAtckAddRate")?,
        weapon_stamina: f32::from_le_bytes(stamina.try_into().unwrap()),
        accuracy_wait: f(&basic, "accurateWait")?,
        rows: shoots,
    })
}

/// Normal arrow's supplemental row base, small override table 0x015F76B8.
const NORMAL_ARROW_ROW: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowParam {
    pub speed: f32,
    pub speed_max: f32,
    pub acceleration: f32,
    pub gravity: f32,
    pub gravity_min: f32,
    pub gravity_acceleration: f32,
    pub gravity_distance: f32,
    pub death_distance: f32,
    pub death_flags: u32,
    pub sequences: [u32; 3],
    pub collision_distances: [f32; 2],
    pub repelled_distance: f32,
}

impl ArrowParam {
    /// cShlParamSpeed reflection 0x00C137F0; cShlParamArrow 0x00BCFE50.
    /// Rejects optional flight branches this module does not execute.
    pub fn from_list(bytes: &[u8], group: usize, index: usize) -> Result<Self, String> {
        let x = mtxfs::parse(bytes).map_err(|e| e.to_string())?;
        let object = |o: &mtxfs::Object, name: &str, i: usize| match x.get(o, name).and_then(|v| v.get(i)) {
            Some(mtxfs::Value::Object(Some(c))) => Ok((**c).clone()),
            _ => Err(format!("no {name}[{i}]")),
        };
        let list = object(&x.root, "mParamList", 0)?;
        let group = object(&list, "mpArray", group)?;
        let list = object(&group, "mShlList", 0)?;
        let entry = object(&list, "mpArray", index)?;
        if x.class_of(&entry).name() != Some("cShlParamArrow") {
            return Err("unsupported arrow parameter class".into());
        }
        let f = |name: &str| match x.get(&entry, name).and_then(|v| v.first()) {
            Some(mtxfs::Value::F(v)) => Ok(v[0] as f32),
            _ => Err(format!("no arrow {name}")),
        };
        let u = |name: &str| match x.get(&entry, name).and_then(|v| v.first()) {
            Some(mtxfs::Value::U(v)) => u32::try_from(*v).map_err(|_| format!("bad {name}")),
            Some(mtxfs::Value::S(v)) => u32::try_from(*v).map_err(|_| format!("negative {name}")),
            _ => Err(format!("no arrow {name}")),
        };
        for field in [
            "isAutoLockon",
            "isFuwaMove",
            "isPadCtrl",
            "isRestrict",
            "isSoften",
            "isHitAbsorb",
            "isDieTrigerOff",
            "isStingWall",
        ] {
            if !matches!(
                x.get(&entry, field).and_then(|v| v.first()),
                Some(mtxfs::Value::Bool(false))
            ) {
                return Err(format!("unsupported arrow {field}"));
            }
        }
        Ok(Self {
            speed: f("spd")?,
            speed_max: f("spdMax")?,
            acceleration: f("spdAccel")?,
            gravity: f("gravity")?,
            gravity_min: f("gravityMax")?,
            gravity_acceleration: f("gravityAccel")?,
            gravity_distance: f("gravityDist")?,
            death_distance: f("dieDist")?,
            death_flags: u("dieFlag")?,
            sequences: [u("seq_0")?, u("seq_1")?, u("seq_2")?],
            collision_distances: [f("colTimer_0")?, f("colTimer_1")?],
            repelled_distance: f("repelledDist")?,
        })
    }

    /// Translated from uShlSpeed::vf112, 0x00C13290 (DDDA.exe 2017-12-13 Steam).
    pub fn sequence(&self, distance: f32) -> u32 {
        self.sequences[self.phase(distance)]
    }

    fn phase(&self, distance: f32) -> usize {
        if distance < self.collision_distances[0] {
            0
        } else if distance < self.collision_distances[1] {
            1
        } else {
            2
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArrowData {
    pub param: ArrowParam,
    pub collision: CollisionSet,
    /// Basic arrow supplements from the installed paired collision table.
    pub supplemental: [AttackRow; 3],
}

impl ArrowData {
    /// Basic shot group/index 0/0 from bow_act_param_06 row 0. Collision
    /// resource selection follows mCollisionPath; the paired `_00` attack
    /// list is the ordinary shortbow list attached by the actor.
    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let bytes = load::read(vfs, PARAM_PATH, "rShlParamList")?;
        let x = mtxfs::parse(&bytes).map_err(|e| e.to_string())?;
        let path = match x.get(&x.root, "mCollisionPath").and_then(|v| v.first()) {
            Some(mtxfs::Value::Str(p)) => std::str::from_utf8(p).map_err(|e| e.to_string())?,
            _ => return Err("arrow list has no mCollisionPath".into()),
        };
        let collision = load::collision(vfs, path, Some(&format!("{path}_00")))?;
        let table = &collision
            .attacks
            .as_ref()
            .ok_or("missing arrow attack table")?
            .attacks;
        let supplemental = [0, 1, 2].map(|phase| {
            table
                .get(NORMAL_ARROW_ROW + phase)
                .and_then(Option::as_ref)
                .map(AttackRow::from_attack)
                .ok_or_else(|| format!("missing normal arrow row {}", NORMAL_ARROW_ROW + phase))
        });
        let [a, b, c] = supplemental;
        Ok(Self {
            param: ArrowParam::from_list(&bytes, 0, 0)?,
            collision,
            supplemental: [a?, b?, c?],
        })
    }

    /// Basic unenchanted arrow override translated from 0x00BCF1E0
    /// (DDDA.exe 2017-12-13 Steam), before player attack valuation.
    /// The basic action's stamina additive is zero; special arrows and
    /// nonzero shell attack multipliers require their separate paths.
    pub fn project_attack(&self, mut row: AttackRow, distance: f32) -> AttackRow {
        let extra = self.supplemental[self.param.phase(distance)];
        row.attack = extra.attack + row.attack;
        if extra.shrink_type != 0 {
            row.shrink_type = extra.shrink_type;
            row.shrink_attack = extra.shrink_attack;
        }
        if extra.blow_type != 3 {
            row.blow_type = extra.blow_type;
            row.blow_attack = extra.blow_attack;
        }
        for slot in [0, 2, 3, 4, 5, 10] {
            row.status[slot] = extra.status[slot];
        }
        if distance >= self.param.repelled_distance {
            row.attack = 0.0;
            row.shrink_type = 0;
            row.shrink_attack = 0.0;
            row.blow_type = 3;
            row.blow_attack = 0.0;
        }
        row
    }
}

/// Source action inputs: current bow joint, the resolved world aim target,
/// current smoothed accuracy and two independent engine RNG words.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowAim {
    pub bow_origin: [f32; 3],
    pub target: [f32; 3],
    pub accuracy: f32,
    pub cone_random: u32,
    pub polar_random: u32,
    /// Augment 0x48 skips the additional accuracy cone (0x00B1DCB0).
    pub steady: bool,
}

impl ArrowAim {
    /// Basic row's zero authored angle/spread branch, translated from
    /// 0x00B1D640 / 0x00B1DCB0 and basis construction 0x007AF5B0.
    /// The parallel-up fallback of 0x007AEED0 is not modelled: exact
    /// vertical aim returns None rather than inventing a basis.
    pub fn direction(&self) -> Option<[f32; 3]> {
        let d = std::array::from_fn::<_, 3, _>(|k| self.target[k] - self.bow_origin[k]);
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if length <= f32::EPSILON || !length.is_finite() || !self.accuracy.is_finite() {
            return None;
        }
        let forward = d.map(|v| v / length);
        if self.steady {
            return Some(forward);
        }
        let horizontal = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
        if horizontal * horizontal < f32::EPSILON {
            return None;
        }
        let right = [forward[2] / horizontal, 0.0, -forward[0] / horizontal];
        let up = [
            forward[1] * right[2],
            forward[2] * right[0] - forward[0] * right[2],
            -forward[1] * right[0],
        ];
        // 0x007AEE90 maps the engine's u32 word with 2^-32; vf18 is zero
        // for cPlActWpnBow (0x00B16AE0), so cone limit is accuracy * 4.
        let limit = self.accuracy * 4.0;
        let theta = (-f64::from(limit)
            + f64::from(self.cone_random) * (1.0 / 4294967296.0) * f64::from(limit * 2.0))
            as f32;
        let polar = (-180.0 + f64::from(self.polar_random) * (1.0 / 4294967296.0) * 360.0) as f32;
        let (s, c) = theta.to_radians().sin_cos();
        let (sp, cp) = polar.to_radians().sin_cos();
        Some(std::array::from_fn(|k| {
            right[k] * (sp * s) + up[k] * (-cp * s) + forward[k] * c
        }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub owner: u32,
    pub shooter: u32,
    pub position: [f32; 3],
    pub previous_position: [f32; 3],
    pub direction: [f32; 3],
    pub speed: f32,
    pub gravity: f32,
    pub distance: f32,
    /// Runtime augment 0x46 value, passed explicitly. No assumed default.
    pub range_multiplier: f32,
    pub alive: bool,
}

impl Arrow {
    pub fn aimed(
        p: &ArrowParam,
        owner: u32,
        shooter: u32,
        aim: ArrowAim,
        range_multiplier: f32,
    ) -> Option<Self> {
        Self::fire(
            p,
            owner,
            shooter,
            aim.bow_origin,
            aim.direction()?,
            range_multiplier,
        )
    }
    /// Boundary for a normal-input scheduler event. The caller resolves the
    /// actual posed bow model's joint zero (0x00B1D640 / 0x00421ED0), aim
    /// target and independent RNG words. Unsupported rows are observable.
    pub fn from_shot(
        p: &ArrowParam,
        owner: u32,
        shooter: u32,
        shot: ddsim::BowShot,
        bow_pose: &dyn Pose,
        target: [f32; 3],
        random: [u32; 2],
        steady: bool,
        range_multiplier: f32,
    ) -> Result<Self, String> {
        if shot.row.group != 0
            || shot.row.index != 0
            || shot.row.angle != [0.0; 2]
            || shot.row.spread != [0.0; 2]
            || shot.row.shell_wait != 0.0
            || shot.stamina_attack != 0.0
        {
            return Err("unsupported nonbasic arrow shot".into());
        }
        let matrix = bow_pose.joint_matrix(0).ok_or("missing actual bow joint zero")?;
        Self::aimed(
            p,
            owner,
            shooter,
            ArrowAim {
                bow_origin: [matrix[3][0], matrix[3][1], matrix[3][2]],
                target,
                accuracy: shot.accuracy,
                cone_random: random[0],
                polar_random: random[1],
                steady,
            },
            range_multiplier,
        )
        .ok_or_else(|| "unsupported arrow aim".into())
    }

    /// Attack-only adapter for the ordinary hit pass. The caller supplies
    /// the secondary-bow valued shooter; primary dagger stats must not be
    /// used. Projectile owner/history remain independent for N shooters.
    pub fn fighter(
        &self,
        data: &ArrowData,
        secondary_shooter: &crate::combat::Fighter,
    ) -> crate::combat::Fighter {
        let mut fighter = secondary_shooter.clone();
        fighter.owner = self.owner;
        fighter.attacks = self.volumes(data);
        fighter.hurts.clear();
        fighter.history = Default::default();
        fighter.statuses = Default::default();
        fighter.dots = Default::default();
        fighter.pools = Default::default();
        fighter
    }

    /// The origin and direction already resolved by the shot action.
    /// Initialization translated from 0x00C12240 / 0x00BD6D20.
    pub fn fire(
        p: &ArrowParam,
        owner: u32,
        shooter: u32,
        origin: [f32; 3],
        direction: [f32; 3],
        range_multiplier: f32,
    ) -> Option<Self> {
        let length =
            (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2]).sqrt();
        if !length.is_finite()
            || length <= f32::EPSILON
            || !range_multiplier.is_finite()
            || range_multiplier < 0.0
            || !origin.iter().all(|v| v.is_finite())
        {
            return None;
        }
        Some(Self {
            owner,
            shooter,
            position: origin,
            previous_position: origin,
            direction: direction.map(|v| v / length),
            speed: p.speed,
            gravity: p.gravity,
            distance: 0.0,
            range_multiplier,
            alive: true,
        })
    }

    /// Normal, nonhoming movement translated from 0x00C12F70; death check
    /// before movement from 0x00C12D60 (DDDA.exe 2017-12-13 Steam).
    /// `frames` is the engine's frame delta. Hit slow, target steering,
    /// scripted position overrides and world contact require other paths.
    pub fn step(&mut self, p: &ArrowParam, frames: f32) -> bool {
        if !self.alive || frames <= 0.0 {
            return self.alive;
        }
        if p.death_flags & 1 != 0 && self.distance >= p.death_distance * self.range_multiplier {
            self.alive = false;
            return false;
        }
        self.previous_position = self.position;
        let mut delta = self.direction.map(|v| v * (self.speed * frames));
        if p.gravity_distance <= self.distance {
            delta[1] += self.gravity * frames;
            self.gravity = (self.gravity + frames * p.gravity_acceleration).max(p.gravity_min);
        }
        for (coordinate, change) in self.position.iter_mut().zip(delta) {
            *coordinate += change;
        }
        self.distance += (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
        if p.acceleration < 0.0 {
            self.speed = (self.speed + frames * p.acceleration).max(p.speed_max);
        } else if p.acceleration > 0.0 {
            self.speed = (self.speed + frames * p.acceleration).min(p.speed_max);
        }
        true
    }

    /// Hit/world death bits translated from 0x00C12D60. The caller passes
    /// an accepted contact, not merely a broad-phase overlap.
    pub fn contact(&mut self, p: &ArrowParam, actor_hit: bool, world_hit: bool) {
        if (actor_hit && p.death_flags & 2 != 0) || (world_hit && p.death_flags & 4 != 0) {
            self.alive = false;
        }
    }

    /// Current attack sequence, using the installed geometry and attack row.
    /// Shell collision projection: 0x00BD8C60 / 0x00BD8E40. A moving
    /// sphere becomes a capsule from its current to previous world point
    /// (0x00BD8FA1..0x00BD9057, vf113 0x00BCC870, joint -2 identity
    /// 0x00CCE190). The special world-hit +0x2704 bit 4 branch is excluded.
    pub fn volumes(&self, data: &ArrowData) -> Vec<AttackVolume> {
        if !self.alive {
            return Vec::new();
        }
        let mut volumes = attack_volumes(
            &data.collision.body,
            data.collision.attacks.as_ref(),
            &[Activation {
                resource_id: data.collision.body.resource_id,
                seq: data.param.sequence(self.distance),
            }],
            &ArrowPose(model_matrix(self.position, 0.0)),
            1.0,
            self.owner,
        );
        for volume in &mut volumes {
            volume.row = data.project_attack(volume.row, self.distance);
            let delta = std::array::from_fn::<_, 3, _>(|k| self.position[k] - self.previous_position[k]);
            if delta.iter().map(|v| v * v).sum::<f32>() > f32::EPSILON {
                if let Volume::Sphere { c, r } = volume.volume {
                    volume.volume = Volume::Capsule {
                        p0: c,
                        p1: std::array::from_fn(|k| c[k] - delta[k]),
                        r,
                    };
                }
            }
        }
        volumes
    }
}

struct ArrowPose(Mat4);
impl Pose for ArrowPose {
    fn model_matrix(&self) -> Mat4 {
        self.0
    }
    fn joint_matrix(&self, _: i32) -> Option<Mat4> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Synthetic fixture, not an embedded installed arrow record.
    fn p() -> ArrowParam {
        ArrowParam {
            speed: 12.0,
            speed_max: 8.0,
            acceleration: -2.0,
            gravity: 3.0,
            gravity_min: -1.0,
            gravity_acceleration: -2.0,
            gravity_distance: 0.0,
            death_distance: 24.0,
            death_flags: 7,
            sequences: [11, 12, 13],
            collision_distances: [8.0, 20.0],
            repelled_distance: 100.0,
        }
    }
    #[test]
    fn movement_uses_current_velocity_before_advancing_it() {
        let p = p();
        let mut a = Arrow::fire(&p, 10, 2, [7.0, 1.0, 9.0], [0.0, 0.0, 2.0], 1.0).unwrap();
        assert!(a.step(&p, 1.0));
        assert_eq!(a.position, [7.0, 4.0, 21.0]);
        assert_eq!((a.speed, a.gravity), (10.0, 1.0));
        assert!(a.step(&p, 1.0));
        assert_eq!(a.position, [7.0, 5.0, 31.0]);
        assert_eq!((a.speed, a.gravity), (8.0, -1.0));
        assert!(a.step(&p, 1.0));
        assert_eq!(a.position, [7.0, 4.0, 39.0]);
        assert!(!a.step(&p, 1.0));
        assert!(!a.alive);
    }
    #[test]
    fn sequences_include_exact_distance_thresholds() {
        let p = p();
        assert_eq!(
            [
                p.sequence(7.99),
                p.sequence(8.0),
                p.sequence(19.99),
                p.sequence(20.0)
            ],
            [11, 12, 12, 13]
        );
    }
    #[test]
    fn contact_and_range_are_per_shooter() {
        let p = p();
        let mut a = Arrow::fire(&p, 100, 0, [0.0; 3], [0.0, 0.0, 1.0], 1.0).unwrap();
        let mut b = Arrow::fire(&p, 101, 1, [0.0; 3], [0.0, 0.0, 1.0], 2.0).unwrap();
        a.contact(&p, true, false);
        assert!(!a.alive);
        assert!(b.alive);
        for _ in 0..4 {
            assert!(b.step(&p, 1.0));
        }
        assert_eq!((a.shooter, b.shooter), (0, 1));
        b.contact(&p, false, true);
        assert!(!b.alive);
    }
    #[test]
    fn bow_origin_and_rng_are_explicit_and_do_not_retarget_in_flight() {
        let aim = ArrowAim {
            bow_origin: [40.0, 80.0, 30.0],
            target: [40.0, 80.0, 1030.0],
            accuracy: 22.5,
            cone_random: 0,
            polar_random: 0x80000000,
            steady: false,
        };
        let d = aim.direction().unwrap();
        assert!(d[0].abs() < 1e-6 && (d[1] - 1.0).abs() < 1e-6 && d[2].abs() < 1e-6);
        let mut a = Arrow::aimed(&p(), 100, 2, ArrowAim { steady: true, ..aim }, 1.0).unwrap();
        assert_eq!(a.position, aim.bow_origin);
        assert_eq!(a.direction, [0.0, 0.0, 1.0]);
        a.step(&p(), 1.0);
        assert_eq!(a.direction, [0.0, 0.0, 1.0]);
        assert!(
            ArrowAim {
                target: [40.0, 1080.0, 30.0],
                ..aim
            }
            .direction()
            .is_none()
        );
    }

    #[test]
    fn accuracy_cone_matches_original_helper_on_oblique_aim() {
        // Synthetic fixtures run through 0x00B1DCB0 with its original
        // 0x007AF5B0 basis and RNG-word mapping in private Unicorn.
        // Only sin/cos calls were supplied by the harness; no game data.
        let cases = [
            (0.25, 0, 0, [0.42927885, 0.5512077, 0.7154647]),
            (0.25, u32::MAX, 0x40000000, [0.40923414, 0.5655993, 0.71597826]),
            (1.0, 0x10000000, 0x70000000, [0.42709047, 0.6111402, 0.6664094]),
        ];
        for (accuracy, cone_random, polar_random, expected) in cases {
            let actual = ArrowAim {
                bow_origin: [0.0; 3],
                target: [0.3, 0.4, 0.5],
                accuracy,
                cone_random,
                polar_random,
                steady: false,
            }
            .direction()
            .unwrap();
            for k in 0..3 {
                assert!(
                    (actual[k] - expected[k]).abs() < 2e-7,
                    "{actual:?} != {expected:?}"
                );
            }
        }
    }
}
