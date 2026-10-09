//! Acceptance replay (the M2 gate, docs/ACCEPTANCE.md).
//!
//! A route is a short scripted action recorded in the original through the
//! oracle (`research/ddda-oracle/routes.py`): per game frame, the keys held,
//! the frame's delta, and the player's resulting state. Here the same keys
//! and deltas drive `ddsim` on `ddworld` collision around the start, and
//! the two runs are compared quantity by quantity against our own
//! tolerances.
//!
//! Traces are recordings of the original and stay private
//! (`E:\ddda-re\notes\acceptance\`); nothing here embeds their data.

use std::collections::HashMap;
use std::path::Path;

use ddsim::{Input, JumpParam, MoveParam, RootMotion, Sim, State};

// ------------------------------------------------------------------ trace

/// Key bits in a trace's `keys` column (the recorder's own encoding).
pub const KEY_W: u32 = 1;
pub const KEY_A: u32 = 2;
pub const KEY_S: u32 = 4;
pub const KEY_D: u32 = 8;
pub const KEY_SPRINT: u32 = 16;
pub const KEY_WALK: u32 = 32;
pub const KEY_JUMP: u32 = 64;

/// One recorded game frame: the input held during the frame and the state
/// sampled after it (right after sMain::vf06).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub keys: u32,
    /// sMain.mDeltaTime (+0x70), in 30 fps frames.
    pub dt_main: f32,
    /// The player's own cUnit.mDeltaTime (+0x14), in 30 fps frames.
    pub dt_unit: f32,
    /// Feet position, stage frame (cm): uPlayer.mPos minus
    /// sWorldOffset.mWorldAbsoluteOffset.
    pub pos: [f32; 3],
    /// uPlayer +0x2530.
    pub vel: [f32; 3],
    /// uPlayer +0x257C (radians, 0 faces +Z).
    pub yaw: f32,
    /// uPlayer +0x2DD4.
    pub action: u32,
    /// uPlayer +0xF40 (requested motion).
    pub motion: u32,
    /// uPlayer +0x3CC (motion frame).
    pub mframe: f32,
    /// cPlayerInfo +0x2A8.
    pub stamina: f32,
    /// Camera yaw from uCameraGame mCameraPos -> mTargetPos (radians, 0 looks along +Z).
    pub cam_yaw: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub route: String,
    pub stage: u32,
    /// Floating-origin cell (A, B) at the start; the collision around it is loaded.
    pub cell: (i32, i32),
    /// Frames [a, b) over which the route's steady speed is measured.
    pub window: Option<(usize, usize)>,
    pub frames: Vec<Frame>,
}

const COLUMNS: &str = "keys,dt_main,dt_unit,x,y,z,vx,vy,vz,yaw,action,motion,mframe,stamina,cam_yaw";

/// Parses a trace: `# key=value` header lines, a column line, then one row per frame.
pub fn parse_trace(text: &str) -> Result<Trace, String> {
    let mut route = String::new();
    let (mut stage, mut cell, mut window) = (100, (0, 0), None);
    let mut frames = Vec::new();
    let mut header_seen = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix('#') {
            let Some((k, v)) = h.trim().split_once('=') else {
                continue;
            };
            let nums: Vec<i64> = v.split_whitespace().filter_map(|s| s.parse().ok()).collect();
            match k.trim() {
                "route" => route = v.trim().to_string(),
                "stage" => stage = *nums.first().ok_or("bad stage")? as u32,
                "cell" if nums.len() == 2 => cell = (nums[0] as i32, nums[1] as i32),
                "window" if nums.len() == 2 => window = Some((nums[0] as usize, nums[1] as usize)),
                _ => {}
            }
            continue;
        }
        if !header_seen {
            if line != COLUMNS {
                return Err(format!("unexpected columns {line:?}, want {COLUMNS:?}"));
            }
            header_seen = true;
            continue;
        }
        let v: Vec<f64> = line
            .split(',')
            .map(|s| s.trim().parse::<f64>().map_err(|e| format!("{s:?}: {e}")))
            .collect::<Result<_, _>>()?;
        if v.len() != 15 {
            return Err(format!("row with {} values", v.len()));
        }
        let f = |i: usize| v[i] as f32;
        frames.push(Frame {
            keys: v[0] as u32,
            dt_main: f(1),
            dt_unit: f(2),
            pos: [f(3), f(4), f(5)],
            vel: [f(6), f(7), f(8)],
            yaw: f(9),
            action: v[10] as u32,
            motion: v[11] as u32,
            mframe: f(12),
            stamina: f(13),
            cam_yaw: f(14),
        });
    }
    if frames.len() < 2 {
        return Err("trace has fewer than 2 frames".into());
    }
    Ok(Trace {
        route,
        stage,
        cell,
        window,
        frames,
    })
}

