//! Building pawn records: a generated pawn from its template and level
//! (`0x00795610`), and the 20 story pawns of the stock (`0x00796420`).

use crate::cmc::{Cmc, Equip};
use crate::rift::{PawnRng, XorShift128};
use crate::tables::{CharaInfo, ItemKeyTable, ItemSlots, SkillList, SkillMasks, Study};
use crate::{NoraPawn, PawnNames};

/// Everything `cRomPawnManager` holds besides the templates
/// (`0x00796160`).
pub struct BuilderData<'a> {
    pub skills: &'a SkillList,
    /// `rPlLevel` tables in loader order: Fighter, Strider, MagicUser,
    /// Battler, Hunter, Wizard (`cRomPawnManager+0x98..+0xac`).
    pub levels: [&'a ddparam::Level; 6],
    pub stamina: &'a ddparam::Stamina,
    pub chara: &'a CharaInfo,
    pub study: &'a Study,
    pub items: &'a ItemSlots,
    pub masks: &'a SkillMasks,
    pub names: &'a PawnNames,
    pub item_keys: &'a ItemKeyTable,
    /// The seed `0x0045e130` gives its generator: `sItemManager+0x5b00`,
    /// whose origin is not traced. Only items 0x74b..0x76c depend on it.
    pub item_key_seed: u32,
    /// `sGameSys.mLanguage` (+0x16d8): 0 Japanese keeps the data's names.
    pub language: u32,
}

/// Weapon-skill slotting per vocation: `(weapon class, first template
/// custom skill, count)` pairs, from the switch in `0x00795610` (the
/// register `EAX` at each call of `0x00796080`). Warrior's two classes both
/// read template skills 0..3.
fn weapon_slots(job: u8) -> &'static [(usize, usize, usize)] {
    match job {
        2 => &[(4, 0, 3), (10, 3, 3)], // Strider: dagger, bow
        3 => &[(5, 0, 6)],             // Mage: staff
        7 => &[(3, 0, 3), (7, 0, 3)],  // Warrior: longsword, warhammer
        8 => &[(4, 0, 3), (11, 3, 3)], // Ranger: dagger, longbow
        9 => &[(6, 0, 6)],             // Sorcerer: archistaff
        _ => &[(1, 0, 3), (8, 3, 3)],  // Fighter (and anything else): sword, shield
    }
}

/// The `rPlLevel` table for a vocation (same switch).
fn level_table(job: u8) -> usize {
    match job {
        2 => 1,
        3 => 2,
        7 => 3,
        8 => 4,
        9 => 5,
        _ => 0,
    }
}

fn set_bit(words: &mut [u32], bit: i32) {
    // The exe's signed `bit / 32` and `bit % 32`; ids here are never negative.
    words[(bit >> 5) as usize] |= 1u32 << (bit & 31);
}

/// Places one weapon skill list. Translated from `0x00796080` (DDDA.exe
/// 2017-12-13 Steam): for `count` template custom skills from `first`, an
/// id that is not -1 is looked up in the skill list (an id past the end
/// reads entry 0); the entry is placed in slot `k` of `mWeaponSkill[weapon]`
/// and its bit set in `mSkillLv1` (skill level 1) or `mSkillLv2` only if
/// the vocation's job level reaches the entry's and the skill table allows
/// the vocation. Slot `k` is the template position, so refused skills leave
/// gaps (-1).
fn place_weapon_skills(
    data: &BuilderData,
    t: &NoraPawn,
    cmc: &mut Cmc,
    weapon: usize,
    first: usize,
    count: usize,
) -> Result<(), String> {
    for k in 0..count {
        let id = t.custom_skill[first + k];
        if id == -1 {
            continue;
        }
        let mut i = id as i32;
        if i >= data.skills.custom.len() as i32 {
            i = 0;
        }
        let e = data.skills.custom[i as usize];
        let job = cmc.param.job;
        let jl = cmc.param.job_level[job as usize] as i32;
        let mask = *data.masks.masks.get(e.id as usize).ok_or("skill id outside the skill table")?;
        if jl >= e.job_level && mask & (1u32 << (job & 31)) != 0 {
            cmc.param.weapon_skill[weapon - 1][k] = e.id as i16;
            if e.skill_lv == 1 {
                set_bit(&mut cmc.param.skill_lv1, e.id);
            } else {
                set_bit(&mut cmc.param.skill_lv2, e.id);
            }
        }
    }
    Ok(())
}

