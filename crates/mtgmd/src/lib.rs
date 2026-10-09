//! Read-only GMD 1.2.1 message reader. Strings borrow the input and preserve
//! line endings, inline tags, empty entries, and duplicate labels exactly.
//! Layout: DDDA Steam data and loader 0x00dde1c0; see docs/TEXT.md.

use std::fmt;

pub const VERSION: u32 = 0x0001_0201;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label<'a> {
    pub message_index: u32,
    /// Serialized authoring pointer. The loader replaces it; never a file offset.
    pub stored_pointer: u32,
    pub name: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFile<'a> {
    /// Observed: 0 Japanese, 1 English, 2 French, 3 Spanish, 4 German,
    /// 5 Italian, 7 Traditional Chinese. Unknown IDs remain representable.
    pub language: u32,
    /// Header bytes at 0x0c; zero throughout the measured install.
    pub reserved: [u8; 8],
    pub name: &'a str,
    pub labels: Vec<Label<'a>>,
    pub messages: Vec<&'a str>,
}

impl<'a> MessageFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let mut input = Cursor(data);
        if input.take(4)? != b"GMD\0" {
            return Err(Error("not a GMD resource".into()));
        }
        let version = input.u32()?;
        if version != VERSION {
            return Err(Error(format!("unsupported GMD version {version:#x}")));
        }
        let language = input.u32()?;
        let reserved = input.take(8)?.try_into().unwrap();
        let label_count = input.u32()? as usize;
        let message_count = input.u32()? as usize;
        let label_bytes = input.u32()? as usize;
        let message_bytes = input.u32()? as usize;
        let name_len = input.u32()? as usize;
        let name = utf8(input.take(name_len)?)?;
        if name.as_bytes().contains(&0) || input.take(1)? != [0] {
            return Err(Error("invalid resource name terminator".into()));
        }
        let table_size = label_count
            .checked_mul(8)
            .ok_or_else(|| Error("label table size overflow".into()))?;
        let table = input.take(table_size)?;
        let label_names = strings(input.take(label_bytes)?, label_count, "labels")?;
        let messages = strings(input.take(message_bytes)?, message_count, "messages")?;
        if !input.0.is_empty() {
            return Err(Error("trailing bytes after message block".into()));
        }
        let mut labels = Vec::with_capacity(label_count);
        for (entry, name) in table.chunks_exact(8).zip(label_names) {
            let message_index = u32::from_le_bytes(entry[..4].try_into().unwrap());
            if message_index as usize >= message_count {
                return Err(Error(format!(
                    "label {name:?} refers to missing message {message_index}"
                )));
            }
            labels.push(Label {
                message_index,
                stored_pointer: u32::from_le_bytes(entry[4..].try_into().unwrap()),
                name,
            });
        }
        Ok(Self {
            language,
            reserved,
            name,
            labels,
            messages,
        })
    }

    /// Exact, case-sensitive label lookup. First occurrence wins for duplicates.
    /// Translated from 0x00dde580 (DDDA.exe 2017-12-13 Steam).
    /// Rust returns None for the original's 0xffffffff sentinel.
    pub fn message_index(&self, label: &str) -> Option<u32> {
        self.labels
            .iter()
            .find(|entry| entry.name == label)
            .map(|entry| entry.message_index)
    }

    /// Zero-based message lookup, as at 0x00dde5f0. Out-of-range is None;
    /// the engine returns a global fallback pointer whose contents are untraced.
    pub fn message(&self, index: u32) -> Option<&'a str> {
        self.messages.get(index as usize).copied()
    }

    pub fn by_label(&self, label: &str) -> Option<&'a str> {
        self.message(self.message_index(label)?)
    }
}

struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], Error> {
        if len > self.0.len() {
            return Err(Error(format!(
                "truncated GMD: need {len} bytes, have {}",
                self.0.len()
            )));
        }
        let (head, tail) = self.0.split_at(len);
        self.0 = tail;
        Ok(head)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

fn utf8(bytes: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(bytes).map_err(|e| Error(format!("invalid UTF-8: {e}")))
}

fn strings<'a>(block: &'a [u8], count: usize, kind: &str) -> Result<Vec<&'a str>, Error> {
    // Validate the count before allocating; even empty strings need a NUL byte.
    if count > block.len() {
        return Err(Error(format!("{kind} count exceeds block size")));
    }
    if block.is_empty() && count == 0 {
        return Ok(Vec::new());
    }
    let Some(body) = block.strip_suffix(&[0]) else {
        return Err(Error(format!("unterminated {kind} block")));
    };
    let result: Vec<_> = body.split(|&b| b == 0).map(utf8).collect::<Result<_, _>>()?;
    if result.len() != count {
        return Err(Error(format!(
            "{kind} count mismatch: expected {count}, got {}",
            result.len()
        )));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic fixture: no game assets embedded in the repository.
    fn fixture() -> Vec<u8> {
        let labels = b"greeting\0greeting\0empty\0";
        let messages = "Hello\r\n<SIZE 20>世界\0Second\nline\0\0".as_bytes();
        let mut data = b"GMD\0".to_vec();
        for word in [
            VERSION,
            1,
            0,
            0,
            3,
            3,
            labels.len() as u32,
            messages.len() as u32,
            4,
        ] {
            data.extend(word.to_le_bytes());
        }
        data.extend(b"Test\0");
        for (index, pointer) in [(0u32, 0x1000u32), (1, 0x1009), (2, 0x1012)] {
            data.extend(index.to_le_bytes());
            data.extend(pointer.to_le_bytes());
        }
        data.extend(labels);
        data.extend(messages);
        data
    }

    #[test]
    fn preserves_utf8_and_lookup_semantics() {
        let bytes = fixture();
        let file = MessageFile::parse(&bytes).unwrap();
        assert_eq!(file.name, "Test");
        assert_eq!(file.language, 1);
        assert_eq!(file.message_index("greeting"), Some(0));
        assert_eq!(file.by_label("greeting"), Some("Hello\r\n<SIZE 20>世界"));
        assert_eq!(file.by_label("Greeting"), None);
        assert_eq!(file.message_index("missing"), None);
        assert_eq!(file.message(1), Some("Second\nline"));
        assert_eq!(file.by_label("empty"), Some(""));
        assert_eq!(file.message(3), None);
        assert_eq!(file.message(u32::MAX), None);
        assert_eq!(file.labels[1].stored_pointer, 0x1009);
    }

    #[test]
    fn rejects_truncation_and_invalid_structure() {
        let original = fixture();
        for end in 0..original.len() {
            assert!(MessageFile::parse(&original[..end]).is_err(), "end {end}");
        }
        for (offset, value) in [
            (0, 0),
            (4, 0),
            (20, u32::MAX),
            (24, u32::MAX),
            (28, u32::MAX),
            (32, u32::MAX),
            (36, u32::MAX),
            (45, 3),
        ] {
            let mut bytes = original.clone();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(MessageFile::parse(&bytes).is_err(), "offset {offset}");
        }
        let mut bytes = original.clone();
        bytes.push(0);
        assert!(MessageFile::parse(&bytes).is_err());
        let mut bytes = original.clone();
        *bytes.last_mut().unwrap() = 1;
        assert!(MessageFile::parse(&bytes).is_err());
        let mut bytes = original;
        bytes[40] = 0xff;
        assert!(MessageFile::parse(&bytes).is_err());
    }

    #[test]
    fn accepts_empty_file_and_unknown_header_values() {
        let mut bytes = b"GMD\0".to_vec();
        for word in [VERSION, 99, 42, 43, 0, 0, 0, 0, 0] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.push(0);
        let file = MessageFile::parse(&bytes).unwrap();
        assert_eq!(file.language, 99);
        assert!(file.messages.is_empty());
        assert!(file.labels.is_empty());
    }
}