/// The ddsim input for held keys: a unit stick (screen-right +x, forward +y;
/// diagonals normalised, *assumed* as for a keyboard in the original).
///
/// The recorder's Ctrl bit (`KEY_WALK`) does not make the original walk: in
/// the `drop` route, recorded with Ctrl held, the original runs (action 2,
/// |velocity| 10.165 = the relaxed run, unit time x1.15). So it is not
/// mapped to `Input::walk`.
pub fn input_of(keys: u32, cam_yaw: f32) -> Input {
    let x = f32::from(u8::from(keys & KEY_D != 0)) - f32::from(u8::from(keys & KEY_A != 0));
    let y = f32::from(u8::from(keys & KEY_W != 0)) - f32::from(u8::from(keys & KEY_S != 0));
    let n = (x * x + y * y).sqrt();
    Input {
        stick: if n > 0.0 { [x / n, y / n] } else { [0.0, 0.0] },
        walk: false,
        sprint: keys & KEY_SPRINT != 0,
        jump: keys & KEY_JUMP != 0,
        camera_yaw: cam_yaw,
    }
}

// ------------------------------------------------------------------ world

pub const NATIVE_PC: &str = r"E:\SteamLibrary\steamapps\common\DDDA\nativePC";
/// Same data as the viewer's `--play` (crates/ddview/src/play.rs).
pub const MOTIONS: &str = r"motion\pl\m\m00\m0000_co\m0000_co";
pub const DAGGER_MOTIONS: &str = r"motion\pl\m\m00\m0004_co\m0004_co";
pub const JUMP_PARAMS: &str = r"param\pl\other\pljumpparam";
pub const ADJUST_PARAMS: &str = r"param\pl\other\m_pl000_00";
const USED_MOTIONS: [u16; 25] = [
    0, 3, 4, 7, 10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 43, 45, 46, 47, 48, 49, 50, 0x54, 0x55, 0x611,
];