/// Augment number stored for a template augment byte (`0x00795610`):
/// `c < 63: (c / 7) * 10 + c % 7`, `c >= 63: c + 37`; 69 is first changed to
/// 68 (in the template itself).
pub fn ability_number(c: i8) -> i8 {
    let c = if c == 0x45 { 0x44 } else { c };
    if c >= 0x3f {
        c.wrapping_add(0x25)
    } else {
        (c / 7) * 10 + c % 7
    }
}

/// Inclinations and talk/skill-use from `AI\preset_charainfo`.
///
/// Translated from `0x00ca7220` (DDDA.exe 2017-12-13 Steam). Record
/// `chara_no` (none if out of range): inclinations `e0`, `e1`, `e2` (0xff
/// none, only `< 9`) get 800.0 (`0x017f42e4`), 700.0 and 660.0;
/// `mInfoEx[TALK]` gets 100/300/500/700/900 for kinds 0/1/2/3/other, and
/// `mInfoEx[SKILL_USE]` 200/500/800 for kinds 0/1/other.
pub fn apply_chara_info(chara: &CharaInfo, chara_no: i32, cmc: &mut Cmc) {
    let Some(e) = usize::try_from(chara_no).ok().and_then(|i| chara.entries.get(i)) else { return };
    for (slot, value) in [(e[0], 800.0), (e[1], 700.0), (e[2], 660.0)] {
        if slot != 0xff && slot < 9 {
            // A negative index would write outside mInfo in the exe; data has none.
            if let Ok(s) = usize::try_from(slot) {
                cmc.info_value[s] = value;
            }
        }
    }
    cmc.info_ex_value[0] = match e[3] {
        0 => 100.0,
        1 => 300.0,
        2 => 500.0,
        3 => 700.0,
        _ => 900.0,
    };
    cmc.info_ex_value[1] = match e[4] {
        0 => 200.0,
        1 => 500.0,
        _ => 800.0,
    };
}

/// `mUseSceneTalk` from the chara number.
///
/// Translated from `0x00ca7320` (DDDA.exe 2017-12-13 Steam): `h` = the save
/// checksum (CRC-32 without final inversion) of the number's four
/// little-endian bytes; the six values are `(h >> 5k) % 7` for k = 0..4 and
/// `(h >> 25) % 7`.
pub fn scene_talk(chara_no: i32) -> [u32; 6] {
    let h = ddsave::checksum(&(chara_no as u32).to_le_bytes());
    [h % 7, (h >> 5) % 7, (h >> 10) % 7, (h >> 15) % 7, (h >> 20) % 7, (h >> 25) % 7]
}

/// The built-in skills of an item 0x74b..0x76c.
///
/// Translated from `0x0045e130` (DDDA.exe 2017-12-13 Steam): the table rows
/// for the item are collected; if any, a generator seeded with `seed`
/// (`0x00d006c0`) draws one, giving `(a, b)`. `0x00795610` packs them into a
/// key that `0x0045e510` unpacks into `mChgNum = a` and `mDay1 = b` (a
/// 0x3ff field reads back as 0xffff), with `mDay2 = mDay3 = 0xffff`.
pub fn item_key_extra(keys: &ItemKeyTable, seed: u32, item: i16) -> Option<[u16; 4]> {
    let rows: Vec<_> = keys.rows.iter().filter(|r| r.0 == item).collect();
    if rows.is_empty() {
        return None;
    }
    let mut rng = XorShift128::seeded(seed);
    let r = rows[(rng.next_u32() % rows.len() as u32) as usize];
    let field = |v: u16| if v & 0x3ff == 0x3ff { 0xffff } else { v & 0x3ff };
    Some([field(r.1), field(r.2), 0xffff, 0xffff])
}

