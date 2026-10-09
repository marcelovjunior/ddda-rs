//! Source-derived chimera death dispatch primitives.
//!
//! These select actions after the combat layer requests death. They do not
//! infer death from absent actors, timers, animation completion or child loss.
//! Damage routing and the host's persistent placement binding remain separate.

/// Runtime resource/action catalogue for the ordinary em5200 family.
/// Gorechimera variants are intentionally not selected by this API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Body,
    Child00,
    Child01,
}

/// Initial think-controller state and only the records reachable from it.
/// Later CtrlThinkTbl switches and the unit-specific think/action driver
/// remain separate from this initialization prefix.
#[derive(Debug, Clone)]
pub struct ThinkInitialization {
    pub tables: ddai::thinktbl::Tables,
    pub current: u32,
    pub default: u32,
    pub base: u32,
    /// Manager +0xea4; body explicitly locks its selected controller table.
    pub lock: bool,
}

impl ThinkInitialization {
    /// `controller` is Some(index) only when body +0x5c14 is enabled; the
    /// index is body +0x5c18. For children a missing parent is None as well.
    /// Root records are read from the executable, never compiled into Rust.
    // Translated from 0x009a0880, 0x009b0f50 and 0x009b8f10
    // (DDDA.exe 2017-12-13 Steam).
    pub fn read(exe: &ddexe::Exe<'_>, unit: Unit, controller: Option<u32>) -> Result<Self, String> {
        let (current, fallback, lock) = match unit {
            Unit::Body => {
                let fallback = 0x0170_f060;
                let current = if let Some(index) = controller {
                    let address = index
                        .checked_mul(4)
                        .and_then(|offset| 0x016c_d234u32.checked_add(offset))
                        .ok_or("chimera controller index overflow")?;
                    exe.u32(address)?
                } else {
                    fallback
                };
                (current, fallback, controller.is_some())
            }
            Unit::Child00 => {
                let root = if controller == Some(2) {
                    0x016f_f368
                } else {
                    0x016f_e6e8
                };
                (root, root, false)
            }
            Unit::Child01 => {
                let root = if controller == Some(2) {
                    0x016f_9a28
                } else {
                    0x016f_8ee8
                };
                (root, root, false)
            }
        };
        if exe.section_of(current) != Some(".data") {
            return Err(format!(
                "chimera controller root {current:#x} is not in executable data"
            ));
        }
        // Body writes selected defaults in the enabled branch, then always
        // overwrites +0x6e9c/+0x6ea0 with fallback at the end of initialization.
        let tables = ddai::thinktbl::Tables::read_roots(exe, vec![current, fallback], &[])
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tables,
            current,
            default: fallback,
            base: fallback,
            lock,
        })
    }

    /// The caller supplies actor +0x5ae4's range scale. This constructs a
    /// manager; it does not supply target slots, think flags or action logic.
    pub fn manager(&self, scale: f32) -> ddai::thinktbl::ThinkMgr {
        let mut manager = ddai::thinktbl::ThinkMgr::new();
        manager.table = self.current;
        manager.root = self.current;
        manager.default_table = self.default;
        manager.base_table = self.base;
        manager.lock = self.lock;
        manager.scale_on = scale > 0.0 && scale != 1.0;
        manager.scale = if manager.scale_on { scale } else { 1.0 };
        manager
    }
}

/// Loaded `cCharParamEm5200` +8/+0xc values (actor +0x7268/+0x726c).
/// Reflected labels identify the lion-head magick bonuses. No defaults are
/// supplied: the values belong to the installed specific-parameter resource.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyDamageBonus {
    pub child00_alive: f32,
    pub child00_missing_or_dead: f32,
}

impl BodyDamageBonus {
    /// Reflection 0x0074ec30 maps these labels to +8/+0xc; initialization
    /// 0x009a0918 copies the specific resource to actor +0x7260 via 0x00756990.
    pub fn from_object(object: &mtxfs::Xfs) -> Result<Self, String> {
        let read = |name| match object.get(&object.root, name) {
            Some([mtxfs::Value::F(values)]) if values.len() == 1 => Ok(values[0] as f32),
            _ => Err(format!("chimera specific parameters: missing scalar {name}")),
        };
        Ok(Self {
            child00_alive: read("ライオン頭部のみへの魔法ダメージボーナス（２頭時のみ）")?,
            child00_missing_or_dead: read("ライオン頭部のみへの魔法ダメージボーナス（１頭時のみ）")?,
        })
    }

