fn main() {
    let exe = ddexe::ExeFile::open(r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe").unwrap();
    let mut m = ddruntime::memory::GuestMemory::new();
    ddruntime::image::load(&mut m, &exe).unwrap();
    for i in ddruntime::image::bind_imports(&mut m, &exe).unwrap() {
        println!("{}", i.full_name());
    }
}
