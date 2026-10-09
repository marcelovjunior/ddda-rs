//! Rebuilds the pawns of a real save from their IDs and compares them field
//! for field (read-only; skips without an install, an exe or a save copy).
//! The save was written by the original game, so this compares the
//! translations of 0x00795610 and 0x00796420 with the original's output.
//! Only field paths and counts are printed, never personal values.

use ddpawn::builder::{self, BuilderData};
use ddpawn::cmc::Cmc;
use ddpawn::tables::{CharaInfo, ItemKeyTable, ItemSlots, SkillList, SkillMasks, Study};
use ddpawn::{nora_path, NoraFile, PawnNames, RomPawnId, RomPawnInfo, NORA_FILES};
use std::path::PathBuf;

struct Data {
    nora: Vec<NoraFile>,
    info: RomPawnInfo,
    info_name_no: Vec<u32>,
    names: PawnNames,
    skills: SkillList,
    levels: Vec<ddparam::Level>,
    stamina: ddparam::Stamina,
    chara: CharaInfo,
    study: Study,
    items: ItemSlots,
    masks: SkillMasks,
    keys: ItemKeyTable,
    npc_names: Vec<(i16, Vec<u8>, Vec<u8>)>,
}

fn install() -> Option<(ddvfs::Vfs, PathBuf)> {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    let exe = root.parent()?.join("DDDA.exe");
    if !root.is_dir() || !exe.is_file() {
        eprintln!("skipped: no install with DDDA.exe at {}", root.display());
        return None;
    }
    Some((ddvfs::Vfs::open(root).unwrap(), exe))
}

fn load(vfs: &ddvfs::Vfs, exe: &PathBuf) -> Data {
    let read = |path: &str, class: &str| vfs.read(path, mtarc::type_hash(class)).unwrap();
    let nora = (0..NORA_FILES).map(|i| NoraFile::parse(&read(&nora_path(i), "rRomNoraPawn")).unwrap()).collect();
    let info_bytes = read(r"etc\editPattern\romPawnInfo", "rRomPawnInfo");
    let info = RomPawnInfo::parse(&info_bytes).unwrap();
    let levels = ["LvFighter", "LvStrider", "LvMagicUser", "LvBattler", "LvHunter", "LvWizard"]
        .iter()
        .map(|n| ddparam::Level::from_bytes(&read(&format!(r"param\pl\level\{n}"), "rPlLevel")).unwrap())
        .collect();
    let ledger_bytes = read("etc/item/npclist", "rNpcLedgerList");
    let ledger = ddnpc::Ledger::parse(&ledger_bytes).unwrap();
    Data {
        nora,
        info_name_no: info.pawns.iter().map(|p| p.name_no).collect(),
        info,
        names: PawnNames::parse(&read(r"etc\editPattern\romPawnNameAlphabet", "rRomPawnName")).unwrap(),
        skills: SkillList::parse(&read(r"etc\skill\skill", "rSkillList")).unwrap(),
        levels,
        stamina: ddparam::Stamina::from_bytes(&read(r"param\pl\stamina\PlStaminaMax", "rPlStamina")).unwrap(),
        chara: CharaInfo::parse(&read(r"AI\preset_charainfo", "rAIPresetCharaInfo")).unwrap(),
        study: Study::parse(&read(r"AI\preset_study", "rAIPresetStudy")).unwrap(),
        items: ItemSlots::parse(&read(r"etc\item\itemList", "rItemList")).unwrap(),
        masks: SkillMasks::from_exe(&std::fs::read(exe).unwrap()).unwrap(),
        keys: ItemKeyTable::from_exe(&std::fs::read(exe).unwrap()).unwrap(),
        npc_names: ledger.entries.iter().map(|e| (e.id, e.name.to_vec(), e.name_j.to_vec())).collect(),
    }
}

fn builder_data(d: &Data, language: u32) -> BuilderData<'_> {
    BuilderData {
        skills: &d.skills,
        levels: [&d.levels[0], &d.levels[1], &d.levels[2], &d.levels[3], &d.levels[4], &d.levels[5]],
        stamina: &d.stamina,
        chara: &d.chara,
        study: &d.study,
        items: &d.items,
        masks: &d.masks,
        names: &d.names,
        item_keys: &d.keys,
        item_key_seed: 0,
        language,
    }
}

fn save_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DDDA_SAVE") {
        return Some(PathBuf::from(p));
    }
    let mut dirs: Vec<_> =
        std::fs::read_dir(r"E:\ddda-re\save-backups").ok()?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter().rev().map(|d| d.join(r"367500\remote\DDDA.sav")).find(|p| p.is_file())
}

