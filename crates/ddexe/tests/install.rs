//! Checks against the player's installed DDDA.exe. `DDDA_EXE` overrides the
//! path; otherwise the exe next to `DDDA_NATIVE_PC` (default the
//! maintainer's install). Skipped when absent.
use ddexe::{locate, ExeFile, PE_TIMESTAMP};
use std::path::PathBuf;

fn exe() -> Option<ExeFile> {
    let path = match std::env::var_os("DDDA_EXE") {
        Some(p) => PathBuf::from(p),
        None => {
            let native = std::env::var_os("DDDA_NATIVE_PC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
            match locate(&native) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("SKIP ddexe install tests: {e}");
                    return None;
                }
            }
        }
    };
    if !path.is_file() {
        eprintln!("SKIP ddexe install tests: {} absent", path.display());
        return None;
    }
    Some(ExeFile::open(path).unwrap())
}

#[test]
fn installed_exe() {
    let Some(file) = exe() else { return };
    let exe = file.image().expect("supported build");
    assert_eq!(exe.timestamp(), PE_TIMESTAMP);
    assert_eq!(exe.image_base(), 0x0040_0000);
    let names: Vec<_> = exe.sections().iter().map(|s| s.name.as_str()).collect();
    assert!(names.starts_with(&[".text", ".rdata", ".data"]), "{names:?}");
    // A path string in .rdata (referenced by 0x006fbfeb and 0x007961fc).
    assert_eq!(exe.cstr(0x0158_7954).unwrap(), "etc\\skill\\skill");
    assert_eq!(exe.section_of(0x0158_7954), Some(".rdata"));
    // rSkillList's loader starts the "skl" magic check (0x00cdec00 is code).
    assert_eq!(exe.section_of(0x00cd_ec00), Some(".text"));
}

#[test]
fn installed_class_names() {
    let Some(file) = exe() else { return };
    let names = file.image().unwrap().class_names();
    assert!(names.len() > 3000, "{} classes", names.len());
    assert_eq!(names.get(&0x019a_26e0).map(String::as_str), Some("cPlActWpnDaggerCstmHyakuretsu"));
    assert_eq!(names.get(&0x019a_922c).map(String::as_str), Some("rSkillList"));
}
