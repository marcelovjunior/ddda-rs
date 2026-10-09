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

fn bucket(x: f32, z: f32) -> (i32, i32) {
    ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32)
}

impl Terrain {
    pub fn empty() -> Self {
        Terrain {
            tris: Vec::new(),
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

    /// Loads the cells' collision meshes (`st<S>h_` and `st<S>e_<A>m<B>n_mrg00`,
    /// cell-local like the terrain model) instead of the render terrain.
    pub fn load_collision(vfs: &Vfs, stage: u32, a0: i32, b0: i32, radius: i32) -> Self {
        let mut t = Terrain::empty();
        for a in a0 - radius..=a0 + radius {
            for b in b0 - radius..=b0 + radius {
                let off = [(b - 50) as f32 * 10000.0, 0.0, (a - 50) as f32 * 10000.0];
                let mut any = false;
                for kind in ["h", "e"] {
                    let path = format!(
                        r"scr\st{stage}\collision\m{}\marge\st{stage}{kind}_{a}m{b}n_mrg00",
                        a / 10 * 10
                    );
                    let Ok(bytes) = vfs.read(&path, RCOLLISION) else {
                        continue;
                    };
                    let Ok(c) = mtsbc::parse(&bytes) else { continue };
                    for (p, _) in c.world_triangles() {
                        t.add(p.map(|q| [q[0] + off[0], q[1], q[2] + off[2]]));
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
    /// instance whose model has `model\om\...\collision\<id>_h00` / `_e00`,
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
            if let Some(id) = base.strip_suffix("_h00").or_else(|| base.strip_suffix("_e00")) {
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
                        for (tri, _) in c.world_triangles() {
                            self.add(tri.map(|q| {
                                let s = [q[0] * pl.scale[0], q[1] * pl.scale[1], q[2] * pl.scale[2]];
                                let mut w = [0.0f32; 3];
                                for (i, wi) in w.iter_mut().enumerate() {
                                    *wi = m[i][0] * s[0] + m[i][1] * s[1] + m[i][2] * s[2] + pl.position[i];
                                }
                                w
                            }));
                        }
                    }
                    instances += 1;
                }
            }
        }
        instances
    }

    pub fn add(&mut self, p: [[f32; 3]; 3]) {
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
}