/// Equips one item. Translated from `0x00786b90` (DDDA.exe 2017-12-13
/// Steam): count 1, flags 3 (plus 0x800 in template position 10); the slot
/// is the item's category (`itemList +0x34 >> 28`), except that category 9
/// uses the template position; items with `+0x38 & 0xc00` are skipped.
pub fn equip_item(items: &ItemSlots, cmc: &mut Cmc, position: usize, item: i16) -> Result<(), String> {
    equip_item_extra(items, cmc, position, item, [0; 4])
}

/// [`equip_item`] with a key's unpacked fields.
pub fn equip_item_extra(
    items: &ItemSlots,
    cmc: &mut Cmc,
    position: usize,
    item: i16,
    extra: [u16; 4],
) -> Result<(), String> {
    let i = usize::try_from(item).map_err(|_| "negative item")?;
    let slot = *items.slot.get(i).ok_or("item past the item list")?;
    if items.not_equippable[i] {
        return Ok(());
    }
    let mut flag = 3u32;
    if position == 10 {
        flag |= 0x800;
    }
    let slot = if slot == 9 { position } else { slot as usize };
    cmc.param.equip[slot] = Equip { num: 1, item_no: item, flag, extra };
    Ok(())
}

/// Equips an item the way the story stock code does inline (`0x00796420`:
/// `0x004629b0(0)`, flags `| 3`, slot = category, skipped if
/// `+0x38 & 0xc00`).
pub fn equip_plain(items: &ItemSlots, cmc: &mut Cmc, item: i16) -> Result<(), String> {
    let i = usize::try_from(item).map_err(|_| "negative item")?;
    let slot = *items.slot.get(i).ok_or("item past the item list")? as usize;
    if !items.not_equippable[i] {
        *cmc.param.equip.get_mut(slot).ok_or("slot past 12")? = Equip { num: 1, item_no: item, flag: 3, extra: [0; 4] };
    }
    Ok(())
}

