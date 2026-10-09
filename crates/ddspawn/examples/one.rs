//! Prints one group: `one <stage> <slot>`.
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
    let g = list.group(a[1].parse().unwrap()).unwrap();
    println!("{:?}\n{:?}\nscalars {:?}", g.units, g.cells, g.s.0);
}
