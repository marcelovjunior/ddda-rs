//! World queries over stage data, with no rendering.
//!
//! `Terrain` answers "how high is the ground here" from the stage cells'
//! terrain models (cell-local x, z; absolute y), placed in the stage frame
//! (origin cell 50m50n, docs/WORLD.md). It is a stand-in for the real
//! collision (`rCollision`/SBC), which is not read yet: scenery and walls
//! do not block, and the terrain model is a render mesh.

use std::collections::HashMap;

use ddvfs::Vfs;
use mtshader::ShaderPackage;

const RMODEL: u32 = 0x58A1_5856;
const RCOLLISION: u32 = 0x51FC_779F;
const RLAYOUT: u32 = 0x1530_2EF4;
/// Side of a grid bucket, cm.
const BUCKET: f32 = 400.0;

pub struct Terrain {
    tris: Vec<[[f32; 3]; 3]>,
    /// Per triangle: its collision material's flag word (the material
    /// record's first word; 0 for render terrain).
    flags: Vec<u32>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    pub cells: usize,
}

/// Rotation matrix (rows) for intrinsic X, then Y, then Z Euler angles,
/// matching `Quat::from_euler(EulerRot::XYZ, ..)` in the viewer.
fn euler_xyz(a: [f32; 3]) -> [[f32; 3]; 3] {
    let (sx, cx) = a[0].sin_cos();
    let (sy, cy) = a[1].sin_cos();
    let (sz, cz) = a[2].sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let mul = |p: [[f32; 3]; 3], q: [[f32; 3]; 3]| {
        let mut r = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = (0..3).map(|k| p[i][k] * q[k][j]).sum();
            }
        }
        r
    };
    mul(mul(rx, ry), rz)
}

/// The flag word of a collision triangle's material (the first of its
/// record's eight words, e.g. 0x2000 or 0x2 for the ground sCollision
/// counts as wall whatever its slope, `sCollision` +0x4C).
fn material_flags(c: &mtsbc::Collision, t: &mtsbc::Triangle) -> u32 {
    c.materials.get(usize::from(t.material)).map_or(0, |m| m[0])
}

fn bucket(x: f32, z: f32) -> (i32, i32) {
    ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32)
}

