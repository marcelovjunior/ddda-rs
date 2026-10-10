//! Combat observations consumed by the existing stage-200 challenge rule.
use ddenemyrt::{combat::Victim, fight::Fight};
use ddstory::dragon::{ChallengeDragon, ChallengePlayer};

/// The opening branch of the native editor's confirmed exit. The caller
/// supplies the real new-game context and confirms every local profile
/// before calling this; stages 802-804 and the stage210 alternate exit
/// are not supported here. A request does not acknowledge a loaded stage.
///
/// `fade` is caller-owned presentation policy: 0x00401160 does not write
/// the quest JumpArea fade field. Do not infer it from stage+0x814.
// Translated from 0x0052B2D0, 0x0052B740, 0x0052B3D9 and 0x00401160
// (DDDA.exe 2017-12-13 Steam).
pub fn editor_exit_request(
    stage: u32,
    sce_no: u32,
    flags: &ddstory::story::GameFlags,
    all_profiles_confirmed: bool,
    fade: u32,
) -> Result<Option<ddstory::Request>, String> {
    if stage != 801 {
        return Err("opening editor exit requires stage801".into());
    }
    if !all_profiles_confirmed { return Ok(None); }
    if sce_no != 1000 || flags.b8844 & 0x0002_0000 == 0 {
        return Err("opening editor exit lacks the native new-game context".into());
    }
    Ok(Some(ddstory::Request::JumpArea { stage: 200, pos: 3, fade }))
}

/// The first native life-vector pointer, not a lookup by region id.
/// cObjCollision+0xc2c/+0xc38 is actor+0x1d0c/+0x1d18. 0x00770910
/// appends the selected RST kind's elements in resource order; the opening
/// constructor selects kind0 at 0x00a49f2d. Missing pointers remain missing.
// Translated from 0x0051ccc0 (DDDA.exe 2017-12-13 Steam).
pub fn first_life_hp(victim: &Victim) -> Option<f32> {
    match victim {
        Victim::Enemy { regions, .. } => regions.first().map(|region| region.hp),
        _ => None,
    }
}

/// Snapshot existing combat bodies at the host's story phase, before hits.
/// Fight::pawns identifies pawn bodies separately from Arisen seats.
/// Aggregating all present Arisen is the explicit co-op policy already
/// documented by ddstory::dragon::DragonChallenge; the original checks0.
pub fn challenge_actors(fight: Option<&Fight>) -> (Option<ChallengeDragon>, Vec<ChallengePlayer>) {
    let Some(fight) = fight else { return (None, Vec::new()) };
    let dragon = fight.dragon.as_ref().map(|dragon| ChallengeDragon {
        life_hp: first_life_hp(&dragon.actor.fighter.victim),
    });
    let players = fight.arisen.iter().filter(|(seat, _)| !fight.pawns.contains_key(seat))
        .filter_map(|(_, actor)| match actor.fighter.victim {
            Victim::Player { hp, hp_max, .. } => Some(ChallengePlayer {
                life_hp: Some(hp), effective_max_hp: hp_max,
            }),
            _ => None,
        }).collect();
    (dragon, players)
}
