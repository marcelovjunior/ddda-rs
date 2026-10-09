//! Structural pawn-save report. Never emit names, network IDs or saved values.
use std::path::Path;

pub fn run(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|_| "cannot read save copy".to_owned())?;
    let save = ddsave::SaveFile::parse(&bytes).map_err(|_| "invalid save container".to_owned())?;
    let document = save.document().map_err(|_| "invalid save XML".to_owned())?;
    let mut records = 0;
    let mut equipment = 0;
    let mut knowledge = 0;
    let mut skills = 0;
    for node in document
        .iter()
        .filter(|n| n.tag() == "class" && n.ty().as_deref() == Some("cSAVE_DATA_CMC"))
    {
        let pawn =
            ddsave::pawns::PawnFields::read(node).map_err(|_| "invalid typed pawn fields".to_owned())?;
        records += 1;
        equipment += usize::from(pawn.equipment.is_some());
        knowledge += usize::from(pawn.knowledge.study_flags.is_some());
        skills += pawn.skills.weapon.iter().filter(|s| s.is_some()).count();
    }
    println!(
        "pawn records: {records}; equipment arrays: {equipment}; study arrays: {knowledge}; weapon-skill arrays: {skills}"
    );
    Ok(())
}