/// Triangles steeper than this (|normal.y| below it) are walls.
const WALL_MAX_NY: f32 = 0.7;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Closest point on triangle `t` to `p` (Ericson, Real-Time Collision
/// Detection, 5.1.5).
fn closest_on_triangle(p: [f32; 3], t: &[[f32; 3]; 3]) -> [f32; 3] {
    let [a, b, c] = *t;
    let lerp = |o: [f32; 3], d: [f32; 3], k: f32| [o[0] + d[0] * k, o[1] + d[1] * k, o[2] + d[2] * k];
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return lerp(a, ab, d1 / (d1 - d3));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return lerp(a, ac, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return lerp(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [
        a[0] + ab[0] * v + ac[0] * w,
        a[1] + ab[1] * v + ac[1] * w,
        a[2] + ab[2] * v + ac[2] * w,
    ]
}

impl Terrain {
    pub fn empty() -> Self {
        Terrain {
            tris: Vec::new(),
            flags: Vec::new(),
            grid: HashMap::new(),
            cells: 0,
        }
    }

    /// Loads the terrain of cells (a0 +- radius, b0 +- radius) of `stage`.
    pub fn load(vfs: &Vfs, mfx: &ShaderPackage, stage: u32, a0: i32, b0: i32, radius: i32) -> Self {
        let mut t = Terrain::empty();
        for a in a0 - radius..=a0 + radius {
            for b in b0 - radius..=b0 + radius {
                let path = format!("scr\\st{stage}\\model\\m{}\\st{stage}_{a}m{b}n", a / 10 * 10);
                let Ok(bytes) = vfs.read(&path, RMODEL) else {
                    continue;
                };
                let Ok(model) = mtmod::parse(&bytes) else { continue };
                let off = [(b - 50) as f32 * 10000.0, 0.0, (a - 50) as f32 * 10000.0];
                for m in &model.meshes {
                    if m.lod & 1 == 0 {
                        continue;
                    }
                    let Some(layout) = mfx.layout(m.vertex_format) else {
                        continue;
                    };
                    let (Ok(v), Ok(tris)) = (model.decode(m, layout), model.triangles(m)) else {
                        continue;
                    };
                    for tri in tris {
                        let p = tri.map(|i| {
                            let q = v.positions[usize::from(i)];
                            [q[0] + off[0], q[1], q[2] + off[2]]
                        });
                        t.add(p);
                    }
                }
                t.cells += 1;
            }
        }
        t
    }

    /// Loads the cells' merged stage collision (`st<S>h_<A>m<B>n_mrg00`,
    /// cell-local like the terrain model) instead of the render terrain.
    /// The `h` file is what characters stand on and run into: the loader
    /// (`0x00C5F680`) stores it as `cSplitSbc::mpClsnScrMrgRsrc`; the `e` file
    /// is `mpClsnEffMrgRsrc` (effects) and is not used for movement.
    pub fn load_collision(vfs: &Vfs, stage: u32, a0: i32, b0: i32, radius: i32) -> Self {
        let mut t = Terrain::empty();
        for a in a0 - radius..=a0 + radius {
            for b in b0 - radius..=b0 + radius {
                let off = [(b - 50) as f32 * 10000.0, 0.0, (a - 50) as f32 * 10000.0];
                let mut any = false;
                for kind in ["h"] {
                    let path = format!(
                        r"scr\st{stage}\collision\m{}\marge\st{stage}{kind}_{a}m{b}n_mrg00",
                        a / 10 * 10
                    );
                    let Ok(bytes) = vfs.read(&path, RCOLLISION) else {
                        continue;
                    };
                    let Ok(c) = mtsbc::parse(&bytes) else { continue };
                    for (p, tri) in c.world_triangles() {
                        t.add_flagged(
                            p.map(|q| [q[0] + off[0], q[1], q[2] + off[2]]),
                            material_flags(&c, tri),
                        );
                    }
                    any = true;
                }
                if any {
                    t.cells += 1;
                }
            }
        }
        t
    }

    /// Adds the collision of each cell's placed scenery (`_s00` layout): every
    /// instance whose model has `model\om\...\collision\<id>_h00` (the
    /// stage-collision half; `_e00` is the effect half, as for the cells),
    /// transformed by the instance's scale, rotation and position into the
    /// stage frame (layout positions are already stage-frame; docs/WORLD.md).
    /// Rotation uses the same Euler convention as the viewer (*assumed* XYZ:
    /// R = Rx * Ry * Rz on column vectors).
    pub fn add_scenery(&mut self, vfs: &Vfs, stage: u32, a0: i32, b0: i32, radius: i32) -> usize {
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for p in vfs
            .paths_of_type(RCOLLISION)
            .filter(|p| p.starts_with(r"model\om\"))
        {
            let base = p.rsplit('\\').next().unwrap_or(p);
            if let Some(id) = base.strip_suffix("_h00") {
                by_name.entry(id.to_string()).or_default().push(p.to_string());
            }
        }
        let mut instances = 0;
        for a in a0 - radius..=a0 + radius {
            for b in b0 - radius..=b0 + radius {
                let lot = format!(r"scr\st{stage}\etc\st{stage}_{a}m{b}n_s00");
                let Ok(bytes) = vfs.read(&lot, RLAYOUT) else {
                    continue;
                };
                let Ok(records) = mtlot::parse(&bytes) else {
                    continue;
                };
                for r in &records {
                    let Some(pl) = r.placement() else { continue };
                    let id = pl.name.split('_').next().unwrap_or("").to_ascii_lowercase();
                    let Some(paths) = by_name.get(&id) else { continue };
                    let m = euler_xyz(pl.angle);
                    for path in paths {
                        let Ok(cb) = vfs.read(path, RCOLLISION) else {
                            continue;
                        };
                        let Ok(c) = mtsbc::parse(&cb) else { continue };
                        for (tri, rec) in c.world_triangles() {
                            let flags = material_flags(&c, rec);
                            self.add_flagged(
                                tri.map(|q| {
                                    let s = [q[0] * pl.scale[0], q[1] * pl.scale[1], q[2] * pl.scale[2]];
                                    let mut w = [0.0f32; 3];
                                    for (i, wi) in w.iter_mut().enumerate() {
                                        *wi =
                                            m[i][0] * s[0] + m[i][1] * s[1] + m[i][2] * s[2] + pl.position[i];
                                    }
                                    w
                                }),
                                flags,
                            );
                        }
                    }
                    instances += 1;
                }
            }
        }
        instances
    }

    pub fn add(&mut self, p: [[f32; 3]; 3]) {
        self.add_flagged(p, 0);
    }

    /// Adds a triangle whose collision material has the flag word `flags`.
    pub fn add_flagged(&mut self, p: [[f32; 3]; 3], flags: u32) {
        let id = self.tris.len() as u32;
        let (minx, maxx) = (
            p[0][0].min(p[1][0]).min(p[2][0]),
            p[0][0].max(p[1][0]).max(p[2][0]),
        );
        let (minz, maxz) = (
            p[0][2].min(p[1][2]).min(p[2][2]),
            p[0][2].max(p[1][2]).max(p[2][2]),
        );
        let (b0, b1) = (bucket(minx, minz), bucket(maxx, maxz));
        for bx in b0.0..=b1.0 {
            for bz in b0.1..=b1.1 {
                self.grid.entry((bx, bz)).or_default().push(id);
            }
        }
        self.tris.push(p);
        self.flags.push(flags);
    }

    pub fn triangles(&self) -> usize {
        self.tris.len()
    }
}

impl ddsim_ground::Ground for Terrain {
    fn height(&self, x: f32, z: f32, below_y: f32) -> Option<f32> {
        let ids = self.grid.get(&bucket(x, z))?;
        let mut best: Option<f32> = None;
        for &id in ids {
            let p = &self.tris[id as usize];
            let d = (p[1][2] - p[2][2]) * (p[0][0] - p[2][0]) + (p[2][0] - p[1][0]) * (p[0][2] - p[2][2]);
            if d.abs() < 1e-6 {
                continue;
            }
            let l0 = ((p[1][2] - p[2][2]) * (x - p[2][0]) + (p[2][0] - p[1][0]) * (z - p[2][2])) / d;
            let l1 = ((p[2][2] - p[0][2]) * (x - p[2][0]) + (p[0][0] - p[2][0]) * (z - p[2][2])) / d;
            let l2 = 1.0 - l0 - l1;
            if l0 < -1e-4 || l1 < -1e-4 || l2 < -1e-4 {
                continue;
            }
            let y = l0 * p[0][1] + l1 * p[1][1] + l2 * p[2][1];
            if y <= below_y && best.is_none_or(|b| y > b) {
                best = Some(y);
            }
        }
        best
    }

    /// Wall triangles (steeper than `WALL_MAX_NY`) near the vertical segment,
    /// sampled at heights `low`..`high` no further apart than `radius`.
    fn wall(&self, p: [f32; 3], radius: f32, low: f32, high: f32) -> Option<[f32; 2]> {
        let (b0, b1) = (
            bucket(p[0] - radius, p[2] - radius),
            bucket(p[0] + radius, p[2] + radius),
        );
        let samples = ((high - low) / radius).ceil().max(1.0) as usize;
        let mut best: Option<(f32, [f32; 2])> = None;
        let mut seen = std::collections::HashSet::new();
        for bx in b0.0..=b1.0 {
            for bz in b0.1..=b1.1 {
                for &id in self.grid.get(&(bx, bz)).into_iter().flatten() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let t = &self.tris[id as usize];
                    let (ylo, yhi) = (
                        t[0][1].min(t[1][1]).min(t[2][1]),
                        t[0][1].max(t[1][1]).max(t[2][1]),
                    );
                    if yhi < low || ylo > high {
                        continue;
                    }
                    let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
                    let len = dot(n, n).sqrt();
                    if len < 1e-6 || (n[1] / len).abs() >= WALL_MAX_NY {
                        continue;
                    }
                    for k in 0..=samples {
                        let y = low + (high - low) * k as f32 / samples as f32;
                        let q = [p[0], y, p[2]];
                        let c = closest_on_triangle(q, t);
                        let (dx, dz) = (q[0] - c[0], q[2] - c[2]);
                        let d = (dx * dx + (q[1] - c[1]).powi(2) + dz * dz).sqrt();
                        if d >= radius || best.is_some_and(|(bd, _)| bd <= d) {
                            continue;
                        }
                        let h = (dx * dx + dz * dz).sqrt();
                        let dir = if h > 1e-3 {
                            [dx / h, dz / h]
                        } else {
                            // on the wall: the face normal's side facing the segment
                            let hn = (n[0] * n[0] + n[2] * n[2]).sqrt();
                            [n[0] / hn, n[2] / hn]
                        };
                        best = Some((d, dir));
                    }
                }
            }
        }
        best.map(|(_, dir)| dir)
    }

    /// Nearest triangle crossing of the segment (Moller-Trumbore, both
    /// faces). A triangle the segment starts on does not count.
    fn ray(&self, from: [f32; 3], to: [f32; 3]) -> Option<ddsim::RayHit> {
        let d = sub(to, from);
        let (b0, b1) = (
            bucket(from[0].min(to[0]), from[2].min(to[2])),
            bucket(from[0].max(to[0]), from[2].max(to[2])),
        );
        let mut best: Option<(f32, [f32; 3])> = None;
        let mut seen = std::collections::HashSet::new();
        for bx in b0.0..=b1.0 {
            for bz in b0.1..=b1.1 {
                for &id in self.grid.get(&(bx, bz)).into_iter().flatten() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let t = &self.tris[id as usize];
                    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
                    let p = cross(d, e2);
                    let det = dot(e1, p);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let s = sub(from, t[0]);
                    let u = dot(s, p) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = cross(s, e1);
                    let v = dot(d, q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let k = dot(e2, q) * inv;
                    if !(1e-4..=1.0).contains(&k) || best.is_some_and(|(bk, _)| bk <= k) {
                        continue;
                    }
                    let n = cross(e1, e2);
                    let len = dot(n, n).sqrt();
                    let sign = if dot(n, d) > 0.0 { -1.0 } else { 1.0 } / len;
                    best = Some((k, [n[0] * sign, n[1] * sign, n[2] * sign]));
                }
            }
        }
        best.map(|(k, normal)| ddsim::RayHit {
            point: [from[0] + d[0] * k, from[1] + d[1] * k, from[2] + d[2] * k],
            normal,
        })
    }

    /// The triangles whose bounding boxes meet the box `lo`-`hi`.
    fn triangles_in(&self, lo: [f32; 3], hi: [f32; 3], out: &mut Vec<ddsim::sweep::Face>) -> bool {
        let (b0, b1) = (bucket(lo[0], lo[2]), bucket(hi[0], hi[2]));
        let mut seen = std::collections::HashSet::new();
        for bx in b0.0..=b1.0 {
            for bz in b0.1..=b1.1 {
                for &id in self.grid.get(&(bx, bz)).into_iter().flatten() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let t = &self.tris[id as usize];
                    let inside = (0..3).all(|k| {
                        let (a, b) = (
                            t[0][k].min(t[1][k]).min(t[2][k]),
                            t[0][k].max(t[1][k]).max(t[2][k]),
                        );
                        b >= lo[k] && a <= hi[k]
                    });
                    if inside {
                        out.push((*t, self.flags[id as usize]));
                    }
                }
            }
        }
        true
    }

    /// Any triangle closer than `radius` to `center`.
    fn sphere(&self, center: [f32; 3], radius: f32) -> bool {
        let (b0, b1) = (
            bucket(center[0] - radius, center[2] - radius),
            bucket(center[0] + radius, center[2] + radius),
        );
        for bx in b0.0..=b1.0 {
            for bz in b0.1..=b1.1 {
                for &id in self.grid.get(&(bx, bz)).into_iter().flatten() {
                    let c = closest_on_triangle(center, &self.tris[id as usize]);
                    let e = sub(center, c);
                    if dot(e, e) < radius * radius {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Re-export of the ground trait so dependants need not name ddsim.
mod ddsim_ground {
    pub use ddsim::Ground;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddsim::Ground;

    #[test]
    fn height_on_a_slope() {
        let mut t = Terrain::empty();
        // plane y = x / 10 over a 1000 cm square, two triangles
        t.add([[0.0, 0.0, 0.0], [1000.0, 100.0, 0.0], [0.0, 0.0, 1000.0]]);
        t.add([[1000.0, 100.0, 0.0], [1000.0, 100.0, 1000.0], [0.0, 0.0, 1000.0]]);
        let h = t.height(500.0, 300.0, 1e9).unwrap();
        assert!((h - 50.0).abs() < 1e-3, "{h}");
        assert_eq!(t.height(2000.0, 300.0, 1e9), None);
        // below_y excludes surfaces above it
        assert_eq!(t.height(500.0, 300.0, 10.0), None);
    }

    #[test]
    fn triangles_in_a_box_carry_their_material_flags() {
        let mut t = Terrain::empty();
        t.add_flagged([[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 0.0, 1000.0]], 0x2000);
        t.add_flagged(
            [[5000.0, 0.0, 0.0], [6000.0, 0.0, 0.0], [5000.0, 0.0, 1000.0]],
            0x1,
        );
        let mut out = Vec::new();
        assert!(t.triangles_in([100.0, -10.0, 100.0], [200.0, 10.0, 200.0], &mut out));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, 0x2000);
        // above the triangle's box: nothing
        out.clear();
        t.triangles_in([100.0, 50.0, 100.0], [200.0, 60.0, 200.0], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn walls_are_found_within_the_radius() {
        let mut t = Terrain::empty();
        // a wall in the plane x = 100, z 0..1000, y 0..300
        t.add([[100.0, 0.0, 0.0], [100.0, 300.0, 0.0], [100.0, 0.0, 1000.0]]);
        t.add([[100.0, 300.0, 0.0], [100.0, 300.0, 1000.0], [100.0, 0.0, 1000.0]]);
        // a floor is not a wall
        t.add([[0.0, 0.0, 0.0], [0.0, 0.0, 1000.0], [90.0, 0.0, 0.0]]);
        let n = t.wall([80.0, 0.0, 500.0], 35.0, 60.0, 170.0).unwrap();
        assert!((n[0] + 1.0).abs() < 1e-4 && n[1].abs() < 1e-4, "{n:?}");
        let n = t.wall([120.0, 0.0, 500.0], 35.0, 60.0, 170.0).unwrap();
        assert!((n[0] - 1.0).abs() < 1e-4, "{n:?}");
        assert_eq!(t.wall([50.0, 0.0, 500.0], 35.0, 60.0, 170.0), None);
        // above the wall's top
        assert_eq!(t.wall([80.0, 0.0, 500.0], 35.0, 400.0, 500.0), None);
    }

    /// A 300 cm wall in the plane x = 100 (z 0..1000) on a floor at y = 0.
    fn wall_and_floor() -> Terrain {
        let mut t = Terrain::empty();
        t.add([[100.0, 0.0, 0.0], [100.0, 300.0, 0.0], [100.0, 0.0, 1000.0]]);
        t.add([[100.0, 300.0, 0.0], [100.0, 300.0, 1000.0], [100.0, 0.0, 1000.0]]);
        t.add([[-500.0, 0.0, 0.0], [-500.0, 0.0, 1000.0], [100.0, 0.0, 0.0]]);
        t.add([[100.0, 0.0, 0.0], [-500.0, 0.0, 1000.0], [100.0, 0.0, 1000.0]]);
        t
    }

    #[test]
    fn casts_hit_the_nearest_face_from_either_side() {
        let t = wall_and_floor();
        // toward the wall: hit at x = 100, normal facing back at the cast
        let h = t.ray([0.0, 150.0, 500.0], [200.0, 150.0, 500.0]).unwrap();
        assert!(
            (h.point[0] - 100.0).abs() < 1e-3 && (h.point[1] - 150.0).abs() < 1e-3,
            "{h:?}"
        );
        assert!((h.normal[0] + 1.0).abs() < 1e-5, "{h:?}");
        // from the other side the normal flips
        let h = t.ray([200.0, 150.0, 500.0], [0.0, 150.0, 500.0]).unwrap();
        assert!((h.normal[0] - 1.0).abs() < 1e-5, "{h:?}");
        // down onto the floor
        let h = t.ray([0.0, 150.0, 500.0], [0.0, -50.0, 500.0]).unwrap();
        assert!(
            h.point[1].abs() < 1e-3 && (h.normal[1] - 1.0).abs() < 1e-5,
            "{h:?}"
        );
        // short of the wall, over its top, and starting on the floor
        assert_eq!(t.ray([0.0, 150.0, 500.0], [90.0, 150.0, 500.0]), None);
        assert_eq!(t.ray([0.0, 350.0, 500.0], [200.0, 350.0, 500.0]), None);
        assert_eq!(t.ray([0.0, 0.0, 500.0], [0.0, 100.0, 500.0]), None);
    }

    #[test]
    fn spheres_touch_within_the_radius() {
        let t = wall_and_floor();
        assert!(t.sphere([85.0, 150.0, 500.0], 20.0));
        assert!(!t.sphere([75.0, 150.0, 500.0], 20.0));
        assert!(t.sphere([0.0, 10.0, 500.0], 20.0));
        assert!(!t.sphere([0.0, 30.0, 500.0], 20.0));
    }
}
