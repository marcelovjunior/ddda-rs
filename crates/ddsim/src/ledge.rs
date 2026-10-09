//! Ledge grab, hang and climb geometry, translated from DDDA.exe
//! (2017-12-13 Steam). docs/MOVEMENT.md lists which rules are traced and
//! which are still placeholders.
//!
//! The original works in `uPlayer` fields; the names used here:
//! - `edge` (uPlayer +0x3090): the ledge edge, on top of the wall, at the
//!   wall face;
//! - `normal` (+0x30C0): the wall's normal flattened to the horizontal,
//!   pointing out of the wall; `wall_normal` (+0x30D0) is the raw hit normal;
//! - `foot` (+0x3100): where the feet (the character's origin) hang;
//! - `body` (+0x3110): the body point the hang's IK aims at;
//! - `hands` (cPlCliffHangBegin +0xC0): the hands, just out from the edge.
//!
//! Collision queries (`0x00DEA870`, a segment cast that returns hit flags,
//! and `0x00FAF4B0`, a sphere overlap) are modelled by `Ground::ray` and
//! `Ground::sphere`. The original tests flag bit 0 for "hit a floor" and bit
//! 2 for "hit a wall"; how the engine sets those bits is not traced, so a
//! hit is classified by its normal (`FLOOR_MIN_NY`, *assumed*).

use crate::{Ground, RayHit};

pub type V3 = [f32; 3];

/// `rAdjustParam` ids (`param\pl\other\m_pl000_00`) the ledge code reads.
/// Probe length in cPlActDashJumpLand and cPlActEvasion (150).
pub const ADJUST_LEDGE_REACH_LAND: usize = 0x5A;
/// Probe length while falling with the stick held (100).
pub const ADJUST_LEDGE_REACH_FALL: usize = 0x5D;
/// Height above the feet of the highest probe; the down cast starts
/// `ADJUST_LEDGE_TOP_MARGIN` above it (200).
pub const ADJUST_LEDGE_PROBE_HIGH: usize = 0x5B;
/// Height above the feet of the lowest probe and bottom of the down cast (50).
pub const ADJUST_LEDGE_PROBE_LOW: usize = 0x5C;
/// The ledge must be more than this above the ground under the player (100).
pub const ADJUST_LEDGE_MIN_HEIGHT: usize = 0x5E;
/// The down cast starts this far above the highest probe (25).
pub const ADJUST_LEDGE_TOP_MARGIN: usize = 0x5F;
/// Hands-to-body-point distance, free hang / braced hang (93, 60).
pub const ADJUST_HANG_BODY: [usize; 2] = [0x189, 0x190];
/// Hands out from the edge, free hang / braced hang (22, 33).
pub const ADJUST_HANG_HANDS: [usize; 2] = [0x18A, 0x191];
/// A grab rises into the hang when the feet are at least this far below
/// the hang's foot point (30).
pub const ADJUST_GRAB_RISE_MARGIN: usize = 0x17C;
/// Rising speed of that grab, cm per frame (20).
pub const ADJUST_GRAB_RISE_SPEED: usize = 0x17D;

/// The ledge values from the `rAdjustParam` table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedgeParam {
    pub reach_land: f32,
    pub reach_fall: f32,
    pub probe_high: f32,
    pub probe_low: f32,
    pub min_height: f32,
    pub top_margin: f32,
    /// Indexed by `braced as usize`.
    pub hang_body: [f32; 2],
    pub hang_hands: [f32; 2],
    pub rise_margin: f32,
    pub rise_speed: f32,
}

impl LedgeParam {
    pub fn from_adjust(table: &[f32]) -> Option<Self> {
        let get = |i: usize| table.get(i).copied();
        Some(LedgeParam {
            reach_land: get(ADJUST_LEDGE_REACH_LAND)?,
            reach_fall: get(ADJUST_LEDGE_REACH_FALL)?,
            probe_high: get(ADJUST_LEDGE_PROBE_HIGH)?,
            probe_low: get(ADJUST_LEDGE_PROBE_LOW)?,
            min_height: get(ADJUST_LEDGE_MIN_HEIGHT)?,
            top_margin: get(ADJUST_LEDGE_TOP_MARGIN)?,
            hang_body: [get(ADJUST_HANG_BODY[0])?, get(ADJUST_HANG_BODY[1])?],
            hang_hands: [get(ADJUST_HANG_HANDS[0])?, get(ADJUST_HANG_HANDS[1])?],
            rise_margin: get(ADJUST_GRAB_RISE_MARGIN)?,
            rise_speed: get(ADJUST_GRAB_RISE_SPEED)?,
        })
    }
}

