//! Stage scenarios (rScenario): trigger volumes fed with unit positions.
//!
//! Detection (sAreaHit vf06 0x0041a2c0 -> cAreaHit vf07 0x00742810) gives
//! each volume a mask of who is inside; evaluation (sScenario vf06
//! 0x0049a0e0 -> 0x00797df0) decides by boot type when to fire the task
//! callback (0x00798ce0).
//!
//! Co-op rule (not in the original, which has one Arisen): detection bit 0
//! ("party member 0") is set when any Arisen of the session is inside;
//! bits 1..3 are the host's pawns.

use crate::{Env, Error, Inputs, Request};
use ddquest::order::Fields;
use mtfsm::{Parameter, Record};
use std::sync::Arc;

/// Observed stage-100 collision cells, corresponding to completed (status 3)
/// entries in uStageSplitCtrl +0x7c/+0x88. This is not a wanted-cell list.
/// Grid values must come from the installed stage descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedFieldCells {
    pub origin: [f32; 2],
    pub cell: [f32; 2],
    /// Filename offsets in X/Z (N/M) order.
    pub name_offset: [i32; 2],
    /// Completed cells in filename (M, N) order.
    pub loaded: Vec<(i32, i32)>,
}

impl LoadedFieldCells {
    /// Translated from 0x005072e0 and 0x00c5f630
    /// (DDDA.exe 2017-12-13 Steam). Conversion truncates toward zero,
    /// unlike the general streaming candidate-grid floor operation.
    pub fn contains(&self, pos: [f32; 3]) -> bool {
        if self.cell.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || self.origin.iter().any(|v| !v.is_finite())
            || !pos[0].is_finite() || !pos[2].is_finite() {
            return false;
        }
        let n = ((pos[0] - self.origin[0]) / self.cell[0]) as i32;
        let m = ((pos[2] - self.origin[1]) / self.cell[1]) as i32;
        let (Some(n), Some(m)) = (n.checked_add(self.name_offset[0]), m.checked_add(self.name_offset[1])) else {
            return false;
        };
        self.loaded.contains(&(m, n))
    }
}

/// An XFS object as an owned record (same shape mtfsm uses).
pub fn to_record(x: &mtxfs::Xfs, o: &mtxfs::Object) -> Record {
    let c = x.class_of(o);
    Record {
        class_hash: c.hash,
        class_name: c.name(),
        fields: c
            .properties
            .iter()
            .zip(&o.fields)
            .map(|(p, vs)| {
                (
                    p.name.clone(),
                    vs.iter()
                        .map(|v| match v {
                            mtxfs::Value::Object(o) => Parameter::Object(o.as_ref().map(|o| Box::new(to_record(x, o)))),
                            _ => Parameter::Scalar(v.clone()),
                        })
                        .collect(),
                )
            })
            .collect(),
    }
}

fn floats(r: &Record, n: &str) -> Option<Vec<f32>> {
    let (_, vs) = r.fields.iter().find(|(k, _)| k == n)?;
    let mut out = vec![];
    for v in vs {
        if let Parameter::Scalar(mtxfs::Value::F(f)) = v {
            out.extend(f.iter().map(|&x| x as f32));
        }
    }
    Some(out)
}

/// A zone shape (nZone::ShapeInfo*), contains-point tests from the zones'
/// vf23.
#[derive(Debug, Clone, PartialEq)]
pub enum Zone {
    Area { v: [[f32; 3]; 4], bottom: f32, height: f32, cross: [f32; 3], convex: bool, concave_status: u32 },
    Sphere { c: [f32; 3], r: f32 },
    Cylinder { p0: [f32; 3], p1: [f32; 3], r: f32 },
    /// Rows: three axes then the translation; `ext` half extents.
    Obb { m: [[f32; 4]; 4], ext: [f32; 3] },
    Aabb { min: [f32; 3], max: [f32; 3] },
}

fn v3(a: &[f32]) -> [f32; 3] {
    [a[0], a[1], a[2]]
}

impl Zone {
    /// Static scenario shape anchor from 0x007450e0. This is not always
    /// the volume centre: area height is removed, boxes use bottom Y,
    /// and cylinders use their first endpoint (shape type 6).
    fn detection_anchor(&self) -> [f32; 3] {
        match self {
            Zone::Area { v, .. } => std::array::from_fn(|i| (v[0][i] + v[1][i] + v[2][i] + v[3][i]) * 0.25),
            Zone::Sphere { c, .. } => *c,
            Zone::Cylinder { p0, .. } => *p0,
            Zone::Obb { m, ext } => [m[3][0], m[3][1] - ext[1], m[3][2]],
            Zone::Aabb { min, max } => [(min[0] + max[0]) * 0.5, min[1], (min[2] + max[2]) * 0.5],
        }
    }

