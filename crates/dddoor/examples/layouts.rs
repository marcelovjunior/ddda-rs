//! Inspect authored object layouts without extracting game resources.
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("expected nativePC and stage")?;
    let stage: u32 = args.next().ok_or("expected stage")?.parse().map_err(|_| "invalid stage")?;
    let vfs = ddvfs::Vfs::open(root)?;
    let prefix = format!(r"scr\st{stage:03}\etc\");
    let kind = mtarc::type_hash("rLayout");
    let mut paths: Vec<_> = vfs.paths_of_type(kind).filter(|p| p.starts_with(&prefix)).collect();
    paths.sort_unstable();
    for path in paths {
        for record in mtlot::parse(&vfs.read(path, kind)?).map_err(|e| e.to_string())? {
            if !record.class.contains("Om") { continue; }
            let Some(placement) = record.placement() else { continue };
            println!("{path} #{} {} {placement:?}", record.id, record.class);
        }
    }
    Ok(())
}