/// `m_pl000_00` as shipped.
pub const LEDGE_SHIPPED: LedgeParam = LedgeParam {
    reach_land: 150.0,
    reach_fall: 100.0,
    probe_high: 200.0,
    probe_low: 50.0,
    min_height: 100.0,
    top_margin: 25.0,
    hang_body: [93.0, 60.0],
    hang_hands: [22.0, 33.0],
    rise_margin: 30.0,
    rise_speed: 20.0,
};

// Constants in the code (not in the table).
/// The third probe's down cast stops this much lower (0x015184E0).
const LOW_PROBE_EXTRA: f32 = 20.0;
/// The down cast runs this far inside the wall (0x01520BAC).
const INTO_WALL: f32 = 10.0;
/// Headroom cast above the player: high probe +- this (0x0150A088).
const HEADROOM: f32 = 50.0;
/// Clear space required above the ledge top (0x0152CF24).
const CLEAR_ABOVE: f32 = 150.0;
/// Radius of the two clearance spheres (0x015184E0).
const SPHERE_RADIUS: f32 = 20.0;
/// The second sphere is this far out from the edge (0x017F4244).
const SPHERE_OUT: f32 = 75.0;
/// A wall whose normal is flatter than this horizontally is not grabbed
/// (0x014EAE5C, on the horizontal length of the hit normal).
const MIN_WALL_HORIZONTAL: f32 = 0.1;
/// Distance of the ground probe under the ledge (0x00B6FA00).
const GROUND_PROBE: f32 = 500.0;
/// Foot search: steps out from the edge (0x014EA574) and their count.
const FOOT_STEP: f32 = 15.0;
const FOOT_STEPS: i32 = 13;
/// Hands-to-feet length of a free hang (0x0161CA08 / 0x0161CDE8).
const FREE_REACH: f32 = 210.0;
/// Hands-to-feet length of a braced hang, and the foot search's minimum
/// distance from the edge (0x01531AE8).
const BRACED_REACH: f32 = 100.0;
/// Feet stand this far above the ground the foot search finds (0x015184E0).
const FOOT_ABOVE_GROUND: f32 = 20.0;
/// The body line from the feet to the edge must rise at least this much
/// (0x0161CDE0 = 20 degrees, compared as a sine).
const MIN_BODY_RISE_RAD: f64 = 0.349_065_870_046_615_6;
/// The feet are found from a point this far out from the player (0x015184E0).
const FOOT_SIGHT_OUT: f32 = 20.0;
/// cPlCliffHangBegin::vf05 searches for the wall behind the body point
/// between these distances out from it (0x014EA160, 0x01531AE8).
const BODY_SEARCH_NEAR: f32 = 30.0;
const BODY_SEARCH_FAR: f32 = 100.0;
/// The wall search through the foot point and the climb's wall search span
/// this far each side (cPlCliffHangBegin::vf06, cPlCliffHangClimb::vf05).
const WALL_SEARCH: f32 = 100.0;
/// The rising grab keeps the feet this far off the wall (vf06 case 2).
const RISE_WALL_GAP: f32 = 10.0;
/// The rising grab snaps onto the foot point when it ends within this (vf06 case 2).
const RISE_SNAP: f32 = 20.0;
/// The ledge-present test: a vertical cast this far each side of the edge,
/// this far into the wall (cPlCliffHang::vf06).
const HOLD_SPAN: f32 = 50.0;
const HOLD_IN: f32 = 5.0;
/// Climb: the target is cast against walls from this far out
/// (cPlCliffHangClimb::vf06), and the climb ends this far in from the edge
/// and this far above it.
const CLIMB_TARGET_OUT: f32 = 20.0;
const CLIMB_END_IN: f32 = 10.0;
const CLIMB_END_UP: f32 = 5.0;
/// Letting go pushes the player out from the wall, cm per frame
/// (cPlCliffHang::vf06: velocity = normal x 2).
pub const LET_GO_SPEED: f32 = 2.0;
/// Stick push along the facing that climbs (above) or lets go (below the
/// negative) (cPlCliffHang::vf06 compares uPlayer +0x32CC with 0.8).
pub const HANG_STICK: f32 = 0.8;

/// Hits whose normal has at least this upward component count as floors,
/// steeper ones as walls. *Assumed* (the engine's hit flags are not
/// traced); the same split as `ddworld`'s wall test.
pub const FLOOR_MIN_NY: f32 = 0.7;

