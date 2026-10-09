//! Hex dump of an install resource: `hexdump <path> <type> [bytes]`.
fn main() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let a: Vec<String> = std::env::args().skip(1).collect();
    let b = vfs.read(&a[0], mtarc::type_hash(&a[1])).unwrap();
    let n = a
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(512usize)
        .min(b.len());
    println!("{} bytes", b.len());
    for (i, c) in b[..n].chunks(16).enumerate() {
        let hex: Vec<String> = c.iter().map(|x| format!("{x:02x}")).collect();
        let asc: String = c
            .iter()
            .map(|&x| if (32..127).contains(&x) { x as char } else { '.' })
            .collect();
        println!("{:06x}  {:<48}  {}", i * 16, hex.join(" "), asc);
    }
}
