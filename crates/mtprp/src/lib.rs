//! PRPZ wrapper around an XFS object. See docs/ENEMIES.md for evidence.
//! Not restricted to enemies: lights and other objects also use this resource.

pub const MAGIC: &[u8; 4] = b"PRPZ";
pub const MARKER: u32 = 0x77ce_d14c;

#[derive(Debug, Clone, PartialEq)]
pub struct PropParam {
    pub class_hash: u32,
    pub object: mtxfs::Xfs,
}

/// Translated from 0x00fde3c0 (DDDA.exe 2017-12-13 Steam): magic and
/// marker checks, class-hash dispatch, then XFS deserialization. This reader
/// additionally checks that the wrapper and serialized root classes agree.
pub fn parse(data: &[u8]) -> Result<PropParam, String> {
    if data.len() < 12 || &data[..4] != MAGIC {
        return Err("missing 12-byte PRPZ header".into());
    }
    let word = |at| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    if word(4) != MARKER {
        return Err(format!("unknown PRPZ marker {:08x}", word(4)));
    }
    let class_hash = word(8);
    let object = mtxfs::parse(&data[12..]).map_err(|e| e.to_string())?;
    if object.class_of(&object.root).hash != class_hash {
        return Err("PRPZ class differs from XFS root".into());
    }
    Ok(PropParam { class_hash, object })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic empty XFS object; no game data.
    fn sample() -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        for n in [MARKER, 123, 0x0053_4658, 0x0109, 1, 1, 16, 4, 123, 0, 0, 1, 4] {
            b.extend(n.to_le_bytes());
        }
        b
    }

    #[test]
    fn wrapper_and_root_agree() {
        assert_eq!(parse(&sample()).unwrap().class_hash, 123);
    }

    #[test]
    fn rejects_invalid_wrapper_and_truncation() {
        let b = sample();
        for len in 0..b.len() {
            assert!(parse(&b[..len]).is_err());
        }
        for at in [0, 4, 8] {
            let mut bad = b.clone();
            bad[at] ^= 1;
            assert!(parse(&bad).is_err());
        }
        let mut bad = b;
        bad.push(0);
        assert!(parse(&bad).is_err());
    }
}