const EPS: f32 = 1.192_092_9e-7;
const UP: V3 = [0.0, 1.0, 0.0];

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
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > EPS {
        mul(a, 1.0 / l)
    } else {
        a
    }
}
pub(crate) fn lerp(a: V3, b: V3, t: f32) -> V3 {
    add(a, mul(sub(b, a), t))
}

/// The collision cast with the original's "segment shorter than 0.1 cm
/// is no cast" guard (`len^2 < 0.01` before every `0x00DEA870`).
fn cast(ground: &dyn Ground, from: V3, to: V3) -> Option<RayHit> {
    let d = sub(to, from);
    if dot(d, d) < 0.01 {
        return None;
    }
    ground.ray(from, to)
}

fn is_floor(h: &RayHit) -> bool {
    h.normal[1] >= FLOOR_MIN_NY
}

fn is_wall(h: &RayHit) -> bool {
    h.normal[1].abs() < FLOOR_MIN_NY
}

/// A grabbable ledge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ledge {
    pub edge: V3,
    pub normal: V3,
    pub wall_normal: V3,
}

/// One probe of the ledge test (the original's normal mode: a player
/// that is not AI-driven and not climbing a monster).
///
/// `top` and `bottom` bound the down cast (above the feet), `height` is
/// the forward cast's height above the feet and `reach` its length.
// Translated from 0x00B6FD20 (DDDA.exe 2017-12-13 Steam)
#[allow(clippy::too_many_arguments)]
fn probe(
    ground: &dyn Ground,
    feet: V3,
    yaw: f32,
    top: f32,
    bottom: f32,
    height: f32,
    reach: f32,
    p: &LedgeParam,
) -> Option<Ledge> {
    let fwd = [yaw.sin(), 0.0, yaw.cos()];
    // forward from `height` above the feet: must hit a wall
    let from = add(feet, mul(UP, height));
    let wall = cast(ground, from, add(from, mul(fwd, reach))).filter(is_wall)?;
    // (the original also rejects a hit whose attribute byte has bit 7 set,
    // 0x007C6F90; not modelled)
    let n = wall.normal;
    let h = (n[0] * n[0] + n[2] * n[2]).sqrt();
    if h < MIN_WALL_HORIZONTAL {
        return None;
    }
    let normal = [n[0] / h, 0.0, n[2] / h];
    let inward = mul(normal, INTO_WALL);
    // down inside the wall from above the high probe: must hit the top
    let x = sub(wall.point, inward);
    let top_hit = cast(
        ground,
        [x[0], feet[1] + top + p.top_margin, x[2]],
        [x[0], feet[1] + bottom, x[2]],
    )
    .filter(is_floor)?;
    let edge = add(top_hit.point, inward);
    // headroom over the player
    let head = feet[1] + top + HEADROOM;
    if cast(
        ground,
        [feet[0], feet[1] + top - HEADROOM, feet[2]],
        [feet[0], head, feet[2]],
    )
    .is_some()
    {
        return None;
    }
    // a clear line from over the player's head to just over the edge
    let over_head = [feet[0], head, feet[2]];
    let over_edge = add(sub(edge, inward), mul(UP, 5.0));
    let back = sub(over_head, over_edge);
    if dot(back, back) < 0.1 {
        return None;
    }
    if cast(ground, over_head, add(over_edge, norm(back))).is_some() {
        return None;
    }
    // room above the top, and the line back to the player both ways
    if cast(ground, over_edge, add(over_edge, mul(UP, CLEAR_ABOVE))).is_some() {
        return None;
    }
    if cast(ground, over_edge, over_head).is_some() {
        return None;
    }
    // nothing on the top where the body goes, nothing in front of the wall
    let on_top = sub(edge, inward);
    if ground.sphere([on_top[0], edge[1] + HEADROOM, on_top[2]], SPHERE_RADIUS) {
        return None;
    }
    if ground.sphere(add(edge, mul(normal, SPHERE_OUT)), SPHERE_RADIUS) {
        return None;
    }
    Some(Ledge {
        edge,
        normal,
        wall_normal: n,
    })
}

