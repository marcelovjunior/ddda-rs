//! Pure destination decoding, not permission to travel or a stage transition.
//! The caller owns the actor, inventory, current stage and shared world state.
use crate::StartPositions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageRequest {
    pub stage: i32,
    pub start: i32,
}

/// Decode a supported Ferrystone destination selector (three fixed + ten placed).
/// Returns None outside that menu domain; the executable itself does not bound
/// every stale selector. Availability, item identity and consumption are separate.
/// Translated from 0x00b06c30, especially disassembly 0x00b07064..0x00b070b9
/// (DDDA.exe 2017-12-13 Steam): ECX carries stage, stack carries start.
pub fn ferrystone_request(selector: i32, scenario: i32) -> Option<StageRequest> {
    let (stage, start) = match selector {
        0 if scenario < 7800 => (220, 13),
        0 => (230, 8),
        1 => (100, 56),
        2 => (400, 7),
        3..=12 => (999, selector - 3),
        _ => return None,
    };
    Some(StageRequest { stage, start })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrivalSource {
    StartPosition,
    /// Raw low-byte slot, including values >=10 on the original fallback path.
    Portcrystal(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedStage {
    pub stage: i32,
    pub start: i32,
    pub source: ArrivalSource,
}

/// Decode the original stage-999 indirection without adding host validation.
/// Empty slots can yield stage 0; an invalid low-byte slot yields stage 10.
/// These observable fallback rules are NOT safe-travel approval rules.
/// Translated from 0x004fbd50 (DDDA.exe 2017-12-13 Steam).
pub fn resolve_stage(request: StageRequest, slot_stages: &[i32; 10]) -> Option<ResolvedStage> {
    if request.stage == -1 {
        return None;
    }
    let (stage, source) = if request.stage == 999 {
        let slot = request.start as u8;
        (
            slot_stages.get(slot as usize).copied().unwrap_or(10),
            ArrivalSource::Portcrystal(slot),
        )
    } else {
        (request.stage, ArrivalSource::StartPosition)
    };
    Some(ResolvedStage {
        stage,
        start: request.start,
        source,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrivalSeed {
    /// Before the original's vertical collision query and later world transform.
    pub position: [f32; 3],
    /// Stored start angle; Portcrystal arrival initializes this to zero.
    pub angle: f32,
}

/// Extract the supported arrival seed. Caller must supply this stage's resource.
/// Missing start data and invalid slots return None rather than reproducing the
/// original's zero-vector fallback. Collision projection is intentionally absent.
/// Translated field selection from 0x004ff910 / 0x007a6780 / 0x00ce8530 and
/// 0x0044d0d0 (DDDA.exe 2017-12-13 Steam); no restore-save branch is modeled.
pub fn arrival_seed(
    resolved: ResolvedStage,
    starts: Option<&StartPositions>,
    slot_positions: &[[f32; 4]; 10],
) -> Option<ArrivalSeed> {
    let (position, angle) = match resolved.source {
        ArrivalSource::StartPosition => {
            let row = starts?.entries.get(resolved.start.max(0) as usize)?;
            (row.position, row.angle)
        }
        ArrivalSource::Portcrystal(slot) => (*slot_positions.get(slot as usize)?, 0.),
    };
    Some(ArrivalSeed {
        position: [position[0], position[1], position[2]],
        angle,
    })
}
