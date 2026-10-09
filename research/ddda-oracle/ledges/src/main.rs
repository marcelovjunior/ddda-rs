//! Lists ledge candidates near a point of stage 100, from the merged stage
//! collision (`st100h_<A>m<B>n_mrg00`, what characters stand on).
//!
//! Usage: `ddda-ledges <nativePC> <A0> <B0> <x> <y> <z> [radius_cm] [min_step] [max_step]`
//! where (A0, B0) is the floating-origin cell and (x, y, z) a position in
//! that local frame (e.g. uPlayer.mPos). Output positions are in the same
//! local frame, so they can be written to mPos directly.
//!
//! Method: sample a 25 cm grid; at each sample, cast a vertical ray and keep
//! the highest surface within 6 m of y (upward-facing triangles only). A
//! ledge candidate is a pair of 4-neighbours whose surface heights differ
//! by min_step..max_step (default 100..250 cm), i.e. a vertical step.

use std::collections::HashMap;

const CELL: f32 = 10000.0;
const STEP: f32 = 25.0;

fn surface_at(tris: &[[[f32; 3]; 3]], grid: &HashMap<(i32, i32), Vec<usize>>, x: f32, z: f32, y0: f32) -> Option<f32> {
    let key = ((x / 200.0).floor() as i32, (z / 200.0).floor() as i32);
    let mut best: Option<f32> = None;
    for &i in grid.get(&key)? {
        let [a, b, c] = tris[i];
        // upward-facing only
        let n = [
            (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
            (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len == 0.0 || (n[1] / len).abs() < 0.5 {
            continue;
        }
        // barycentric in xz
        let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if d.abs() < 1e-6 {
            continue;
        }
        let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
        let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
        let l3 = 1.0 - l1 - l2;
        if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
            continue;
        }
        let y = l1 * a[1] + l2 * b[1] + l3 * c[1];
        if (y - y0).abs() > 600.0 {
            continue;
        }
        if best.is_none_or(|b| y > b) {
            best = Some(y);
        }
    }
    best
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 6 {
        eprintln!("usage: ddda-ledges <nativePC> <A0> <B0> <x> <y> <z> [radius_cm] [min_step] [max_step]");
        std::process::exit(2);
    }
    let f = |i: usize, d: f32| a.get(i).map(|s| s.parse::<f32>().unwrap()).unwrap_or(d);
    let (a0, b0) = (a[1].parse::<i32>().unwrap(), a[2].parse::<i32>().unwrap());
    let (px, py, pz) = (f(3, 0.0), f(4, 0.0), f(5, 0.0));
    let (radius, min_step, max_step) = (f(6, 1500.0), f(7, 100.0), f(8, 250.0));
    let vfs = ddvfs::Vfs::open(&a[0]).unwrap();
    let rcol = mtarc::type_hash("rCollision");
    let mut tris: Vec<[[f32; 3]; 3]> = Vec::new();
    for ca in a0 - 1..=a0 + 1 {
        for cb in b0 - 1..=b0 + 1 {
            let path = format!(r"scr\st100\collision\m{}\marge\st100h_{ca}m{cb}n_mrg00", ca / 10 * 10);
            let Ok(bytes) = vfs.read(&path, rcol) else { continue };
            let Ok(c) = mtsbc::parse(&bytes) else { continue };
            // cell-local x/z, absolute y; shift into the origin cell's local frame
            let off = [(cb - b0) as f32 * CELL, (ca - a0) as f32 * CELL];
            for (p, _) in c.world_triangles() {
                tris.push(p.map(|q| [q[0] + off[0], q[1], q[2] + off[1]]));
            }
        }
    }
    // xz bucket grid (200 cm) of triangles near the area of interest
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        let (mut x0, mut x1, mut z0, mut z1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for q in t {
            x0 = x0.min(q[0]);
            x1 = x1.max(q[0]);
            z0 = z0.min(q[2]);
            z1 = z1.max(q[2]);
        }
        if x1 < px - radius - 400.0 || x0 > px + radius + 400.0 || z1 < pz - radius - 400.0 || z0 > pz + radius + 400.0 {
            continue;
        }
        for gx in (x0 / 200.0).floor() as i32..=(x1 / 200.0).floor() as i32 {
            for gz in (z0 / 200.0).floor() as i32..=(z1 / 200.0).floor() as i32 {
                grid.entry((gx, gz)).or_default().push(i);
            }
        }
    }
    eprintln!("{} triangles loaded", tris.len());
    let n = (radius / STEP) as i32;
    let mut h: HashMap<(i32, i32), f32> = HashMap::new();
    for i in -n..=n {
        for j in -n..=n {
            let (x, z) = (px + i as f32 * STEP, pz + j as f32 * STEP);
            if let Some(y) = surface_at(&tris, &grid, x, z, py) {
                h.insert((i, j), y);
            }
        }
    }
    if std::env::var("LEDGES_AT").is_ok() {
        // the walkable surface under (x, z) within 6 m of y, for teleports
        match surface_at(&tris, &grid, px, pz, py) {
            Some(y) => println!("{y}"),
            None => {
                println!("none");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Ok(r) = std::env::var("LEDGES_FLAT") {
        // open flat areas: centres on a 5 m lattice whose disc of radius r cm
        // (1 m samples) has ground everywhere, steps < 25 cm between 1 m
        // neighbours and a height range < 150 cm (merged stage collision only)
        let r: f32 = r.parse().unwrap_or(1500.0);
        let k = (r / 100.0) as i32;
        let mut found = Vec::new();
        let lattice = (radius / 500.0) as i32;
        for ci in -lattice..=lattice {
            for cj in -lattice..=lattice {
                let (cx, cz) = (px + ci as f32 * 500.0, pz + cj as f32 * 500.0);
                let mut ok = true;
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                let mut prev_row: Vec<Option<f32>> = vec![None; (2 * k + 1) as usize];
                'outer: for a in -k..=k {
                    let mut prev: Option<f32> = None;
                    for b in -k..=k {
                        if a * a + b * b > k * k {
                            prev = None;
                            continue;
                        }
                        let Some(y) = surface_at(&tris, &grid, cx + a as f32 * 100.0, cz + b as f32 * 100.0, py) else {
                            ok = false;
                            break 'outer;
                        };
                        let idx = (b + k) as usize;
                        if prev.is_some_and(|q| (q - y).abs() > 25.0) || prev_row[idx].is_some_and(|q| (q - y).abs() > 25.0) {
                            ok = false;
                            break 'outer;
                        }
                        prev = Some(y);
                        prev_row[idx] = Some(y);
                        lo = lo.min(y);
                        hi = hi.max(y);
                    }
                }
                if ok && hi - lo < 150.0 {
                    let d = ((cx - px).powi(2) + (cz - pz).powi(2)).sqrt();
                    found.push((d, cx, cz, lo, hi));
                }
            }
        }
        found.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        println!("dist_cm centre_x centre_z min_y max_y   (flat discs of radius {r} cm)");
        for (d, x, z, lo, hi) in found.iter().take(30) {
            println!("{d:7.0} {x:9.1} {z:9.1} {lo:9.1} {hi:9.1}");
        }
        return;
    }
    if std::env::var("LEDGES_GRID").is_ok() {
        // height above py in dm, rows = z (top = +z), cols = x
        println!("grid: heights above y={py} in dm; '.' = no surface; rows z from {} down, cols x from {}", pz + n as f32 * STEP, px - n as f32 * STEP);
        for j in (-n..=n).rev() {
            let row: Vec<String> = (-n..=n)
                .map(|i| h.get(&(i, j)).map(|y| format!("{:3.0}", (y - py) / 10.0)).unwrap_or_else(|| "  .".into()))
                .collect();
            println!("{:8.0} {}", pz + j as f32 * STEP, row.join(""));
        }
        return;
    }
    let mut out = Vec::new();
    for (&(i, j), &y) in &h {
        for (di, dj) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
            if let Some(&y2) = h.get(&(i + di, j + dj)) {
                let step = y - y2;
                // the top must be a walkable shelf: still at the same height
                // 1 m inland (away from the drop), so thin walls are skipped
                let inland = h.get(&(i - 4 * di, j - 4 * dj)).is_some_and(|&yi| (yi - y).abs() < 20.0);
                if step >= min_step && step <= max_step && inland {
                    // top at (i,j), bottom at the neighbour
                    let (x, z) = (px + i as f32 * STEP, pz + j as f32 * STEP);
                    let d = ((x - px).powi(2) + (z - pz).powi(2)).sqrt();
                    out.push((d, x, y, z, y2, di, dj));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    println!("dist_cm top_x top_y top_z bottom_y step dir_x dir_z  (dir points from top to bottom)");
    for (d, x, y, z, y2, di, dj) in out.iter().take(60) {
        println!("{d:7.0} {x:9.1} {y:9.1} {z:9.1} {y2:9.1} {:6.1} {di:2} {dj:2}", y - y2);
    }
}