    pub fn from_record(z: &Record) -> Result<Zone, Error> {
        let bad = || Error::Data(format!("zone {:?}", z.class_name));
        Ok(match z.class_name {
            Some("nZone::ShapeInfoArea") => {
                let v = floats(z, "mVertex").ok_or_else(bad)?;
                if v.len() < 16 {
                    return Err(bad());
                }
                let c = floats(z, "mConcaveCrossPos").ok_or_else(bad)?;
                Zone::Area {
                    v: [v3(&v[0..]), v3(&v[4..]), v3(&v[8..]), v3(&v[12..])],
                    bottom: z.f("mBottom").ok_or_else(bad)?,
                    height: z.f("mHeight").ok_or_else(bad)?,
                    cross: v3(&c),
                    convex: z.b("mFlgConvex").ok_or_else(bad)?,
                    concave_status: z.u("mConcaveStatus").ok_or_else(bad)? as u32,
                }
            }
            Some("nZone::ShapeInfoSphere") => {
                let s = floats(z, "mSphere").ok_or_else(bad)?;
                Zone::Sphere { c: v3(&s), r: s[3] }
            }
            Some("nZone::ShapeInfoCylinder") => {
                let s = floats(z, "mCylinder").ok_or_else(bad)?;
                Zone::Cylinder { p0: v3(&s[0..]), p1: v3(&s[4..]), r: s[8] }
            }
            Some("nZone::ShapeInfoOBB") => {
                let m = floats(z, "mOBB.coord").ok_or_else(bad)?;
                let e = floats(z, "mOBB.extent").ok_or_else(bad)?;
                let mut mm = [[0f32; 4]; 4];
                for (i, row) in mm.iter_mut().enumerate() {
                    row.copy_from_slice(&m[i * 4..i * 4 + 4]);
                }
                Zone::Obb { m: mm, ext: v3(&e) }
            }
            Some("nZone::ShapeInfoAABB") => {
                let a = floats(z, "mAABB").ok_or_else(bad)?;
                Zone::Aabb { min: v3(&a[0..]), max: v3(&a[4..]) }
            }
            other => return Err(Error::Unimplemented(format!("zone shape {other:?}"))),
        })
    }

    /// The zones' vf23 contains-point tests.
    pub fn contains(&self, p: [f32; 3]) -> bool {
        match self {
            // Translated from 0x00fca970 (Area): Y band, then XZ point in
            // one of two triangles chosen by the concave status.
            Zone::Area { v, bottom, height, cross, convex, concave_status } => {
                if p[1] < *bottom || p[1] > bottom + height {
                    return false;
                }
                let c = *cross;
                let [a, b, cc, d] = *v;
                let tris = if *convex || *concave_status == 0 {
                    [(a, d, cc), (a, cc, b)]
                } else {
                    match concave_status {
                        1 => [(b, d, cc), (a, d, b)],
                        2 => [(c, d, cc), (a, b, c)],
                        _ => [(c, cc, b), (a, c, d)],
                    }
                };
                tris.iter().any(|&(t0, t1, t2)| in_triangle_xz(p, t0, t1, t2))
            }
            // Translated from 0x00d134f0.
            Zone::Sphere { c, r } => dist2(p, *c) <= r * r,
            // Translated from 0x00d19cb0: distance to the axis line, between
            // the end planes.
            Zone::Cylinder { p0, p1, r } => {
                let d = sub(*p1, *p0);
                let dd = dot(d, d);
                if dd == 0.0 {
                    return false;
                }
                let t = dot(sub(p, *p0), d) / dd;
                let q = [p0[0] + d[0] * t, p0[1] + d[1] * t, p0[2] + d[2] * t];
                dist2(p, q) <= r * r && dot(sub(*p0, p), d) <= 0.0 && dot(sub(*p1, p), d) >= 0.0
            }
            // Translated from 0x00d1bc60: into the box frame (rows = axes,
            // row 3 = translation; the axes are orthonormal in the data).
            Zone::Obb { m, ext } => {
                let rel = [p[0] - m[3][0], p[1] - m[3][1], p[2] - m[3][2]];
                (0..3).all(|i| {
                    let axis = [m[i][0], m[i][1], m[i][2]];
                    let l = dot(axis, axis);
                    let local = if l > 0.0 { dot(rel, axis) / l.sqrt() } else { 0.0 };
                    local.abs() <= ext[i]
                })
            }
            // Translated from 0x00767a50.
            Zone::Aabb { min, max } => (0..3).all(|i| min[i] <= p[i] && p[i] <= max[i]),
        }
    }

