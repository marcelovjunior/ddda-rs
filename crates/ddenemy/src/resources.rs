//! Bounded goblin resource selection; no combat or archive-precedence rules.
use crate::{Region, RegionStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRef {
    pub path: String,
    pub class: &'static str,
}
impl ResourceRef {
    pub fn new(path: impl Into<String>, class: &'static str) -> Self {
        Self {
            path: path.into(),
            class,
        }
    }
    /// Retain every archive alternative. Empty means absent from this VFS, not optional.
    pub fn alternatives<'a>(&self, vfs: &'a ddvfs::Vfs) -> &'a [ddvfs::Location] {
        vfs.locate(&self.path, mtarc::type_hash(self.class))
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoblinResources {
    /// Byte at actor+0x2d, within cUnit.mUnitParam. Not an em#### number.
    pub unit_kind: u8,
    /// Ordered async requests, including duplicates, from the selected engine list.
    pub requests: Vec<ResourceRef>,
    /// Separately appended shared equipment requests.
    pub equipment_requests: Vec<ResourceRef>,
    pub common: ResourceRef,
    pub regions: ResourceRef,
    pub adjust: ResourceRef,
    pub shells: ResourceRef,
    pub collision: ResourceRef,
    pub status: ResourceRef,
}
impl GoblinResources {
    /// Only ordinary em0100 (5) and em0101 (6) are traced. Other kinds fail closed.
    /// Translated from 0x008cdc80 and resource lists 0x018379c8/0x01837c08,
    /// direct bindings 0x008cdd80 and status binding 0x00aa8830
    /// (DDDA.exe 2017-12-13 Steam). See docs/ENEMY_RESOURCES.md.
    pub fn select(unit_kind: u8) -> Option<Self> {
        let id = match unit_kind {
            5 => "0100",
            6 => "0101",
            _ => return None,
        };
        let r = ResourceRef::new;
        let common = r(format!(r"charparam\em\em{id}_cmn"), "rPropParam");
        let regions = r(format!(r"charparam\em\em{id}"), "rRegionStatus");
        let adjust = r(format!(r"param\em{id}"), "rAdjustParam");
        let shells = ResourceRef::new(r"param\em0100", "rShlParamList");
        let collision = r(format!(r"collision\em\e01\e{id}\e{id}_00"), "rObjCollision");
        let status = ResourceRef::new(r"param\status\enemy", "rStatusParam");
        let mut requests = vec![ResourceRef::new(r"model\em\e01\e0100\e0100", "rModel")];
        for suffix in ["co", "at", "dm", "ex", "pl"] {
            requests.push(r(
                format!(r"motion\em\e01\e0100_{suffix}\e0100_{suffix}"),
                "rMotionList",
            ));
        }
        for (path, class) in [
            (r"model\em\e01\e0100\collision\e0100", "rRagdoll"),
            (r"model\em\e01\e0100\e0100", "rJointEx"),
            (r"collision\em\e01\e0100\e0100", "rObjCollision"),
            (r"model\em\e01\e0100\CnsIkLegR", "rCnsIK"),
            (r"model\em\e01\e0100\CnsIkLegL", "rCnsIK"),
            (r"effect\epv\em\em_cm\p_em_cm", "rEffectProvider"),
            (r"effect\epv\em\em01\p_em0100", "rEffectProvider"),
            (r"sound\se\em\e01\e0100\e0100", "rSoundMotionSe"),
            (r"sound\se\em\e01\e0100\e0100", "rSoundRequest"),
            (r"param\lockon\e0100", "rLockOnTarget"),
        ] {
            requests.push(ResourceRef::new(path, class));
        }
        requests.push(shells.clone());
        requests.push(ResourceRef::new(
            r"AI\SensorTarget\em0100",
            "rAISensorTargetGroup",
        ));
        requests.push(ResourceRef::new(
            r"AI\Character\Enemy\em0100_enemy_act_param",
            "rAIEnemyActionParameter",
        ));
        let mut equipment_requests = Vec::new();
        for (path, class) in [
            (r"model\em\e01\e0100\e0100_03", "rModel"),
            (r"effect\epv\em\em01\p_em0100", "rEffectProvider"),
            (r"model\em\e01\e0100\e0100_20", "rModel"),
            (r"effect\epv\em\em01\p_em0100_20", "rEffectProvider"),
            (r"model\om\om10440\model\om10440", "rModel"),
            (r"model\om\om10465\model\om10465", "rModel"),
        ] {
            equipment_requests.push(ResourceRef::new(path, class));
        }
        requests.extend(equipment_requests.clone());
        requests.push(if unit_kind == 5 {
            ResourceRef::new(r"model\em\e01\e0100\e0100_a", "rMaterial")
        } else {
            ResourceRef::new(r"effect\epv\em\em01\p_em0101", "rEffectProvider")
        });
        for lang in ["", "_jp"] {
            for voice in ["a", "b", "c"] {
                requests.push(r(
                    format!(r"sound\se\em_vo{lang}\e01\e{id}\{voice}\e{id}_vo_{voice}{lang}"),
                    "rSoundRequest",
                ));
            }
        }
        requests.extend([collision.clone(), common.clone(), regions.clone()]);
        for suffix in [
            "damage01_AM",
            "damage02_AM",
            "all_BM",
            "all_CMM",
            "equipment_NUKI",
            "equipment_NM",
            "equipment_CMM",
        ] {
            let texture_id = if suffix.starts_with("damage") || suffix == "equipment_NM" {
                "0100"
            } else {
                id
            };
            requests.push(r(
                format!(r"model\em\e01\e{texture_id}\e{texture_id}_{suffix}"),
                "rTexture",
            ));
        }
        requests.push(r(format!(r"AI\Sensor\Enemy\em{id}A"), "rAISensorExt"));
        if unit_kind == 6 {
            requests.push(ResourceRef::new(r"model\em\e01\e0101\e0101_a", "rMaterial"));
        }
        for path in if unit_kind == 5 {
            [r"model\em\e11\e1103\e1103_00", r"model\em\e11\e1109\e1109_01"]
        } else {
            [r"model\em\e11\e1101\e1101_04", r"model\em\e11\e1109\e1109_00"]
        } {
            requests.push(ResourceRef::new(path, "rModel"));
        }
        for suffix in ["03", "20"] {
            requests.push(r(format!(r"model\em\e01\e{id}\e{id}_{suffix}_a"), "rMaterial"));
        }
        requests.push(adjust.clone());
        for equipment_id in ["0100", "0101", "0102"] {
            for suffix in ["03", "20"] {
                equipment_requests.push(r(
                    format!(r"model\em\e01\e{equipment_id}\e{equipment_id}_{suffix}_a"),
                    "rMaterial",
                ));
            }
        }
        Some(Self {
            unit_kind,
            requests,
            equipment_requests,
            common,
            regions,
            adjust,
            shells,
            collision,
            status,
        })
    }
}

/// First matching region type, preserving source order and all its elements.
/// Translated from the selection loop at 0x00770910 (DDDA.exe 2017-12-13 Steam).
/// Goblin initialization passes 0 at 0x008cdf72 and 0x008cdfd6. No HP aggregation.
pub fn region_by_type(status: &RegionStatus, kind: u32) -> Option<&Region> {
    status.regions.iter().find(|r| r.kind == kind)
}