/// The ledge test: three probes (high, middle, low), then the ledge must
/// stand more than `min_height` above the ground under the player.
/// The caller decides when it runs (`Sim`): the original's action and
/// input conditions are there.
// Translated from 0x00B6FA00 (DDDA.exe 2017-12-13 Steam)
pub fn detect(ground: &dyn Ground, p: &LedgeParam, feet: V3, yaw: f32, reach: f32) -> Option<Ledge> {
    let (hi, lo) = (p.probe_high, p.probe_low);
    let ledge = probe(ground, feet, yaw, hi, lo, hi, reach, p)
        .or_else(|| probe(ground, feet, yaw, hi, lo, (hi + lo) * 0.5, reach, p))
        .or_else(|| probe(ground, feet, yaw, hi, lo - LOW_PROBE_EXTRA, lo, reach, p))?;
    let from = [feet[0], ledge.edge[1], feet[2]];
    let below = cast(ground, from, sub(from, mul(UP, GROUND_PROBE)))
        .filter(is_floor)
        .map_or(GROUND_PROBE, |h| ledge.edge[1] - h.point[1]);
    (below > p.min_height).then_some(ledge)
}

/// Where the feet hang, and whether the hang is braced (the body line
/// from the hands meets something within 210 cm, so a 100 cm body is
/// used). None when no foot position fits.
// Translated from 0x00ADBEB0 (DDDA.exe 2017-12-13 Steam)
pub fn foot_point(ground: &dyn Ground, ledge: &Ledge, feet: V3) -> Option<(bool, V3)> {
    let (l, n) = (ledge.edge, ledge.normal);
    // step out from the edge until the ground below is far enough away
    let mut found = None;
    for i in 1..=FOOT_STEPS {
        let a = add(l, mul(n, FOOT_STEP * i as f32));
        let b = add(l, mul(n, FOOT_STEP * (i + 1) as f32));
        let x = cast(ground, a, b).map_or(b, |h| h.point);
        let down = sub(x, mul(UP, FREE_REACH));
        let f = cast(ground, x, down).map_or(down, |h| add(h.point, mul(UP, FOOT_ABOVE_GROUND)));
        if len(sub(f, l)) >= BRACED_REACH {
            found = Some(f);
            break;
        }
    }
    let f = found?;
    let d = sub(f, l);
    let rest = FREE_REACH - len(d);
    let (mut braced, mut foot) = if rest > EPS {
        let e = add(f, mul(norm(d), rest));
        if cast(ground, f, e).is_some() {
            (true, f)
        } else {
            (false, e)
        }
    } else {
        (false, f)
    };
    // the body must rise at least 20 degrees from the feet to the edge
    let v = sub(l, foot);
    let lv = len(v);
    if lv > EPS && (v[1] / lv) < MIN_BODY_RISE_RAD.sin() as f32 {
        return None;
    }
    // seen from just out of the player: if blocked, try a 100 cm body
    let sight = add(feet, mul(n, FOOT_SIGHT_OUT));
    let to_feet = sub(foot, l);
    if cast(ground, sight, foot).is_some() && dot(to_feet, to_feet) > EPS {
        foot = add(l, mul(norm(to_feet), BRACED_REACH));
        if cast(ground, sight, foot).is_some() {
            return None;
        }
        braced = true;
    }
    Some((braced, foot))
}

/// The hang's key points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hang {
    pub braced: bool,
    pub hands: V3,
    pub body: V3,
    pub foot: V3,
}

/// Hands, body point and feet of the hang from the foot search.
// Translated from 0x00AD9180 (DDDA.exe 2017-12-13 Steam), cPlCliffHangBegin::vf05
pub fn hang_pose(ground: &dyn Ground, p: &LedgeParam, ledge: &Ledge, braced: bool, foot: V3) -> Hang {
    let (l, n) = (ledge.edge, ledge.normal);
    let v = usize::from(braced);
    let (along, out) = (p.hang_body[v], p.hang_hands[v]);
    let hands = add(l, mul(n, out));
    let mut body = add(hands, mul(norm(sub(foot, hands)), along));
    let mut foot = foot;
    // find the wall behind the body point and keep the body `out` off it
    let far = add(body, mul(n, BODY_SEARCH_FAR));
    let start = cast(ground, add(body, mul(n, BODY_SEARCH_NEAR)), far).map_or(far, |h| h.point);
    if let Some(w) = cast(ground, start, sub(body, mul(n, out))) {
        body = add(w.point, mul(w.normal, out));
        let reach = if braced { BRACED_REACH } else { FREE_REACH };
        let end = add(hands, mul(norm(sub(body, hands)), reach));
        foot = cast(ground, hands, end).map_or(end, |h| h.point);
    }
    Hang {
        braced,
        hands,
        body,
        foot,
    }
}

