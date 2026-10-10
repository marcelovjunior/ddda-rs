//! The character's collision against scenery: a capsule swept along the
//! move and slid along what it meets, as `cScrAdjust` does in the original
//! (`0x0079E200` -> `0x010B4960` -> `0x010B2E20` / `0x010B0CA0`, the slide
//! response `0x010AFC60`). docs/MOVEMENT.md lists what is translated and
//! what is replaced.
//!
//! The original's per-triangle contact (MtCollision's GJK / MPR callbacks
//! behind the scene traversal) is replaced by exact geometry: the distance
//! between the capsule's segment and each triangle, advanced along the move
//! until it reaches the radius. The loop around it, the contact categories
//! and the slide rules are the original's.

pub type V3 = [f32; 3];
pub type Tri = [V3; 3];
/// A collision triangle and its material's flag word.
pub type Face = (Tri, u32);

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
/// MtFramework's normalise: unchanged below 1.1920929e-7.
fn norm(a: V3) -> V3 {
    let l = len(a);
    if l >= 1.192_092_9e-7 {
        mul(a, 1.0 / l)
    } else {
        a
    }
}

/// The body capsule: a segment from `bottom` to `top` above the feet, of
/// `radius`. `cScrAdjust::mCapsule` read live from the Arisen (board #931):
/// (0, 35, 0) to (0, 145, 0), radius 35; the radius is the model's
/// scenery-adjust volume (`collision\pl\m0001` group 109).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    pub bottom: f32,
    pub top: f32,
    pub radius: f32,
}

/// A posed capsule's two centres, relative to the origin being moved.
/// cScrAdjust +0xD0/+0xE0/+0xF0 accepts arbitrary endpoints (0x0079DAB0),
/// including the dragon's installed horizontal scenery capsule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapsuleSegment {
    pub start: V3,
    pub end: V3,
    pub radius: f32,
}

/// Explicit optional sphere axis query. A disabled native axis check
/// (cScrAdjust +0x1A0 == 0) supplies `None`, not a failed ground query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisQuery {
    pub center: V3,
    pub radius: f32,
    pub motion: V3,
}

pub const PLAYER_CAPSULE: Capsule = Capsule {
    bottom: 35.0,
    top: 145.0,
    radius: 35.0,
};

/// Contact categories (`0x00DEF0C0` on the contact normal's y, for the
/// query's group 0): floor, steep slope, wall, ceiling.
pub const FLOOR: u32 = 1;
pub const SLOPE: u32 = 2;
pub const WALL: u32 = 4;
pub const CEILING: u32 = 8;
/// Result bit: the sweep met something while the axis check found no
/// ground (`0x010B0CA0` sets 0x400); the character is then not grounded.
pub const NO_AXIS_GROUND: u32 = 0x400;

/// The values the slide uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepParam {
    /// sCollision +0x38: a normal with y above this is a floor. The sine of
    /// 90 degrees minus the floor angle (`0x00FB37F0`), which the game's
    /// sCollision sets to 45 degrees (`0x00422D00`, `0x0041B400`, after the
    /// base constructor `0x00DF02B0` set 60).
    pub floor_min_ny: f32,
    /// sCollision +0x3C: y above this (and not a floor) is a steep slope;
    /// the sine of 90 degrees minus the wall angle (`0x00FB3840`), also 45
    /// degrees in the game, so there is no slope category.
    pub wall_max_ny: f32,
    /// sCollision +0x40: y below this (<= 0) is a ceiling; the sine of the
    /// ceiling angle (`0x00FB38B0`), -10 degrees (`0x00DF02B0`).
    pub ceiling_max_ny: f32,
    /// Moves (contact, slide) per sweep: `sCollision::Param` +0x44 (10,
    /// from the constructor's arguments at every player query).
    pub max_moves: u32,
    /// After a contact the shape stands this far off the surface:
    /// ScrColInfoAdjustPosition +0x200, set from `sCollision::Param` +0x4C
    /// (0.1, `0x00DE4B70`).
    pub skin: f32,
    /// The contact is backed off this far along the move (0x010B0CA0).
    pub back_off: f32,
    /// A slide whose horizontal direction turns this little from the move
    /// (dot below it) becomes a slide along the wall's slope:
    /// `sCollision::Param` +0x68 (0.1, `0x00DE4B70`).
    pub corner_dot: f32,
    /// On the ground, after a wall or ceiling contact, a rest of the move
    /// whose squared length is below this (sCollision +0x18C, 5) is dropped
    /// horizontally when the character has not advanced along the move's
    /// direction: its horizontal displacement's dot with that direction is
    /// below `stall_dot_short` (sCollision +0x190, 0.001) for a move shorter
    /// than `stall_short` (sCollision +0x188, 15), else below
    /// `stall_dot_long` (sCollision +0x194, 1e-5). Values from `0x00DF02B0`.
    pub stall_len2: f32,
    pub stall_short: f32,
    pub stall_dot_short: f32,
    pub stall_dot_long: f32,
    /// The axis check (cScrAdjust `mAxisCheckType` 2): a sphere of this
    /// radius (`mAxisCheckSphereRadius`, 15) cast down from the capsule's
    /// lower centre at the old position.
    pub axis_radius: f32,
    /// A move straight down onto a floor that meets it within this
    /// fraction of the move is undone (sCollision +0x50, 0.0625).
    pub floor_revert: f32,
    /// Material flags that make a contact a floor, a steep slope or a wall
    /// whatever its normal: sCollision +0x44 (0), +0x48 (0x80000000, both
    /// from `0x00DF02B0`) and +0x4C (0x2002, set by the game's sCollision
    /// `0x00422D00`).
    pub floor_flags: u32,
    pub slope_flags: u32,
    pub wall_flags: u32,
}

