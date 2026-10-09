//! Prints a stage start position and its cell: `startpos <path> <index>`.
fn main() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let a: Vec<String> = std::env::args().skip(1).collect();
    let s = ddmap::StartPositions::parse(&vfs.read(&a[0], mtarc::type_hash("rStartPos")).unwrap()).unwrap();
    let i: usize = a[1].parse().unwrap();
    let p = s.entries[i].position;
    // stage frame: x = (n - 50) * 10000 + local, z = (m - 50) * 10000 + local
    println!(
        "{p:?} -> cell {}m{}n",
        (p[2] / 10000.0).floor() as i32 + 50,
        (p[0] / 10000.0).floor() as i32 + 50
    );
}