fn adjust_table(vfs: &ddvfs::Vfs, path: &str) -> Result<Vec<f32>, String> {
    let b = vfs.read(path, mtarc::type_hash("rAdjustParam"))?;
    if b.len() < 12 || &b[..4] != b"ajp\0" {
        return Err(format!("{path}: not an ajp file"));
    }
    let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let body = b
        .get(12..12 + 4 * n)
        .ok_or(format!("{path}: {n} floats do not fit"))?;
    Ok(body
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}

fn jump_params(vfs: &ddvfs::Vfs, indices: &[usize]) -> Result<Vec<JumpParam>, String> {
    let bytes = vfs.read(JUMP_PARAMS, mtarc::type_hash("rPlJumpParam"))?;
    let x = mtxfs::parse(&bytes).map_err(|e| e.to_string())?;
    let rows = x.get(&x.root, "mArray").ok_or("pljumpparam: no mArray")?;
    let f = |o: &mtxfs::Object, n: &str| -> Result<f32, String> {
        match x.get(o, n).and_then(|v| v.first()) {
            Some(mtxfs::Value::F(v)) => Ok(v[0] as f32),
            _ => Err(format!("pljumpparam: no {n}")),
        }
    };
    indices
        .iter()
        .map(|&i| {
            let Some(mtxfs::Value::Object(Some(o))) = rows.get(i) else {
                return Err(format!("pljumpparam: no row {i}"));
            };
            Ok(JumpParam {
                speed_y: f(o, "mSpeedY")?,
                speed_z: f(o, "mSpeedZ")?,
                gravity: f(o, "mGravity")?,
                add_move_xz: f(o, "mAddMoveSpeedXZ")?,
                damping_z: f(o, "mDampingZ")?,
            })
        })
        .collect()
}

/// A Sim configured from the install exactly as the viewer's `--play`.
pub fn new_sim(vfs: &ddvfs::Vfs) -> Result<Sim, String> {
    let rml = mtarc::type_hash("rMotionList");
    let mut lists: HashMap<u16, mtlmt::MotionList> = HashMap::new();
    let mut roots = HashMap::new();
    for id in USED_MOTIONS {
        let bank = id >> 8;
        let path = match bank {
            0 => MOTIONS,
            6 => DAGGER_MOTIONS,
            _ => return Err(format!("bank {bank}")),
        };
        if let std::collections::hash_map::Entry::Vacant(e) = lists.entry(bank) {
            let bytes = vfs.read(path, rml)?;
            e.insert(mtlmt::parse(&bytes).map_err(|e| e.to_string())?);
        }
        let m = lists[&bank]
            .motions
            .get(usize::from(id & 0xFF))
            .cloned()
            .flatten()
            .ok_or(format!("{path}: slot {} missing", id & 0xFF))?;
        let slot = usize::from(id & 0xFF);
        let curves = lists[&bank].curves[slot].clone();
        let events = lists[&bank].events[slot].clone();
        roots.insert(
            id,
            RootMotion::from_motion(&m)
                .with_curves(curves)
                .with_events(events),
        );
    }
    let mut sim = Sim::new(roots);
    let jp = jump_params(vfs, &[0, 9])?;
    sim.jump_normal = jp[0];
    sim.jump_sprint = jp[1];
    let adjust = adjust_table(vfs, ADJUST_PARAMS)?;
    sim.move_param = MoveParam::from_adjust(&adjust).ok_or("adjust table too short")?;
    sim.ledge_param = ddsim::LedgeParam::from_adjust(&adjust).ok_or("no ledge params")?;
    Ok(sim)
}

/// Stage collision plus placed scenery around the cell, as the viewer loads it.
pub fn load_world(vfs: &ddvfs::Vfs, stage: u32, cell: (i32, i32)) -> ddworld::Terrain {
    let mut t = ddworld::Terrain::load_collision(vfs, stage, cell.0, cell.1, 1);
    t.add_scenery(vfs, stage, cell.0, cell.1, 1);
    t
}

// ------------------------------------------------------------------ replay

/// Original action ids (uPlayer +0x2DD4) of ground movement: 2 run/walk,
/// 5 sprint, 125 sprint start, 3 stopping. *Observed* (docs/ACCEPTANCE.md).
pub const ORIG_RUN: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    /// Our state after each frame (index-aligned with the trace).
    pub pos: Vec<[f32; 3]>,
    pub state: Vec<State>,
    pub stamina: Vec<f32>,
    pub relaxed: bool,
}

