//! Real-body scenery adapter for native Demo reactions.
//! This is not yet scheduled by Host. Common action entry, raw layer clock
//! and reaction request consumption remain required before enabling Damage.
//! Geometry uses the existing ddsim exact-distance replacement for native
//! MtCollision GJK/MPR; no original world-contact comparison is claimed.

use crate::npc_motion::Token;
use ddenemyrt::npc_body::NpcFighter;
use ddsim::sweep::{self, AxisQuery, CapsuleSegment, SweepParam};
use super::physics::Scenery;
use std::cell::Cell;

/// Current cScrAdjust settings, supplied by the body provider, not inferred
/// from HP, a motion timer or the previous collision result.
pub(super) struct SceneryInputs {
    /// Body+0x2398. Disabled updates retain the existing observation.
    pub update_flags: u32,
    /// Actual ready/awake body with collision enabled. Unsupported sleeping
    /// and disabled scene-service paths do not produce a fabricated contact.
    pub ready: bool,
    /// Target origin after native movement and ground-offset preparation.
    /// Preparing that target is separate from this scenery query.
    pub target: [f32; 3],
    /// Actual cScrAdjust+0x1AD, distinct from the prior ground byte.
    pub ground_check: bool,
    /// cScrAdjust+0x1A0. This ordinary adapter supports types0 and2.
    pub axis_type: u32,
    /// Actual cScrAdjust+0x1A4; the capsule radius is a different field.
    pub axis_radius: f32,
}

/// Query and commit only to the body named by the current command token.
/// An unavailable provider or retired command leaves the body untouched.
/// The returned contact must be projected in native vf90 order, rather
/// than immediately overwriting the reaction's preceding ground byte.
// Translated adapter of 0x00886530 -> 0x0076C540 -> 0x0079E200,
// using the shared sweep loop (DDDA.exe 2017-12-13 Steam).
pub(super) fn adjust_scenery(body: &mut NpcFighter, token: Token,
    current: Token, adjust: &[f32], input: &SceneryInputs,
    param: &SweepParam, ground: &dyn ddsim::Ground) -> Result<Option<Scenery>, String> {
    if token != current || body.npc != token.owner.npc
        || body.generation != token.owner.generation {
        return Err("Demo scenery command does not own the current body lifetime".into());
    }
    if input.update_flags & 4 == 0 { return Ok(None); }
    if !input.ready { return Err("Demo scenery needs the actual awake collision-ready body".into()); }
    let cap = body.body.mode3_scenery_capsule(adjust)?
        .ok_or("Demo scenery needs the body's supported HumanEdit capsule")?;
    let motion = std::array::from_fn(|i| input.target[i] - body.position[i]);
    if !body.position.iter().chain(&input.target).chain(&motion).all(|v| v.is_finite()) {
        return Err("invalid Demo scenery origin or target".into());
    }
    let axis = axis_query(&cap, body.position, input)?;
    // Same provider availability convention as Sim::push_player. The NPC
    // adapter additionally rejects a missing provider in any subsequent
    // query instead of retaining an unconstrained partial move.
    if !ground.triangles_in(body.position, body.position, &mut Vec::new()) {
        return Err("Demo scenery triangle provider is unavailable".into());
    }
    let unavailable = Cell::new(false);
    let swept = sweep::sweep_capsule(param, &cap, body.position, motion,
        input.ground_check, axis, &|lo, hi, faces| {
            if !ground.triangles_in(lo, hi, faces) { unavailable.set(true); }
        });
    if unavailable.get() || !swept.position.iter().all(|v| v.is_finite()) {
        return Err("Demo scenery query failed; current body position retained".into());
    }
    body.position = swept.position;
    Ok(Some(Scenery { update_flags: input.update_flags, axis_present: axis.is_some(),
        extended_ground_check: input.ground_check, contact_flags: swept.met }))
}