impl Default for SweepParam {
    fn default() -> Self {
        SweepParam {
            floor_min_ny: (45.0f32).to_radians().sin(),
            wall_max_ny: (45.0f32).to_radians().sin(),
            ceiling_max_ny: (-10.0f32).to_radians().sin(),
            max_moves: 10,
            skin: 0.1,
            back_off: 0.01,
            corner_dot: 0.1,
            stall_len2: 5.0,
            stall_short: 15.0,
            stall_dot_short: 0.001,
            stall_dot_long: 1e-5,
            axis_radius: 15.0,
            floor_revert: 0.0625,
            floor_flags: 0,
            slope_flags: 0x8000_0000,
            wall_flags: 0x2002,
        }
    }
}

impl SweepParam {
    /// The category of a contact normal for the query's group 0.
    // Translated from 0x00DEF0C0 (DDDA.exe 2017-12-13 Steam), case 0
    pub fn category(&self, ny: f32) -> u32 {
        if ny > self.floor_min_ny {
            return FLOOR;
        }
        if ny <= self.wall_max_ny {
            let low = if ny <= 0.0 {
                ny < self.ceiling_max_ny
            } else {
                self.wall_max_ny < ny
            };
            return if low { CEILING } else { WALL };
        }
        SLOPE
    }

    /// The category in group 2, which the query switches to when the axis
    /// check found no floor: floors count as steep slopes.
    // Translated from 0x00DEF0C0 (DDDA.exe 2017-12-13 Steam), case 2
    pub fn category_no_floor(&self, ny: f32) -> u32 {
        match self.category(ny) {
            FLOOR => SLOPE,
            c => c,
        }
    }

    /// The category of a contact on a triangle with material `flags`: the
    /// material masks first, then the normal (group 0, or 2 for
    /// `no_floor`).
    // Translated from 0x00DEFD00 (DDDA.exe 2017-12-13 Steam), which the
    // sweep (0x010B0CA0), the slide (0x010AFC60) and the axis check
    // (0x011C5E90) use alike
    pub fn category_of(&self, flags: u32, ny: f32, no_floor: bool) -> u32 {
        if flags & self.floor_flags != 0 {
            FLOOR
        } else if flags & self.slope_flags != 0 {
            SLOPE
        } else if flags & self.wall_flags != 0 {
            WALL
        } else if no_floor {
            self.category_no_floor(ny)
        } else {
            self.category(ny)
        }
    }
}

