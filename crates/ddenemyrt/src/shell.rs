//! The staff's shells: `cPlActWpnWandShot` fires one per volley
//! (`0x00B4FA00`, `ddsim::attack::shot`), and the shell, not the staff,
//! carries the hit.
//!
//! What is read: the shell comes from list uPlayer +0x35BC, group 0,
//! index 0 (the variant's +0x40 / +0x44; elemental enchantments pick
//! indices 4-8, not modelled). `vf198` (0x00B5BA20) loads that list from
//! `param\shl_param_07` for the Arisen and pawns (actor +0x206D & 6), else
//! `param\shl_param_07_he`. Entry 0 of group 0 is a `cShlParamHoming`: its
//! collision is sequence `seq_0` of the list's `mCollisionPath`
//! (`collision\pl\m0007`, the staff's own file: group 1, a sphere of 20 cm
//! on the shell, attack 1 of `m0007_00`), its speed `spd` changing by
//! `spdAccel` a frame toward `spdMax`, and it dies after `dieDist`.
//!
//! *Placeholders* (the shell's actions, `cShlAct*`, are not traced): the
//! spawn point (the staff joint in the original) is taken 130 cm up and
//! 40 cm ahead of the Arisen's feet; the homing (`hmg*`, `hm*` fields) is a
//! straight flight at the target's aim point, re-aimed each frame and
//! stopping on it; the shell dies on its first hit (`dieFlag` 7 is
//! *assumed* to include hitting).

use crate::body::{model_matrix, CollisionSet};
use ddcombat::events::Activation;
use ddcombat::volumes::{attack_volumes, AttackVolume, Mat4, Pose};

/// The shell list of the Arisen.
pub const SHELL_LIST: &str = r"param\shl_param_07";
/// *Placeholder* spawn offset in the Arisen's frame (x right, y up, z ahead).
pub const SPAWN_OFFSET: [f32; 3] = [0.0, 130.0, 40.0];

/// The values of one shell entry the flight reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellParam {
    /// `spd`, `spdMax`, `spdAccel` (cm per frame, per frame squared).
    pub speed: f32,
    pub speed_max: f32,
    pub accel: f32,
    /// `dieDist` (cm).
    pub die_dist: f32,
    /// `seq_0`: the collision sequence its volume switches on.
    pub seq: u32,
}

impl ShellParam {
    /// Entry `index` of group `group` of an `rShlParamList`.
    pub fn from_list(bytes: &[u8], group: usize, index: usize) -> Result<Self, String> {
        let x = mtxfs::parse(bytes).map_err(|e| e.to_string())?;
        let obj = |o: &mtxfs::Object, name: &str, i: usize| -> Result<mtxfs::Object, String> {
            match x.get(o, name).and_then(|v| v.get(i)) {
                Some(mtxfs::Value::Object(Some(c))) => Ok((**c).clone()),
                _ => Err(format!("no {name}[{i}]")),
            }
        };
        let list = obj(&x.root, "mParamList", 0)?;
        let g = obj(&list, "mpArray", group)?;
        let shells = obj(&g, "mShlList", 0)?;
        let s = obj(&shells, "mpArray", index)?;
        let f = |name: &str| -> Result<f32, String> {
            match x.get(&s, name).and_then(|v| v.first()) {
                Some(mtxfs::Value::F(v)) => Ok(v[0] as f32),
                _ => Err(format!("no {name}")),
            }
        };
        let seq = match x.get(&s, "seq_0").and_then(|v| v.first()) {
            Some(mtxfs::Value::S(v)) => u32::try_from(*v).map_err(|_| "seq_0 < 0".to_string())?,
            _ => return Err("no seq_0".into()),
        };
        Ok(ShellParam {
            speed: f("spd")?,
            speed_max: f("spdMax")?,
            accel: f("spdAccel")?,
            die_dist: f("dieDist")?,
            seq,
        })
    }