    /// Apply after common enemy mitigation and before region/rate scaling.
    /// `suppressed` is actor byte +0x7406; its broader meaning is unresolved.
    /// `resolved_pool_id` is HitInfo +0x68, filled from resolved region +4 by
    /// 0x0076db10, rather than the hurt-region index at HitInfo +0x6c.
    /// `child00_alive` means child exists and vf117 (0x004c7590) returns
    /// zero from actor +0x5bd0. HP alone does not establish that observation.
    // Translated from 0x009a44f0 (DDDA.exe 2017-12-13 Steam).
    pub fn apply(
        &self,
        hit: &mut ddcombat::damage::MergedHit,
        resolved_pool_id: u32,
        suppressed: bool,
        child00_alive: bool,
    ) {
        if !suppressed && resolved_pool_id == 1 {
            hit.mag *= if child00_alive {
                self.child00_alive
            } else {
                self.child00_missing_or_dead
            };
        }
    }
}

/// One unit's installed body and combat parameters. Body and children each
/// retain their own pools; this does not infer links or parent kill identity.
#[derive(Debug, Clone)]
pub struct ChimeraData {
    pub catalogue: Catalogue,
    pub body: crate::body::Body,
    pub attacker: ddcombat::damage::EnemyAttacker,
    pub defender: ddcombat::damage::EnemyDefender,
    pub regions: Vec<ddcombat::damage::RegionPool>,
    pub obj_size: u32,
    /// The body-only post-mitigation override; children do not inherit it.
    pub damage_bonus: Option<BodyDamageBonus>,
}