/// Feeds the trace's keys and deltas into `sim` from the first frame's pose.
pub fn replay(mut sim: Sim, ground: &dyn ddsim::Ground, t: &Trace) -> Replay {
    let f0 = t.frames[0];
    sim.add_player(0, f0.pos, f0.yaw);
    // The area's walk/run variant (town = relaxed) is not read from the
    // original yet: take it from the recorded run velocity (|+0x2530|),
    // whichever of our two run speeds it is closer to.
    let runs: Vec<f32> = t
        .frames
        .iter()
        .filter(|f| f.action == ORIG_RUN && f.keys & (KEY_SPRINT | KEY_WALK) == 0)
        .map(|f| (f.vel[0] * f.vel[0] + f.vel[2] * f.vel[2]).sqrt())
        .filter(|&v| v > 1.0)
        .collect();
    let relaxed = !runs.is_empty() && {
        let v = runs.iter().sum::<f32>() / runs.len() as f32;
        (v - sim.move_param.run_relaxed).abs() < (v - sim.move_param.run).abs()
    };
    let p = sim.player_mut(0).unwrap();
    p.relaxed = relaxed;
    // the traces start at full stamina: take the first frame's value as both
    // the stamina and its maximum (the maximum is not read from the original)
    p.stamina = f0.stamina;
    p.stamina_max = f0.stamina;
    // a route that starts in the air (the drop route: action 13 right after
    // the recorder placed the player) starts our player falling with the
    // recorded velocity
    if ORIG_AIR.contains(&f0.action) {
        p.state = State::Fall;
        p.velocity = f0.vel;
    }
    let mut pos = vec![f0.pos];
    let mut state = vec![sim.player(0).unwrap().state];
    let mut stamina = vec![f0.stamina];
    let mut inputs = HashMap::new();
    for f in &t.frames[1..] {
        inputs.insert(0, input_of(f.keys, f.cam_yaw));
        // game time: sMain.mDeltaTime (the player's own delta is scaled by
        // its motion rate, which ddsim models itself)
        sim.step(f.dt_main / ddsim::MOTION_FPS, &inputs, ground);
        let p = sim.player(0).unwrap();
        pos.push(p.position);
        state.push(p.state);
        stamina.push(p.stamina);
    }
    Replay {
        pos,
        state,
        stamina,
        relaxed,
    }
}

// ------------------------------------------------------------------ metrics

/// Original action ids (uPlayer +0x2DD4) seen in the air and on landing.
/// *Observed* in oracle traces (docs/ORACLE.md), not read from the exe.
pub const ORIG_AIR: [u32; 3] = [8, 9, 13];
pub const ORIG_LAND: [u32; 1] = [14];

fn sim_air(s: State) -> bool {
    matches!(s, State::Air { .. } | State::Fall | State::SecondJump)
}
fn sim_land(s: State) -> bool {
    matches!(s, State::Land { .. } | State::SprintJumpLand)
}

fn hdist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Seconds of game time at the end of each frame (sMain.mDeltaTime).
fn times(t: &Trace) -> Vec<f32> {
    let mut acc = 0.0;
    let mut out = vec![0.0];
    for f in &t.frames[1..] {
        acc += f.dt_main / ddsim::MOTION_FPS;
        out.push(acc);
    }
    out
}

/// One compared quantity.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub route: String,
    pub quantity: String,
    pub original: Option<f32>,
    pub ours: Option<f32>,
    pub tolerance: f32,
    pub unit: &'static str,
}

impl Row {
    /// Within tolerance; quantities we do not model (None) are not passing.
    pub fn ok(&self) -> bool {
        match (self.original, self.ours) {
            (Some(a), Some(b)) => (a - b).abs() <= self.tolerance,
            _ => false,
        }
    }
}

/// Our tolerances (not the game's): what M2 accepts as "matching".
pub mod tol {
    /// Largest horizontal gap between the paths at any frame, cm.
    pub const PATH: f32 = 30.0;
    /// Final horizontal position, cm.
    pub const FINAL: f32 = 30.0;
    /// Steady speed, cm/s.
    pub const SPEED: f32 = 10.0;
    /// Jump apex above the takeoff height, cm.
    pub const APEX: f32 = 5.0;
    /// Airtime and landing time, s (two 30 fps frames).
    pub const TIME: f32 = 0.067;
    /// Stopping distance after release, cm.
    pub const STOP: f32 = 20.0;
    /// Stamina change rate, per second.
    pub const STAMINA: f32 = 1.0;
}

