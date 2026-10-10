//! Read-only sound PCK lookup. Indexes and compressed audio stay installed.
//! The caller supplies an allocation budget; no extracted files are written.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
struct Entry { key: u32, offset: u32, length: u32 }

#[derive(Debug)]
pub struct Index { entries: Vec<Entry>, file_length: u64 }

impl Index {
    // Translated from 0x00D0EA80 (DDDA.exe 2017-12-13 Steam): magic,
    // count, twelve-byte records, key search, offset and bounded length.
    // Budget and malformed-index rejection are reader safety policy.
    pub fn read(reader: &mut (impl Read + Seek), budget: usize) -> Result<Self, String> {
        let file_length = reader.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        reader.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut header = [0;8];
        reader.read_exact(&mut header).map_err(|e| format!("PCK header: {e}"))?;
        if u32::from_le_bytes(header[..4].try_into().unwrap()) != 0x5828_4039 {
            return Err("unsupported sound PCK magic".into()); // 0x00D0EAD7
        }
        let count = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let bytes = count.checked_mul(12).ok_or("PCK index size overflow")?;
        let first_payload = 8u64.checked_add(bytes as u64).ok_or("PCK index end overflow")?;
        if bytes > budget || first_payload > file_length { return Err("PCK index exceeds file or budget".into()); }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let mut row = [0;12];
            reader.read_exact(&mut row).map_err(|e| format!("PCK index: {e}"))?;
            let entry = Entry {
                key: u32::from_le_bytes(row[..4].try_into().unwrap()),
                offset: u32::from_le_bytes(row[4..8].try_into().unwrap()),
                length: u32::from_le_bytes(row[8..].try_into().unwrap()),
            };
            if entries.last().is_some_and(|previous: &Entry| previous.key >= entry.key) {
                return Err("PCK keys are not strictly ascending".into());
            }
            if u64::from(entry.offset) < first_payload
                || u64::from(entry.offset) + u64::from(entry.length) > file_length {
                return Err("PCK payload outside file".into());
            }
            entries.push(entry);
        }
        Ok(Self { entries, file_length })
    }

    // Translated from 0x00D0EB2F..0x00D0EB93 (DDDA.exe 2017-12-13 Steam).
    pub fn source(&self, reader: &mut (impl Read + Seek), native_path: &str,
        budget: usize) -> Result<Vec<u8>, String> {
        let key = path_key(native_path);
        let index = self.entries.binary_search_by_key(&key, |entry| entry.key)
            .map_err(|_| format!("PCK stream key {key:08x} absent"))?;
        let entry = self.entries[index];
        if entry.length as usize > budget { return Err("PCK stream exceeds caller budget".into()); }
        if reader.seek(SeekFrom::End(0)).map_err(|e| e.to_string())? != self.file_length {
            return Err("PCK file length changed after index read".into());
        }
        reader.seek(SeekFrom::Start(u64::from(entry.offset))).map_err(|e| e.to_string())?;
        let mut bytes = vec![0;entry.length as usize];
        reader.read_exact(&mut bytes).map_err(|e| format!("PCK stream: {e}"))?;
        Ok(bytes)
    }
}

/// Exact bytes, without case folding or a final complement.
// Translated from 0x00D01B60 with seed -1 at 0x00D0EB33
// (DDDA.exe 2017-12-13 Steam). The reflected polynomial is the table's
// bit128 word at 0x014077C0; bitwise evaluation equals that native table.
pub fn path_key(native_path: &str) -> u32 {
    let mut key = u32::MAX;
    for byte in native_path.bytes().take_while(|byte| *byte != 0) {
        key ^= u32::from(byte);
        for _ in 0..8 { key = (key >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(key & 1)); }
    }
    key
}

/// Native short sound-bank routing list, not a table of stream records.
// Translated from 0x00D0ED3E..0x00D0ED8A (DDDA.exe 2017-12-13 Steam).
fn bank(native_path: &str) -> Option<&'static str> {
    ["npc_base_jp", "npc_base_talk", "npc_quest_jp", "npc_quest_talk", "pwn_jp", "pwn"]
        .into_iter().find(|name| native_path.contains(name))
}

