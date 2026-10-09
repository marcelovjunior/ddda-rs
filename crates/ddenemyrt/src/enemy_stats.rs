//! Shared installed enemy combat parameters. No behavior or motion policy.
use ddcombat::damage::{EnemyAttacker, EnemyDefender, RegionPool, Resist};

pub(crate) struct EnemyStats {
    pub attacker: EnemyAttacker,
    pub defender: EnemyDefender,
    pub regions: Vec<RegionPool>,
    pub obj_size: u32,
}

pub(crate) fn load(vfs: &ddvfs::Vfs, id: &str) -> Result<EnemyStats, String> {
    let gob = ddenemy::EnemyData::load(vfs, ddenemy::EnemyId::parse(id)?)?;
    let stats = &gob.common.as_ref().ok_or("enemy: no common params")?.1;
    let rst = gob.regions.as_ref().ok_or("enemy: no region status")?;
    let region = rst
        .regions
        .iter()
        .find(|r| r.kind == 0)
        .ok_or("enemy: no region type 0")?;
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
    let x = &gob.common.as_ref().ok_or("enemy: no common params")?.0.object;
    let obj_size = match x.get(&x.root, "OBJ補正サイズ") {
        Some([mtxfs::Value::U(v)]) => *v as u32,
        _ => return Err(format!("{id}: no OBJ補正サイズ")),
    };
    Ok(EnemyStats {
        attacker: EnemyAttacker::new(stats.attack, stats.magic_attack),
        defender: EnemyDefender::new(stats.defense, stats.magic_defense),
        regions,
        obj_size,
    })
}