/// Closest points between segments p1-q1 and p2-q2 (Ericson 5.1.9):
/// (squared distance, point on the first, point on the second).
fn seg_seg(p1: V3, q1: V3, p2: V3, q2: V3) -> (f32, V3, V3) {
    let (d1, d2, r) = (sub(q1, p1), sub(q2, p2), sub(p1, p2));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let (s, t);
    if a <= 1e-9 && e <= 1e-9 {
        s = 0.0;
        t = 0.0;
    } else if a <= 1e-9 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= 1e-9 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = dot(d1, d2);
            let den = a * e - b * b;
            let mut s0 = if den != 0.0 {
                ((b * f - c * e) / den).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    let c1 = add(p1, mul(d1, s));
    let c2 = add(p2, mul(d2, t));
    let d = sub(c1, c2);
    (dot(d, d), c1, c2)
}

/// Closest point on a triangle (Ericson 5.1.5).
pub fn closest_on_triangle(p: V3, t: &Tri) -> V3 {
    let [a, b, c] = *t;
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
        return add(a, mul(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, mul(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return add(b, mul(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let den = 1.0 / (va + vb + vc);
    add(a, add(mul(ab, vb * den), mul(ac, vc * den)))
}

/// Where segment p-q crosses the triangle, if it does.
fn seg_cross(p: V3, q: V3, t: &Tri) -> Option<V3> {
    let d = sub(q, p);
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(p, t[0]);
    let u = dot(s, h) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = cross(s, e1);
    let v = dot(d, qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let k = dot(e2, qv) * inv;
    (0.0..=1.0).contains(&k).then(|| add(p, mul(d, k)))
}

/// Distance between segment p-q and a triangle, with the closest points
/// (on the segment, on the triangle).
pub fn seg_tri(p: V3, q: V3, t: &Tri) -> (f32, V3, V3) {
    if let Some(x) = seg_cross(p, q, t) {
        return (0.0, x, x);
    }
    let mut best = {
        let c = closest_on_triangle(p, t);
        (dot(sub(p, c), sub(p, c)), p, c)
    };
    let c = closest_on_triangle(q, t);
    let dq = dot(sub(q, c), sub(q, c));
    if dq < best.0 {
        best = (dq, q, c);
    }
    for k in 0..3 {
        let e = seg_seg(p, q, t[k], t[(k + 1) % 3]);
        if e.0 < best.0 {
            best = e;
        }
    }
    (best.0.sqrt(), best.1, best.2)
}

/// The first contact of the capsule (segment `a`-`b`, radius `r`) moved by
/// `m` with a triangle: (fraction of the move, normal from the triangle to
/// the capsule). Conservative advancement on the exact distance; a capsule
/// already touching returns fraction 0 with its depth (negative distance).
fn first_contact(a: V3, b: V3, r: f32, m: V3, t: &Tri) -> Option<(f32, V3, f32)> {
    let ml = len(m);
    let mut f = 0.0f32;
    for _ in 0..64 {
        let o = mul(m, f);
        let (d, on_seg, on_tri) = seg_tri(add(a, o), add(b, o), t);
        let gap = d - r;
        if gap <= 1e-3 {
            let mut n = sub(on_seg, on_tri);
            if dot(n, n) < 1e-12 {
                // the axis crosses the triangle: its face normal, against the move
                n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
                if dot(n, m) > 0.0 {
                    n = mul(n, -1.0);
                }
            }
            // a penetration only when already overlapping at the start
            let depth = if f == 0.0 && gap < -1e-3 { gap } else { 0.0 };
            return Some((f, norm(n), depth));
        }
        if ml < 1e-6 {
            return None;
        }
        f += gap / ml;
        if f > 1.0 {
            return None;
        }
    }
    None
}

/// Sphere contact using the same geometric replacement as the character
/// capsule: `(fraction, surface-to-sphere normal, negative initial gap)`.
/// This exposes geometry only, without character slide/grounding rules.
/// The original's convex callback machinery is not reproduced here.
pub fn sphere_contact(center: V3, radius: f32, motion: V3, triangle: &Tri) -> Option<(f32, V3, f32)> {
    first_contact(center, center, radius, motion, triangle)
}

/// The result of a sweep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swept {
    /// Where the moved origin ends (the feet for the player wrapper).
    pub position: V3,
    /// The categories met (`FLOOR`, `SLOPE`, `WALL`, `CEILING`).
    pub met: u32,
    /// The last floor normal met.
    pub floor_normal: Option<V3>,
    /// The axis check's category (0: it met nothing).
    pub axis: u32,
}

impl Swept {
    /// The grounded flag uPlayer +0x1ED0 that `uPlayer::vf90` (0x00885E30)
    /// derives from the result: a floor or slope met, unless the axis check
    /// found no ground.
    pub fn grounded(&self) -> bool {
        self.met & NO_AXIS_GROUND == 0 && self.met & (FLOOR | SLOPE) != 0
    }
}

/// The axis check: the category of the nearest triangle facing up that a
/// sphere of `radius` cast from `center` along `cast` meets, 0 for none.
// Translated from 0x011C5E90 (DDDA.exe 2017-12-13 Steam), the axis-check
// callback: triangles facing along the cast (or edge-on) are skipped, the
// nearest hit wins, a tie goes to the steeper one (slope over floor, wall
// over floor and slope). The sphere cast itself (0x00D6ABF0) is replaced by
// the exact distance advancement of `first_contact`.
fn axis_check(p: &SweepParam, center: V3, radius: f32, cast: V3, tris: &[Face]) -> u32 {
    let dir = norm(cast);
    let mut best: Option<(f32, u32)> = None;
    for (t, flags) in tris {
        let n = norm(cross(sub(t[1], t[0]), sub(t[2], t[0])));
        let d = dot(n, dir);
        if d > 0.0 || d.abs() < 0.0001 {
            continue;
        }
        let Some((f, _, _)) = first_contact(center, center, radius, cast, t) else {
            continue;
        };
        let cat = p.category_of(*flags, n[1], false);
        best = match best {
            None => Some((f, cat)),
            Some((bf, _)) if f < bf - 1e-6 => Some((f, cat)),
            Some((bf, bc)) if (f - bf).abs() <= 1e-6 => {
                let steeper = (cat == SLOPE && bc & FLOOR != 0) || (cat == WALL && bc & (FLOOR | SLOPE) != 0);
                Some((bf, if steeper { cat } else { bc }))
            }
            b => b,
        };
    }
    best.map_or(0, |b| b.1)
}

/// Moves the capsule standing on `feet` by `motion`, sliding along what it
/// meets. `tris` lists the triangles that may be met within a box.
// Translated from 0x010B0CA0 (DDDA.exe 2017-12-13 Steam), the loop of
// ScrColInfoAdjustPosition: contact, back off, stand `skin` off the surface,
// slide the rest (0x010AFC60), up to `max_moves` times.
///
/// `ground_check`: the query of a character on the ground (cScrAdjust's
/// `mIsGroundCheck`, set while uPlayer +0x273C bit 0 is, 0x00B63F70). In
/// the air the query has flag 0x40000000 instead (`0x00DE4B70`), which
/// lets slides climb walls.
pub fn sweep(
    p: &SweepParam,
    cap: &Capsule,
    feet: V3,
    motion: V3,
    ground_check: bool,
    tris: &dyn Fn(V3, V3, &mut Vec<Face>),
) -> Swept {
    // Preserve the player's type2 axis query from 0x0079E200: old lower
    // centre, down by radius * sqrt2 and the airborne move's drop.
    let axis_len = cap.radius * std::f32::consts::SQRT_2 - if ground_check { 0.0 } else { motion[1] };
    sweep_capsule(
        p,
        &CapsuleSegment {
            start: [0.0, cap.bottom, 0.0],
            end: [0.0, cap.top, 0.0],
            radius: cap.radius,
        },
        feet,
        motion,
        ground_check,
        Some(AxisQuery {
            center: [feet[0], feet[1] + cap.bottom.min(cap.top), feet[2]],
            radius: p.axis_radius,
            motion: [0.0, -axis_len, 0.0],
        }),
        tris,
    )
}

/// The existing native slide loop with arbitrary posed capsule endpoints.
/// Geometry remains the documented exact-distance replacement; the optional
/// axis check preserves the disabled branch of 0x0079E200/0x010B0CA0.
/// Slide parameters still cover only the branches documented by `slide`.
#[allow(clippy::too_many_arguments)]
pub fn sweep_capsule(
    p: &SweepParam,
    cap: &CapsuleSegment,
    feet: V3,
    motion: V3,
    ground_check: bool,
    axis_query: Option<AxisQuery>,
    tris: &dyn Fn(V3, V3, &mut Vec<Face>),
) -> Swept {
    let mut pos = feet;
    let mut m = motion;
    let first_dir = norm([motion[0], 0.0, motion[2]]);
    let first_horizontal = motion[0].abs() > 0.001 || motion[2].abs() > 0.001;
    let target_y = feet[1] + motion[1];
    let stall_dot = if (motion[0] * motion[0] + motion[2] * motion[2]).sqrt() - p.stall_short < 0.0 {
        p.stall_dot_short
    } else {
        p.stall_dot_long
    };
    let mut met = 0;
    let mut floor_normal = None;
    let mut list = Vec::new();
    let pad = cap.radius + 1.0;
    let axis = axis_query.map_or(0, |query| {
        let pad = query.radius + 1.0;
        let lo = std::array::from_fn(|i| query.center[i].min(query.center[i] + query.motion[i]) - pad);
        let hi = std::array::from_fn(|i| query.center[i].max(query.center[i] + query.motion[i]) + pad);
        tris(lo, hi, &mut list);
        axis_check(p, query.center, query.radius, query.motion, &list)
    });
    // the first contact checks the axis result: no ground under the axis
    // marks the result, no floor turns floors into slopes for the rest
    let mut axis_pending = axis_query.is_some();
    let mut no_floor = false;
    // the last wall plane slid along (adjust info +0x2B5 / +0x2C0)
    let mut crease: Option<(V3, V3)> = None;
    for _ in 0..p.max_moves {
        if dot(m, m) < 1e-5 {
            break;
        }
        // the triangles near the swept capsule
        let lo = std::array::from_fn(|i| pos[i] + cap.start[i].min(cap.end[i]) + m[i].min(0.0) - pad);
        let hi = std::array::from_fn(|i| pos[i] + cap.start[i].max(cap.end[i]) + m[i].max(0.0) + pad);
        list.clear();
        tris(lo, hi, &mut list);
        let a = add(pos, cap.start);
        let b = add(pos, cap.end);
        let mut hit: Option<(f32, V3, f32)> = None;
        let mut hit_flags = 0;
        for (t, flags) in &list {
            // a triangle the move leaves behind (its face turned along the
            // move) is not met unless the capsule already overlaps it
            // (0x01166220)
            let face = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            if dot(face, m) > 0.0 && seg_tri(a, b, t).0 >= cap.radius {
                continue;
            }
            if let Some(h) = first_contact(a, b, cap.radius, m, t) {
                let better = match hit {
                    None => true,
                    // nearest first; at equal distance the deepest, then the
                    // one most against the move
                    Some(c) => h.0 < c.0 - 1e-6 || ((h.0 - c.0).abs() <= 1e-6 && dot(h.1, m) < dot(c.1, m)),
                };
                if better {
                    hit = Some(h);
                    hit_flags = *flags;
                }
            }
        }
        let Some((f, n, depth)) = hit else {
            pos = add(pos, m);
            m = [0.0; 3];
            break;
        };
        let cat = p.category_of(hit_flags, n[1], no_floor);
        met |= cat;
        if axis_pending {
            // Translated from 0x010B0CA0 (DDDA.exe 2017-12-13 Steam): the
            // query's group becomes 2 and, with no axis hit at all, the
            // result gets 0x400
            axis_pending = false;
            if axis == 0 {
                met |= NO_AXIS_GROUND;
                no_floor = true;
            } else if axis & FLOOR == 0 {
                no_floor = true;
            }
        }
        if cat == FLOOR {
            floor_normal = Some(n);
        }
        // the contact, backed off along the move; out of a penetration
        let ml = len(m);
        let dir = mul(m, 1.0 / ml);
        let at = if depth < 0.0 {
            add(pos, mul(n, -depth))
        } else {
            add(pos, mul(dir, (f * ml - p.back_off).max(0.0)))
        };
        let done = len(sub(at, pos));
        let rest = (ml - done).max(0.0);
        let loop_start = pos;
        let loop_start_y = pos[1];
        pos = add(at, mul(n, p.skin));
        let mut left = mul(dir, rest);
        // a wall or ceiling that stops the move: drop a short rest that has
        // not advanced along the move's direction
        if ground_check && cat & (WALL | CEILING) != 0 {
            let moved = norm([pos[0] - feet[0], 0.0, pos[2] - feet[2]]);
            if dot(left, left) < p.stall_len2 && dot(first_dir, moved) < stall_dot {
                left[0] = 0.0;
                left[2] = 0.0;
            }
        }
        // the slide
        let contact = Contact {
            cat,
            normal: n,
            point: at,
            fraction: if depth < 0.0 { 0.0 } else { f },
        };
        match slide(
            p,
            &contact,
            left,
            first_dir,
            first_horizontal,
            ground_check,
            target_y - loop_start_y,
            &mut crease,
        ) {
            Slide::Go(v) => left = v,
            Slide::Stop => {
                m = [0.0; 3];
                break;
            }
            Slide::Revert => {
                pos = loop_start;
                m = [0.0; 3];
                break;
            }
        }
        m = left;
    }
    let pos = add(pos, m);
    Swept {
        position: pos,
        met,
        floor_normal,
        axis,
    }
}

/// One contact of the sweep.
struct Contact {
    cat: u32,
    normal: V3,
    /// Where the capsule's feet are at the contact (backed off).
    point: V3,
    /// The fraction of this move travelled to the contact (adjust info
    /// +0x1B4; 0 for a penetration).
    fraction: f32,
}

/// What the slide makes of the rest of a move.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Slide {
    /// Go on with this rest.
    Go(V3),
    /// Stop where the contact left the capsule.
    Stop,
    /// Stop where this move started (the contact is undone).
    Revert,
}

/// The rest `m` of a move after `c`.
// Translated from 0x010AFC60 (DDDA.exe 2017-12-13 Steam) for the player's
// query (group 0 or 2, flags 0x80000000, in the air 0x40000000; Param +0xB0
// = 0, +0xB4 = 1 on the ground and 0 in the air, +0xB5 = 0: values from the
// constructor 0x00DE45C0 and 0x00DE4B70). The nudge 0x010AF8F0 (a move
// straight down onto an exactly level floor) and the second axis check's
// branches (adjust info +0x298, unused by the player) are left out.
#[allow(clippy::too_many_arguments)]
fn slide(
    p: &SweepParam,
    c: &Contact,
    m: V3,
    first_dir: V3,
    first_horizontal: bool,
    ground_check: bool,
    to_target_y: f32,
    crease: &mut Option<(V3, V3)>,
) -> Slide {
    let n = c.normal;
    let up = [0.0, 1.0, 0.0];
    match c.cat {
        FLOOR => {
            *crease = None;
            // anything but a move straight down slides along the floor,
            // its vertical part dropped
            let d = norm(m);
            if d[0].abs() > 0.001 || d[1] > 1.192_092_9e-7 || d[2].abs() > 0.001 {
                let k = m[0] * n[0] + m[2] * n[2];
                return Slide::Go([m[0] - k * n[0], -(k * n[1]), m[2] - k * n[2]]);
            }
            // straight down: stop, or undo a contact made within the first
            // sCollision +0x50 (0.0625) of the move
            if p.floor_revert < c.fraction {
                Slide::Stop
            } else {
                Slide::Revert
            }
        }
        SLOPE | CEILING => {
            // along the plane
            *crease = None;
            Slide::Go(sub(m, mul(n, dot(m, n))))
        }
        WALL => {
            let pre = m;
            let mut v = sub(m, mul(n, dot(m, n)));
            // on the ground, no climbing along walls: a slide going up
            // follows the wall's horizontal direction
            if ground_check && v[1] > 0.0 {
                let t = cross(n, up);
                v = mul(t, dot(v, t));
            }
            if !ground_check || first_horizontal {
                // the slide hardly keeps the move's (or this rest's)
                // horizontal direction: slide down (or up) the wall by what
                // is left to the target height instead, along the crease
                // with the last wall when this contact came early
                let h = norm([v[0], 0.0, v[2]]);
                let dots = [dot(first_dir, h), dot(norm([pre[0], 0.0, pre[2]]), h)];
                let last = crease.replace((n, c.point));
                if dots.iter().any(|&d| d < p.corner_dot) {
                    if let Some((n0, _)) = last.filter(|_| c.fraction < 0.1) {
                        let l = norm(cross(n0, n));
                        if dot(l, l) > 0.0 {
                            return Slide::Go(mul(l, to_target_y * l[1]));
                        }
                    }
                    let along = norm(cross(n, cross(n, up)));
                    v = mul(along, to_target_y * along[1]);
                }
            }
            Slide::Go(v)
        }
        _ => Slide::Go(m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor at y = 0 and a wall x = 100 (two triangles each).
    fn world(lo: V3, hi: V3, out: &mut Vec<Face>) {
        let _ = (lo, hi);
        out.push((
            [
                [-1000.0, 0.0, -1000.0],
                [-1000.0, 0.0, 1000.0],
                [1000.0, 0.0, -1000.0],
            ],
            1,
        ));
        out.push((
            [
                [1000.0, 0.0, -1000.0],
                [-1000.0, 0.0, 1000.0],
                [1000.0, 0.0, 1000.0],
            ],
            1,
        ));
        // the wall faces -x, towards the capsule
        out.push((
            [
                [100.0, -10.0, -1000.0],
                [100.0, -10.0, 1000.0],
                [100.0, 500.0, -1000.0],
            ],
            1,
        ));
        out.push((
            [
                [100.0, 500.0, -1000.0],
                [100.0, -10.0, 1000.0],
                [100.0, 500.0, 1000.0],
            ],
            1,
        ));
    }

    #[test]
    fn categories_follow_the_thresholds() {
        let p = SweepParam::default();
        assert_eq!(p.category(1.0), FLOOR);
        assert_eq!(p.category(0.5), WALL);
        assert_eq!(p.category(-0.9), CEILING);
    }

    #[test]
    fn horizontal_capsule_lands_with_axis_disabled_but_not_with_a_failed_axis() {
        let p = SweepParam::default();
        let cap = CapsuleSegment { start: [-80.0, 10.0, 0.0], end: [80.0, 10.0, 0.0], radius: 5.0 };
        let start = [0.0, 50.0, 0.0];
        let motion = [0.0, -80.0, 0.0];
        let disabled = sweep_capsule(&p, &cap, start, motion, false, None, &world);
        assert!((disabled.position[1] - (-5.0 + p.skin + p.back_off)).abs() < 0.02, "{disabled:?}");
        assert_eq!(disabled.met & FLOOR, FLOOR);
        assert_eq!(disabled.axis, 0);
        assert!(disabled.grounded());
        // An enabled query that never reaches terrain is different from
        // mAxisCheckType0, even though both return axis category zero.
        let failed = sweep_capsule(&p, &cap, start, motion, false,
            Some(AxisQuery { center: [0.0, 60.0, 0.0], radius: 1.0, motion: [0.0, -1.0, 0.0] }), &world);
        assert_eq!(failed.axis, 0);
        assert_ne!(failed.met & NO_AXIS_GROUND, 0);
        assert!(!failed.grounded());
    }

    #[test]
    fn horizontal_capsule_broad_phase_contains_the_moving_far_endpoint() {
        let p = SweepParam::default();
        let cap = CapsuleSegment { start: [-80.0, 50.0, 0.0], end: [80.0, 50.0, 0.0], radius: 5.0 };
        let query = |lo: V3, hi: V3, out: &mut Vec<Face>| {
            assert!(lo[0] <= -86.0 && hi[0] >= 100.0, "{lo:?}..{hi:?}");
            world(lo, hi, out);
        };
        let hit = sweep_capsule(&p, &cap, [0.0; 3], [50.0, 0.0, 0.0], false, None, &query);
        assert_eq!(hit.met & WALL, WALL);
        assert!((hit.position[0] - (100.0 - 80.0 - 5.0 - p.skin - p.back_off)).abs() < 0.02, "{hit:?}");
    }

    #[test]
    fn segment_triangle_distance() {
        let t = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 0.0, 10.0]];
        let (d, _, c) = seg_tri([1.0, 5.0, 1.0], [1.0, 9.0, 1.0], &t);
        assert!((d - 5.0).abs() < 1e-5, "{d}");
        assert!(len(sub(c, [1.0, 0.0, 1.0])) < 1e-5, "{c:?}");
        // crossing
        assert_eq!(seg_tri([1.0, -1.0, 1.0], [1.0, 1.0, 1.0], &t).0, 0.0);
    }

    #[test]
    fn a_sweep_stops_on_the_floor_and_slides_along_the_wall() {
        let p = SweepParam::default();
        // down onto the floor from 50 cm: stands skin above it
        let s = sweep(
            &p,
            &PLAYER_CAPSULE,
            [0.0, 50.0, 0.0],
            [0.0, -80.0, 0.0],
            true,
            &world,
        );
        assert!((s.position[1] - p.skin).abs() < 0.02, "{s:?}");
        assert_eq!(s.met & FLOOR, FLOOR);
        // diagonally into the wall: stops 35 (+skin) short of it, keeps z
        let s = sweep(
            &p,
            &PLAYER_CAPSULE,
            [0.0, 0.1, 0.0],
            [100.0, -1.0, 100.0],
            true,
            &world,
        );
        assert!((s.position[0] - (100.0 - 35.0 - p.skin)).abs() < 0.05, "{s:?}");
        assert!(s.position[2] > 99.0, "{s:?}");
        assert_eq!(s.met & WALL, WALL);
    }

    #[test]
    fn a_floor_slide_shrinks_up_a_slope_by_cos_squared() {
        let p = SweepParam::default();
        // slope y = 0.2 x
        let tris = |_: V3, _: V3, out: &mut Vec<Face>| {
            out.push((
                [
                    [-1000.0, -200.0, -1000.0],
                    [-1000.0, -200.0, 1000.0],
                    [1000.0, 200.0, -1000.0],
                ],
                1,
            ));
            out.push((
                [
                    [1000.0, 200.0, -1000.0],
                    [-1000.0, -200.0, 1000.0],
                    [1000.0, 200.0, 1000.0],
                ],
                1,
            ));
        };
        let n = norm([-0.2, 1.0, 0.0]);
        // the bottom sphere rests skin off the slope, its feet point above it
        let start = [
            0.0,
            (PLAYER_CAPSULE.radius + p.skin) / n[1] - PLAYER_CAPSULE.radius,
            0.0,
        ];
        let s = sweep(&p, &PLAYER_CAPSULE, start, [10.0, -0.5, 0.0], true, &tris);
        let moved = s.position[0] - start[0];
        assert!((moved - 10.0 / 1.04).abs() < 0.2, "{moved}");
    }

    /// A slope y = 0.5 x (27 degrees) with material `flags`.
    fn slope(flags: u32) -> impl Fn(V3, V3, &mut Vec<Face>) {
        move |_: V3, _: V3, out: &mut Vec<Face>| {
            out.push((
                [
                    [-1000.0, -500.0, -1000.0],
                    [-1000.0, -500.0, 1000.0],
                    [1000.0, 500.0, -1000.0],
                ],
                flags,
            ));
            out.push((
                [
                    [1000.0, 500.0, -1000.0],
                    [-1000.0, -500.0, 1000.0],
                    [1000.0, 500.0, 1000.0],
                ],
                flags,
            ));
        }
    }

    #[test]
    fn standing_on_a_slope_the_ground_check_does_not_creep() {
        let p = SweepParam::default();
        let tris = slope(1);
        let n = norm([-0.5, 1.0, 0.0]);
        let mut feet = [
            0.0,
            (PLAYER_CAPSULE.radius + p.skin) / n[1] - PLAYER_CAPSULE.radius,
            0.0,
        ];
        for _ in 0..30 {
            // the ground check's drop alone (20 x unit delta 0.35)
            let s = sweep(&p, &PLAYER_CAPSULE, feet, [0.0, -7.0, 0.0], true, &tris);
            assert!(s.grounded(), "{s:?}");
            feet = s.position;
        }
        assert!(feet[0].abs() < 0.01, "{feet:?}");
    }

    #[test]
    fn a_wall_material_holds_no_one_up() {
        let p = SweepParam::default();
        // the same slope with flag 0x2000: a wall whatever its normal
        let tris = slope(0x2000);
        let n = norm([-0.5, 1.0, 0.0]);
        let feet = [
            0.0,
            (PLAYER_CAPSULE.radius + p.skin) / n[1] - PLAYER_CAPSULE.radius,
            0.0,
        ];
        let s = sweep(&p, &PLAYER_CAPSULE, feet, [0.0, -7.0, 0.0], false, &tris);
        assert_eq!(s.met & (FLOOR | SLOPE), 0, "{s:?}");
        assert!(!s.grounded());
        // it slides down the slope instead
        assert!(s.position[0] < -0.5, "{s:?}");
    }

    #[test]
    fn off_an_edge_the_axis_check_finds_no_ground() {
        let p = SweepParam::default();
        // a floor ending at x = 0 and a wall face below its edge
        let tris = |_: V3, _: V3, out: &mut Vec<Face>| {
            out.push((
                [
                    [-1000.0, 0.0, -1000.0],
                    [-1000.0, 0.0, 1000.0],
                    [0.0, 0.0, -1000.0],
                ],
                1,
            ));
            out.push((
                [[0.0, 0.0, -1000.0], [-1000.0, 0.0, 1000.0], [0.0, 0.0, 1000.0]],
                1,
            ));
        };
        // the bottom sphere rests on the edge, its centre 30 cm past it
        let feet = [30.0, (35.0f32 * 35.0 - 30.0 * 30.0).sqrt() - 35.0 + p.skin, 0.0];
        let s = sweep(&p, &PLAYER_CAPSULE, feet, [0.0, -7.0, 0.0], true, &tris);
        assert_eq!(s.axis, 0, "{s:?}");
        assert_ne!(s.met & NO_AXIS_GROUND, 0, "{s:?}");
        assert!(!s.grounded());
        // over the floor the axis check meets it
        let s = sweep(
            &p,
            &PLAYER_CAPSULE,
            [-100.0, p.skin, 0.0],
            [0.0, -7.0, 0.0],
            true,
            &tris,
        );
        assert_eq!(s.axis, FLOOR, "{s:?}");
        assert!(s.grounded());
    }

    #[test]
    fn categories_follow_the_material_masks_first() {
        let p = SweepParam::default();
        assert_eq!(p.category_of(0x1, 1.0, false), FLOOR);
        assert_eq!(p.category_of(0x2000, 1.0, false), WALL);
        assert_eq!(p.category_of(0x2, 1.0, false), WALL);
        assert_eq!(p.category_of(0x8000_0000, 1.0, false), SLOPE);
        assert_eq!(p.category_of(0x1, 1.0, true), SLOPE);
        assert_eq!(p.category_of(0x1, -1.0, true), CEILING);
    }
}
