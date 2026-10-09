//! Print an rAdjustParam table (`ajp\0`, u32 0x0201, u32 count, f32 values).
//! The game's getter `rAdjustParam::vf15` (0x00CA47D0) returns values[i].
//! cargo run -p ddcombat --example adj_dump -- <nativePC> param\game
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vfs = ddvfs::Vfs::open(&a[1]).unwrap();
    let d = vfs.read(&a[2], mtarc::type_hash("rAdjustParam")).unwrap();
    assert_eq!(&d[..4], b"ajp\0");
    let n = u32::from_le_bytes(d[8..12].try_into().unwrap()) as usize;
    for i in 0..n {
        let v = f32::from_le_bytes(d[12 + 4 * i..16 + 4 * i].try_into().unwrap());
        println!("{i:#05x} {i:4} {v}");
    }
}