    /// The WandShot shell (group 0, entry 0) from the install.
    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::from_list(&crate::body::load::read(vfs, SHELL_LIST, "rShlParamList")?, 0, 0)
            .map_err(|e| format!("{SHELL_LIST}: {e}"))
    }
}

/// A shell in flight.
#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    /// The owner id its volume carries (each shell its own, so their hit
    /// histories are apart).
    pub owner: u32,
    /// The Arisen that fired it.
    pub shooter: u32,
    pub pos: [f32; 3],
    /// Unit direction of flight.
    pub dir: [f32; 3],
    pub speed: f32,
    pub travelled: f32,
    /// The enemy it flies at.
    pub target: Option<u32>,
}

impl Shell {
    /// A shell fired by an Arisen at `feet` facing `yaw`.
    pub fn fire(
        p: &ShellParam,
        owner: u32,
        shooter: u32,
        feet: [f32; 3],
        yaw: f32,
        target: Option<u32>,
    ) -> Self {
        let (s, c) = yaw.sin_cos();
        let [x, y, z] = SPAWN_OFFSET;
        Shell {
            owner,
            shooter,
            pos: [feet[0] + z * s - x * c, feet[1] + y, feet[2] + z * c + x * s],
            dir: [s, 0.0, c],
            speed: p.speed,
            travelled: 0.0,
            target,
        }
    }

    /// Moves the shell by `step` frames toward `aim` (or straight on);
    /// false once it has flown its distance.
    pub fn fly(&mut self, p: &ShellParam, aim: Option<[f32; 3]>, step: f32) -> bool {
        let mut d = self.speed * step;
        if let Some(a) = aim {
            let v = [a[0] - self.pos[0], a[1] - self.pos[1], a[2] - self.pos[2]];
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if len > 1e-3 {
                self.dir = [v[0] / len, v[1] / len, v[2] / len];
                d = d.min(len);
            } else {
                d = 0.0;
            }
        }
        for k in 0..3 {
            self.pos[k] += self.dir[k] * d;
        }
        self.travelled += d;
        // spd moves by spdAccel a frame toward spdMax
        let v = self.speed + p.accel * step;
        self.speed = if p.accel < 0.0 {
            v.max(p.speed_max)
        } else {
            v.min(p.speed_max)
        };
        self.travelled < p.die_dist
    }

    /// Its attack volumes this frame: sequence `seq` of the staff's
    /// collision set placed on the shell.
    pub fn volumes(&self, set: &CollisionSet, p: &ShellParam) -> Vec<AttackVolume> {
        let act = Activation {
            resource_id: set.body.resource_id,
            seq: p.seq,
        };
        attack_volumes(
            &set.body,
            set.attacks.as_ref(),
            &[act],
            &At(model_matrix(self.pos, 0.0)),
            1.0,
            self.owner,
        )
    }
}

/// A pose of one point: the shell's frame.
struct At(Mat4);

impl Pose for At {
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

    const P: ShellParam = ShellParam {
        speed: 90.0,
        speed_max: 80.0,
        accel: -0.5,
        die_dist: 2000.0,
        seq: 1,
    };

    #[test]
    fn a_shell_slows_to_its_cap_and_dies_after_its_distance() {
        let mut s = Shell::fire(&P, 1000, 0, [0.0; 3], 0.0, None);
        assert_eq!(s.pos, [0.0, 130.0, 40.0]);
        let mut n = 0;
        while s.fly(&P, None, 1.0) {
            n += 1;
        }
        // 90, 89.5, ... down to 80 (21 frames, 1785 cm), then 80 a frame
        assert_eq!(n, 23);
        assert_eq!(s.speed, 80.0);
    }

    #[test]
    fn a_shell_stops_on_its_aim_point() {
        let mut s = Shell::fire(&P, 1000, 0, [0.0; 3], 0.0, Some(100));
        s.fly(&P, Some([0.0, 130.0, 100.0]), 1.0);
        assert_eq!(s.pos, [0.0, 130.0, 100.0]);
    }
}
