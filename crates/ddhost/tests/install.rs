//! The install-built Sim (skips when the install is absent; DDDA_NATIVE_PC).

fn vfs() -> Option<ddvfs::Vfs> {
    let np = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !np.exists() {
        eprintln!("SKIP: DDDA install absent");
        return None;
    }
    Some(ddvfs::Vfs::open(&np).unwrap())
}

#[test]
fn sim_from_install() {
    let Some(vfs) = vfs() else { return };
    let sim = ddhost::install::sim(&vfs).unwrap();
    // The motions the earlier hand lists named (ddview play.rs, ddaccept)
    // are all there, among every slot of both banks.
    for id in [0u16, 3, 4, 7, 10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 43, 45, 46, 47, 48, 49, 50, 0x54, 0x55, 0x611] {
        assert!(sim.motions.contains_key(&id), "motion {id:#x}");
    }
    eprintln!("{} motions", sim.motions.len());
    // ddsim's shipped constants are the install's rows.
    assert_eq!(sim.jump_normal, ddsim::JUMP_NORMAL);
    assert_eq!(sim.jump_sprint, ddsim::JUMP_SPRINT);
    assert_eq!(sim.ledge_param, ddsim::LEDGE_SHIPPED);
    assert_eq!(sim.move_param, ddsim::MOVE_SHIPPED);
}
