//! Building the systems from the player's install: one place for every
//! user of `ddsim` (the network host, `ddview --play`, `ddaccept`), so their
//! simulations cannot drift apart.
//!
//! Unlike the earlier per-user builders, this loads every motion of the
//! mapped banks instead of a hand list of the ids the controller uses: a
//! state added to ddsim finds its motion without touching this file.

use ddsim::{JumpParam, LedgeParam, MoveParam, RootMotion, Sim};
use std::collections::HashMap;

/// Bank 0, the common motions of the male Arisen (docs/ANIMATION.md).
pub const MOTIONS: &str = r"motion\pl\m\m00\m0000_co\m0000_co";
/// Bank 6, the equipped weapon set's motions, here the dagger set's
/// (bank table at 0x014F1D30; which group is the dagger's is not traced,
/// docs/MOVEMENT.md).
pub const DAGGER_MOTIONS: &str = r"motion\pl\m\m00\m0004_co\m0004_co";
/// `rPlJumpParam`: rows 0 (normal jump) and 9 (sprint jump), see ddsim.
pub const JUMP_PARAMS: &str = r"param\pl\other\pljumpparam";
/// `rAdjustParam` of the player: ground speeds and ledge values by id.
pub const ADJUST_PARAMS: &str = r"param\pl\other\m_pl000_00";

/// Motion banks ddsim uses, by bank number (`motion id >> 8`).
pub const BANKS: [(u16, &str); 2] = [(0, MOTIONS), (6, DAGGER_MOTIONS)];

/// The floats of an `rAdjustParam` (`ajp\0`, count at +8, floats from +12).
pub fn adjust_table(vfs: &ddvfs::Vfs, path: &str) -> Result<Vec<f32>, String> {
    let b = vfs.read(path, mtarc::type_hash("rAdjustParam"))?;
    if b.len() < 12 || &b[..4] != b"ajp\0" {
        return Err(format!("{path}: not an ajp file"));
    }
    let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let body = b.get(12..12 + 4 * n).ok_or(format!("{path}: {n} floats do not fit"))?;
    Ok(body.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
}

/// Rows `indices` of `pljumpparam`.
pub fn jump_params(vfs: &ddvfs::Vfs, indices: &[usize]) -> Result<Vec<JumpParam>, String> {
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

/// Every motion of the [`BANKS`], keyed `bank << 8 | slot`, with its root
/// track, curves and events.
pub fn root_motions(vfs: &ddvfs::Vfs) -> Result<HashMap<u16, RootMotion>, String> {
    let rml = mtarc::type_hash("rMotionList");
    let mut roots = HashMap::new();
    for (bank, path) in BANKS {
        let list = mtlmt::parse(&vfs.read(path, rml)?).map_err(|e| format!("{path}: {e}"))?;
        for (slot, m) in list.motions.iter().enumerate() {
            let Some(m) = m else { continue };
            let id = bank << 8 | u16::try_from(slot).map_err(|_| format!("{path}: slot {slot}"))?;
            let r = RootMotion::from_motion(m)
                .with_curves(list.curves[slot].clone())
                .with_events(list.events[slot].clone());
            roots.insert(id, r);
        }
    }
    Ok(roots)
}

/// A `ddsim::Sim` configured from the install: motions, jump rows 0 and 9,
/// ground speeds and ledge values from the adjust table. Stamina keeps
/// ddsim's shipped tables (`StaminaTables::shipped`).
pub fn sim(vfs: &ddvfs::Vfs) -> Result<Sim, String> {
    let mut sim = Sim::new(root_motions(vfs)?);
    let jp = jump_params(vfs, &[0, 9])?;
    sim.jump_normal = jp[0];
    sim.jump_sprint = jp[1];
    let adjust = adjust_table(vfs, ADJUST_PARAMS)?;
    sim.move_param = MoveParam::from_adjust(&adjust).ok_or(format!("{ADJUST_PARAMS}: too short"))?;
    sim.ledge_param = LedgeParam::from_adjust(&adjust).ok_or(format!("{ADJUST_PARAMS}: no ledge params"))?;
    Ok(sim)
}