/// Compares one route. The quantities depend on what the route contains.
pub fn compare(t: &Trace, r: &Replay) -> Vec<Row> {
    let ts = times(t);
    let n = t.frames.len();
    let row = |q: &str, a: Option<f32>, b: Option<f32>, tol: f32, unit| Row {
        route: t.route.clone(),
        quantity: q.to_string(),
        original: a,
        ours: b,
        tolerance: tol,
        unit,
    };
    let mut rows = Vec::new();
    let gaps: Vec<f32> = (0..n).map(|i| hdist(t.frames[i].pos, r.pos[i])).collect();
    let (imax, max_gap) =
        gaps.iter()
            .copied()
            .enumerate()
            .fold((0, 0.0f32), |m, (i, g)| if g > m.1 { (i, g) } else { m });
    let travelled = |p: &dyn Fn(usize) -> [f32; 3]| (1..n).map(|i| hdist(p(i - 1), p(i))).sum::<f32>();
    rows.push(row(
        "path length",
        Some(travelled(&|i| t.frames[i].pos)),
        Some(travelled(&|i| r.pos[i])),
        tol::FINAL,
        "cm",
    ));
    rows.push(Row {
        quantity: format!("max path gap (frame {imax})"),
        ..row("", Some(0.0), Some(max_gap), tol::PATH, "cm")
    });
    rows.push(row(
        "final position gap",
        Some(0.0),
        Some(gaps[n - 1]),
        tol::FINAL,
        "cm",
    ));
    rows.push(row(
        "final height",
        Some(t.frames[n - 1].pos[1] - t.frames[0].pos[1]),
        Some(r.pos[n - 1][1] - r.pos[0][1]),
        tol::APEX,
        "cm",
    ));
    // steady speed over the route's window
    if let Some((a, b)) = t.window {
        let b = b.min(n - 1);
        if a < b {
            let dt = ts[b] - ts[a];
            let sp =
                |p: &dyn Fn(usize) -> [f32; 3]| (a + 1..=b).map(|i| hdist(p(i - 1), p(i))).sum::<f32>() / dt;
            rows.push(row(
                &format!("speed, frames {a}..{b}"),
                Some(sp(&|i| t.frames[i].pos)),
                Some(sp(&|i| r.pos[i])),
                tol::SPEED,
                "cm/s",
            ));
        }
    }
    // jump: first frame with the jump key
    if let Some(press) = t.frames.iter().position(|f| f.keys & KEY_JUMP != 0) {
        let y0o = t.frames[press - 1].pos[1];
        let y0s = r.pos[press - 1][1];
        let apex_o = t.frames[press..]
            .iter()
            .map(|f| f.pos[1])
            .fold(f32::MIN, f32::max)
            - y0o;
        let apex_s = r.pos[press..].iter().map(|p| p[1]).fold(f32::MIN, f32::max) - y0s;
        rows.push(row("jump apex", Some(apex_o), Some(apex_s), tol::APEX, "cm"));
        let first = |pred: &dyn Fn(usize) -> bool| (press..n).find(|&i| pred(i));
        let air_o = first(&|i| ORIG_AIR.contains(&t.frames[i].action));
        let land_o = first(&|i| ORIG_LAND.contains(&t.frames[i].action));
        let air_s = first(&|i| sim_air(r.state[i]));
        let land_s = first(&|i| sim_land(r.state[i]));
        let span = |a: Option<usize>, b: Option<usize>| a.zip(b).map(|(a, b)| ts[b] - ts[a]);
        rows.push(row(
            "takeoff after press",
            span(Some(press - 1), air_o),
            span(Some(press - 1), air_s),
            tol::TIME,
            "s",
        ));
        rows.push(row(
            "airtime",
            span(air_o, land_o),
            span(air_s, land_s),
            tol::TIME,
            "s",
        ));
        rows.push(row(
            "landing after press",
            span(Some(press - 1), land_o),
            span(Some(press - 1), land_s),
            tol::TIME,
            "s",
        ));
        let fwd = |p: [f32; 3], q: [f32; 3]| hdist(p, q);
        rows.push(row(
            "jump distance (press to landing)",
            land_o.map(|l| fwd(t.frames[press - 1].pos, t.frames[l].pos)),
            land_s.map(|l| fwd(r.pos[press - 1], r.pos[l])),
            tol::FINAL,
            "cm",
        ));
    } else if let Some(air) = (1..n).find(|&i| ORIG_AIR.contains(&t.frames[i].action)) {
        // a drop without a jump: fall time and height
        let land_o = (air..n).find(|&i| ORIG_LAND.contains(&t.frames[i].action));
        let air_s = (1..n).find(|&i| sim_air(r.state[i]));
        let land_s = air_s.and_then(|a| (a..n).find(|&i| sim_land(r.state[i])));
        let span = |a: Option<usize>, b: Option<usize>| a.zip(b).map(|(a, b)| ts[b] - ts[a]);
        rows.push(row(
            "fall start",
            Some(ts[air]),
            air_s.map(|i| ts[i]),
            tol::TIME,
            "s",
        ));
        rows.push(row(
            "fall time",
            span(Some(air), land_o),
            span(air_s, land_s),
            tol::TIME,
            "s",
        ));
        rows.push(row(
            "drop height",
            land_o.map(|l| t.frames[air - 1].pos[1] - t.frames[l].pos[1]),
            air_s.zip(land_s).map(|(a, l)| r.pos[a - 1][1] - r.pos[l][1]),
            tol::APEX,
            "cm",
        ));
    }
    // stop: after the last frame with movement keys, the distance to rest
    let mv = KEY_W | KEY_A | KEY_S | KEY_D;
    if let Some(last) = (0..n).rev().find(|&i| t.frames[i].keys & mv != 0) {
        if last + 1 < n && t.frames.iter().all(|f| f.keys & KEY_JUMP == 0) {
            rows.push(row(
                "stopping distance after release",
                Some(hdist(t.frames[last].pos, t.frames[n - 1].pos)),
                Some(hdist(r.pos[last], r.pos[n - 1])),
                tol::STOP,
                "cm",
            ));
        }
    }
    // stamina after a sprint: the hold before recovery and the idle recovery
    // rate, each measured on its own run (idle: original action 0, our Wait)
    if let Some(last_sprint) = (0..n).rev().find(|&i| t.frames[i].keys & KEY_SPRINT != 0) {
        let after = |st: &[f32], idle: &dyn Fn(usize) -> bool| -> (Option<f32>, Option<f32>) {
            let lo = (last_sprint..n).min_by(|&a, &b| st[a].total_cmp(&st[b])).unwrap();
            let Some(up) = (lo..n).find(|&i| st[i] > st[lo] + 1e-3) else {
                return (None, None);
            };
            let idle = (up..n).find(|&i| idle(i)).unwrap_or(up);
            let rate = (ts[n - 1] > ts[idle]).then(|| (st[n - 1] - st[idle]) / (ts[n - 1] - ts[idle]));
            (Some(ts[up] - ts[lo]), rate)
        };
        let st: Vec<f32> = t.frames.iter().map(|f| f.stamina).collect();
        let (hold_o, rate_o) = after(&st, &|i| t.frames[i].action == 0);
        let (hold_s, rate_s) = after(&r.stamina, &|i| r.state[i] == State::Wait);
        if hold_o.is_some() {
            rows.push(row("stamina hold after sprint", hold_o, hold_s, tol::TIME, "s"));
        }
        if rate_o.is_some() {
            rows.push(row(
                "stamina recovery rate (idle)",
                rate_o,
                rate_s,
                tol::STAMINA,
                "/s",
            ));
        }
    }
    // stamina: rate over the window
    if let Some((a, b)) = t.window {
        let b = b.min(n - 1);
        if a < b && ts[b] > ts[a] {
            let rate = |st: &dyn Fn(usize) -> f32| (st(b) - st(a)) / (ts[b] - ts[a]);
            rows.push(row(
                &format!("stamina rate, frames {a}..{b}"),
                Some(rate(&|i| t.frames[i].stamina)),
                Some(rate(&|i| r.stamina[i])),
                tol::STAMINA,
                "/s",
            ));
        }
    }
    rows
}