#[test]
fn data_tables_parse() {
    let Some((vfs, exe)) = install() else { return };
    let d = load(&vfs, &exe);
    assert_eq!((d.skills.normal.len(), d.skills.custom.len(), d.skills.ability.len()), (18, 262, d.skills.ability.len()));
    assert_eq!(d.items.slot.len(), 1901);
    assert_eq!(d.chara.entries.len(), 1024);
    // Every template builds at every level of its band without leaving the
    // modelled paths (no keyed items, rows in range, skill ids in range).
    let data = builder_data(&d, 1);
    let mut built = 0;
    for (f, file) in d.nora.iter().enumerate() {
        let lo = if f == 0 { 1 } else { ddpawn::NORA_LEVEL_BANDS[f - 1] as u32 + 1 };
        let hi = ddpawn::NORA_LEVEL_BANDS[f] as u32;
        for (i, t) in file.pawns.iter().enumerate() {
            for level in [lo, hi] {
                builder::build_nora(&data, t, level, ddpawn::rift::nora_id(level, i as u32))
                    .unwrap_or_else(|e| panic!("romNoraPawn{f:02}[{i}] at {level}: {e}"));
                built += 1;
            }
        }
    }
    eprintln!("built {built} nora pawns");
}

/// Compares a freshly built record with one the original saved later.
///
/// Fields the builder does not own are checked for the change the game
/// makes after building, each with its evidence:
/// - `mHp`, `mHpMaxWhite`: current health (runtime state; 0 for a pawn that
///   died, above `mHpMax` with equipment bonuses);
/// - `mStaminaBase`: recomputed when the pawn is spawned, from its body
///   class (`0x00b81120` picks the `PlStaminaMax` row, `0x00b5bd02` stores
///   it), so it must be one of that table's values, with
///   `mStamina = mStaminaBase + mStaminaLv`;
/// - `mStudyFlag`: knowledge only grows, so the built bits must be a subset;
/// - equipment `mFlag`: the hire adds bit 0x80 (`0x00447000`, inventory
///   owner tagging);
/// - `mInfo[k].mStatus`: 0 from the builder, set later by an untraced
///   writer (only for inclinations the builder gave a value);
/// - `mPawnType`: 2 for story slots 10..14 at build; the saved Rook has 0
///   (the party copy `0x004472d0` keeps it as `cCmcInfo+0x7f4`, where a
///   value of 2 makes the pawn stay; cleared when the story releases it);
/// - `mEmployGamePlayTime`: the hire's play time.
fn check(built: &Cmc, saved: &Cmc, stamina: &ddparam::Stamina) -> Vec<String> {
    let mut out = Vec::new();
    for (path, ours, theirs) in built.diff(saved) {
        let ok = match path.as_str() {
            // Current health (and its recoverable part): runtime state; it
            // can exceed the base `mHpMax` (equipment bonuses), so no bound.
            "mParam/mHp" | "mParam/mHpMaxWhite" => saved.param.hp >= 0.0 && saved.param.hp_max_white >= 0.0,
            "mParam/mStaminaBase" | "mParam/mStamina" => {
                stamina.entries.iter().any(|e| e.stamina == saved.param.stamina_base)
                    && saved.param.stamina == saved.param.stamina_base + saved.param.stamina_lv
            }
            "mStudyFlag" => built.study_flag.iter().zip(&saved.study_flag).all(|(b, s)| b & !s == 0),
            "mEmployGamePlayTime" => built.employ_game_play_time == 0,
            "mPawnType" => built.pawn_type == 2 && saved.pawn_type == 0,
            p if p.ends_with(".mFlag") => {
                let slot: usize = p["mParam/mEquipItem[".len()..p.find(']').unwrap()].parse().unwrap();
                saved.param.equip[slot].flag == built.param.equip[slot].flag | 0x80
            }
            p if p.starts_with("mInfo[") && p.ends_with(".mStatus") => {
                let k = ddpawn::cmc::INFO_TYPES.iter().position(|t| p.contains(&format!("_{t} "))).unwrap();
                built.info_value[k] != 0.0
            }
            _ => false,
        };
        if !ok {
            out.push(format!("{path}: built {ours} saved {theirs}"));
        }
    }
    out
}

