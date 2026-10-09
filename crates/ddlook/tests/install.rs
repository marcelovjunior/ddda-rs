//! Looks from the install (and a private save copy when present). Skips
//! without the install. The save is only read; nothing personal is
//! printed (model paths and counts only).

use std::path::PathBuf;

fn vfs() -> Option<ddvfs::Vfs> {
    let np = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !np.is_dir() {
        eprintln!("SKIP: install absent");
        return None;
    }
    Some(ddvfs::Vfs::open(&np).unwrap())
}

fn exists(v: &ddvfs::Vfs, p: &str) -> bool {
    v.locate(p, mtarc::type_hash("rModel")).is_some_and(|l| !l.is_empty())
}

#[test]
fn st200_ev10_npcs() {
    let Some(v) = vfs() else { return };
    let a = ddlook::npc_look(&v, 1, 1, 22).unwrap();
    let b = ddlook::npc_look(&v, 2, 2, 22).unwrap();
    let paths = |l: &ddlook::NpcLook| l.look.parts.iter().map(|p| p.path.clone()).collect::<Vec<_>>();
    assert_eq!(a.look.base, r"model\pl\m\m_base\m000\m000");
    assert_eq!(
        paths(&a),
        [r"model\pl\npc\e_npc\e_npc004\e_npc004", r"model\pl\hair\hair104\hair104", r"model\pl\beard\beard104\beard104"]
    );
    assert_eq!(b.look.base, r"model\pl\f\f_base\f000\f000");
    assert_eq!(paths(&b), [r"model\pl\npc\e_npc\e_npc005\e_npc005", r"model\pl\hair\hair105\hair105"]);
    // 0x00841e20: outfit parts on, the covered body parts off.
    assert!(a.look.mask.visible(300) && !a.look.mask.visible(24) && !a.look.mask.visible(301));
    assert!(a.look.mask.visible(0) && a.look.mask.visible(50) && a.look.mask.visible(66));
    assert!(b.look.mask.visible(300) && b.look.mask.visible(301));
    for p in [8, 9, 10, 16, 17, 18, 1, 30] {
        assert!(!b.look.mask.visible(p), "{p}");
    }
    assert!(a.look.hair_color.is_some());
    for l in [&a, &b] {
        assert!(exists(&v, &l.look.base));
        for p in &l.look.parts {
            assert!(exists(&v, &p.path), "{}", p.path);
        }
    }
}

#[test]
fn every_armour_piece_resolves() {
    let Some(v) = vfs() else { return };
    let t = ddlook::Tables::load(&v).unwrap();
    // Every model id of every m_parts armour record resolves for both
    // sexes or is absent from that sex's list; every path that resolves
    // exists in the install.
    let mut n = 0;
    let mut missing = vec![];
    for (_, _, _, ids) in &t.table.entries {
        for &m in ids {
            for sex in 0..2 {
                if let Some((_, p)) = t.models[sex].get(m) {
                    if exists(&v, p) {
                        n += 1;
                    } else {
                        missing.push(p.to_string());
                    }
                }
            }
        }
    }
    // One record names an empty path (no model).
    missing.retain(|p| !p.is_empty());
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "{missing:?}");
    eprintln!("resolved {n}, missing {}: {:?}", missing.len(), &missing[..missing.len().min(8)]);
    assert!(n > 1000, "{n}");
}

fn save_xml() -> Option<String> {
    let p = match std::env::var_os("DDDA_SAVE") {
        Some(p) => PathBuf::from(p),
        None => {
            let root = PathBuf::from(r"E:\ddda-re\save-backups");
            let mut d: Vec<_> = std::fs::read_dir(&root).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            d.sort();
            d.into_iter().rev().flat_map(|d| [d.join(r"remote\DDDA.sav"), d.join(r"367500\remote\DDDA.sav")]).find(|p| p.is_file())?
        }
    };
    let b = std::fs::read(p).ok()?;
    Some(ddsave::SaveFile::parse(&b).ok()?.xml)
}

#[test]
fn saved_arisen_and_pawns_are_dressed() {
    let Some(v) = vfs() else { return };
    let Some(xml) = save_xml() else {
        eprintln!("SKIP: no private save copy");
        return;
    };
    let looks = ddlook::save::save_looks(&v, &xml, "mPlayerDataManual").unwrap();
    assert!(looks.len() >= 2, "arisen and at least one pawn");
    for l in &looks {
        eprintln!(
            "{}: base {} parts {} hidden body parts {}",
            l.who,
            l.look.base,
            l.look.parts.len(),
            (0..200).filter(|&p| !l.look.mask.visible(p)).count()
        );
        let f = |n: &str| l.edit.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
        eprintln!(
            "  gender {:?} hair {:?} beard {:?} face base {:?} parts {:?}",
            f("mGender"),
            f("mHairNo"),
            f("mBeardNo"),
            f("mFaceBase"),
            l.look.parts.iter().map(|p| p.path.rsplit(char::from(92)).next().unwrap_or("")).collect::<Vec<_>>()
        );
        // Dressed: at least one equipment model besides hair, all present.
        assert!(l.look.parts.iter().any(|p| !p.hair), "{} wears nothing", l.who);
        for p in &l.look.parts {
            assert!(exists(&v, &p.path), "{}", p.path);
        }
        // Clothes hide some of the body (rArmorPartsOff).
        assert!((0..200).any(|p| !l.look.mask.visible(p)));
    }
}