/// Builds a generated pawn at `level` from its template, as `0x00795610`
/// does (DDDA.exe 2017-12-13 Steam), and gives it `rom_id` the way the
/// callers do (`0x00794f10`, `0x00795010`, `0x007951c0` store the ID after
/// building).
pub fn build_nora(data: &BuilderData, t: &NoraPawn, level: u32, rom_id: u32) -> Result<Cmc, String> {
    let mut c = Cmc::initial();
    // 0x00787b70: the whole appearance, name included.
    c.edit = t.edit_full.clone();
    if data.language != 0 {
        if let Some(name) = data.names.name(t.job, c.edit.get("mGender").unwrap_or(0), t.name_no as usize) {
            c.edit.set_name(name.as_bytes());
        }
    }
    let p = &mut c.param;
    p.level = level as u8;
    p.job = t.job;
    // Job levels: vocations 1-3 and 7-9 from the template, 4-6 set to 1.
    p.job_level[1] = t.job_level[0] as u8;
    p.job_level[2] = t.job_level[1] as u8;
    p.job_level[3] = t.job_level[2] as u8;
    p.job_level[4] = 1;
    p.job_level[5] = 1;
    p.job_level[6] = 1;
    p.job_level[7] = t.job_level[3] as u8;
    p.job_level[8] = t.job_level[4] as u8;
    p.job_level[9] = t.job_level[5] as u8;
    let jl = p.job_level[p.job as usize] as i32;
    // Core skills.
    for &s in &t.normal_skill {
        if s == -1 {
            continue;
        }
        let i = if (s as i32) < data.skills.normal.len() as i32 { s as usize } else { 0 };
        let e = data.skills.normal[i];
        if jl >= e.job_level {
            set_bit(&mut p.skill_lv1, e.id);
        }
    }
    for &(weapon, first, count) in weapon_slots(t.job) {
        place_weapon_skills(data, t, &mut c, weapon, first, count)?;
    }
    // Augments.
    let p = &mut c.param;
    let mut n = 0;
    for &a in &t.ability {
        if a == -1 {
            continue;
        }
        let i = if (a as i32) < data.skills.ability.len() as i32 { a as usize } else { 0 };
        if jl >= data.skills.ability[i].job_level {
            let num = ability_number(a);
            p.ability[n] = num;
            set_bit(&mut p.ability_info, num as i32);
            n += 1;
        }
    }
    // Stats: row 1 for level 1, then each level adds the gains of its row.
    let table = &data.levels[level_table(t.job)].entries;
    let count = table.len();
    let row = |i: usize| table.get(i).ok_or_else(|| format!("rPlLevel row {i} past {count}"));
    let mut cur = row(1)?;
    let mut next = row(2)?;
    let mut idx = 1usize;
    let (mut hp, mut atk, mut def, mut matk, mut mdef, mut st) = (
        cur.hp_max as f32,
        cur.attack as f32,
        cur.defend as f32,
        cur.magic_attack as f32,
        cur.magic_defend as f32,
        cur.stamina_max as f32,
    );
    for lv in 2..=level {
        if idx < count && (lv < cur.level as u32 || lv >= next.level as u32) {
            idx += 1;
            if idx < count - 1 {
                cur = row(idx)?;
                next = row(idx + 1)?;
            } else {
                // The exe reads rows[idx] here even when idx == count.
                cur = row(idx)?;
            }
        }
        hp += cur.hp_max as f32;
        atk += cur.attack as f32;
        def += cur.defend as f32;
        matk += cur.magic_attack as f32;
        mdef += cur.magic_defend as f32;
        st += cur.stamina_max as f32;
    }
    p.hp_max = hp;
    p.attack = atk;
    p.defend = def;
    p.mgc_attack = matk;
    p.mgc_defend = mdef;
    p.stamina_lv = st;
    p.hp = hp;
    p.hp_max_white = hp;
    p.stamina_base = data
        .stamina
        .entries
        .get(t.weight_type as usize)
        .ok_or("weight type past PlStaminaMax")?
        .stamina;
    p.stamina = p.stamina_base + p.stamina_lv;
    for (k, &item) in t.equip_item_id.iter().enumerate().take(11) {
        if (301..0x76d).contains(&item) {
            let extra = if item >= 0x74b {
                item_key_extra(data.item_keys, data.item_key_seed, item).unwrap_or([0; 4])
            } else {
                [0; 4]
            };
            equip_item_extra(data.items, &mut c, k, item, extra)?;
        }
    }
    apply_chara_info(data.chara, t.chara_no as i32, &mut c);
    c.use_scene_talk = scene_talk(t.chara_no as i32);
    if let Ok(ai) = usize::try_from(t.ai_no) {
        if let Some(&e) = data.study.index.get(ai) {
            if (e as usize) < data.study.entries.len() {
                data.study.apply(e as usize, &mut c.study_flag)?;
            }
        }
    }
    c.pawn_type = 0;
    c.status = 0;
    c.employ_game_play_time = 0;
    c.rom_id = rom_id;
    Ok(c)
}

/// Builds a pawn from its ID like `0x00795010` does for nora IDs: the
/// template of the level's band, at that level.
pub fn build_from_nora_id(data: &BuilderData, files: &[crate::NoraFile], id: u32) -> Result<Cmc, String> {
    let (level, file, index) = crate::rift::nora_template(files, id).ok_or("not a nora ID")?;
    build_nora(data, &files[file].pawns[index], level as u32, id)
}

