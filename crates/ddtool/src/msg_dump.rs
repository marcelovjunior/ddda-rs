use std::path::Path;

pub fn msg_dump(root: &Path, path: &str) -> Result<(), String> {
    let vfs = ddvfs::Vfs::open(root)?;
    let bytes = vfs.read(path, mtarc::type_hash("rGUIMessage"))?;
    let file = mtgmd::MessageFile::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
    println!(
        "{path}: language {}, name {:?}, {} messages, {} labels",
        file.language,
        file.name,
        file.messages.len(),
        file.labels.len()
    );
    // Debug string formatting escapes newlines/tabs while retaining Unicode.
    for (index, message) in file.messages.iter().enumerate() {
        let labels: Vec<_> = file
            .labels
            .iter()
            .filter(|label| label.message_index as usize == index)
            .map(|label| label.name)
            .collect();
        println!("{index}\t{labels:?}\t{message:?}");
    }
    Ok(())
}