    /// XZ centre used by the facing test (0x007450e0).
    fn center_xz(&self) -> (f32, f32) {
        match self {
            // ShapeInfoArea vf19 0x00fc7600: average of the four vertices.
            Zone::Area { v, .. } => ((v[0][0] + v[1][0] + v[2][0] + v[3][0]) * 0.25, (v[0][2] + v[1][2] + v[2][2] + v[3][2]) * 0.25),
            Zone::Sphere { c, .. } => (c[0], c[2]),
            // Guessed for non-vertical cylinders: the axis midpoint (the data's
            // TowardFlag cylinders are vertical, so x/z are exact).
            Zone::Cylinder { p0, p1, .. } => ((p0[0] + p1[0]) * 0.5, (p0[2] + p1[2]) * 0.5),
            Zone::Obb { m, .. } => (m[3][0], m[3][2]),
            Zone::Aabb { min, max } => ((min[0] + max[0]) * 0.5, (min[2] + max[2]) * 0.5),
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = sub(a, b);
    dot(d, d)
}

// Translated from 0x00fbd6a0: the triangle is put in a fixed winding
// (b <-> c when needed), then the point must be on the inner side of
// every edge.
fn in_triangle_xz(p: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> bool {
    let cross_y = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
    let (b, c) = if cross_y < 0.0 { (c, b) } else { (b, c) };
    let edge = |p0: [f32; 3], q: [f32; 3]| (q[2] - p0[2]) * (p[0] - p0[0]) - (p[2] - p0[2]) * (q[0] - p0[0]) >= 0.0;
    edge(a, b) && edge(b, c) && edge(c, a)
}

fn wrap_2pi(f: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut r = f - ((f * 0.159_154_94) as i32 as f32) * tau;
    if f < 0.0 {
        r += tau;
    }
    r
}

/// AreaHitShape: the zone plus the angle and facing filters.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub zone: Zone,
    pub angle: Option<(f32, f32)>,
    pub toward: Option<f32>,
}

impl Shape {
    fn from_record(r: &Record) -> Result<Shape, Error> {
        let z = r.object("mpZone").ok_or_else(|| Error::Data("AreaHitShape without zone".into()))?;
        let zone = Zone::from_record(z)?;
        Ok(Shape {
            zone,
            angle: if r.b("AngleFlag").unwrap_or(false) {
                Some((r.f("CheckAngle").unwrap_or(0.0), r.f("CheckRange").unwrap_or(0.0)))
            } else {
                None
            },
            toward: if r.b("TowardFlag").unwrap_or(false) { Some(r.f("CheckToward").unwrap_or(0.0)) } else { None },
        })
    }

    /// cAreaHit vf09 0x00742e60 for a unit at `pos` with yaw `yaw`: the test
    /// point is the unit position raised by 25 (0x007435b0); TowardFlag
    /// requires facing the zone (vf10 0x007432f0); AngleFlag requires the
    /// yaw within the range (0x00745380); then the zone test.
    // Translated from 0x00742e60, 0x007435b0, 0x007432f0, 0x00745380
    // (DDDA.exe 2017-12-13 Steam). The atan2 operand order of 0x012f1109
    // is taken as atan2(dx, dz) (guessed).
    pub fn hit(&self, pos: [f32; 3], yaw: f32) -> bool {
        let p = [pos[0], pos[1] + 25.0, pos[2]];
        if let Some(range) = self.toward {
            let (cx, cz) = self.zone.center_xz();
            let mut d = wrap_2pi((cx - pos[0]).atan2(cz - pos[2]) - yaw);
            if d >= std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            }
            if d.abs() >= range * 0.5 {
                return false;
            }
        }
        if let Some((angle, range)) = self.angle {
            if range <= wrap_2pi(yaw - angle + range * 0.5) {
                return false;
            }
        }
        self.zone.contains(p)
    }
}

/// cScenarioParam (offsets used by 0x00797df0: mTaskType +4, mBootType +8,
/// mCheckType +0xc, priority +0x10, mGsfNo +0x14, mDetectAttr +0x24).
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub task: u32,
    pub boot: u32,
    pub check: u32,
    pub gsf: i32,
    pub detect: u32,
    pub priority: u32,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub param: Param,
    pub shape: Option<Shape>,
    pub arg: Option<Arc<Record>>,
    /// Status (cScenario +0x1c): 1 fired this frame, 2 end requested, 4
    /// disabled, 8 has fired, 0x10 forced, 0x20 removal pending.
    pub status: u32,
    /// Detection mask this frame (cAreaHit +0x10) and the one stored at the
    /// last evaluation (+0x20).
    pub mask: u32,
    pub prev: u32,
    /// End done (+0x24 bit 4).
    pub(crate) ended: bool,
}

impl Entry {
    /// 0x007984b0 / 0x007984f0.
    fn test(&self, mask: u32) -> bool {
        let d = self.param.detect;
        match self.param.check {
            0 => mask & d != 0,
            1 => mask & d == d,
            _ => false,
        }
    }
    pub fn hit_now(&self) -> bool {
        self.shape.is_some() && self.test(self.mask)
    }
}

/// sScenario's lists for the loaded stage.
#[derive(Debug, Default)]
pub struct Scenarios {
    /// `tasks[task]` = entries of that family, in file order
    /// (sScenario +0xd4[task]).
    pub tasks: Vec<Vec<Entry>>,
    /// Evaluation order: priority lists 0..8, load order inside.
    order: Vec<(usize, usize)>,
    /// Best interaction prompt this frame (+0x10c id).
    pub prompt: u32,
}

