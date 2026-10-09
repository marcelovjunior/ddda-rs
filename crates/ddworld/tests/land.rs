//! Bodies on the stage collision: regression for a body lost below the
//! ground (docs/tasks/land-fix.md). Skipped without the install.

use ddsim::{Ground, Input, JumpParam, MoveParam, RootMotion, Sim};
use std::collections::HashMap;

const MOTIONS: &str = r"motion\pl\m\m00\m0000_co\m0000_co";
const DAGGER_MOTIONS: &str = r"motion\pl\m\m00\m0004_co\m0004_co";
const USED: [u16; 25] = [
    0, 3, 4, 7, 10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 43, 45, 46, 47, 48, 49, 50, 0x54, 0x55, 0x611,
];

fn vfs() -> Option<ddvfs::Vfs> {
    let p = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !p.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    Some(ddvfs::Vfs::open(p).unwrap())
}

fn adjust(vfs: &ddvfs::Vfs) -> Vec<f32> {
    let b = vfs
        .read(r"param\pl\other\m_pl000_00", mtarc::type_hash("rAdjustParam"))
        .unwrap();
    let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    b[12..12 + 4 * n]
        .chunks(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn jumps(vfs: &ddvfs::Vfs) -> Vec<JumpParam> {
    let b = vfs
        .read(r"param\pl\other\pljumpparam", mtarc::type_hash("rPlJumpParam"))
        .unwrap();
    let x = mtxfs::parse(&b).unwrap();
    let rows = x.get(&x.root, "mArray").unwrap();
    let f = |o: &mtxfs::Object, n: &str| match x.get(o, n).and_then(|v| v.first()) {
        Some(mtxfs::Value::F(v)) => v[0] as f32,
        _ => panic!("{n}"),
    };
    [0usize, 9]
        .iter()
        .map(|&i| {
            let Some(mtxfs::Value::Object(Some(o))) = rows.get(i) else {
                panic!()
            };
            JumpParam {
                speed_y: f(o, "mSpeedY"),
                speed_z: f(o, "mSpeedZ"),
                gravity: f(o, "mGravity"),
                add_move_xz: f(o, "mAddMoveSpeedXZ"),
                damping_z: f(o, "mDampingZ"),
            }
        })
        .collect()
}

/// A Sim configured from the install as the viewer's `--play`.
fn sim(vfs: &ddvfs::Vfs) -> Sim {
    let rml = mtarc::type_hash("rMotionList");
    let lists: HashMap<u16, mtlmt::MotionList> = [(0u16, MOTIONS), (6, DAGGER_MOTIONS)]
        .into_iter()
        .map(|(b, p)| (b, mtlmt::parse(&vfs.read(p, rml).unwrap()).unwrap()))
        .collect();
    let mut roots = HashMap::new();
    for id in USED {
        let l = &lists[&(id >> 8)];
        let slot = usize::from(id & 0xff);
        let m = l.motions[slot].as_ref().unwrap();
        roots.insert(
            id,
            RootMotion::from_motion(m)
                .with_curves(l.curves[slot].clone())
                .with_events(l.events[slot].clone()),
        );
    }
    let mut s = Sim::new(roots);
    let j = jumps(vfs);
    s.jump_normal = j[0];
    s.jump_sprint = j[1];
    let a = adjust(vfs);
    s.move_param = MoveParam::from_adjust(&a).unwrap();
    s.ledge_param = ddsim::LedgeParam::from_adjust(&a).unwrap();
    s
}

/// Stage 100 around 58m50n, as the net demo loads it (radius 2).
fn terrain(vfs: &ddvfs::Vfs) -> ddworld::Terrain {
    let mut t = ddworld::Terrain::load_collision(vfs, 100, 58, 50, 2);
    t.add_scenery(vfs, 100, 58, 50, 2);
    t
}

/// The net demo's unattended input (ddview `--demo` before 5e85234): a
/// stick turning at `rate` radians a second in the camera's frame, and a
/// jump press every `every` seconds (a second press 0.4 s later with
/// `double`).
struct Demo {
    start: [f32; 3],
    cam: f32,
    off: f32,
    rate: f32,
    every: f32,
    walk: bool,
    double: bool,
}

impl Demo {
    fn input(&self, f: u32) -> Input {
        let tt = self.off + f as f32 / 30.0;
        let ph = tt % self.every;
        Input {
            stick: [(tt * self.rate).sin(), (tt * self.rate).cos()],
            walk: self.walk,
            jump: ph < 0.1 || (self.double && (0.4..0.5).contains(&ph)),
            camera_yaw: self.cam,
            ..Default::default()
        }
    }

    /// Runs `frames` frames; returns the deepest the body went below the
    /// lowest surface under it (cm, 0 when never below) and the frame.
    fn run(
        &self,
        vfs: &ddvfs::Vfs,
        t: &ddworld::Terrain,
        frames: u32,
    ) -> (f32, u32, Vec<([f32; 3], ddsim::State)>) {
        let mut s = sim(vfs);
        s.add_player(0, self.start, 0.0);
        let p = s.player_mut(0).unwrap();
        p.relaxed = true;
        p.double_jump = true;
        let mut worst = (0.0f32, 0u32);
        let mut path = vec![];
        for f in 0..frames {
            s.step(1.0 / 30.0, &HashMap::from([(0, self.input(f))]), t);
            let p = s.player(0).unwrap();
            path.push((p.position, p.state));
            let [x, y, z] = p.position;
            // below every surface at (x, z): the lowest one's height
            if t.height(x, z, y).is_none() {
                if let Some(lowest) = lowest_surface(t, x, z) {
                    let d = lowest - y;
                    if d > worst.0 {
                        worst = (d, f);
                    }
                }
            }
        }
        (worst.0, worst.1, path)
    }
}

/// The lowest surface at (x, z), by stepping the height query down.
fn lowest_surface(t: &ddworld::Terrain, x: f32, z: f32) -> Option<f32> {
    let mut h = t.height(x, z, f32::MAX)?;
    while let Some(g) = t.height(x, z, h - 0.5) {
        h = g;
    }
    Some(h)
}

/// #1149: a jump onto higher ground on the plateau of stage 100 by 58m50n
/// sank the body under the single-layer ground (ddsim before the capsule
/// sweep, f892a7e: the landing looked for ground only below the body).
/// These demo runs lost the body that way (found by a random search over
/// the demo's inputs; the old code loses them at frames 641 and 130).
#[test]
fn demo_jumps_onto_higher_ground_stay_on_it() {
    let Some(vfs) = vfs() else { return };
    let t = terrain(&vfs);
    let runs = [
        // a jump landing into rising ground on the plateau
        Demo {
            start: [103.519745, 41054.625, 83904.07],
            cam: 3.7549088,
            off: 7.4379873,
            rate: 0.3754241,
            every: 3.106542,
            walk: true,
            double: false,
        },
        // a second jump into a rising slope
        Demo {
            start: [153.59512, 41022.645, 83574.46],
            cam: 4.1759553,
            off: 6.9215355,
            rate: 0.9947867,
            every: 2.201448,
            walk: false,
            double: false,
        },
    ];
    for (i, d) in runs.iter().enumerate() {
        let (worst, at, path) = d.run(&vfs, &t, 900);
        eprintln!("run {i}: deepest {worst} at {at}; end {:?}", path.last());
        assert!(
            worst < 50.0,
            "run {i}: {worst} cm below the ground at frame {at}: {:?}",
            &path[at.saturating_sub(5) as usize..=at as usize]
        );
    }
}
