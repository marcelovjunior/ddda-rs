//! Writes a save's XML to stdout: `savexml <DDDA.sav>`.
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let f = ddsave::SaveFile::parse(&std::fs::read(p).unwrap()).unwrap();
    print!("{}", f.xml);
}
