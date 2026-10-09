//! Source-derived chimera death dispatch primitives.
//!
//! These select actions after the combat layer requests death. They do not
//! infer death from absent actors, timers, animation completion or child loss.
//! Damage routing and the host's persistent placement binding remain separate.

/// Common enemy action arbitration, actor +0x2e34/+0x2e38.
/// These are priority masks, **not** chimera body-part masks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Priorities {
    pub current: u32,
    pub pending: u32,
}

impl Priorities {
    // Translated from 0x009a4dc0, 0x009a5880 and 0x009a5a00
    // (DDDA.exe 2017-12-13 Steam). A blocked request remains pending.
    fn request(&mut self, priority: u32) -> bool {
        self.pending |= 1 << priority;
        (self.current | self.pending).trailing_zeros() == priority
    }
}

/// Inputs common to the three vf72 death callbacks. The caller must obtain
/// `has_damage_context` from accepted combat, not create a synthetic context.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeathContext {
    pub has_damage_context: bool,
    /// Return value of the actor's vf73 (vtable +0x1d4).
    pub already_dead: bool,
}

/// Body state consumed by death and parent-reaction selectors.
#[derive(Debug, Clone, Copy, Default)]
pub struct BodyState {
    /// Actor +0x2dd4, current local action.
    pub action: u32,
    /// Actor +0x384, low 16 bits of current motion.
    pub motion: u16,
    /// Actor +0x1ed0. Exact semantic name remains unresolved.
    pub alternate_pose: bool,
}

/// Select the body death action. `Some` means the original calls common
/// uEnemy::vf72 *before* assigning this action; it is not animation cleanup.
// Translated from 0x009a4dc0 (DDDA.exe 2017-12-13 Steam).
pub fn body_death(priorities: &mut Priorities, context: DeathContext, body: BodyState) -> Option<u32> {
    if !context.has_damage_context || context.already_dead || !priorities.request(2) {
        return None;
    }
    Some(if body.action == 0x89 {
        0x57
    } else if !body.alternate_pose {
        0x3a
    } else {
        match body.motion {
            0x212 => 0x55,
            0x213 => 0x56,
            0x227 => 0x53,
            0x228 => 0x54,
            0x239 => 0x52,
            _ => 0x58,
        }
    })
}

/// Child identity follows the actual classes and body links. Anatomical
/// names are deliberately withheld until their collision routing is traced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child {
    /// uEm5200_00: body +0x72e0, child parent +0x7278.
    Unit00,
    /// uEm5200_01: body +0x72e4, child parent +0x72c0.
    Unit01,
}

/// Extra guards used only by uEm5200_01::vf72.
#[derive(Debug, Clone, Copy, Default)]
pub struct Child01State {
    /// Child +0x72c4; zero is the attached unit's initial mode.
    pub mode: u32,
    pub action: u32,
    /// A missing parent does not block the original callback.
    pub parent_dead: bool,
}

/// Select child death. The caller must dispatch common enemy death for this
/// child's own identity and then notify its parent. A child action is never
/// evidence that the parent's placement has died.
// Translated from 0x009b46b0 and 0x009bd060 (DDDA.exe 2017-12-13 Steam).
pub fn child_death(
    child: Child,
    priorities: &mut Priorities,
    context: DeathContext,
    child01: Child01State,
) -> Option<u32> {
    if !context.has_damage_context || context.already_dead {
        return None;
    }
    if child == Child::Unit01
        && (child01.mode != 0 || matches!(child01.action, 0x35 | 0x36) || child01.parent_dead)
    {
        return None;
    }
    if !priorities.request(2) {
        return None;
    }
    Some(match child {
        Child::Unit00 => 0x32,
        Child::Unit01 => 0x38,
    })
}

/// Select the parent's reaction to a child death. `impact_side` is the
/// result of 0x0099e7d0 when the callback has a damage context; its geometric
/// interpretation is not inferred here. Missing context takes the negative
/// branch. Unit01's additional 0x0099e9b0 side effect is not implemented.
// Translated from 0x009a5880 and 0x009a5a00 (DDDA.exe 2017-12-13 Steam).
pub fn parent_reaction(
    child: Child,
    priorities: &mut Priorities,
    body: BodyState,
    impact_side: Option<f32>,
) -> Option<u32> {
    if !priorities.request(if body.alternate_pose { 12 } else { 11 }) {
        return None;
    }
    if !body.alternate_pose {
        return Some(match child {
            Child::Unit00 => 0x3c,
            Child::Unit01 => 0x3d,
        });
    }
    let base = match body.motion {
        0x212 => 0x67,
        0x213 => 0x68,
        0x227 => 0x65,
        0x228 => 0x66,
        0x233 | 0x234 => return Some(if child == Child::Unit00 { 0x8b } else { 0x8c }),
        0x239 => 0x64,
        _ => {
            return Some(match (child, impact_side.is_some_and(|side| side >= 0.0)) {
                (Child::Unit00, true) => 0x45,
                (Child::Unit00, false) => 0x46,
                (Child::Unit01, true) => 0x47,
                (Child::Unit01, false) => 0x48,
            })
        }
    };
    Some(base + if child == Child::Unit00 { 0 } else { 5 })
}
