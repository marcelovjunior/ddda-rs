//! Scalar ordinary NPC death action and separate corpse intel clock.

/// Ordinary corpse intel fields, independent of every player seat. The
/// actor and intel update in separate native unit phases and use separate
/// deltas; callers must preserve that order rather than decrement on hits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorpseIntel {
    pub wait: f32,
    pub alpha: f32,
}

impl CorpseIntel {
    // Translated from uNpcIntel::ctor 0x00BBBFA0 (+0x484/+0x4E0)
    // (DDDA.exe 2017-12-13 Steam).
    pub fn new(base_fps: f32) -> Result<Self, String> {
        if !base_fps.is_finite() || base_fps <= 0.0 {
            return Err("invalid NPC corpse base FPS".into());
        }
        let wait = base_fps * 60.0;
        if !wait.is_finite() {
            return Err("invalid NPC corpse wait".into());
        }
        Ok(Self { wait, alpha: 1.0 })
    }

    // Translated timer subset of uNpcIntel::vf08 0x00BBCB80:
    // +0x4E0 uses sMain+0x70, not the actor's scaled unit delta.
    pub fn advance(
        &mut self,
        apparent: bool,
        permanent: bool,
        flags: u32,
        main_delta: f32,
    ) -> Result<(), String> {
        if !main_delta.is_finite() || main_delta < 0.0 {
            return Err("invalid NPC corpse intel delta".into());
        }
        if apparent && !permanent && flags & 0x0040_0000 == 0 {
            if self.wait > 0.0 {
                self.wait -= main_delta;
            }
            if self.wait <= 0.0 {
                self.wait = 0.0;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathPhase {
    Initialize,
    Motion,
    CorpseWait,
    Fade,
    Finished,
}

/// Effects emitted at actual death-action boundaries, never directly from HP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeathEffects {
    pub apparent: bool,
    pub begin_fade: bool,
    pub permanent: bool,
}

/// Scalar death-action subset. Actual motion launch/terminal observation,
/// collision flags, root/bone locking, body removal and the shared bank
/// writes remain the host actor's responsibility. This helper is not yet
/// connected to the ordinary host death-action selection path.
#[derive(Debug, Clone, Copy)]
pub struct DeathAction {
    phase: DeathPhase,
}

impl DeathAction {
    // Translated from cNpcActDead::vf05 0x00B988B0: a previously apparent
    // death skips the motion phase. No HP test selects that shortcut.
    pub fn new(already_apparent: bool) -> Self {
        Self {
            phase: if already_apparent {
                DeathPhase::CorpseWait
            } else {
                DeathPhase::Initialize
            },
        }
    }

    pub fn phase(&self) -> DeathPhase {
        self.phase
    }

    // Translated scalar phase switch from cNpcActDead::vf06 0x00B98AE0
    // (DDDA.exe 2017-12-13 Steam). Native 0x01057100 supplies layer_ended.
    // Phase2 also clears actor+0x1065; its wider meaning is unported here.
    pub fn advance(
        &mut self,
        layer_ended: bool,
        flags: u32,
        intel: Option<&mut CorpseIntel>,
        actor_delta: f32,
    ) -> Result<DeathEffects, String> {
        if !actor_delta.is_finite() || actor_delta < 0.0 {
            return Err("invalid NPC death actor delta".into());
        }
        let mut effects = DeathEffects::default();
        match self.phase {
            DeathPhase::Initialize => self.phase = DeathPhase::Motion,
            DeathPhase::Motion if layer_ended => {
                self.phase = DeathPhase::CorpseWait;
                effects.apparent = true;
            }
            DeathPhase::CorpseWait
                if flags & 0x0040_0000 == 0 && intel.as_ref().map_or(true, |intel| intel.wait <= 0.0) =>
            {
                self.phase = DeathPhase::Fade;
                effects.begin_fade = true;
            }
            DeathPhase::Fade => {
                let alpha = intel.as_ref().map_or(1.0, |intel| intel.alpha);
                if !alpha.is_finite() {
                    return Err("invalid NPC corpse alpha".into());
                }
                let alpha = alpha.max(0.0);
                let alpha = alpha - actor_delta * (1.0 / 30.0);
                if alpha <= 0.0 {
                    self.phase = DeathPhase::Finished;
                    effects.permanent = true;
                } else if let Some(intel) = intel {
                    intel.alpha = alpha;
                }
            }
            _ => {}
        }
        Ok(effects)
    }
}
