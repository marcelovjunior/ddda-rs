//! Cassardis's original stage-specific challenge check, separate from q0000.
//!
//! aStage200::vf12 (0x0051ccc0) writes GSF 379; the quest graph consumes it.
//! This is deliberately stage-specific because the original stage is. Enemy
//! AI, damage, and quest progression remain independent data-driven systems.

/// A live Arisen's life-region HP and effective maximum HP. The latter is
/// uPlayerBase vf+0x148 (0x00b845d0 -> 0x00782040), not current HP.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChallengePlayer {
    /// A present player with no first life region follows the <= 1 path.
    pub life_hp: Option<f32>,
    pub effective_max_hp: f32,
}

/// The enemy search found a dragon; no life region reads as zero. Absence
/// of this whole value means no dragon was found and must not latch HP.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChallengeDragon {
    pub life_hp: Option<f32>,
}

/// Per-loaded-stage state: aStage200 +0xa88 (baseline) / +0xa8c (captured).
/// Recreate on stage load; this is not a saved world flag.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DragonChallenge {
    pub baseline_hp: Option<f32>,
}

impl DragonChallenge {
    /// Whether this update requests GSF 379. Pass only present, valid Arisen
    /// units (the exe checks unit flags & 7 == 2). The original checks party
    /// seat zero; any Arisen can satisfy it here, an explicit co-op policy.
    ///
    /// `dragon` is the host's result of the original type-0x5a enemy lookup,
    /// not a synthesized defeated unit. GSF 378 gates only the dragon branch;
    /// a player at <= 1 HP can end the challenge even before that flag.
    // Translated from 0x0051ccc0..0x0051ce19 (DDDA.exe 2017-12-13 Steam).
    pub fn update(
        &mut self,
        check_dragon: bool,
        already_complete: bool,
        dragon: Option<ChallengeDragon>,
        players: &[ChallengePlayer],
    ) -> bool {
        if already_complete {
            return false;
        }
        let mut complete = false;
        if check_dragon {
            if let Some(dragon) = dragon {
                let hp = dragon.life_hp.unwrap_or(0.0);
                if let Some(baseline) = self.baseline_hp {
                    // SUBSS/COMISS at 0x0051cd8e..0x0051cd99; constant at
                    // 0x01518668. Equality and unordered comparisons do not pass.
                    complete = hp < baseline - 4.0;
                } else {
                    self.baseline_hp = Some(hp);
                }
            }
        }
        complete
            || players.iter().any(|p| {
                // COMISS/JB at 0x0051cdeb and FCOMIP/JBE at 0x0051cdfe.
                // Unordered HP does not pass either comparison. A missing life
                // region bypasses the first comparison and checks maximum HP.
                p.life_hp.is_none_or(|hp| hp <= 1.0) && p.effective_max_hp > 0.0
            })
    }
}
