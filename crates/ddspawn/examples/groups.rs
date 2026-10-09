//! Lists a stage's enemy groups touching a cell: `groups <stage> <a> <b> [scene] [hour]`.
fn main() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let a: Vec<String> = std::env::args().skip(1).collect();
    let stage: u32 = a[0].parse().unwrap();
    let list = ddspawn::gpl::GroupList::parse(
        &vfs.read(
            &format!(r"scr\st{stage:03}\etc\st{stage:03}_e"),
            mtarc::type_hash("rLayoutGroupParamList"),
        )
        .unwrap(),
    )
    .unwrap();
    println!("{} groups", list.groups.len());
    let cell = (a[1].parse().unwrap(), a[2].parse().unwrap());
    for g in list
        .groups
        .iter()
        .filter(|g| g.cells.iter().any(|c| (c.m, c.n) == cell))
    {
        let units: Vec<&str> = g.units.iter().map(|u| u.name.as_str()).collect();
        println!(
            "group {:3} units {:?} scene {:?} flag {:?}/{:?} hours {:?} respawn type {} days {} despawn {:b} disabled {} other {:?} cells {}",
            g.slot, units, g.scene_window, g.layout_flag, g.layout_flag2, g.hours, g.respawn & 7, (g.respawn >> 3) & 0xff, g.despawn, g.disabled, g.other_conditions, g.cells.len()
        );
    }
    let read = |p: &str| vfs.read(p, mtarc::type_hash("rLayout")).ok();
    let scene = a.get(3).map_or(3400, |s| s.parse().unwrap());
    let hour = a.get(4).map_or(12, |s| s.parse().unwrap());
    let flag = |n: u16| {
        std::env::var("FLAGS")
            .ok()
            .is_some_and(|v| v.split(",").any(|x| x.parse::<u16>().ok() == Some(n)))
    };
    let w = ddspawn::World {
        scene,
        hour,
        flag: &flag,
    };
    let (gates, spawns) = ddspawn::spawns(stage, &list, cell, &read, &w, &[]).unwrap();
    println!("gates (all flags on, scene {scene}, hour {hour}): {gates:?}");
    for s in spawns {
        println!(
            "  g{} id{} rid{} {} {} {:?}",
            s.group, s.set_id, s.record.id, s.class, s.name, s.position
        );
    }
}