/// Appearance overrides for story stock slots 10..16: 20 bytes each, from
/// the stack table built at the start of `0x00796420` (immediates in the
/// code, so not readable from the encrypted `.text` of the player's exe).
/// Byte k goes to the fields of [`STORY_EDIT_TARGETS`]`[k]`; byte 19 is
/// unused.
pub const STORY_EDIT_OVERRIDES: [[u8; 20]; 7] = [
    [0xc9, 0xc9, 2, 0, 2, 3, 2, 0, 0, 0, 2, 4, 4, 8, 2, 2, 0, 0, 0, 0],
    [0xca, 0xca, 2, 0, 2, 2, 2, 2, 0, 0, 0x0b, 0x11, 0x11, 0, 0x0b, 0x0b, 0, 0x27, 3, 0],
    [0xcb, 0xcb, 2, 0, 2, 2, 2, 0, 0, 0, 4, 0x0d, 0x0d, 0x13, 4, 4, 0, 0x18, 0x17, 0],
    [0xcc, 0xcc, 0, 0, 0, 4, 1, 3, 0, 0, 0x0b, 0x11, 0x11, 0x0e, 0x0b, 0x0b, 0, 0x27, 0x17, 0],
    [0xc8, 0xc8, 2, 0, 2, 2, 2, 1, 0, 0, 0x0d, 0x0d, 0x0d, 0x1c, 0x0d, 0x0d, 0, 0x24, 0x11, 0],
    [0xc9, 0xc9, 2, 0, 2, 3, 2, 0, 0, 0, 2, 4, 4, 8, 2, 2, 0, 0, 0, 0],
    [0xca, 0xca, 2, 0, 2, 2, 2, 2, 0, 0, 0x0b, 0x11, 0x11, 0, 0x0b, 0x0b, 0, 0x27, 3, 0],
];

/// Edit fields written from each override byte (by the edit offsets the
/// code stores to; the byte order of the fields was confirmed by Rook's
/// saved appearance).
pub const STORY_EDIT_TARGETS: [&[&str]; 19] = [
    &["mFaceBase", "mFaceEyebrow", "mFaceEye", "mFaceNose", "mFaceMouth", "mFaceEar"],
    &["mHumanBodyAll", "mHumanBody", "mHumanHead", "mHumanArm", "mHumanLeg"],
    &["mBodySlender"],
    &["mBodyMuscle"],
    &["mBustScale"],
    &["mMovement"],
    &["mPosture"],
    &["mWrinkleFace"],
    &["mScarNo"],
    &["mMakeupFace"],
    &["mColorHair"],
    &["mColorEyeR"],
    &["mColorEyeL"],
    &["mColorBody"],
    &["mColorBeard"],
    &["mColorEyebrow"],
    &["mColorMakeup"],
    &["mHairNo"],
    &["mBeardNo"],
];

/// Fixed equipment of story slots 14, 15 and 16 (item numbers that the
/// code equips after clearing the slots, `0x00796420`), and the NPC ledger
/// numbers whose names they take (`0x004769c0` with 0x1f8, 0x380, 0x381).
pub const STORY_SPECIAL: [(usize, u32, &[i16]); 3] = [
    (14, 0x1f8, &[414, 575, 634, 985, 1062]),
    (15, 0x380, &[601, 602, 603, 604, 605, 606, 607, 608]),
    (16, 0x381, &[609, 610, 611, 612, 613, 614, 615, 616]),
];

