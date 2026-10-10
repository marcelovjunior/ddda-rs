//! Installed first-page character presets and the native character-name rules.
//! Bulk appearance, substitution and blacklist data stay in the player's install.

use crate::{Error, Source};
use ddpawn::cmc::Edit;

/// Field bindings translated from 0x00617420 (DDDA.exe 2017-12-13 Steam).
/// Native spellings are part of the XFS schema, including Mouse and Eyeblow.
const ALL_FIELDS: [(&str, &str); 38] = [
    ("Face_Base", "mFaceBase"),
    ("Eyebrow", "mFaceEyebrow"),
    ("Eye", "mFaceEye"),
    ("Nose", "mFaceNose"),
    ("Mouse", "mFaceMouth"),
    ("Ear", "mFaceEar"),
    ("EyeOfsX", "mOfsXFaceEye"),
    ("EyeOfsY", "mOfsYFaceEye"),
    ("EyebrowOfsX", "mOfsXFaceEyebrow"),
    ("EyebrowOfsY", "mOfsYFaceEyebrow"),
    ("NoseOfsY", "mOfsYFaceNose"),
    ("MouseOfsY", "mOfsYFaceMouth"),
    ("EarOfsY", "mOfsYFaceEar"),
    ("EarOfsZ", "mOfsZFaceEar"),
    ("Eyescale", "mSclFaceEye"),
    ("Nosescale", "mSclFaceNose"),
    ("slender", "mBodySlender"),
    ("Muscle", "mBodyMuscle"),
    ("Bust", "mBustScale"),
    ("Bodyall", "mHumanBodyAll"),
    ("Body", "mHumanBody"),
    ("Head", "mHumanHead"),
    ("Arm", "mHumanArm"),
    ("Leg", "mHumanLeg"),
    ("Movement", "mMovement"),
    ("Posture", "mPosture"),
    ("WinkleFace", "mWrinkleFace"),
    ("Scar", "mScarNo"),
    ("Makeup", "mMakeupFace"),
    ("HairColor", "mColorHair"),
    ("EyeRColor", "mColorEyeR"),
    ("EyeLColor", "mColorEyeL"),
    ("BodyColor", "mColorBody"),
    ("BeardColor", "mColorBeard"),
    ("EyeblowColor", "mColorEyebrow"),
    ("MakeupColor", "mColorMakeup"),
    ("HairType", "mHairNo"),
    ("BeardType", "mBeardNo"),
];

#[derive(Debug, Clone)]
pub struct Presets {
    pub all: [Vec<[u8; 38]>; 2],
}

impl Presets {
    pub fn load<S: Source + ?Sized>(source: &S) -> Result<Self, Error> {
        let bytes = source
            .resource(r"etc\editPattern\editPresetData", "rCharacterEditPreset")?
            .ok_or_else(|| Error::Data("editPresetData missing".into()))?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let x = mtxfs::parse(bytes).map_err(|e| Error::Data(format!("editPresetData: {e}")))?;
        let mut all = [Vec::new(), Vec::new()];
        // 0x00617420: 16 records, stride 0x2c; female block starts +0x7a0.
        // 0x00615B20 likewise visits the 16 full-preset preview bodies.
        for (sex, suffix) in ["M", "F"].into_iter().enumerate() {
            for index in 0..16 {
                let property = format!("mAllPresetInputData{suffix}[{index}]");
                let Some([mtxfs::Value::Object(Some(record))]) = x.get(&x.root, &property) else {
                    return Err(Error::Data(format!("editPresetData: malformed {property}")));
                };
                let mut values = [0; 38];
                for (i, (field, _)) in ALL_FIELDS.iter().enumerate() {
                    values[i] = match x.get(record, field) {
                        Some([mtxfs::Value::U(value)]) => u8::try_from(*value).ok(),
                        Some([mtxfs::Value::S(value)]) => u8::try_from(*value).ok(),
                        _ => None,
                    }
                    .ok_or_else(|| Error::Data(format!("editPresetData: {property}/{field} is not u8")))?;
                }
                all[sex].push(values);
            }
        }
        Ok(Self { all })
    }

