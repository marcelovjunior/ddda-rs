//! Print a typed player parameter resource from the user's read-only install.
use std::path::Path;
pub fn run(dir: &Path, path: &str) -> Result<(), String> {
    let vfs = ddvfs::Vfs::open(dir)?;
    let mut found = false;
    for &class in ddparam::RESOURCE_CLASSES {
        let hash = mtarc::type_hash(class);
        if vfs.locate(path, hash).is_none() {
            continue;
        }
        let bytes = vfs.read(path, hash)?;
        let parameter =
            ddparam::Parameter::from_bytes(class, &bytes).map_err(|e| format!("{path} ({class}): {e}"))?;
        println!("{path} ({class})\n{parameter:#?}");
        found = true;
    }
    if found {
        Ok(())
    } else {
        Err(format!("{path}: no supported player parameter resource found"))
    }
}