/// Markdown table of rows.
pub fn table(rows: &[Row]) -> String {
    let mut s = String::from("| route | quantity | original | ours | difference | tolerance | ok |\n|---|---|---|---|---|---|---|\n");
    let f = |v: Option<f32>| v.map_or("not modelled".into(), |x| format!("{x:.3}"));
    for r in rows {
        let d = r
            .original
            .zip(r.ours)
            .map_or("-".into(), |(a, b)| format!("{:+.3}", b - a));
        s += &format!(
            "| {} | {} | {} | {} | {} | {} {} | {} |\n",
            r.route,
            r.quantity,
            f(r.original),
            f(r.ours),
            d,
            r.tolerance,
            r.unit,
            if r.ok() { "yes" } else { "**no**" }
        );
    }
    s
}

/// Directory of the private traces (override with `DDACCEPT_TRACES`).
pub fn trace_dir() -> std::path::PathBuf {
    std::env::var_os("DDACCEPT_TRACES")
        .map(Into::into)
        .unwrap_or_else(|| Path::new(r"E:\ddda-re\notes\acceptance").to_path_buf())
}

/// Every `*.csv` trace in `dir`, sorted by name.
pub fn load_traces(dir: &Path) -> Vec<(String, Result<Trace, String>)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "csv"))
        .collect();
    v.sort();
    v.into_iter()
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let r = std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|s| parse_trace(&s));
            (name, r)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_a_camera_relative_stick() {
        let i = input_of(KEY_W | KEY_D | KEY_SPRINT, 1.0);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((i.stick[0] - h).abs() < 1e-6 && (i.stick[1] - h).abs() < 1e-6);
        assert!(i.sprint && !i.walk && !i.jump);
        assert!(!input_of(KEY_W | KEY_WALK, 0.0).walk);
        assert_eq!(i.camera_yaw, 1.0);
        assert_eq!(input_of(0, 0.0).stick, [0.0, 0.0]);
    }

    #[test]
    fn parses_a_trace() {
        let text = format!(
            "# route=run\n# stage=100\n# cell=63 52\n# window=1 2\n{COLUMNS}\n\
             0,0.5,0.5,1,2,3,0,0,0,0.1,0,0,0,640,0.2\n\
             1,0.5,0.5,1,2,4,0,0,1,0.1,2,85,1,640,0.2\n"
        );
        let t = parse_trace(&text).unwrap();
        assert_eq!(t.route, "run");
        assert_eq!(t.cell, (63, 52));
        assert_eq!(t.window, Some((1, 2)));
        assert_eq!(t.frames.len(), 2);
        assert_eq!(t.frames[1].keys, KEY_W);
        assert_eq!(t.frames[1].motion, 85);
        assert!(parse_trace("x").is_err());
    }

    #[test]
    fn compare_reports_gaps_and_stamina() {
        let fr = |z: f32, keys: u32| Frame {
            keys,
            dt_main: 1.0,
            dt_unit: 1.0,
            pos: [0.0, 0.0, z],
            vel: [0.0; 3],
            yaw: 0.0,
            action: 2,
            motion: 4,
            mframe: 0.0,
            stamina: 600.0 - z,
            cam_yaw: 0.0,
        };
        let t = Trace {
            route: "t".into(),
            stage: 100,
            cell: (0, 0),
            window: Some((0, 2)),
            frames: vec![fr(0.0, 0), fr(10.0, KEY_W), fr(20.0, KEY_W)],
        };
        let r = Replay {
            pos: vec![[0.0; 3], [0.0, 0.0, 10.0], [0.0, 0.0, 25.0]],
            state: vec![State::Wait; 3],
            stamina: vec![600.0, 590.0, 570.0],
            relaxed: false,
        };
        let rows = compare(&t, &r);
        let get = |q: &str| rows.iter().find(|x| x.quantity.starts_with(q)).unwrap();
        assert_eq!(get("final position gap").ours, Some(5.0));
        assert!(get("final position gap").ok());
        // 20 cm over 2 frames of 1/30 s
        assert!((get("speed").original.unwrap() - 300.0).abs() < 1e-3);
        // original -20 over 2/30 s = -300/s; ours -30 -> -450/s
        let st = get("stamina rate");
        assert!((st.original.unwrap() + 300.0).abs() < 1e-3);
        assert!((st.ours.unwrap() + 450.0).abs() < 1e-3);
        assert!(!st.ok());
    }
}