    /// Replace appearance while retaining the name, moniker, voice and personality.
    // Translated from 0x00617420 (DDDA.exe 2017-12-13 Steam).
    pub fn apply(&self, edit: &mut Edit, sex: u8, index: usize) -> Result<(), Error> {
        let values = self
            .all
            .get(usize::from(sex))
            .and_then(|rows| rows.get(index))
            .ok_or_else(|| Error::Data("character preset outside installed sex/index".into()))?;
        let mut candidate = edit.clone();
        candidate.set("mGender", sex);
        for ((_, field), value) in ALL_FIELDS.iter().zip(values) {
            candidate.set(field, *value);
        }
        // 0x00617524 and 0x0061752B both read +0x23, not +0x22.
        // Serializer0x00787010 identifies destinations +0x49/+0x4a as
        // mColorEyeR/mColorEyeL. Both native full-preset eyes use EyeLColor;
        // beard/brow instead copy their own +0x25/+0x26 fields normally.
        candidate.set("mColorEyeR", values[31]);
        *edit = candidate;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct NameRules {
    substitutions: Vec<(u16, u16)>,
    punctuation: Vec<u16>,
    wide_punctuation: Vec<u16>,
    replacement: u16,
    blacklist: Vec<Vec<u16>>,
}

impl NameRules {
    /// The native cName sanitizer 0x007676E0 reads these three exe tables.
    pub fn load<S: Source + ?Sized>(source: &S, exe: &ddexe::Exe<'_>) -> Result<Self, Error> {
        let substitutions = exe
            .u16s(0x0150_E93C, 30)
            .map_err(|e| Error::Data(e.0))?
            .chunks_exact(2)
            .map(|row| (row[0], row[1]))
            .collect();
        let punctuation = exe.u16s(0x0150_F69C, 19).map_err(|e| Error::Data(e.0))?;
        let wide_punctuation = exe.u16s(0x0150_F6C4, 5).map_err(|e| Error::Data(e.0))?;
        let bytes = source
            .resource(r"etc\blacklist\character_name\character_name", "rBlacklist")?
            .ok_or_else(|| Error::Data("character_name blacklist missing".into()))?;
        let (replacement, blacklist) = Self::parse_blacklist(&bytes)?;
        Ok(Self {
            substitutions,
            punctuation,
            wide_punctuation,
            replacement,
            blacklist,
        })
    }

    // Translated reader from 0x00CACAD0 (DDDA.exe 2017-12-13 Steam).
    fn parse_blacklist(bytes: &[u8]) -> Result<(u16, Vec<Vec<u16>>), Error> {
        let fail = || Error::Data("malformed BLL character-name blacklist".into());
        if bytes.len() < 16
            || &bytes[..4] != b"BLL\0"
            || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 0x77DE_FB94
            || (bytes.len() - 16) % 2 != 0
        {
            return Err(fail());
        }
        let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let replacement = u16::from_le_bytes(bytes[12..14].try_into().unwrap());
        let words: Vec<_> = bytes[16..]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let mut at = 0;
        let mut rows = Vec::new();
        if count > words.len() {
            return Err(fail());
        }
        for _ in 0..count {
            let end = words[at..].iter().position(|&w| w == 0).ok_or_else(fail)? + at;
            if end == at {
                return Err(fail());
            }
            rows.push(words[at..end].to_vec());
            at = end + 1;
        }
        if words[at..].iter().any(|&w| w != 0) {
            return Err(fail());
        }
        Ok((replacement, rows))
    }

    /// Native normalization and censorship, followed by the 25-byte save copy.
    /// The frontend passes committed text, so IME composition/repeat are separate.
    // Translated from 0x007676E0, 0x00CACCC0, 0x00CACDD0 and
    // 0x0060F0B2 (DDDA.exe 2017-12-13 Steam).
    pub fn normalize(&self, text: &str) -> [u8; 25] {
        let mut words: Vec<u16> = text
            .encode_utf16()
            .take_while(|&w| w != 0)
            .filter(|&w| w >= 0x20)
            // 0x007678A0 limits the native UTF-16 scan to 0x100 words.
            .take(256)
            .collect();
        let mut limit = 12;
        let mut spaces_only = true;
        for word in &mut words {
            // Input callback 0x0075D79E skips controls and replaces <> with *.
            if *word == 0x3c || *word == 0x3e {
                *word = 0x2a;
            }
            for &(from, to) in &self.substitutions {
                if *word == from {
                    *word = to;
                }
            }
            spaces_only &= *word == 0x20;
            if self.wide_punctuation.contains(word) {
                limit = 8;
                continue;
            }
            if self.punctuation.contains(word) || matches!(*word, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a) {
                continue;
            }
            if matches!(*word, 0xff10..=0xff19 | 0xff21..=0xff3a | 0xff41..=0xff5a) {
                *word = word.wrapping_add(0x120);
                continue;
            }
            if *word > 0x7f {
                limit = 8;
            }
            if !matches!(*word, 0x3041..=0x3093 | 0x30a1..=0x30f6) {
                *word = self.replacement;
            }
        }
        words.truncate(limit);
        if spaces_only {
            words.fill(self.replacement);
        }
        // 0x00CACDD0 folds only full-width ASCII, hiragana and lowercase ASCII.
        let folded = |mut w: u16| {
            if matches!(w, 0xff10..=0xff19 | 0xff21..=0xff3a | 0xff41..=0xff5a) {
                w = w.wrapping_add(0x120);
            }
            if matches!(w, 0x3041..=0x3093) {
                w += 0x60;
            } else if matches!(w, 0x61..=0x7a) {
                w -= 0x20;
            }
            w
        };
        let mut comparison: Vec<_> = words.iter().copied().map(folded).collect();
        for banned in &self.blacklist {
            // Native repeats this same row after censoring, then advances rows.
            while let Some(at) = comparison.windows(banned.len()).position(|w| w == banned) {
                if comparison[at..at + banned.len()]
                    .iter()
                    .all(|&w| w == self.replacement)
                {
                    break;
                }
                words[at..at + banned.len()].fill(self.replacement);
                comparison[at..at + banned.len()].fill(self.replacement);
            }
        }
        let text = String::from_utf16_lossy(&words);
        let mut result = [0; 25];
        let count = text.len().min(25);
        result[..count].copy_from_slice(&text.as_bytes()[..count]);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_rules() -> NameRules {
        NameRules {
            substitutions: vec![(0x2019, 0x27)],
            punctuation: vec![0x20, 0x2a, 0x27, 0x2d],
            wide_punctuation: vec![0x30fb],
            replacement: 0x2a,
            blacklist: vec!["BAD".encode_utf16().collect(), "アク".encode_utf16().collect()],
        }
    }

    fn text(bytes: [u8; 25]) -> String {
        String::from_utf8(bytes.into_iter().take_while(|&b| b != 0).collect()).unwrap()
    }

    #[test]
    fn native_full_preset_keeps_identity_and_reproduces_left_right_eye_binding() {
        let row = std::array::from_fn(|i| (i + 10) as u8);
        let presets = Presets {
            all: [vec![row], vec![row]],
        };
        let mut edit = Edit {
            fields: [0; 41],
            nickname: 1234,
            name: [0; 25],
        };
        edit.set_name(b"Alba");
        edit.set("mVoice", 5);
        edit.set("mPersonality", 3);
        presets.apply(&mut edit, 1, 0).unwrap();
        assert_eq!(edit.name_str(), "Alba");
        assert_eq!(edit.nickname, 1234);
        assert_eq!(edit.get("mGender"), Some(1));
        assert_eq!(edit.get("mVoice"), Some(5));
        assert_eq!(edit.get("mPersonality"), Some(3));
        assert_eq!(edit.get("mHumanBody"), Some(30));
        assert_eq!(edit.get("mHumanHead"), Some(31));
        assert_eq!(edit.get("mScarNo"), Some(37));
        assert_eq!(edit.get("mMakeupFace"), Some(38));
        assert_eq!(edit.get("mColorEyeR"), Some(41));
        assert_eq!(edit.get("mColorEyeL"), Some(41));
        assert_eq!(edit.get("mColorBeard"), Some(43));
        assert_eq!(edit.get("mColorEyebrow"), Some(44));
        let old = edit.clone();
        assert!(presets.apply(&mut edit, 2, 0).is_err());
        assert!(presets.apply(&mut edit, 1, 1).is_err());
        assert_eq!(edit, old);
    }

    #[test]
    fn native_name_normalization_changes_limits_and_censors_repeated_substrings() {
        let rules = name_rules();
        assert_eq!(text(rules.normalize("ABCDEFGHIJKLM")), "ABCDEFGHIJKL");
        assert_eq!(text(rules.normalize("あいうえおかきくけ")), "あいうえおかきく");
        assert_eq!(text(rules.normalize("Ｆｕｌｌ１２３")), "Full123");
        assert_eq!(text(rules.normalize("éABCDEFGHIJKL")), "*ABCDEFG");
        assert_eq!(text(rules.normalize("badBADあく")), "********");
        assert_eq!(text(rules.normalize("A\u{7F}B")), "A*B");
        assert_eq!(text(rules.normalize("Alba’s\n<>")), "Alba's**");
        assert_eq!(text(rules.normalize("   ")), "***");
        assert_eq!(text(rules.normalize("")), "");
    }

    #[test]
    fn blacklist_reader_rejects_missing_terminators_and_keeps_installed_replacement() {
        let mut bytes = b"BLL\0".to_vec();
        bytes.extend_from_slice(&0x77DE_FB94u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0x23u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for w in [0x42u16, 0x41, 0x44, 0] {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        assert_eq!(
            NameRules::parse_blacklist(&bytes).unwrap(),
            (0x23, vec![vec![0x42, 0x41, 0x44]])
        );
        assert!(NameRules::parse_blacklist(&bytes[..bytes.len() - 2]).is_err());
        bytes[4] ^= 1;
        assert!(NameRules::parse_blacklist(&bytes).is_err());
    }
}
