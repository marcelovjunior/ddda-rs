//! Pure per-viewer Rift filtering and a seven-row visible window.
//! Source and limits: docs/RIFT_LIST.md. No hiring or world-state mutation.

/// None means the corresponding original bypass byte is set. Some with all
/// false is an active empty selection, not bypass. No UI defaults are assumed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filters {
    pub vocations: Option<[bool; 6]>,
    pub skill_categories: Option<[bool; 12]>,
    pub genders: Option<[bool; 2]>,
}

impl Filters {
    pub fn all_bypassed(&self) -> bool {
        self.vocations.is_none() && self.skill_categories.is_none() && self.genders.is_none()
    }
}

/// Explicit runtime cCmcInfo values: do not mistake the saved per-weapon skill
/// arrays for the six currently equipped skill ids at +0x270..+0x284.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PawnFields {
    pub vocation: u32,
    pub gender: u32,
    pub equipped_skills: [i32; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListRow {
    /// Original state uses equality, not bitmask membership.
    pub state: u32,
    pub pawn: Option<PawnFields>,
}

/// Caller resolves these from the installed exe: skill record +4, then its
/// text index at +0x1c to text record +0xa. A missing text index yields None.
/// ddskill::catalogue::ExeTables already reads both tables at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillCategory {
    pub action: u32,
    pub text_flags: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    MissingPawn { raw_index: usize },
    InvalidVocation { raw_index: usize, vocation: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Pawn,
    Loading,
    LoadFailed,
    OfflineNetwork,
    /// Safe API extension for absent data; not a reconstructed engine state.
    MissingData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRow {
    pub raw_index: usize,
    pub kind: RowKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub excluded: Vec<bool>,
    pub visible_count: usize,
    pub rows: Vec<VisibleRow>,
}

/// Menu state belongs to the viewing player, not to a shared pawn pool.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Viewer {
    pub filters: Filters,
    /// Offset in the filtered sequence (not a raw index). Past-end is empty.
    /// Cursor/wrap animation policy is deliberately left to the caller.
    pub first_visible: usize,
    pub online: bool,
}

/// Translated from 0x006d4c50 (DDDA.exe 2017-12-13 Steam).
fn excluded(
    pawn: PawnFields,
    filters: &Filters,
    skill: &impl Fn(i32) -> Option<SkillCategory>,
    raw_index: usize,
) -> Result<bool, Error> {
    if let Some(selected) = filters.vocations {
        let index = match pawn.vocation {
            1..=6 => pawn.vocation - 1,
            7..=9 => pawn.vocation - 4,
            vocation => return Err(Error::InvalidVocation { raw_index, vocation }),
        };
        if !selected[index as usize] {
            return Ok(true);
        }
    }
    if let Some(selected) = filters.skill_categories {
        // The source shifts right and inserts bit11 twelve times: selection
        // i finishes in bit i, including the first and last choices.
        let mask = selected
            .iter()
            .enumerate()
            .fold(0u16, |m, (i, &s)| m | (u16::from(s) << i));
        let any = pawn.equipped_skills.iter().any(|&id| {
            // Original checks -1 and <430; reject other negative ids safely.
            if !(0..430).contains(&id) {
                return false;
            }
            skill(id).is_some_and(|s| {
                s.action != u32::MAX && s.text_flags.is_some_and(|f| (f & 0x3fff & mask) != 0)
            })
        });
        if !any {
            return Ok(true);
        }
    }
    if let Some(selected) = filters.genders {
        if !selected[usize::from(pawn.gender != 0)] {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Exclusion/count translated from 0x006d4bb0; row skipping, seven-row limit
/// and placeholder state equality from 0x006d5090 (DDDA.exe 2017-12-13 Steam).
/// Filtered ordinal offset is this API's explicit scrolling contract, not a
/// translation of the original cursor controller. Input ordering is retained.
pub fn project(
    source: &[ListRow],
    viewer: &Viewer,
    skill: impl Fn(i32) -> Option<SkillCategory>,
) -> Result<Projection, Error> {
    let mut exclusions = Vec::with_capacity(source.len());
    for (raw_index, row) in source.iter().enumerate() {
        exclusions.push(if row.state == 0x8000 {
            !viewer.filters.all_bypassed()
        } else if viewer.filters.all_bypassed() {
            false
        } else {
            excluded(
                row.pawn.ok_or(Error::MissingPawn { raw_index })?,
                &viewer.filters,
                &skill,
                raw_index,
            )?
        });
    }
    let visible_count = exclusions.iter().filter(|&&e| !e).count();
    let rows = source
        .iter()
        .enumerate()
        .filter(|(i, _)| !exclusions[*i])
        .skip(viewer.first_visible)
        .take(7)
        .map(|(raw_index, row)| VisibleRow {
            raw_index,
            kind: match row.state {
                0x1000 => RowKind::Loading,
                0x8000 => RowKind::LoadFailed,
                0x2000 if !viewer.online => RowKind::OfflineNetwork,
                _ if row.pawn.is_none() => RowKind::MissingData,
                _ => RowKind::Pawn,
            },
        })
        .collect();
    Ok(Projection {
        excluded: exclusions,
        visible_count,
        rows,
    })
}