// Translated from 0x0079E58A..0x0079E6C3 (DDDA.exe 2017-12-13 Steam).
fn axis_query(cap: &CapsuleSegment, origin: [f32; 3], input: &SceneryInputs)
    -> Result<Option<AxisQuery>, String> {
    if input.axis_type == 0 { return Ok(None); }
    if input.axis_type != 2 { return Err("unsupported Demo scenery axis query type".into()); }
    if !input.axis_radius.is_finite() || input.axis_radius <= 0.0 {
        return Err("Demo scenery needs the actual positive axis sphere radius".into());
    }
    // Original double at0x0161C9F0, not Rust's higher-precision SQRT_2.
    // 0x0079E5AA..B8 widens the f32 radius and narrows the product first.
    let mut length = (f64::from(cap.radius) * 1.41421356_f64) as f32;
    if !input.ground_check { length = (origin[1] - input.target[1]) + length; }
    let lower = if cap.start[1] > cap.end[1] { cap.end } else { cap.start };
    let center = std::array::from_fn(|i| origin[i] + lower[i]);
    if !length.is_finite() || !center.iter().all(|v| v.is_finite()) {
        return Err("Demo scenery axis query overflow".into());
    }
    Ok(Some(AxisQuery { center, radius: input.axis_radius, motion: [0.0, -length, 0.0] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_scenery_radius95_distinguishes_native_double_from_sqrt2() {
        let cap = CapsuleSegment { start: [0.0, 95.0, 0.0], end: [0.0, 105.0, 0.0], radius: 95.0 };
        let input = SceneryInputs { update_flags: 4, ready: true, target: [0.0; 3],
            ground_check: true, axis_type: 2, axis_radius: 15.0 };
        let axis = axis_query(&cap, [0.0; 3], &input).unwrap().unwrap();
        // Original f64 at0x0161C9F0 and native widen/multiply/narrow ordering
        // at0x0079E5AA..B8. This synthetic radius exposes the one-ULP difference.
        assert_eq!((-axis.motion[1]).to_bits(), 0x4306_59ac);
        assert_ne!((-axis.motion[1]).to_bits(),
            ((95.0_f64 * std::f64::consts::SQRT_2) as f32).to_bits());
    }

    #[test]
    fn demo_scenery_axis_uses_current_radius_native_double_and_airborne_target() {
        let cap = CapsuleSegment { start: [2.0, 80.0, 4.0], end: [3.0, 35.0, 5.0], radius: 35.0 };
        let mut input = SceneryInputs { update_flags: 4, ready: true, target: [100.0, 150.0, 300.0],
            ground_check: false, axis_type: 2, axis_radius: 15.0 };
        let axis = axis_query(&cap, [100.0, 200.0, 300.0], &input).unwrap().unwrap();
        assert_eq!(axis.center, [103.0, 235.0, 305.0]);
        assert_eq!(axis.radius, 15.0);
        assert_eq!(axis.motion, [0.0, -(50.0 + (35.0_f64 * 1.41421356) as f32), 0.0]);
        input.ground_check = true;
        input.axis_radius = 9.0;
        let axis = axis_query(&cap, [100.0, 200.0, 300.0], &input).unwrap().unwrap();
        assert_eq!(axis.radius, 9.0, "actual axis radius does not follow capsule radius35");
        assert_eq!(axis.motion[1], -((35.0_f64 * 1.41421356) as f32));
        input.axis_type = 0;
        input.axis_radius = f32::NAN;
        assert_eq!(axis_query(&cap, [0.0; 3], &input).unwrap(), None);
        input.axis_type = 1;
        assert!(axis_query(&cap, [0.0; 3], &input).is_err());
        input.axis_type = 2;
        assert!(axis_query(&cap, [0.0; 3], &input).is_err());
    }

    #[test]
    fn installed_demo_scenery_moves_current_body_only_with_available_triangle_provider() {
        use std::sync::Arc;
        struct Floor { available_calls: usize, calls: Cell<usize> }
        impl ddsim::Ground for Floor {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
            fn triangles_in(&self, _: [f32; 3], _: [f32; 3], out: &mut Vec<sweep::Face>) -> bool {
                let call = self.calls.get();
                self.calls.set(call + 1);
                if call >= self.available_calls { return false; }
                out.extend([
                    ([[-1000.0, 0.0, -1000.0], [1000.0, 0.0, 1000.0], [1000.0, 0.0, -1000.0]], 0),
                    ([[-1000.0, 0.0, -1000.0], [-1000.0, 0.0, 1000.0], [1000.0, 0.0, 1000.0]], 0),
                ]);
                true
            }
        }
        let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() { eprintln!("SKIP: DDDA install absent"); return; }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let file = ddexe::ExeFile::next_to(&native).unwrap();
        let exe = file.image().unwrap();
        let body = Arc::new(ddenemyrt::npc_body::NpcBody::load(&vfs, &exe, 704, 704, 0, 0, 0).unwrap());
        let bytes = vfs.read(&exe.cstr(exe.u32(0x0163_e660).unwrap()).unwrap(), mtarc::type_hash("rAdjustParam")).unwrap();
        let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let adjust: Vec<f32> = bytes[12..12 + count * 4].chunks_exact(4)
            .map(|word| f32::from_le_bytes(word.try_into().unwrap())).collect();
        // Explicit combat-pool fixture: scenery cannot change these values.
        let region = ddcombat::damage::RegionPool::from_element(0, 23.0, 7.0, 1.0, 9.0, 1.0,
            1.0, 1.0, 0, 1, 1.0, 1.0);
        let mut fighter = NpcFighter::new(704, 2, 19, body, region.clone(),
            ddenemyrt::body::MotionId::new(1, 2));
        fighter.position = [0.0, 100.0, 0.0];
        let token = Token { owner: crate::story_npcs::NpcLifetime { npc: 704, generation: 2 }, command: 4, launch: 8 };
        let mut input = SceneryInputs { update_flags: 4, ready: true, target: [80.0, -100.0, 0.0],
            ground_check: false, axis_type: 0, axis_radius: 15.0 };
        let param = SweepParam::default();
        for available_calls in [0, 1] {
            let ground = Floor { available_calls, calls: Cell::new(0) };
            assert!(adjust_scenery(&mut fighter, token, token, &adjust, &input, &param, &ground).is_err());
            assert_eq!(fighter.position, [0.0, 100.0, 0.0], "missing initial or later geometry is atomic");
        }
        let ground = Floor { available_calls: usize::MAX, calls: Cell::new(0) };
        let stale = Token { launch: 7, ..token };
        assert!(adjust_scenery(&mut fighter, stale, token, &adjust, &input, &param, &ground).is_err());
        fighter.generation = 3;
        assert!(adjust_scenery(&mut fighter, token, token, &adjust, &input, &param, &ground).is_err());
        fighter.generation = 2;
        input.update_flags = 0;
        assert!(adjust_scenery(&mut fighter, token, token, &[], &input, &param, &ground).unwrap().is_none());
        assert_eq!(fighter.position, [0.0, 100.0, 0.0]);
        assert_eq!(ground.calls.get(), 0, "stale and disabled operations cannot query or move");
        input.update_flags = 4;
        let contact = adjust_scenery(&mut fighter, token, token, &adjust, &input, &param, &ground).unwrap().unwrap();
        assert_ne!(fighter.position, input.target, "actual floor collision constrains the requested move");
        assert_eq!(contact.contact_flags & sweep::FLOOR, sweep::FLOOR);
        assert!(!contact.axis_present && !contact.extended_ground_check);
        assert!(fighter.position[1] >= 0.0);
        let ddenemyrt::combat::Victim::Npc { region: after, .. } = &fighter.fighter.victim else { panic!("NPC victim replaced") };
        assert_eq!(after, &region, "scenery integration does not synthesize contact damage");
    }
}
