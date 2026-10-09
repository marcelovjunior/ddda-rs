//! Prints selected values from the user's own installation, never bundled data.
use ddenemy::*;
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("usage: inspect <nativePC> [em#### ...]")?;
    let vfs = ddvfs::Vfs::open(root)?;
    for id in args {
        let data = EnemyData::load(&vfs, EnemyId::parse(&id)?)?;
        println!("{id}");
        if let Some((_, s)) = data.common {
            println!("  stats: {s:?}");
        }
        if let Some(r) = data.regions {
            for region in r.regions {
                println!(
                    "  region {} type {}: {:?}",
                    region.no,
                    region.kind,
                    region
                        .elements
                        .iter()
                        .map(|e| (e.no, e.id, e.hp_max))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    Ok(())
}
