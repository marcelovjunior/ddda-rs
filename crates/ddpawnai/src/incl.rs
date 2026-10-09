//! Inclinations: the nine values of a pawn and which of them are active.
//!
//! The values sit in the pawn record (`cCmcInfo +0x14BC + k*0xC`, bound by
//! 0x0057EC40) and are clamped to 0..=1000 (0x0057EE00). 0x0057FC50 sorts
//! them and keeps at most two active entries with a status (1 or 2); the
//! priority tables and the follow distance test those entries.

/// Inclination indices, in the order of `personalityName_eng` and of the
/// elixir items 1520..=1528.
pub const SCATHER: u8 = 0;
pub const MEDICANT: u8 = 1;
pub const MITIGATOR: u8 = 2;
pub const CHALLENGER: u8 = 3;
pub const UTILITARIAN: u8 = 4;
pub const GUARDIAN: u8 = 5;
pub const NEXUS: u8 = 6;
pub const PIONEER: u8 = 7;
pub const ACQUISITOR: u8 = 8;

/// The primary threshold for status 2 (`0x017f4220`, 750.0).
pub const STRONG: f32 = 750.0;
/// The threshold for status 1 (600.0, immediate in 0x0057FC50).
pub const ACTIVE: f32 = 600.0;

/// An active inclination and its status (1, or 2 for a strong primary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Active {
    pub inclination: u8,
    pub status: u8,
}

/// The active inclinations of nine values (0x0057FC50): the highest value
/// gets status 2 from 750 and 1 from 600, the second gets 1 from 600; the
/// rest are inactive. Ties keep the lower index first (*assumed*: the
/// game's sort order on ties is not traced).
// Translated from 0x0057FC50 (DDDA.exe 2017-12-13 Steam).
pub fn active(values: &[f32; 9]) -> Vec<Active> {
    let mut order: Vec<u8> = (0..9).collect();
    order.sort_by(|&a, &b| values[b as usize].total_cmp(&values[a as usize]).then(a.cmp(&b)));
    let mut out = Vec::new();
    let top = values[order[0] as usize];
    if top >= STRONG {
        out.push(Active {
            inclination: order[0],
            status: 2,
        });
    } else if top >= ACTIVE {
        out.push(Active {
            inclination: order[0],
            status: 1,
        });
    } else {
        return out;
    }
    if values[order[1] as usize] >= ACTIVE {
        out.push(Active {
            inclination: order[1],
            status: 1,
        });
    }
    out
}

/// 0x0057EF40: does inclination `k` pass a check for `state`? It must be
/// one of the active entries; state 0 accepts any status, otherwise the
/// status must equal it.
// Translated from 0x0057EF40 (DDDA.exe 2017-12-13 Steam).
pub fn passes(active: &[Active], k: i64, state: i64) -> bool {
    active.iter().take(2).any(|a| {
        i64::from(a.inclination) == k
            && if state == 0 {
                a.status != 0
            } else {
                i64::from(a.status) == state
            }
    })
}

/// Has inclination `k` with status `status` (the follow-mode tests of
/// 0x00598390 compare entry +0 and +8).
pub fn has(active: &[Active], k: u8, status: u8) -> bool {
    active
        .iter()
        .take(2)
        .any(|a| a.inclination == k && a.status == status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        let mut v = [500.0; 9];
        assert!(active(&v).is_empty());
        v[GUARDIAN as usize] = 700.0;
        v[NEXUS as usize] = 660.0;
        assert_eq!(
            active(&v),
            [
                Active {
                    inclination: GUARDIAN,
                    status: 1
                },
                Active {
                    inclination: NEXUS,
                    status: 1
                }
            ]
        );
        v[GUARDIAN as usize] = 750.0;
        v[NEXUS as usize] = 599.0;
        assert_eq!(
            active(&v),
            [Active {
                inclination: GUARDIAN,
                status: 2
            }]
        );
        let a = active(&v);
        assert!(passes(&a, 5, 0) && passes(&a, 5, 2) && !passes(&a, 5, 1) && !passes(&a, 6, 0));
    }
}
