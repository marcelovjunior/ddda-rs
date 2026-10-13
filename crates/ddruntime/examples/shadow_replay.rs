//! Offline L172 capture replay. Does not create a session or game window.
fn main() {
    let path = std::env::args_os()
        .nth(1)
        .expect("shadow_replay <private .ddshadow capture>");
    std::env::set_var("DDRUNTIME_PARALLEL_SHADOW", "1");
    match ddruntime::shadow_replay::replay_file(std::path::Path::new(&path)) {
        Ok(entries) => println!("serial/parallel proof passed: {entries} entries"),
        Err(reason) => {
            eprintln!("serial/parallel proof rejected: {reason}");
            std::process::exit(1);
        }
    }
}
