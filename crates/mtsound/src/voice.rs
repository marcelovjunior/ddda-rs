//! Event voice conditions. The host supplies the event's character records;
//! no global player or default voice is assumed by the audio reader.

/// Raw fields used by the original conditional selector. For cPlayerInfo,
/// these are mModelType (+0xc) and mVoiceType (+0x294), reflected by
/// 0x0077d980. For cSAVE_DATA_EDIT, use mGender (+0x28) and mVoice (+0x29),
/// reflected by 0x00787010. Do not invert the model/gender code here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant {
    pub model_or_gender: u32,
    pub voice: u32,
}

impl Variant {
    /// Convert saved edit fields to cPlayerInfo's sound selector fields.
    /// Invalid voice bytes leave the original's previous voice untouched;
    /// without that prior state, report unknown instead of inventing one.
    // Translated from 0x00780fa0 (DDDA.exe 2017-12-13 Steam).
    pub fn from_edit(gender: u8, voice: u8) -> Option<Self> {
        (voice < 12).then_some(Self { model_or_gender: u32::from(gender & 1),
            voice: u32::from(voice) })
    }
}

/// One event's explicitly selected actor context. Missing records suppress
/// their conditional sounds; they must not silently become another actor.
#[derive(Debug, Clone, Copy, Default)]
pub struct EventVoices {
    /// Original sGameSys cPlayerInfo at +0xa76d0.
    pub player: Option<Variant>,
    /// Original save-edit gender/voice fields at +0x150a4/+0x150a5.
    pub edit: Option<Variant>,
    /// Original first pawn cPlayerInfo at +0xa7ec0, shared by groups 2 and 3.
    pub pawn: Option<Variant>,
}

impl EventVoices {
    /// `None` means mConditionalFlag is clear, so no actor context is needed.
    // Translated from uSoundSe::vf21, 0x00c78570
    // (DDDA.exe 2017-12-13 Steam).
    pub fn allows(&self, condition: Option<i32>) -> bool {
        let Some(condition) = condition else { return true; };
        let (base, record) = match condition {
            0..=3 => (0, self.player),
            4..=7 => (4, self.edit),
            8..=11 => (8, self.pawn),
            12..=15 => (12, self.pawn),
            _ => return false,
        };
        let Some(record) = record else { return false; };
        let gender_offset = if record.model_or_gender == 0 { 2 } else { 0 };
        condition == base + gender_offset + ((record.voice / 3) & 1) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_conversion_masks_gender_and_rejects_unknown_prior_voice() {
        assert_eq!(Variant::from_edit(2, 11), Some(Variant { model_or_gender: 0, voice: 11 }));
        assert_eq!(Variant::from_edit(3, 0), Some(Variant { model_or_gender: 1, voice: 0 }));
        assert_eq!(Variant::from_edit(0, 12), None);
        assert_eq!(Variant::from_edit(1, 255), None);
    }

    #[test]
    fn each_character_group_selects_exactly_one_voice_variant() {
        let context = EventVoices {
            player: Some(Variant { model_or_gender: 0, voice: 0 }),
            edit: Some(Variant { model_or_gender: 1, voice: 3 }),
            pawn: Some(Variant { model_or_gender: 0, voice: 5 }),
        };
        let selected: Vec<_> = (0..16).filter(|&n| context.allows(Some(n))).collect();
        assert_eq!(selected, [2, 5, 11, 15]);
        assert!(!context.allows(Some(-1)));
        assert!(!context.allows(Some(16)));
    }

    #[test]
    fn division_then_parity_and_missing_context_are_preserved() {
        for (voice, selected) in [(0, 0), (2, 0), (3, 1), (5, 1), (6, 0), (8, 0), (9, 1)] {
            let context = EventVoices { player: Some(Variant { model_or_gender: 1, voice }),
                ..Default::default() };
            assert!(context.allows(Some(selected)));
            assert!(!context.allows(Some(1 - selected)));
            assert!((4..16).all(|n| !context.allows(Some(n))));
        }
        let unknown = EventVoices::default();
        assert!(unknown.allows(None));
        assert!((0..16).all(|n| !unknown.allows(Some(n))));
    }
}