/// The rising grab's foot point: through the foot point along the wall
/// normal, 10 cm off the wall it meets.
// Translated from 0x00ADA980 (DDDA.exe 2017-12-13 Steam), cPlCliffHangBegin::vf06 case 1
pub fn rise_foot(ground: &dyn Ground, ledge: &Ledge, foot: V3) -> V3 {
    let n = ledge.normal;
    match cast(
        ground,
        add(foot, mul(n, WALL_SEARCH)),
        sub(foot, mul(n, WALL_SEARCH)),
    ) {
        Some(h) => add(h.point, mul(n, RISE_WALL_GAP)),
        None => foot,
    }
}

/// One step of the rising grab: rise by `rise` toward `foot`, kept off the
/// wall. Returns the new position.
// Translated from 0x00ADA980 (DDDA.exe 2017-12-13 Steam), cPlCliffHangBegin::vf06 case 2
pub fn rise_step(ground: &dyn Ground, ledge: &Ledge, start: V3, foot: V3, y: f32, t: f32, rise: f32) -> V3 {
    let n = ledge.normal;
    let q = [
        foot[0] * t + start[0] * (1.0 - t),
        y + rise,
        foot[2] * t + start[2] * (1.0 - t),
    ];
    match cast(ground, add(q, mul(n, WALL_SEARCH)), sub(q, mul(n, WALL_SEARCH))) {
        Some(h) => add(h.point, mul(n, RISE_WALL_GAP)),
        None => q,
    }
}

/// Whether the rising grab, having stopped at height `y`, may snap onto
/// the foot point (else it falls).
pub fn rise_snaps(foot: V3, y: f32) -> bool {
    foot[1] - RISE_SNAP <= y
}

/// The ledge is still there: a vertical cast across the edge, just inside
/// the wall, hits something.
// Translated from 0x00ADCDD0 (DDDA.exe 2017-12-13 Steam), cPlCliffHang::vf06
// (the same test ends cPlCliffHangBegin::vf06 and cPlCliffHangClimb::vf06)
pub fn holds(ground: &dyn Ground, ledge: &Ledge) -> bool {
    let x = sub(ledge.edge, mul(ledge.normal, HOLD_IN));
    cast(ground, add(x, mul(UP, HOLD_SPAN)), sub(x, mul(UP, HOLD_SPAN))).is_some()
}

/// The climb's wall point: the body point at height `y`, moved onto the
/// wall face.
// Translated from 0x00ADD670 (DDDA.exe 2017-12-13 Steam), cPlCliffHangClimb::vf05
pub fn climb_wall_point(ground: &dyn Ground, ledge: &Ledge, body: V3, y: f32) -> V3 {
    let (l, n) = (ledge.edge, ledge.normal);
    let w = [body[0], y, body[2]];
    match cast(ground, sub(w, mul(n, WALL_SEARCH)), add(w, mul(n, WALL_SEARCH))) {
        Some(h) => {
            let s = dot(n, sub(h.point, l));
            if s <= 0.0 {
                add(w, mul(n, -s))
            } else {
                h.point
            }
        }
        None => w,
    }
}

/// The climb's target at progress `t`: the wall point, raised from
/// `from_y` to just over the edge, cast against walls from 20 cm out.
// Translated from 0x00ADDC00 (DDDA.exe 2017-12-13 Steam), cPlCliffHangClimb::vf06
pub fn climb_target(ground: &dyn Ground, ledge: &Ledge, wall: V3, from_y: f32, t: f32) -> V3 {
    let top = ledge.edge[1] + CLIMB_END_UP;
    let y = if t < 1.0 {
        from_y * (1.0 - t) + top * t
    } else {
        top
    };
    let q = [wall[0], y, wall[2]];
    cast(ground, add(q, mul(ledge.normal, CLIMB_TARGET_OUT)), q).map_or(q, |h| h.point)
}

/// Where the climb ends: 10 cm in from the edge, 5 cm above it.
// Translated from 0x00ADDC00 (DDDA.exe 2017-12-13 Steam), cPlCliffHangClimb::vf06 phase 1
pub fn climb_end(ledge: &Ledge) -> V3 {
    let e = sub(ledge.edge, mul(ledge.normal, CLIMB_END_IN));
    [e[0], ledge.edge[1] + CLIMB_END_UP, e[2]]
}

/// Facing into the wall (radians, 0 faces +Z).
pub fn facing(ledge: &Ledge) -> f32 {
    (-ledge.normal[0]).atan2(-ledge.normal[2])
}