/// Builds the story stock (slots 0..19) from `rRomPawnInfo`.
///
/// Translated from `0x00796420` (DDDA.exe 2017-12-13 Steam). Each slot `i`
/// copies record `i` (appearance, parameters, knowledge, inclinations,
/// pawn type) and gets ID `0x2000_0000 + i`. Then:
/// - if `language != 0`, the name is replaced from the name lists with
///   `mNameNoArray[i]` (read as a signed byte), the record's vocation and
///   gender;
/// - slots 10..16 get [`STORY_EDIT_OVERRIDES`], personality 0, and pawn type
///   2 (slots 10..14) or 0 (15, 16);
/// - slots 14..16 take their name from the NPC ledger (`npc_name`; the exe
///   does this only while the NPC manager exists) and get fixed equipment
///   ([`STORY_SPECIAL`]) after all equipment is cleared.
pub fn build_story_stock(
    data: &BuilderData,
    info: &[Cmc],
    name_no: &[u32],
    npc_name: &dyn Fn(u32) -> Option<Vec<u8>>,
) -> Result<Vec<Cmc>, String> {
    let mut out = Vec::with_capacity(20);
    for i in 0..20usize {
        let src = info.get(i).ok_or("romPawnInfo has fewer than 20 records")?;
        let mut c = Cmc::initial();
        c.edit = src.edit.clone();
        c.param = src.param.clone();
        c.status = src.status;
        c.study_flag = src.study_flag.clone();
        c.info_value = src.info_value;
        c.info_status = src.info_status;
        c.info_ex_value = src.info_ex_value;
        c.use_scene_talk = src.use_scene_talk;
        c.pawn_type = src.pawn_type;
        c.employ_game_play_time = src.employ_game_play_time;
        c.rom_id = 0x2000_0000 + i as u32;
        if data.language != 0 {
            let no = *name_no.get(i).ok_or("mNameNoArray too short")? as u8 as i8;
            if let Ok(no) = usize::try_from(no) {
                if let Some(name) = data.names.name(c.param.job, c.edit.get("mGender").unwrap_or(0), no) {
                    c.edit.set_name(name.as_bytes());
                }
            }
        }
        if (10..17).contains(&i) {
            let t = &STORY_EDIT_OVERRIDES[i - 10];
            for (k, fields) in STORY_EDIT_TARGETS.iter().enumerate() {
                for f in *fields {
                    c.edit.set(f, t[k]);
                }
            }
            c.edit.set("mPersonality", 0);
            c.pawn_type = if i - 10 > 4 { 0 } else { 2 };
        }
        if let Some((_, npc, items)) = STORY_SPECIAL.iter().find(|s| s.0 == i) {
            if let Some(name) = npc_name(*npc) {
                c.edit.set_name(&name);
            }
            c.param.equip = [Equip::EMPTY; 12];
            for &item in items.iter() {
                // Key 0, flags 3, slot = the item's category. The last item
                // of slot 16 goes through 0x00786b90 with position -1, which
                // gives the same result.
                equip_plain(data.items, &mut c, item)?;
            }
        }
        out.push(c);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_talk_matches_the_original() {
        // mUseSceneTalk of two generated pawns in a save written by the
        // original game, with their templates' mRomPawnCharaNo.
        assert_eq!(scene_talk(167), [1, 3, 0, 0, 0, 0]);
        assert_eq!(scene_talk(386), [3, 5, 4, 3, 5, 6]);
    }

    #[test]
    fn ability_numbers() {
        assert_eq!(ability_number(0), 0);
        assert_eq!(ability_number(6), 6);
        assert_eq!(ability_number(7), 10);
        assert_eq!(ability_number(62), 86);
        assert_eq!(ability_number(63), 100);
        assert_eq!(ability_number(69), 105);
    }

    #[test]
    fn chara_info_values() {
        let chara = CharaInfo { entries: vec![[0xff, 0, 1, 2, 0], [6, 0xff, 3, 4, 2]] };
        let mut c = Cmc::initial();
        apply_chara_info(&chara, 0, &mut c);
        assert_eq!((c.info_value[0], c.info_value[1]), (700.0, 660.0));
        assert_eq!(c.info_ex_value, [500.0, 200.0]);
        let mut c = Cmc::initial();
        apply_chara_info(&chara, 1, &mut c);
        assert_eq!((c.info_value[6], c.info_value[3]), (800.0, 660.0));
        assert_eq!(c.info_ex_value, [900.0, 800.0]);
        // Out of range: untouched.
        let mut c = Cmc::initial();
        apply_chara_info(&chara, 7, &mut c);
        assert_eq!(c, Cmc::initial());
    }
}
