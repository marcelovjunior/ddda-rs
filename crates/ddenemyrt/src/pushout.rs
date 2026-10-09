//! Characters pushed out of each other (docs/FRAME.md step 38).
//!
//! Translated from 0x004797E0 (DDDA.exe 2017-12-13 Steam), run after both
//! move lines. The collision manager pairs the characters' push-out
//! volumes (groups with `mIsObjAdj`, switched on by the motions like the
//! hit volumes); each overlapping pair carries a contact (0x00773D30:
//! point, normal at +0x690, depth at +0x6A0). For each pair:
//! - the character with the *higher* push rank (actor +0x1DF0, the
//!   collision's +0xD10) is pushed. On equal ranks the pair's first
//!   character is pushed, unless it is the Arisen (actor +0x206D bit 1,
//!   set by `uPlayer`'s ctor 0x00B55900) and the second a pawn or an NPC
//!   (bits 2 / 3, `uCmc` 0x00B53220, `uNpc` 0x00BAB3A0): then the second;
//!   two pawns share the push by halves when the manager's flag +0x9C528
//!   is set (not modelled). The order of a pair is the collision manager's
//!   (*not traced*); the fight lists the Arisen first;
//! - a rank of 0 is never pushed;
//! - the push is recorded on the pushed collision (0x0076D420: the deepest
//!   contact of the frame wins, depth +0x8C8, direction +0x890) and applied
//!   in pass C before the scenery sweep (0x0076D6B0, from `uObjModel::vf09`):
//!   x and z move by depth x direction, y does not.
//!
//! Ranks: the Arisen's actions set theirs (cPlActWait 13, the walk 14, the
//! run and sprint 15, landings and most attack actions 15; writers listed
//! in docs/COMBAT.md); an enemy's is its `cCharParamEnemy` `OBJ補正サイズ`
//! (+0xD4; goblin 13), plus 1 while its action sets +0x2360 bits 0x104
//! (attacks, walks) or +0x2364 bit 0x8000000, else plus 2 for bits 0x18
//! (runs, dashes) or +0x2364 bits 0x50000004 (`uEnemy::vf90` 0x00AA96E0).

use ddcombat::volumes::{Vec3, Volume};

/// One character's part in the push-out.
#[derive(Debug, Clone, PartialEq)]
pub struct PushBody {
    /// Push rank (actor +0x1DF0).
    pub rank: u32,
    /// Actor +0x206D: bit 1 (2) the Arisen, bit 2 (4) a pawn, bit 3 (8) an
    /// NPC; 0 for enemies.
    pub kind: u8,
    /// Its push-out volumes this frame.
    pub volumes: Vec<Volume>,
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The closest points of two segments (Ericson 5.1.9).
fn closest(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
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
    let at = |p: Vec3, d: Vec3, k: f32| [p[0] + d[0] * k, p[1] + d[1] * k, p[2] + d[2] * k];
    (at(p1, d1, s), at(p2, d2, t))
}

fn core(v: &Volume) -> (Vec3, Vec3, f32) {
    match *v {
        Volume::Sphere { c, r } => (c, c, r),
        Volume::Capsule { p0, p1, r } => (p0, p1, r),
    }
}

/// The contact of two overlapping volumes: the unit direction from `a`'s
/// closest core point to `b`'s, and the depth (sum of radii minus the
/// distance). *Assumed* to match the MtCollision contact 0x00773D30
/// stores (point, normal, depth); the shape routines are not read.
pub fn contact(a: &Volume, b: &Volume) -> Option<(Vec3, f32)> {
    let (a0, a1, ra) = core(a);
    let (b0, b1, rb) = core(b);
    let (pa, pb) = closest(a0, a1, b0, b1);
    let d = sub(pb, pa);
    let dist = dot(d, d).sqrt();
    let depth = ra + rb - dist;
    if depth < 0.0 {
        return None;
    }
    let n = if dist > 1e-6 {
        [d[0] / dist, d[1] / dist, d[2] / dist]
    } else {
        [0.0, 0.0, 1.0]
    };
    Some((n, depth))
}

/// The push of one pair: which body moves (0 for `a`, 1 for `b`) and by
/// how much in x and z, or None when they do not overlap or the pushed one
/// has rank 0. The deepest overlap of their volumes is taken (0x0076D420
/// keeps the deepest contact).
// Translated from 0x004797E0 and 0x0076D420 (DDDA.exe 2017-12-13 Steam).
pub fn push(a: &PushBody, b: &PushBody) -> Option<(usize, [f32; 2])> {
    let mut best: Option<(Vec3, f32)> = None;
    for va in &a.volumes {
        for vb in &b.volumes {
            if let Some((n, depth)) = contact(va, vb) {
                if best.is_none_or(|(_, d)| depth > d) {
                    best = Some((n, depth));
                }
            }
        }
    }
    let (n, depth) = best?;
    // the higher rank is pushed; on equal ranks the first, unless the
    // Arisen meets a pawn or NPC
    let pushed = if a.rank != b.rank {
        usize::from(b.rank > a.rank)
    } else {
        usize::from(a.kind & 2 != 0 && b.kind & 0xc != 0)
    };
    let rank = if pushed == 0 { a.rank } else { b.rank };
    if rank == 0 || depth <= 0.0 {
        return None;
    }
    // away from the other: `a` against the normal, `b` along it
    let s = if pushed == 0 { -depth } else { depth };
    Some((pushed, [n[0] * s, n[2] * s]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(rank: u32, arisen: bool, x: f32) -> PushBody {
        PushBody {
            rank,
            kind: if arisen { 2 } else { 0 },
            volumes: vec![Volume::Sphere {
                c: [x, 50.0, 0.0],
                r: 35.0,
            }],
        }
    }

    #[test]
    fn the_higher_rank_is_pushed_out_by_the_depth() {
        // overlap 70 - 50 = 20 cm
        let (who, d) = push(&body(13, true, 0.0), &body(15, false, 50.0)).unwrap();
        assert_eq!(who, 1);
        assert!((d[0] - 20.0).abs() < 1e-4 && d[1].abs() < 1e-6, "{d:?}");
        let (who, d) = push(&body(15, true, 0.0), &body(13, false, 50.0)).unwrap();
        assert_eq!(who, 0);
        assert!((d[0] + 20.0).abs() < 1e-4, "{d:?}");
    }

    #[test]
    fn on_equal_ranks_the_first_moves_and_rank_0_never_does() {
        let (who, _) = push(&body(13, true, 0.0), &body(13, false, 50.0)).unwrap();
        assert_eq!(who, 0);
        let (who, _) = push(&body(13, false, 0.0), &body(13, true, 50.0)).unwrap();
        assert_eq!(who, 0);
        let pawn = PushBody {
            kind: 4,
            ..body(13, false, 50.0)
        };
        let (who, _) = push(&body(13, true, 0.0), &pawn).unwrap();
        assert_eq!(who, 1);
        assert!(push(&body(0, true, 0.0), &body(0, false, 50.0)).is_none());
        assert!(push(&body(13, true, 0.0), &body(13, false, 80.0)).is_none());
    }
}