#[test]
fn save_pawns_rebuild_field_for_field() {
    let Some((vfs, exe)) = install() else { return };
    let Some(save_path) = save_path() else {
        eprintln!("skipped: no save copy");
        return;
    };
    let d = load(&vfs, &exe);
    let data = builder_data(&d, 1);
    let save = ddsave::SaveFile::parse(&std::fs::read(save_path).unwrap()).unwrap();
    let doc = save.document().unwrap();
    let root = doc.root();
    let npc_name = |id: u32| d.npc_names.iter().find(|n| n.0 as u32 == id).map(|n| n.2.clone());
    let stock = builder::build_story_stock(&data, &d.info.records, &d.info_name_no, &npc_name).unwrap();
    let mut nora_ids = std::collections::BTreeSet::new();
    let mut checked = (0, 0);
    let mut problems = Vec::new();
    for slot in ["mPlayerDataManual", "mPlayerDataBase"] {
        let player = root.child(slot).unwrap();
        let present: Vec<bool> = player.child("mPlGameData").unwrap().array("mCmcBeFlag").unwrap();
        let party = player.path("mPlCmcEditAndParam/mCmc").unwrap();
        let net = player.child("mNetGameData").unwrap();
        let friends_n: usize = net.get("mCloseFriendPawnCount").unwrap();
        // Absent party slots keep a stale record whose items were taken
        // back on dismissal (0x0043e8b0), so only occupied slots count.
        let records = party
            .children()
            .enumerate()
            .filter(|(i, _)| present[*i])
            .map(|(_, n)| ("party", n))
            .chain(net.child("mCloseFriendPawn").unwrap().children().take(friends_n).map(|n| ("close friend", n)));
        for (list, node) in records {
            let saved = Cmc::from_save(node).expect("save record reads");
            let built = match RomPawnId::decode(saved.rom_id) {
                RomPawnId::Nora { .. } => {
                    checked.0 += 1;
                    nora_ids.insert(saved.rom_id);
                    builder::build_from_nora_id(&data, &d.nora, saved.rom_id).unwrap()
                }
                RomPawnId::Pool { index } if index < 20 => {
                    checked.1 += 1;
                    stock[index as usize].clone()
                }
                _ => continue,
            };
            for p in check(&built, &saved, &d.stamina) {
                problems.push(format!("{slot} {list} {:#x}: {p}", saved.rom_id));
            }
        }
    }
    eprintln!(
        "compared {} nora records ({} distinct pawns) and {} story records",
        checked.0,
        nora_ids.len(),
        checked.1
    );
    for p in &problems {
        eprintln!("{p}");
    }
    assert!(problems.is_empty(), "{} differing fields", problems.len());
    assert!(checked.0 >= 1 && checked.1 >= 1);
}

/// The pawn summon history keeps appearance, level, vocation and
/// equipment of every pawn hired. Each nora and story entry must match the
/// rebuilt pawn.
#[test]
fn history_entries_match() {
    let Some((vfs, exe)) = install() else { return };
    let Some(save_path) = save_path() else { return };
    let d = load(&vfs, &exe);
    let data = builder_data(&d, 1);
    let save = ddsave::SaveFile::parse(&std::fs::read(save_path).unwrap()).unwrap();
    let doc = save.document().unwrap();
    let npc_name = |id: u32| d.npc_names.iter().find(|n| n.0 as u32 == id).map(|n| n.2.clone());
    let stock = builder::build_story_stock(&data, &d.info.records, &d.info_name_no, &npc_name).unwrap();
    let mut n = 0;
    for slot in ["mPlayerDataManual", "mPlayerDataBase"] {
        let net = doc.root().child(slot).unwrap().child("mNetGameData").unwrap();
        let count: usize = net.get("mPawnHistoryNum").unwrap();
        for e in net.child("mPawnHistory").unwrap().children().take(count) {
            let h = ddsave::pawn::HistoryEntry::read(e).unwrap();
            let built = match h.online.rom_id {
                RomPawnId::Nora { .. } => builder::build_from_nora_id(&data, &d.nora, h.online.rom_id_raw).unwrap(),
                RomPawnId::Pool { index } if index < 20 => stock[index as usize].clone(),
                _ => continue,
            };
            n += 1;
            let edit = Cmc::from_save_edit(e.child("mPawnEdit").unwrap()).unwrap();
            assert_eq!(edit, built.edit, "{slot}: history appearance of {:#x}", h.online.rom_id_raw);
            assert_eq!((h.level, h.job), (built.param.level as u32, built.param.job as i32));
            let equip: Vec<i32> = built.param.equip.iter().map(|q| q.item_no as i32).collect();
            assert_eq!(h.equipment, equip, "{slot}: history equipment of {:#x}", h.online.rom_id_raw);
        }
    }
    eprintln!("history entries matched: {n}");
    assert!(n >= 1);
}