/// Keep the existing loose stream backend first (project compatibility
/// policy approved by head); on absence use the native packed sound bank.
/// Native 0x00D0EC00 itself tries its selected PCK before loose fallback.
pub fn read_stream(native_pc: &Path, source: &str, budget: usize) -> Result<Vec<u8>, String> {
    let relative = crate::stream_path(source)?;
    let loose = native_pc.join(&relative);
    match std::fs::File::open(&loose) {
        Ok(mut file) => {
            let length = file.metadata().map_err(|e| e.to_string())?.len();
            if length > budget as u64 { return Err("loose stream exceeds caller budget".into()); }
            let mut bytes = vec![0;length as usize];
            file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
            return Ok(bytes);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", loose.display())),
    }
    let native_path = relative.replace('/', "\\");
    let bank = bank(&native_path).ok_or("stream has neither loose file nor native packed sound bank")?;
    let path = native_pc.join("rom/sound").join(format!("{bank}.pck"));
    let mut file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Index::read(&mut file,budget)?.source(&mut file,&native_path,budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture(path: &str, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [0x5828_4039,1,path_key(path),20,payload.len() as u32] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn native_pck_exact_path_hash_selects_bounded_payload_without_case_or_extension_aliases() {
        let path = r"sound\stream\pwn\m1\wave09\pwn_9780_m1.sngw";
        // Actual native key selected by captured STRQ cue0; no audio bytes.
        assert_eq!(path_key(path),0xEB96_1C30);
        assert_eq!(path_key(""),u32::MAX);
        assert_eq!(path_key("abc\0ignored"),path_key("abc"));
        let mut file = Cursor::new(fixture(path,b"synthetic stream"));
        let index = Index::read(&mut file,12).unwrap();
        assert_eq!(index.source(&mut file,path,16).unwrap(),b"synthetic stream");
        assert!(index.source(&mut file,&path.to_uppercase(),16).is_err());
        assert!(index.source(&mut file,path.trim_end_matches(".sngw"),16).is_err());
        assert!(index.source(&mut file,path,15).is_err());
        assert_eq!(bank(r"sound\stream\pwn_jp\m1\voice.sngw"),Some("pwn_jp"));
    }

    #[test]
    fn pck_rejects_truncated_index_duplicate_keys_outside_payload_and_allocation_overflow() {
        let original = fixture("stream",b"test");
        for (offset,value) in [(0,0),(4,u32::MAX),(12,19),(12,u32::MAX),(16,u32::MAX)] {
            let mut bytes = original.clone();
            bytes[offset..offset+4].copy_from_slice(&value.to_le_bytes());
            assert!(Index::read(&mut Cursor::new(bytes),1024).is_err());
        }
        assert!(Index::read(&mut Cursor::new(original[..19].to_vec()),1024).is_err());
        assert!(Index::read(&mut Cursor::new(original.clone()),11).is_err());
        let mut duplicate = original[..8].to_vec();
        duplicate[4..8].copy_from_slice(&2u32.to_le_bytes());
        for _ in 0..2 {
            for value in [1u32,32,0] { duplicate.extend_from_slice(&value.to_le_bytes()); }
        }
        assert!(Index::read(&mut Cursor::new(duplicate),1024).is_err());
        let mut file = Cursor::new(original);
        let index = Index::read(&mut file,1024).unwrap();
        file.get_mut().pop();
        assert!(index.source(&mut file,"stream",1024).is_err());
    }

    #[test]
    fn installed_captured_cmc_cue_resolves_packed_vorbis_and_decodes_nonzero_audio() {
        let native_pc = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native_pc.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(&native_pc).unwrap();
        let requests = vfs.read(r"sound\stream\pwn\em0100\m1\em0100_m1",
            mtarc::type_hash("rSoundStreamRequest")).unwrap();
        let requests = crate::StreamRequests::parse(&requests).unwrap();
        let source = requests.event_source(0,0).unwrap();
        let bytes = read_stream(&native_pc,source.path,40_000_000).unwrap();
        assert_eq!(&bytes[..4],b"OggS");
        let pcm = crate::decode::vorbis(&bytes,40_000_000).unwrap();
        assert_eq!((pcm.channels,pcm.sample_rate),(1,48_000));
        assert!(!pcm.samples.is_empty());
        assert!(pcm.samples.iter().all(|sample| sample.is_finite()));
        assert!(pcm.samples.iter().any(|sample| *sample != 0.0));
    }
}
