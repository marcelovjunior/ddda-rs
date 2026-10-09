//! Print an rStatusParam table (index, category, overwrite, count, timer, params).
//! cargo run -p ddcombat --example status_dump -- <nativePC> param\status\enemy
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vfs = ddvfs::Vfs::open(&a[1]).unwrap();
    let d = vfs.read(&a[2], mtarc::type_hash("rStatusParam")).unwrap();
    let s = ddenemy::StatusParam::parse(&d).unwrap();
    for (i, e) in s.entries.iter().enumerate() {
        match e {
            Some(e) => println!("{i:3} cat {:3} ow {} n {} timer {} p0 {} p1 {}", e.category, e.is_overwrite as u8, e.overwrite_num, e.timer, e.param0, e.param1),
            None => println!("{i:3} -"),
        }
    }
}
