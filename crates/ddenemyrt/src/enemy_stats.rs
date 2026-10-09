//! Shared installed enemy combat parameters. No behavior or motion policy.
use ddcombat::damage::{EnemyAttacker, EnemyDefender, RegionPool, Resist};

pub(crate) struct EnemyStats {
    pub attacker: EnemyAttacker,
    pub defender: EnemyDefender,
    pub regions: Vec<RegionPool>,
    pub obj_size: u32,
}

pub(crate) fn load(vfs: &ddvfs::Vfs, id: &str) -> Result<EnemyStats, String> {
    let id = ddenemy::EnemyId::parse(id)?;
    let region_path = format!(r"charparam\em\{}", id.as_str());
    load_paths(vfs, &format!("{region_path}_cmn"), &region_path)
}

/// Both paths come from the selected unit's resource bindings. Kind zero
/// remains deliberate: goblin/wolf/harpy and chimera initializers select it.
pub(crate) fn load_paths(
    vfs: &ddvfs::Vfs,
    common_path: &str,
    region_path: &str,
) -> Result<EnemyStats, String> {
    let (common, stats) = ddenemy::CommonStats::load(vfs, common_path)?;
    let bytes = vfs.read(region_path, mtarc::type_hash("rRegionStatus"))?;
    let rst = ddenemy::RegionStatus::parse(&bytes).map_err(|e| format!("{region_path}: {e}"))?;
    let region = rst
        .regions
        .iter()
        .find(|r| r.kind == 0)
        .ok_or_else(|| format!("{region_path}: no region type 0"))?;
    let res = |k: &str| stats.resistances.get(k).copied().unwrap_or(1.0);
    let resist = Resist {
        element: [res("耐炎"), res("耐氷"), res("耐雷"), res("耐聖"), res("耐魔")],
        slash: res("耐斬"),
        strike: res("耐打"),
    };
    let regions = region
        .elements
        .iter()
        .map(|e| {
            let mut r = RegionPool::from_element(
                e.id,
                e.hp_max,
                e.dp_max,
                e.dp_speed,
                e.bp_max,
                e.bp_speed,
                e.damage_adjustment,
                e.hit_stop_adjustment,
                e.attributes,
                e.surface,
                e.dp_reset_timer_max,
                e.bp_reset_timer_max,
            );
            r.resist = resist;
            // status thresholds (0x00770FD0), memory order of
            // cCharParamEnemy +0x54..+0xA0; absent labels keep 999
            let th = |k: &str| stats.resistances.get(k).copied().unwrap_or(999.0);
            r.status = ddcombat::status::StatusPools::from_char_param([
                th("耐毒"),
                th("耐のろま"),
                th("耐暗闇"),
                th("耐睡眠"),
                th("耐油まみれ"),
                th("耐ずぶ濡れ"),
                th("耐敵化"),
                th("耐沈黙"),
                th("耐スキル封印"),
                th("耐呪い"),
                th("耐氷漬け"),
                th("耐延焼"),
                th("耐落雷"),
                th("耐久聖"),
                th("耐久魔"),
                th("耐石化"),
                th("耐攻撃力ダウン"),
                th("耐防御力ダウン"),
                th("耐魔法攻撃力ダウン"),
                th("耐魔法防御力ダウン"),
            ]);
            r
        })
        .collect();
    let x = &common.object;
    let obj_size = match x.get(&x.root, "OBJ補正サイズ") {
        Some([mtxfs::Value::U(v)]) => *v as u32,
        _ => return Err(format!("{common_path}: no OBJ補正サイズ")),
    };
    Ok(EnemyStats {
        attacker: EnemyAttacker::new(stats.attack, stats.magic_attack),
        defender: EnemyDefender::new(stats.defense, stats.magic_defense),
        regions,
        obj_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_explicit_paths_preserve_legacy_stats_and_load_children() {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
        if !native.is_dir() {
            eprintln!("DDDA install absent; skipped explicit-path stats regression");
            return;
        }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        for id in ["em0100", "em5200", "em5800"] {
            let path = format!(r"charparam\em\{id}");
            let common_path = format!("{path}_cmn");
            let original = ddenemy::EnemyData::load(&vfs, ddenemy::EnemyId::parse(id).unwrap()).unwrap();
            let (_, explicit_common) = ddenemy::CommonStats::load(&vfs, &common_path).unwrap();
            assert_eq!(original.common.unwrap().1, explicit_common);
            let conventional = load(&vfs, id).unwrap();
            let explicit = load_paths(&vfs, &common_path, &path).unwrap();
            assert_eq!(conventional.attacker, explicit.attacker);
            assert_eq!(conventional.defender, explicit.defender);
            assert_eq!(conventional.regions, explicit.regions);
            assert_eq!(conventional.obj_size, explicit.obj_size);
            assert_eq!(
                explicit.attacker,
                EnemyAttacker::new(explicit_common.attack, explicit_common.magic_attack)
            );
            assert_eq!(
                explicit.defender,
                EnemyDefender::new(explicit_common.defense, explicit_common.magic_defense)
            );
        }
        // Initializers 0x009a0bd1, 0x009b1118 and 0x009b909c select kind 0.
        // Compare against parsed install data rather than embedding its HP table.
        for id in ["em5200", "em5200_00", "em5200_01"] {
            let path = format!(r"charparam\em\{id}");
            let actual = load_paths(&vfs, &format!("{path}_cmn"), &path).unwrap();
            let bytes = vfs.read(&path, mtarc::type_hash("rRegionStatus")).unwrap();
            let parsed = ddenemy::RegionStatus::parse(&bytes).unwrap();
            let kind_zero = parsed.regions.iter().find(|r| r.kind == 0).unwrap();
            assert_eq!(actual.regions.len(), kind_zero.elements.len());
            for (pool, element) in actual.regions.iter().zip(&kind_zero.elements) {
                assert_eq!(pool.hp, element.hp_max);
                assert_eq!(pool.attr, element.attributes);
            }
        }
        let missing = r"charparam\em\missing_chimera_test";
        let valid = r"charparam\em\em5200";
        let error = load_paths(&vfs, missing, valid).err().unwrap();
        assert!(error.contains(missing), "{error}");
        let error = load_paths(&vfs, &format!("{valid}_cmn"), missing).err().unwrap();
        assert!(error.contains(missing), "{error}");
        assert!(load(&vfs, "em5200_00").is_err());
    }
}
