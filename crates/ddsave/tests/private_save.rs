//! Optional read-only test against a private copy of a real save.
//!
//! Set `DDDA_SAVE` to a `DDDA.sav` copy; otherwise the newest backup under
//! `E:\ddda-re\save-backups\*\367500\remote\DDDA.sav` is used, and the test
//! skips if there is none. Never point it at the live Steam folder. The save
//! is only read. Nothing personal is printed: only counts and pass/fail.
//!
//! With the install present (`DDDA_NATIVE_PC` or the default path), every
//! generated ("nora") pawn in the save is compared with the record its
//! `mRomPawnUniqueID` names in `etc\editPattern\romNoraPawnNN`: the save was
//! written by the original game, so this checks the ID encoding and level
//! bands traced in 0x00794f10 / 0x00795010 against the original's output.

use ddsave::pawn::{Edit, PlayerPawns, RomPawnId};
use std::path::PathBuf;

fn save_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DDDA_SAVE") {
        let p = PathBuf::from(p);
        assert!(p.is_file(), "DDDA_SAVE does not exist: {}", p.display());
        return Some(p);
    }
    let root = PathBuf::from(r"E:\ddda-re\save-backups");
    let mut dirs: Vec<_> = std::fs::read_dir(&root).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter()
        .rev()
        .map(|d| d.join(r"367500\remote\DDDA.sav"))
        .find(|p| p.is_file())
}

fn native_pc() -> Option<PathBuf> {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if root.is_dir() {
        Some(root)
    } else {
        assert!(explicit.is_none(), "DDDA_NATIVE_PC does not exist: {}", root.display());
        None
    }
}

/// The fields of one `rRomNoraPawn::cRomNoraPawn` record the save copies.
struct NoraRecord {
    gender: u8,
    nickname: u32,
    voice: u8,
    hair: u8,
    job: u8,
    name_no: usize,
}

fn uint(v: &[mtxfs::Value]) -> u64 {
    match v.first() {
        Some(mtxfs::Value::U(u)) => *u,
        Some(mtxfs::Value::S(s)) => *s as u64,
        other => panic!("not an integer: {other:?}"),
    }
}

fn load_nora(vfs: &ddvfs::Vfs, file: usize) -> Vec<NoraRecord> {
    let path = format!(r"etc\editPattern\romNoraPawn{file:02}");
    let bytes = vfs.read(&path, mtarc::type_hash("rRomNoraPawn")).unwrap();
    let xfs = mtxfs::parse(&bytes).unwrap();
    let mut out = Vec::new();
    for o in xfs.walk() {
        if xfs.class_of(o).name() != Some("rRomNoraPawn::cRomNoraPawn") {
            continue;
        }
        let Some(mtxfs::Value::Object(Some(edit))) = xfs.get(o, "mRomPawnEdit").unwrap().first() else {
            panic!("record without mRomPawnEdit")
        };
        out.push(NoraRecord {
            gender: uint(xfs.get(edit, "mGender").unwrap()) as u8,
            nickname: uint(xfs.get(edit, "mNickname").unwrap()) as u32,
            voice: uint(xfs.get(edit, "mVoice").unwrap()) as u8,
            hair: uint(xfs.get(edit, "mHairNo").unwrap()) as u8,
            job: uint(xfs.get(o, "mRomPawnJob").unwrap()) as u8,
            name_no: uint(xfs.get(o, "mRomPawnNameNo").unwrap()) as usize,
        });
    }
    out
}

