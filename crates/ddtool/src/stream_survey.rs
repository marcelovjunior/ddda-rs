//! Aggregate authored group spatial gates; no gameplay or process access.
use ddspawn::gpl::{GroupList, Shape};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default)]
struct Summary {
    lists: usize,
    groups: usize,
    enabled: usize,
    linked: usize,
    empty_enabled: usize,
    shapes: BTreeMap<&'static str, usize>,
    radius_min: Option<f32>,
    radius_max: Option<f32>,
}

pub fn run(root: &Path) -> Result<(), String> {
    let vfs = ddvfs::Vfs::open(root).map_err(|e| e.to_string())?;
    let kind = mtarc::type_hash("rLayoutGroupParamList");
    let mut paths: Vec<_> = vfs.paths_of_type(kind).collect();
    paths.sort_unstable();
    if paths.is_empty() {
        return Err("no rLayoutGroupParamList resources in this install".into());
    }
    let mut summaries: BTreeMap<String, Summary> = BTreeMap::new();
    for path in paths {
        let bytes = vfs.read(path, kind).map_err(|e| format!("{path}: {e}"))?;
        let list = GroupList::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let category = path.rsplit_once('_').map_or("other", |(_, suffix)| suffix);
        let summary = summaries.entry(category.to_owned()).or_default();
        summary.lists += 1;
        for group in list.groups {
            summary.groups += 1;
            // cGroupParam::vf09 0x00cc4690 loads scalar 38 into +0x70.
            // 0x00cc5670 consults areas2 only when that field is nonzero;
            // +0x130/+0x134 (tail.2/.3) may redirect to a linked group.
            if group.tail.2 != 0 {
                summary.linked += 1;
            }
            if group.s.0[38] == 0 {
                continue;
            }
            summary.enabled += 1;
            if group.areas2.is_empty() {
                summary.empty_enabled += 1;
            }
            for area in group.areas2 {
                let (name, radius) = match area.shape {
                    Shape::None => ("none", None),
                    Shape::Area { .. } => ("area", None),
                    Shape::Sphere { radius, .. } => ("sphere", Some(radius)),
                    Shape::Cylinder { radius, .. } => ("cylinder", Some(radius)),
                    Shape::Other { .. } => ("other", None),
                };
                *summary.shapes.entry(name).or_default() += 1;
                if let Some(radius) = radius {
                    if !radius.is_finite() || radius < 0.0 {
                        return Err(format!("{path} group {}: invalid radius {radius}", group.slot));
                    }
                    summary.radius_min = Some(summary.radius_min.map_or(radius, |r| r.min(radius)));
                    summary.radius_max = Some(summary.radius_max.map_or(radius, |r| r.max(radius)));
                }
            }
        }
    }
    println!("Authored group areas2; resource suffixes are categories, not inferred runtime classes.");
    println!("Linked groups may use another group's gate; shape radii are not universal spawn distances.");
    for (suffix, s) in summaries {
        println!(
            "{suffix}: lists={} groups={} spatial_enabled={} linked={} enabled_empty={} shapes={:?} radius_range={:?}..{:?}",
            s.lists, s.groups, s.enabled, s.linked, s.empty_enabled, s.shapes, s.radius_min, s.radius_max
        );
    }
    Ok(())
}