impl Scenarios {
    /// 0x005083c0 / 0x0049a490.
    pub fn load(files: &[(u32, Arc<mtxfs::Xfs>)]) -> Result<Scenarios, Error> {
        let mut tasks: Vec<Vec<Entry>> = (0..13).map(|_| vec![]).collect();
        for (task, x) in files {
            let root = to_record(x, &x.root);
            let mut list = vec![];
            collect(&root, &mut list);
            for d in list {
                let p = d.object("Param").ok_or_else(|| Error::Data("scenario without Param".into()))?;
                let param = Param {
                    task: p.u("mTaskType").unwrap_or(*task as u64) as u32,
                    boot: p.u("mBootType").unwrap_or(0) as u32,
                    check: p.u("mCheckType").unwrap_or(0) as u32,
                    gsf: p.s("mGsfNo").unwrap_or(-1) as i32,
                    detect: p.u("mDetectAttr").unwrap_or(0x10) as u32,
                    priority: p.u("mPriority").unwrap_or(4) as u32,
                };
                let shape_rec = d.object("Shape");
                let name = shape_rec
                    .and_then(|s| match s.value("Name") {
                        Some(mtxfs::Value::Str(b)) => Some(String::from_utf8_lossy(b).into_owned()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let shape = match shape_rec {
                    Some(s) => Some(Shape::from_record(s)?),
                    None => None,
                };
                tasks[*task as usize].push(Entry {
                    name,
                    status: p.u("mStatus").unwrap_or(0) as u32,
                    param,
                    shape,
                    arg: d.object("Arg").cloned().map(Arc::new),
                    mask: 0,
                    prev: 0,
                    ended: false,
                });
            }
        }
        let mut order = vec![];
        for pri in 0..9 {
            for (t, list) in tasks.iter().enumerate() {
                for (i, e) in list.iter().enumerate() {
                    if e.param.priority.min(8) == pri {
                        order.push((t, i));
                    }
                }
            }
        }
        Ok(Scenarios { tasks, order, prompt: 0 })
    }

    pub fn entry(&self, task: u32, no: u32) -> Option<&Entry> {
        self.tasks.get(task as usize)?.get(no as usize)
    }
    pub fn entry_mut(&mut self, task: u32, no: u32) -> Option<&mut Entry> {
        self.tasks.get_mut(task as usize)?.get_mut(no as usize)
    }

    /// Detection (cAreaHit vf07 0x00742810) for every entry with a shape.
    // Translated from 0x00742810 / 0x00742fe0 / 0x00743070 / 0x00743100
    // (DDDA.exe 2017-12-13 Steam), with the co-op rule above.
    fn detect(&mut self, stage: u32, inputs: &Inputs) -> Result<(), Error> {
        for list in self.tasks.iter_mut() {
            for e in list.iter_mut() {
                e.mask = 0;
                let Some(shape) = &e.shape else { continue };
                let flags = e.param.detect;
                if flags & !0x7f != 0 {
                    return Err(Error::Unimplemented(format!("detection flags {flags:#x} ({})", e.name)));
                }
                if stage == 100 {
                    let anchor = shape.zone.detection_anchor();
                    // 0x00742810: loaded collision cell OR squared distance
                    // <=225000000 (15000 cm). 0x00443a60 supplies the Arisen
                    // world position, or zero when absent. Co-op extension:
                    // any Arisen admits the volume, never only the followed seat.
                    let in_range = |pos| {
                        let d = sub(anchor, pos);
                        d[1] * d[1] + d[0] * d[0] + d[2] * d[2] <= 225_000_000.0
                    };
                    let near = if inputs.players.is_empty() { in_range([0.0; 3]) }
                        else { inputs.players.iter().any(|player| in_range(player.pos)) };
                    if !near && !inputs.loaded_field_cells.as_ref().is_some_and(|cells| cells.contains(anchor)) {
                        continue;
                    }
                }
                if flags & 0x1f != 0 {
                    let members = if flags & 0xf != 0 { flags & 0xf } else { 0xf };
                    if members & 1 != 0 && inputs.players.iter().any(|pl| shape.hit(pl.pos, pl.yaw)) {
                        e.mask |= 1;
                    }
                    for (i, pawn) in inputs.pawns.iter().take(3).enumerate() {
                        if members & (2 << i) != 0 && shape.hit(pawn.pos, pawn.yaw) {
                            e.mask |= 2 << i;
                        }
                    }
                    if e.mask & 0xf != 0 {
                        e.mask |= 0x10;
                    }
                }
                if flags & 0x20 != 0 && inputs.enemies.iter().any(|en| shape.hit(en.pos, en.yaw)) {
                    e.mask |= 0x20;
                }
                if flags & 0x40 != 0 && inputs.npcs.iter().any(|n| shape.hit(n.unit.pos, n.unit.yaw)) {
                    e.mask |= 0x40;
                }
            }
        }
        Ok(())
    }

    /// sScenario vf06 0x0049a0e0 (+ detection first; the original's order
    /// between sAreaHit and sScenario is not traced).
    // Translated from 0x0049a0e0 and 0x00797df0 (DDDA.exe 2017-12-13 Steam).
    pub fn frame(&mut self, env: &mut Env, inputs: &Inputs) -> Result<(), Error> {
        self.detect(env.stage.stage_no, inputs)?;
        // Cleared every frame: AreaStayFlag, GSF 1501, GSF 167.
        env.world.area_stay = 0;
        env.world.gsf.set(1501, false);
        env.world.gsf.set(167, false);
        self.prompt = 0;
        let mut best = 8;
        let button = inputs.interact;
        env.stage.interacting = button;
        for &(t, i) in &self.order.clone() {
            let e = &self.tasks[t][i];
            if e.status & 0x22 == 0 {
                if let Some(prompt) = self.evaluate(env, t, i, button)? {
                    // 0x0049a960: keep the best prompt (priority below 8).
                    let pri = self.tasks[t][i].param.priority;
                    if prompt != 0 && pri < best {
                        best = pri;
                        self.prompt = prompt;
                    }
                }
            } else if e.status & 2 != 0 && e.status & 0x20 == 0 {
                self.tasks[t][i].status |= 0x20;
                self.end(t, i)?;
            }
            // Status 0x20: the game removes the entry (0x0049a7f0); here it
            // stays in place (so later indices keep their meaning) and is
            // never evaluated again.
        }
        Ok(())
    }

    pub(crate) fn end(&mut self, t: usize, i: usize) -> Result<(), Error> {
        let e = &mut self.tasks[t][i];
        if !e.ended {
            if e.param.task == 11 {
                return Err(Error::Unimplemented("npc scenario end callback 0x0079d7a0".into()));
            }
            e.ended = true;
        }
        Ok(())
    }

    /// 0x00797df0 for one entry; returns the prompt it registers, if any.
    fn evaluate(&mut self, env: &mut Env, t: usize, i: usize, button: Option<usize>) -> Result<Option<u32>, Error> {
        let button = button.is_some();
        let (status, gsf, boot, has_shape, check) = {
            let e = &self.tasks[t][i];
            (e.status, e.param.gsf, e.param.boot, e.shape.is_some(), e.param.check)
        };
        if status & 4 != 0 || status & 0x22 != 0 {
            return Ok(None);
        }
        // Gate 0x00798620: inactive while GSF mGsfNo is set (pawn task:
        // while it is clear).
        if (0..0x600).contains(&gsf) {
            let set = env.world.gsf.get(gsf as usize);
            if if t == 9 { !set } else { set } {
                return Ok(None);
            }
        }
        self.tasks[t][i].status &= !1;
        let (h, p) = {
            let e = &self.tasks[t][i];
            (e.test(e.mask), e.test(e.prev))
        };
        let latched = status & 8 != 0;
        let forced = status & 0x10 != 0;
        let mut prompt = None;
        macro_rules! need_shape {
            () => {
                if !has_shape || check == 2 {
                    return Ok(None);
                }
            };
        }
        match boot {
            0 => self.fire(env, t, i)?,
            1 => {
                if !has_shape {
                    return Ok(None);
                }
                if latched || h || forced {
                    self.fire(env, t, i)?;
                    let e = &mut self.tasks[t][i];
                    e.status = (e.status & !0x10) | 9;
                }
            }
            2 => {
                need_shape!();
                if h || forced {
                    self.fire(env, t, i)?;
                    self.tasks[t][i].status |= 9;
                    if t != 7 {
                        prompt = Some(self.prompt_of(env, t, i)?);
                    }
                } else if p {
                    self.end(t, i)?;
                }
            }
            3 => {
                need_shape!();
                if latched {
                    return Ok(None);
                }
                if h {
                    self.fire(env, t, i)?;
                    self.end(t, i)?;
                    self.tasks[t][i].status |= 9;
                } else if forced {
                    self.fire(env, t, i)?;
                    self.end(t, i)?;
                }
            }
            4 => {
                need_shape!();
                if forced || (h && !p) {
                    self.fire(env, t, i)?;
                    self.end(t, i)?;
                    self.tasks[t][i].status |= 9;
                }
            }
            5 | 6 => {
                if boot == 6 && latched {
                    return Ok(None);
                }
                if h || forced {
                    if button {
                        self.fire(env, t, i)?;
                        self.end(t, i)?;
                        self.tasks[t][i].status |= 9;
                    } else {
                        prompt = Some(self.prompt_of(env, t, i)?);
                    }
                }
            }
            8 => {
                need_shape!();
                if latched {
                    return Ok(None);
                }
                if h || forced {
                    self.fire(env, t, i)?;
                    let e = &mut self.tasks[t][i];
                    e.status = (e.status & !0x18) | 1;
                    prompt = Some(self.prompt_of(env, t, i)?);
                } else if p {
                    self.end(t, i)?;
                    self.tasks[t][i].status |= 9;
                }
            }
            9 => {
                need_shape!();
                if latched {
                    return Ok(None);
                }
                if !h || forced {
                    self.fire(env, t, i)?;
                    self.end(t, i)?;
                    self.tasks[t][i].status |= 9;
                }
            }
            10 => {
                need_shape!();
                if !h && p {
                    self.fire(env, t, i)?;
                    self.end(t, i)?;
                    self.tasks[t][i].status |= 9;
                }
                if h && forced {
                    self.fire(env, t, i)?;
                    self.end(t, i)?;
                }
            }
            other => return Err(Error::Unimplemented(format!("scenario boot type {other}"))),
        }
        let e = &mut self.tasks[t][i];
        if e.shape.is_some() {
            e.prev = e.mask;
        }
        Ok(prompt)
    }

    /// Prompt ids (0x00798530).
    fn prompt_of(&self, env: &Env, t: usize, i: usize) -> Result<u32, Error> {
        let arg = self.tasks[t][i].arg.as_deref();
        let field = |n: &str| arg.and_then(|a| a.u(n)).unwrap_or(0) as u32;
        Ok(match t {
            2 => field("mTouchType"),
            4 => field("mTouchType"),
            7 => 0xd,
            8 => {
                if env.stage.message_showing {
                    0
                } else if field("mPlAct") != 0 {
                    0x26
                } else {
                    3
                }
            }
            10 => 3,
            _ => 0,
        })
    }

    /// 0x00798250: run the task callback; 1 disables, 2 ends and removes.
    fn fire(&mut self, env: &mut Env, t: usize, i: usize) -> Result<(), Error> {
        self.tasks[t][i].status |= 1;
        let r = task_callback(env, &self.tasks[t][i])?;
        match r {
            1 => self.tasks[t][i].status |= 4,
            2 => {
                self.end(t, i)?;
                self.tasks[t][i].status |= 2;
            }
            _ => {}
        }
        Ok(())
    }
}

/// The task callbacks of 0x00798ce0 the opening's stages use.
fn task_callback(env: &mut Env, e: &Entry) -> Result<u32, Error> {
    let arg = e.arg.as_deref();
    let field_u = |n: &str| arg.and_then(|a| a.u(n));
    let field_s = |n: &str| arg.and_then(|a| a.s(n));
    match e.param.task {
        // Translated from 0x00799180 -> 0x007990d0 (flag).
        1 => {
            let act = field_u("mActType").unwrap_or(0);
            if act & 1 != 0 {
                let n = field_s("mFSMAreaHitFlagNo").unwrap_or(-1);
                if (0..32).contains(&n) {
                    let set = field_u("mFSMAreaHitFlagSet").unwrap_or(0) == 1;
                    env.world.set_area_hit(env.stage.stage_no, n as usize, set);
                }
            }
            if act & 2 != 0 {
                let n = field_s("mFSMAreaStayFlagNo").unwrap_or(-1);
                if (0..32).contains(&n) {
                    let m = 0x8000_0000u32 >> n;
                    if field_u("mFSMAreaStayFlagSet").unwrap_or(0) == 1 {
                        env.world.area_stay |= m;
                    } else {
                        env.world.area_stay &= !m;
                    }
                }
            }
            Ok(0)
        }
        // Translated from 0x00799560 -> 0x00799480 (door): area jump when
        // the trigger includes the Arisen (mask bit 0). The stage
        // 401/402/601 GSF write is not needed by the opening.
        2 => {
            if e.mask & 1 != 0 {
                let stage = field_u("mStageNo").unwrap_or(0) as u32;
                let pos = field_u("mPosNo").unwrap_or(0) as u32;
                if matches!(env.stage.stage_no, 401 | 402 | 601) {
                    return Err(Error::Unimplemented("door GSF on stages 401/402/601".into()));
                }
                if (stage, pos) != (env.world.stage_no, env.world.start_pos) {
                    env.world.stage_no = stage;
                    env.world.start_pos = pos;
                    env.out.push(Request::JumpArea { stage, pos, fade: field_u("mFadeType").unwrap_or(0) as u32 });
                }
            }
            Ok(0)
        }
        // Translated from 0x007997c0 (gsf): type 0 only; set 0 / clear 1.
        3 => {
            if field_u("mFlagType").unwrap_or(0) == 0 {
                let n = field_u("mFlagNo").unwrap_or(u64::MAX);
                match field_u("mSetType").unwrap_or(0) {
                    0 => env.world.gsf.set(n as usize, true),
                    1 => env.world.gsf.set(n as usize, false),
                    _ => return Err(Error::Unimplemented("gsf scenario set type 2 (0x00798b70)".into())),
                }
            }
            Ok(0)
        }
        // Translated from 0x00799ca0 -> 0x00799be0 (fsm): start each script
        // of mFSMList (0x005080e0).
        4 => {
            if field_u("mActType").unwrap_or(0) == 0 {
                if let Some(a) = arg {
                    let mut paths = vec![];
                    fsm_names(a, &mut paths);
                    for p in paths {
                        env.cmds.push(crate::story::Command::StartScript { path: p });
                    }
                }
            }
            Ok(0)
        }
        // Translated from 0x0079d010 (etc): act 0 raises request bit 24,
        // which stage play mode turns into the vocation menu (mode 0x19).
        10 => match field_u("mActType").unwrap_or(0) {
            0 => {
                // The request bit stays set until stage mode 0x19 takes it;
                // while the menu is open a second press adds nothing.
                if !env.stage.menu_open {
                    env.stage.menu_open = true;
                    let player = env.stage.interacting.unwrap_or(0);
                    env.out.push(Request::OpenMenu { menu: crate::Menu::Vocation, player });
                }
                Ok(0)
            }
            other => Err(Error::Unimplemented(format!("etc scenario act {other}"))),
        },
        // Translated from 0x0079c970 (pawn): if every "check ON" flag is set
        // and every "check OFF" flag clear, set the "ON" flags and clear the
        // "OFF" flags. cAIFlag FLAG_TYPE 1 = GSF FLAG_NO, 2 = quest QUEST_NO
        // bit FLAG_NO (code (QUEST_NO + 10000) * 10000 + FLAG_NO, 0x005b5540).
        // Types 3 (pawn flags) and 4 (+0xb8730 bits) are not translated.
        // What the host does with the pawns is handed over as a request.
        9 => {
            let Some(a) = arg else { return Ok(2) };
            let lists = |n: &str| -> Result<Vec<(u64, u64, u64)>, Error> {
                let mut out = vec![];
                if let Some((_, vs)) = a.fields.iter().find(|(k, _)| k == n) {
                    for v in vs {
                        if let Parameter::Object(Some(arr)) = v {
                            for (_, items) in arr.fields.iter().filter(|(k, _)| k == "mpArray") {
                                for it in items {
                                    if let Parameter::Object(Some(f)) = it {
                                        let t = f.u("FLAG_TYPE").unwrap_or(0);
                                        if t == 0 {
                                            continue;
                                        }
                                        if t > 2 {
                                            return Err(Error::Unimplemented(format!("pawn scenario flag type {t}")));
                                        }
                                        out.push((t, f.u("QUEST_NO").unwrap_or(0), f.u("FLAG_NO").unwrap_or(0)));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(out)
            };
            let get = |w: &crate::World, (t, q, n): (u64, u64, u64)| {
                if t == 1 {
                    w.gsf.get(n as usize)
                } else {
                    w.quest_bit(q as usize, n as usize)
                }
            };
            let check_on = lists("チェックするONフラグ")?;
            let check_off = lists("チェックするOFFフラグ")?;
            if !check_on.iter().all(|&f| get(env.world, f)) || check_off.iter().any(|&f| get(env.world, f)) {
                return Ok(0);
            }
            for (on, n) in [(true, "ONするフラグ"), (false, "OFFするフラグ")] {
                for (t, q, no) in lists(n)? {
                    if t == 1 {
                        env.world.gsf.set(no as usize, on);
                    } else {
                        env.world.set_quest_bit(q as usize, no as usize, on);
                    }
                }
            }
            env.out.push(Request::Scenario { task: 9, name: e.name.clone(), arg: None });
            Ok(0)
        }
        // Ladder (6), camera (7), message (8): host-side actions the story
        // does not interpret; handed over as a request.
        6..=8 => {
            env.out.push(Request::Scenario { task: e.param.task, name: e.name.clone(), arg: e.arg.as_deref().cloned() });
            Ok(0)
        }
        // Translated from 0x0079d730 (DDDA.exe 2017-12-13 Steam).
        // NPC archive scenarios only acquire their resources within the
        // authored half-open SceNo interval. Active archive ownership
        // (0x0079d530 / 0x0079d7a0) remains explicitly unsupported; therefore
        // there cannot yet be an acquired lease to release outside it.
        11 => {
            let Some(arg) = arg.filter(|a| a.class_name == Some("cScenarioArg_NpcArc")) else {
                return Ok(2);
            };
            let start = arg.u("mSceNo_Start").unwrap_or(0) as u16;
            let end = arg.u("mSceNo_End").unwrap_or(0) as u16;
            let scene = env.world.sce_no as i32;
            if scene < i32::from(start) || scene >= i32::from(end) {
                Ok(0)
            } else {
                Err(Error::Unimplemented("active NPC scenario archive ownership (0x0079d530)".into()))
            }
        }
        // Translated from 0x00798ce0 / 0x00798250 (DDDA.exe 2017-12-13 Steam):
        // only IDs 1..=11 have callbacks. A null callback retains result zero,
        // so it neither disables nor ends the scenario and emits no action.
        0 | 12.. => Ok(0),
        other => Err(Error::Unimplemented(format!("scenario task {other}"))),
    }
}

fn fsm_names(r: &Record, out: &mut Vec<String>) {
    if let Some(mtxfs::Value::Str(s)) = r.value("mFSMName") {
        if !s.is_empty() {
            out.push(String::from_utf8_lossy(s).into_owned());
        }
    }
    for (_, vs) in &r.fields {
        for v in vs {
            if let Parameter::Object(Some(o)) = v {
                fsm_names(o, out);
            }
        }
    }
}

fn collect(r: &Record, out: &mut Vec<Record>) {
    if r.class_name == Some("cScenarioData") {
        out.push(r.clone());
        return;
    }
    for (_, vs) in &r.fields {
        for v in vs {
            if let Parameter::Object(Some(o)) = v {
                collect(o, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_task_callbacks_preserve_status_and_emit_no_actions() {
        let host = crate::scripted::ScriptedHost::default();
        let mut world = crate::World::default();
        let mut stage = crate::story::StageState::default();
        let mut out = vec![];
        let mut cmds = vec![];
        let mut env = Env { world: &mut world, stage: &mut stage, host: &host,
            time: Default::default(), out: &mut out, cmds: &mut cmds, events: None };
        let mut areas = enemy_area([0.0; 3]);
        for task in [0, 12, 13, u32::MAX] {
            areas.tasks[0][0].param.task = task;
            areas.tasks[0][0].status = 0x18;
            areas.fire(&mut env, 0, 0).unwrap();
            assert_eq!(areas.tasks[0][0].status, 0x19, "only the caller's fired bit changes");
            assert!(!areas.tasks[0][0].ended);
        }
        assert!(env.out.is_empty());
        assert!(env.cmds.is_empty());
        // Known non-null callbacks must still expose missing implementations.
        areas.tasks[0][0].param.task = 5;
        assert!(matches!(task_callback(&mut env, &areas.tasks[0][0]), Err(Error::Unimplemented(_))));
    }

    #[test]
    fn npc_archives_only_load_inside_the_authored_scene_interval() {
        let host = crate::encounter::EncounterFacts::default();
        let mut world = crate::World::default();
        let mut stage = crate::story::StageState::default();
        let mut out = vec![];
        let mut cmds = vec![];
        let mut env = Env { world: &mut world, stage: &mut stage, host: &host,
            time: Default::default(), out: &mut out, cmds: &mut cmds, events: None };
        let mut areas = enemy_area([0.0; 3]);
        let entry = &mut areas.tasks[0][0];
        entry.param.task = 11;
        assert_eq!(task_callback(&mut env, entry).unwrap(), 2);
        entry.arg = Some(Arc::new(Record { class_hash: 0, class_name: Some("cScenarioArg_NpcArc"),
            fields: vec![("mSceNo_Start".into(), vec![Parameter::Scalar(mtxfs::Value::U(100))]),
                ("mSceNo_End".into(), vec![Parameter::Scalar(mtxfs::Value::U(200))])] }));
        for scene in [0, 99, 200, 201, u32::MAX] {
            env.world.sce_no = scene;
            assert_eq!(task_callback(&mut env, entry).unwrap(), 0);
        }
        for scene in [100, 150, 199] {
            env.world.sce_no = scene;
            assert!(matches!(task_callback(&mut env, entry), Err(Error::Unimplemented(_))));
        }
        assert!(out.is_empty());
    }

    fn enemy_area(anchor: [f32; 3]) -> Scenarios {
        Scenarios { tasks: vec![vec![Entry {
            name: "range fixture".into(),
            param: Param { task: 0, boot: 0, check: 0, gsf: -1, detect: 0x20, priority: 0 },
            shape: Some(Shape { zone: Zone::Sphere { c: anchor, r: 1.0 }, angle: None, toward: None }),
            arg: None, status: 4, mask: 0, prev: 0, ended: false,
        }]], ..Default::default() }
    }

    #[test]
    fn field_detection_has_inclusive_3d_range_and_any_arisen_admission() {
        let mut areas = enemy_area([50_000.0, 0.0, 0.0]);
        let player = |pos| crate::Player { pos, yaw: 0.0 };
        let mut input = Inputs {
            players: vec![player([35_000.0, 0.0, 0.0])],
            enemies: vec![player([50_000.0, -25.0, 0.0])], ..Default::default()
        };
        areas.detect(100, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0x20, "distance equality admits");
        input.players[0].pos[1] = 100.0;
        areas.detect(100, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0, "vertical separation matters and stale masks clear");
        input.players.push(player([50_000.0, 0.0, 0.0]));
        areas.detect(100, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0x20, "second Arisen admits independently");
        input.players.clear();
        areas.detect(100, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0, "absent actor position falls back to zero");
        areas.detect(200, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0x20, "whole stages have no range filter");
    }

    #[test]
    fn loaded_collision_exception_requires_matching_cell_and_preserves_shape_test() {
        let mut areas = enemy_area([50_000.0, 0.0, 0.0]);
        let mut input = Inputs {
            players: vec![crate::Player::default()],
            enemies: vec![crate::Player { pos: [50_000.0, -25.0, 0.0], yaw: 0.0 }],
            loaded_field_cells: Some(LoadedFieldCells {
                origin: [0.0; 2], cell: [10_000.0; 2], name_offset: [30, 40], loaded: vec![(40, 35)],
            }), ..Default::default()
        };
        areas.detect(100, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0x20);
        input.enemies[0].pos[0] += 10.0;
        areas.detect(100, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0, "loaded cell does not fabricate a hit");
        input.enemies[0].pos[0] -= 10.0;
        input.loaded_field_cells.as_mut().unwrap().loaded.clear();
        areas.detect(100, &input).unwrap();
        assert_eq!(areas.tasks[0][0].mask, 0, "unloaded cell stops far detection");
    }

    #[test]
    fn field_cell_lookup_truncates_and_uses_filename_axis_order() {
        let cells = LoadedFieldCells { origin: [10.0, 20.0], cell: [10.0, 10.0],
            name_offset: [100, 200], loaded: vec![(200, 100)] };
        assert!(cells.contains([9.0, 99_999.0, 19.0]));
        assert!(!cells.contains([0.0, 0.0, 19.0]));
        assert!(!cells.contains([20.0, 0.0, 20.0]));
    }

    #[test]
    fn detection_anchor_preserves_source_bottom_and_endpoint_rules() {
        let area = Zone::Area { v: [[0.0, 2.0, 0.0], [4.0, 2.0, 0.0], [4.0, 6.0, 4.0], [0.0, 6.0, 4.0]],
            bottom: 99.0, height: 20.0, cross: [0.0; 3], convex: true, concave_status: 0 };
        assert_eq!(area.detection_anchor(), [2.0, 4.0, 2.0]);
        assert_eq!(Zone::Cylinder { p0: [1.0, 2.0, 3.0], p1: [8.0, 9.0, 10.0], r: 2.0 }.detection_anchor(), [1.0, 2.0, 3.0]);
        assert_eq!(Zone::Aabb { min: [0.0, 2.0, 4.0], max: [6.0, 8.0, 10.0] }.detection_anchor(), [3.0, 2.0, 7.0]);
        let mut m = [[0.0; 4]; 4];
        m[3] = [5.0, 8.0, 9.0, 1.0];
        assert_eq!(Zone::Obb { m, ext: [1.0, 3.0, 2.0] }.detection_anchor(), [5.0, 5.0, 9.0]);
    }

    #[test]
    fn obb_and_cylinder() {
        let obb = Zone::Obb {
            m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [10.0, 0.0, 0.0, 1.0]],
            ext: [2.0, 2.0, 2.0],
        };
        assert!(obb.contains([11.0, 1.0, -1.0]));
        assert!(!obb.contains([13.0, 0.0, 0.0]));
        let cyl = Zone::Cylinder { p0: [0.0, 0.0, 0.0], p1: [0.0, 10.0, 0.0], r: 2.0 };
        assert!(cyl.contains([1.0, 5.0, 1.0]));
        assert!(!cyl.contains([0.0, 11.0, 0.0]));
        assert!(!cyl.contains([3.0, 5.0, 0.0]));
    }

    #[test]
    fn area_quad() {
        let a = Zone::Area {
            v: [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.0, 10.0], [0.0, 0.0, 10.0]],
            bottom: 0.0,
            height: 5.0,
            cross: [0.0; 3],
            convex: true,
            concave_status: 0,
        };
        assert!(a.contains([5.0, 1.0, 5.0]));
        assert!(!a.contains([5.0, 6.0, 5.0]));
        assert!(!a.contains([11.0, 1.0, 5.0]));
    }

    #[test]
    fn angle_filter() {
        let s = Shape {
            zone: Zone::Sphere { c: [0.0; 3], r: 100.0 },
            angle: Some((3.53, 2.09)),
            toward: None,
        };
        assert!(s.hit([0.0, 0.0, 0.0], 3.53));
        assert!(!s.hit([0.0, 0.0, 0.0], 0.0));
    }
}
