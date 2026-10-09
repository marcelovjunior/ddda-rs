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

/// The economy's data from the install: item list, upgrade table, shop
/// lists as `uNpc::vf32` (0x00BACE50) picks them (the `dl1` copies for NPCs
/// 784 and 156) and the Suasion factors (`plAbilityParam` record 0x54).
pub fn catalog(vfs: &ddvfs::Vfs) -> Result<ddeconomy::Catalog, String> {
    let read = |class: &str, path: &str| vfs.read(path, mtarc::type_hash(class));
    let items = dditem::itl::ItemList::parse(&read("rItemList", r"etc\item\itemList")?).map_err(|e| format!("{e:?}"))?;
    let lvup = dditem::lvup::EquipLvUp::parse(&read("rEquipLvUp", r"etc\item\equipLvUp")?).map_err(|e| format!("{e:?}"))?;
    let mut shops = std::collections::BTreeMap::new();
    let h = mtarc::type_hash("rShopList");
    for p in vfs.paths_of_type(h) {
        let name = p.rsplit('\\').next().unwrap_or("");
        let Some(no) = name.get(1..4).and_then(|n| n.parse::<u32>().ok()) else { continue };
        if (no == 784 || no == 156) != p.starts_with("dl1") {
            continue;
        }
        shops.insert(no, dditem::shop::ShopList::parse(&vfs.read(p, h)?).map_err(|e| format!("{p}: {e:?}"))?);
    }
    let x = mtxfs::parse(&read("rPlAbilityParam", r"param\pl\other\plAbilityParam")?).map_err(|e| e.to_string())?;
    let objs = x.walk();
    let rec = *objs.get(1 + 0x54).ok_or("plAbilityParam: no record 0x54")?;
    let f = |n: &str| match x.get(rec, n).and_then(|v| v.first()) {
        Some(mtxfs::Value::F(v)) => Ok(v[0] as f32),
        _ => Err(format!("plAbilityParam 0x54: no {n}")),
    };
    Ok(ddeconomy::Catalog { items, shops, lvup, suasion: (f("mFVal0")?, f("mFVal1")?) })
}

/// Progression's tables from the install and the player's `DDDA.exe`.
pub fn tables(vfs: &ddvfs::Vfs, exe: &std::path::Path) -> Result<ddprogress::Tables, String> {
    let file = ddexe::ExeFile::open(exe).map_err(|e| e.0)?;
    let image = file.image().map_err(|e| e.0)?;
    ddprogress::Tables::load(vfs, &image)
}

/// The stage collision of a single-model stage (Cassardis and the other
/// non-grid stages: `scr\stSSS\collision\stSSSh_mrg00`, the half characters
/// stand on, as `ddworld::Terrain::load_collision` reads it per cell on
/// st100). The material flag words are not carried (`Terrain::add`).
pub fn stage_collision(vfs: &ddvfs::Vfs, stage: u32) -> Result<ddworld::Terrain, String> {
    let path = format!(r"scr\st{stage}\collision\st{stage}h_mrg00");
    let b = vfs.read(&path, mtarc::type_hash("rCollision"))?;
    let c = mtsbc::parse(&b).map_err(|e| format!("{path}: {e:?}"))?;
    let mut t = ddworld::Terrain::empty();
    for (p, _) in c.world_triangles() {
        t.add(p);
    }
    t.cells = 1;
    Ok(t)
}

/// A stage's start positions (`scr\stSSS\etc\stSSS`, rStartPos), which
/// `PlaceParty { start_pos }` and `JumpArea { pos }` name.
pub fn start_positions(vfs: &ddvfs::Vfs, stage: u32) -> Result<ddmap::StartPositions, String> {
    let path = format!(r"scr\st{stage}\etc\st{stage}");
    let b = vfs.read(&path, mtarc::type_hash("rStartPos"))?;
    ddmap::StartPositions::parse(&b).map_err(|e| format!("{path}: {e}"))
}

/// NPC ids whose daily controller exists for `stage` at SceNo `sce`
/// (`ddnpc::schedule_path` with mUse24Schedule 1, then the install). The
/// layout flags that gate each placement group are not applied
/// (*placeholder*: every NPC with a controller is present).
pub fn stage_npcs(vfs: &ddvfs::Vfs, stage: u32, sce: u32) -> Vec<u32> {
    let fsm = mtarc::type_hash("rAIFSM");
    (0..900)
        .filter(|&id| {
            ddnpc::schedule_path(stage as i32, sce as i32, id as i32, 1)
                .is_some_and(|p| vfs.locate(&p.to_ascii_lowercase(), fsm).is_some() || vfs.locate(&p, fsm).is_some())
        })
        .collect()
}