/// English nora names: `[job - 1][gender]` lists in property order.
fn load_names(vfs: &ddvfs::Vfs) -> Vec<Vec<String>> {
    let bytes = vfs.read(r"etc\editPattern\romPawnNameAlphabet", mtarc::type_hash("rRomPawnName")).unwrap();
    let xfs = mtxfs::parse(&bytes).unwrap();
    let props = [
        "mNameFighterMale",
        "mNameFighterFemale",
        "mNameStriderMale",
        "mNameStriderFemale",
        "mNameMageMale",
        "mNameMageFemale",
    ];
    props
        .iter()
        .map(|p| {
            xfs.get(&xfs.root, p)
                .unwrap()
                .iter()
                .map(|v| match v {
                    mtxfs::Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
                    other => panic!("{p}: {other:?}"),
                })
                .collect()
        })
        .collect()
}

#[test]
fn private_save_pawns() {
    let Some(path) = save_path() else {
        eprintln!("skipped: no save copy (set DDDA_SAVE)");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let save = ddsave::SaveFile::parse(&bytes).unwrap();
    assert!(save.trailing_zero);
    assert_eq!(save.header.reserved, 0);
    let doc = save.document().unwrap();
    let root = doc.root();
    assert_eq!(root.ty().as_deref(), Some("sSave::saveDataAllDA"));
    assert_eq!(root.get::<u32>("mCreateNo"), Some(save.header.version));
    eprintln!(
        "save: version {}, {} -> {} bytes, {} elements",
        save.header.version,
        save.header.compressed_len,
        save.header.inflated_len,
        doc.len()
    );

    // Gather every Edit that claims to be a nora pawn, with its level.
    let mut nora: Vec<(u8, u16, u8, Edit)> = Vec::new();
    for slot in ["mPlayerDataManual", "mPlayerDataBase"] {
        let p = PlayerPawns::read(root.child(slot).unwrap())
            .unwrap_or_else(|| panic!("{slot}: pawn records do not read"));
        assert_eq!(p.party.len(), 3, "{slot}: party slots");
        assert_eq!(p.party_present.len(), 3);
        for list in [p.favorites.len(), p.borrowed.len(), p.lent.len(), p.history.len()] {
            assert!(list <= 100);
        }
        assert!(p.close_friends.len() <= 4);
        eprintln!(
            "{slot}: party {} (present {}), favourites {}, past summons {}, lent {}, close friends {}, history {}",
            p.party.len(),
            p.party_present.iter().filter(|&&b| b).count(),
            p.favorites.len(),
            p.borrowed.len(),
            p.lent.len(),
            p.close_friends.len(),
            p.history.len()
        );
        for pawn in p.party.iter().chain(&p.close_friends) {
            if let RomPawnId::Nora { level, index } = pawn.online.rom_id {
                // The builder (0x00795610) stores the requested level.
                assert_eq!(pawn.param.level, level, "{slot}: nora level byte vs mLevel");
                nora.push((level, index, pawn.param.job, pawn.edit.clone()));
            }
        }
        for h in &p.history {
            if let RomPawnId::Nora { level, index } = h.online.rom_id {
                nora.push((level, index, h.job as u8, h.edit.clone()));
            }
        }
    }
    eprintln!("nora pawns in save: {}", nora.len());

    let Some(root_pc) = native_pc() else {
        eprintln!("skipped data comparison: no install");
        return;
    };
    let vfs = ddvfs::Vfs::open(root_pc).unwrap();
    let names = load_names(&vfs);
    for (level, index, job, edit) in &nora {
        let file = RomPawnId::nora_file(*level).expect("level within bands");
        let records = load_nora(&vfs, file);
        assert_eq!(records.len(), 900, "romNoraPawn{file:02} record count");
        let r = &records[*index as usize];
        assert_eq!(
            (edit.gender, edit.nickname, edit.voice, edit.hair, *job),
            (r.gender, r.nickname, r.voice, r.hair, r.job),
            "romNoraPawn{file:02}[{index}] differs from the save"
        );
        let list = &names[(r.job as usize - 1) * 2 + r.gender as usize];
        assert_eq!(edit.name, list[r.name_no], "English name lookup for romNoraPawn{file:02}[{index}]");
    }
    eprintln!("nora pawns matching game data: {}", nora.len());
}