impl ChimeraData {
    /// Scale comes from the owning placement (children copy their parent's
    /// scale in 0x009a0880), rather than a chosen constructor default.
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, unit: Unit, scale: f32) -> Result<Self, String> {
        let catalogue = Catalogue::read(exe, unit)?;
        let body = catalogue.load_body(vfs, scale)?;
        let common = catalogue.unique_bound("rPropParam", |r| r.path.ends_with("_cmn"))?;
        let region = catalogue.unique_bound("rRegionStatus", |_| true)?;
        // Initializers 0x009a0bd1, 0x009b1118 and 0x009b909c select kind 0.
        let stats = crate::enemy_stats::load_paths(vfs, &common.path, &region.path)?;
        let damage_bonus = if unit == Unit::Body {
            let parameters = ddenemy::EnemyData::load(vfs, ddenemy::EnemyId::parse("em5200")?)?;
            let specific = parameters
                .specific
                .ok_or("chimera: missing charparam/em/em5200 specific parameters")?;
            Some(BodyDamageBonus::from_object(&specific.object)?)
        } else {
            None
        };
        Ok(Self {
            catalogue,
            body,
            attacker: stats.attacker,
            defender: stats.defender,
            regions: stats.regions,
            obj_size: stats.obj_size,
            damage_bonus,
        })
    }

    /// Instantiate independent combat state for an explicit owner. Collision
    /// rates/flags are caller-supplied until the unit's initialization and
    /// action-specific flag writes are fully translated.
    pub fn fighter(&self, owner: u32, rates: ddcombat::damage::CollisionRates) -> crate::combat::Fighter {
        use crate::combat::{Attacker, Fighter, Victim};
        Fighter {
            owner,
            attacker: Attacker::Enemy(self.attacker),
            victim: Victim::Enemy {
                defender: self.defender,
                regions: self.regions.clone(),
                rates,
            },
            attacks: Vec::new(),
            hurts: Vec::new(),
            history: Default::default(),
            statuses: Default::default(),
            dots: Default::default(),
            pools: Default::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRecord {
    pub class: String,
    /// First copied parameter (+4 in the record, +0x48 in the action).
    /// Its meaning depends on `class`; death/fall actions use a variant.
    pub argument: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRequest {
    pub path: String,
    pub class: String,
    /// Effective flags after the exe's CRT initializer, not its zero-filled
    /// file word. Bit 0 participates in 0x008892b0 resource binding.
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct Catalogue {
    pub unit: Unit,
    pub actions: Vec<ActionRecord>,
    /// Original order, including repeated requests. The body's list also
    /// prepares its children; do not treat all models as the body skeleton.
    pub resources: Vec<ResourceRequest>,
    /// Collision hit-region -> life-pool ID (+0xcf4 on cObjCollision).
    /// IDs are resolved against the selected region set, not used as indices.
    pub hit_to_pool: Vec<u32>,
}

impl Catalogue {
    fn unique_bound(
        &self,
        class: &str,
        select: impl Fn(&ResourceRequest) -> bool,
    ) -> Result<&ResourceRequest, String> {
        let mut matches = self
            .resources
            .iter()
            .filter(|r| r.flags & 1 != 0 && r.class == class && select(r));
        let selected = matches
            .next()
            .ok_or_else(|| format!("chimera {:?}: missing bound {class}", self.unit))?;
        if matches.next().is_some() {
            return Err(format!("chimera {:?}: ambiguous bound {class}", self.unit));
        }
        Ok(selected)
    }

    /// Translated from 0x008892b0 / 0x00889510 (DDDA.exe 2017-12-13 Steam):
    /// only bit-zero resources bind; motion-bank order follows descriptors.
    /// Collision event activations resolve the files by their resource IDs.
    pub fn load_body(&self, vfs: &ddvfs::Vfs, scale: f32) -> Result<crate::body::Body, String> {
        use crate::body::{load, Body};
        let model = self.unique_bound("rModel", |_| true)?;
        let skeleton = load::skeleton(vfs, &model.path)?;
        let bound = || self.resources.iter().filter(|r| r.flags & 1 != 0);
        let banks = bound()
            .filter(|r| r.class == "rMotionList")
            .map(|r| load::bank(vfs, &r.path).map(Some))
            .collect::<Result<_, _>>()?;
        let collision = bound()
            .filter(|r| r.class == "rObjCollision")
            .map(|r| load::collision(vfs, &r.path, None))
            .collect::<Result<_, _>>()?;
        Ok(Body {
            skeleton,
            banks,
            collision,
            scale,
        })
    }

    /// Read bulk records from the supported installed exe. Class identities
    /// come from its own DTI registrations, never a copied class-name table.
    pub fn read(exe: &ddexe::Exe<'_>, unit: Unit) -> Result<Self, String> {
        // Action registrations: 0x009a0ccc, 0x009b11cb, 0x009b9135.
        // Resource registrations: 0x009a0820, 0x009b0ef0, 0x009b8eb0.
        // DDDA.exe 2017-12-13 Steam.
        let (actions_va, action_count, resources_va, resource_count, extra_resource) = match unit {
            Unit::Body => (0x0184_bcd8, 142, 0x0184_e290, 85, 0x0187_2450),
            Unit::Child00 => (0x0184_ec00, 85, 0x0185_0298, 33, 0x0187_2450),
            Unit::Child01 => (0x0185_0630, 90, 0x0185_1e18, 26, 0x0187_230c),
        };
        let names = exe.class_names();
        let class = |dti: u32| {
            names
                .get(&dti)
                .cloned()
                .ok_or_else(|| format!("chimera: unknown DTI {dti:#x}"))
        };
        let mut actions = Vec::new();
        // 0x007f19c0 advances each action record by 0x44. Unlike em0100,
        // these tables put the action DTI at +0, not +0x28.
        for index in 0..action_count {
            let va = actions_va + index * 0x44;
            actions.push(ActionRecord {
                class: class(exe.u32(va)?)?,
                argument: exe.u32(va + 4)?,
            });
        }
        let mut resources = Vec::new();
        // Translated from 0x00889de0 (DDDA.exe 2017-12-13 Steam):
        // 12-byte path/DTI/flags entries; null paths are skipped.
        for (index, va) in (0..resource_count)
            .map(|index| resources_va + index * 12)
            .chain(std::iter::once(extra_resource))
            .enumerate()
        {
            let path = exe.u32(va)?;
            if path != 0 {
                resources.push(ResourceRequest {
                    path: exe.cstr(path)?.to_owned(),
                    class: class(exe.u32(va + 4)?)?,
                    flags: initialized_resource_flags(unit, index, exe.u32(va + 8)?),
                });
            }
        }
        // Initializers 0x009a0bd6, 0x009b111d, 0x009b90a1 install
        // these mappings and count 12 at collision +0xd78. Bulk values
        // are read from the install; 0x00770f00 resolves the mapped ID.
        let map = match unit {
            Unit::Body => 0x014f_e1e0,
            Unit::Child00 => 0x014f_de28,
            Unit::Child01 => 0x014f_da78,
        };
        Ok(Self {
            unit,
            actions,
            resources,
            hit_to_pool: exe.u32s(map, 12)?,
        })
    }

    /// Resolve an installed hit-region mapping by pool identity. Unknown
    /// regions remain unresolved rather than silently damaging the main pool.
    // Translated from 0x00770f00 (DDDA.exe 2017-12-13 Steam), with a
    // bounds check for malformed caller input before the original lookup.
    pub fn pool_index(&self, hit_region: usize, pools: &[ddenemy::RegionElement]) -> Option<usize> {
        let id = self.hit_to_pool.get(hit_region)?;
        pools.iter().position(|pool| pool.id == *id)
    }

    /// Parse every region-status resource requested by this unit. The caller
    /// must still select the source's region kind; no HP aggregation or pool
    /// ownership is inferred from the catalogue.
    pub fn regions(&self, vfs: &ddvfs::Vfs) -> Result<Vec<(String, ddenemy::RegionStatus)>, String> {
        self.resources
            .iter()
            .filter(|r| r.class == "rRegionStatus")
            .map(|r| {
                let data = vfs.read(&r.path, mtarc::type_hash(&r.class))?;
                Ok((r.path.clone(), ddenemy::RegionStatus::parse(&data)?))
            })
            .collect()
    }
}

/// Resource-table CRT fixups. `index` is the original list index; its
/// separately appended collision request is index 85/33/26 respectively.
// Translated from 0x0135a120..0x0135a6e0 (body),
// 0x0135b8a0..0x0135badc (00), 0x0135c440..0x0135c602 (01),
// 0x01371900 and 0x013716b0 (appended collision),
// DDDA.exe 2017-12-13 Steam.
pub(crate) fn initialized_resource_flags(unit: Unit, index: usize, stored: u32) -> u32 {
    let bind = match unit {
        Unit::Body => (index < 40 && index != 1) || index == 56 || index == 85,
        Unit::Child00 => index < 26 || index == 32 || index == 33,
        Unit::Child01 => true,
    };
    if bind {
        (stored & !4) | 3
    } else {
        (stored & !5) | 2
    }
}

/// Common enemy action arbitration, actor +0x2e34/+0x2e38.
/// These are priority masks, **not** chimera body-part masks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Priorities {
    pub current: u32,
    pub pending: u32,
}

impl Priorities {
    // Translated from 0x009a4dc0, 0x009a5880 and 0x009a5a00
    // (DDDA.exe 2017-12-13 Steam). A blocked request remains pending.
    fn request(&mut self, priority: u32) -> bool {
        self.pending |= 1 << priority;
        (self.current | self.pending).trailing_zeros() == priority
    }
}

/// Inputs common to the three vf72 death callbacks. The caller must obtain
/// `has_damage_context` from accepted combat, not create a synthetic context.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeathContext {
    pub has_damage_context: bool,
    /// Return value of the actor's vf73 (vtable +0x1d4).
    pub already_dead: bool,
}

/// Body state consumed by death and parent-reaction selectors.
#[derive(Debug, Clone, Copy, Default)]
pub struct BodyState {
    /// Actor +0x2dd4, current local action.
    pub action: u32,
    /// Actor +0x384, low 16 bits of current motion.
    pub motion: u16,
    /// Actor +0x1ed0. Exact semantic name remains unresolved.
    pub alternate_pose: bool,
}

/// Select the body death action. `Some` means the original calls common
/// uEnemy::vf72 *before* assigning this action; it is not animation cleanup.
// Translated from 0x009a4dc0 (DDDA.exe 2017-12-13 Steam).
pub fn body_death(priorities: &mut Priorities, context: DeathContext, body: BodyState) -> Option<u32> {
    if !context.has_damage_context || context.already_dead || !priorities.request(2) {
        return None;
    }
    Some(if body.action == 0x89 {
        0x57
    } else if !body.alternate_pose {
        0x3a
    } else {
        match body.motion {
            0x212 => 0x55,
            0x213 => 0x56,
            0x227 => 0x53,
            0x228 => 0x54,
            0x239 => 0x52,
            _ => 0x58,
        }
    })
}

/// Child identity follows the actual classes and body links. Anatomical
/// names are deliberately withheld until their collision routing is traced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child {
    /// uEm5200_00: body +0x72e0, child parent +0x7278.
    Unit00,
    /// uEm5200_01: body +0x72e4, child parent +0x72c0.
    Unit01,
}

/// Extra guards used only by uEm5200_01::vf72.
#[derive(Debug, Clone, Copy, Default)]
pub struct Child01State {
    /// Child +0x72c4; zero is the attached unit's initial mode.
    pub mode: u32,
    pub action: u32,
    /// A missing parent does not block the original callback.
    pub parent_dead: bool,
}

/// Select child death. The caller must dispatch common enemy death for this
/// child's own identity and then notify its parent. A child action is never
/// evidence that the parent's placement has died.
// Translated from 0x009b46b0 and 0x009bd060 (DDDA.exe 2017-12-13 Steam).
pub fn child_death(
    child: Child,
    priorities: &mut Priorities,
    context: DeathContext,
    child01: Child01State,
) -> Option<u32> {
    if !context.has_damage_context || context.already_dead {
        return None;
    }
    if child == Child::Unit01
        && (child01.mode != 0 || matches!(child01.action, 0x35 | 0x36) || child01.parent_dead)
    {
        return None;
    }
    if !priorities.request(2) {
        return None;
    }
    Some(match child {
        Child::Unit00 => 0x32,
        Child::Unit01 => 0x38,
    })
}

/// Select the parent's reaction to a child death. `impact_side` is the
/// result of 0x0099e7d0 when the callback has a damage context; its geometric
/// interpretation is not inferred here. Missing context takes the negative
/// branch. Unit01's additional 0x0099e9b0 side effect is not implemented.
// Translated from 0x009a5880 and 0x009a5a00 (DDDA.exe 2017-12-13 Steam).
pub fn parent_reaction(
    child: Child,
    priorities: &mut Priorities,
    body: BodyState,
    impact_side: Option<f32>,
) -> Option<u32> {
    if !priorities.request(if body.alternate_pose { 12 } else { 11 }) {
        return None;
    }
    if !body.alternate_pose {
        return Some(match child {
            Child::Unit00 => 0x3c,
            Child::Unit01 => 0x3d,
        });
    }
    let base = match body.motion {
        0x212 => 0x67,
        0x213 => 0x68,
        0x227 => 0x65,
        0x228 => 0x66,
        0x233 | 0x234 => return Some(if child == Child::Unit00 { 0x8b } else { 0x8c }),
        0x239 => 0x64,
        _ => {
            return Some(match (child, impact_side.is_some_and(|side| side >= 0.0)) {
                (Child::Unit00, true) => 0x45,
                (Child::Unit00, false) => 0x46,
                (Child::Unit01, true) => 0x47,
                (Child::Unit01, false) => 0x48,
            })
        }
    };
    Some(base + if child == Child::Unit00 { 0 } else { 5 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_initializers_preserve_upper_flags_and_binding_exceptions() {
        for (unit, index, bound) in [
            (Unit::Body, 0, true),
            (Unit::Body, 1, false),
            (Unit::Body, 39, true),
            (Unit::Body, 40, false),
            (Unit::Body, 55, false),
            (Unit::Body, 56, true),
            (Unit::Body, 57, false),
            (Unit::Body, 85, true),
            (Unit::Child00, 25, true),
            (Unit::Child00, 26, false),
            (Unit::Child00, 31, false),
            (Unit::Child00, 32, true),
            (Unit::Child00, 33, true),
            (Unit::Child01, 0, true),
            (Unit::Child01, 26, true),
        ] {
            for stored in [0, 7, 0xffff_ffff, 0x1234_5678] {
                let flags = initialized_resource_flags(unit, index, stored);
                assert_eq!(flags & !7, stored & !7);
                assert_eq!(flags & 7, if bound { 3 } else { 2 });
            }
        }
    }
}
