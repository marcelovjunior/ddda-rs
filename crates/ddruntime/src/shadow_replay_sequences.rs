//! Deterministic recording-counter relocation for contiguous private lanes.
//! Header layout/increment: 00FEF830, 00FFC8E6; counter reset: 00FED200;
//! ascending lane merge: 00FED3B0 (DDDA.exe 2017-12-13 Steam).
//! This changes candidate bytes to the serial counter domain; it never masks
//! the comparator. Every local ordinal must be accounted for before any store.
use super::*;
use std::collections::BTreeSet;

pub(super) fn rebase(
    memory: &mut dyn Mem,
    parent: u32,
    lane_base: u32,
    participants: u32,
    initial_counter: u32,
    initial_roots: u32,
) -> Result<usize, String> {
    if participants == 0 || participants > 8 || memory.u32(parent + 0x74) != initial_counter {
        return Err("recording counter parent changed outside private lanes".into());
    }
    fn gather(
        m: &dyn Mem,
        entries: u32,
        count: u32,
        ordinals: &mut BTreeMap<u32, (u32, u32)>,
        visited: &mut BTreeSet<u32>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 128 || count > 32768 || entries.checked_add(count.saturating_mul(8)).is_none() {
            return Err("recording counter traversal bound".into());
        }
        for index in 0..count {
            let record = m.u32(entries + index * 8 + 4);
            if record == 0 || !visited.insert(record) {
                return Err("recording counter repeated/null record".into());
            }
            let header = m.u32(record);
            match (header >> 16) & 15 {
                0..=3 | 7 => {}
                6 => gather(
                    m,
                    m.u32(record + 8),
                    m.u32(record + 12),
                    ordinals,
                    visited,
                    depth + 1,
                )?,
                kind => {
                    return Err(format!(
                        "recording counter unsupported associated-record kind {kind}"
                    ))
                }
            }
            let ordinal = header & 0x7fff; // Producer 00FEF830 preserves all other bits.
            if ordinals.insert(ordinal, (record, header)).is_some() {
                return Err("recording counter duplicate ordinal".into());
            }
        }
        Ok(())
    }
    let mut patches = Vec::new();
    let mut counter = initial_counter;
    let mut roots = initial_roots;
    for lane in 0..participants {
        let context = lane_base + lane * LANE_BYTES;
        let count = memory.u32(context + 0x74);
        if count > 32768 {
            return Err("recording counter lane wraps its ordinal domain".into());
        }
        let lane_roots = memory.u32(context + 0x48);
        roots = roots
            .checked_add(lane_roots)
            .ok_or("recording root count overflow")?;
        let mut ordinals = BTreeMap::new();
        gather(
            memory,
            memory.u32(context + 0x44),
            lane_roots,
            &mut ordinals,
            &mut BTreeSet::new(),
            0,
        )?;
        if ordinals.len() != count as usize || ordinals.keys().copied().ne(0..count) {
            return Err("recording counter has missing local ordinals".into());
        }
        for (ordinal, (record, header)) in ordinals {
            patches.push((
                record,
                (header & !0x7fff) | (counter.wrapping_add(ordinal) & 0x7fff),
            ));
        }
        counter = counter.wrapping_add(count);
    }
    if memory.u32(parent + 0x48) != roots {
        return Err("recording roots changed outside private lanes".into());
    }
    for &(record, _) in &patches {
        memory
            .probe(record, 4, true)
            .map_err(|address| format!("recording header permission {address:#x}"))?;
    }
    memory
        .probe(parent + 0x74, 4, true)
        .map_err(|address| format!("recording counter permission {address:#x}"))?;
    for &(record, header) in &patches {
        memory.set_u32(record, header);
    }
    memory.set_u32(parent + 0x74, counter);
    Ok(patches.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GuestMemory, READ, WRITE};

    fn input() -> GuestMemory {
        let mut m = GuestMemory::new();
        m.map(0x1000, 0x1000, READ | WRITE).unwrap();
        m.map(0x4000, 0x68000, READ | WRITE).unwrap();
        m.map(0x100000, 0x2000, READ | WRITE).unwrap();
        m.set_u32(0x1074, 0x7ffe);
        m.set_u32(0x1048, 5); // Two existing roots, three appended roots.
        for (lane, roots) in [(0, 2), (1, 1)] {
            let context = 0x4000 + lane * LANE_BYTES;
            m.set_u32(context + 0x44, 0x101000 + lane * 0x20);
            m.set_u32(context + 0x48, roots);
            m.set_u32(context + 0x74, 2);
        }
        m.set_u32(0x101004, 0x100000);
        m.set_u32(0x10100c, 0x100020);
        m.set_u32(0x101024, 0x100060);
        m.set_u32(0x101044, 0x100040);
        m.set_u32(0x100000, 0xabc1_8000);
        m.set_u32(0x100020, 0x1231_0001);
        m.set_u32(0x100040, 0x9871_0000);
        m.set_u32(0x100060, 0x0096_0001);
        m.set_u32(0x100068, 0x101040);
        m.set_u32(0x10006c, 1);
        m
    }

    #[test]
    fn serial_counter_rebase_preserves_every_other_header_bit_and_nested_order() {
        let mut m = input();
        assert_eq!(rebase(&mut m, 0x1000, 0x4000, 2, 0x7ffe, 2), Ok(4));
        assert_eq!(m.u32(0x100000), 0xabc1_fffe);
        assert_eq!(m.u32(0x100020), 0x1231_7fff);
        assert_eq!(m.u32(0x100040), 0x9871_0000);
        assert_eq!(m.u32(0x100060), 0x0096_0001);
        assert_eq!(m.u32(0x1074), 0x8002);
    }

    #[test]
    fn invalid_counter_domains_refuse_before_changing_any_header() {
        for (address, value) in [
            (0x100020, 0x1231_0000), // Duplicate.
            (0x100020, 0x1231_0003), // Missing.
            (0x1048, 6),             // Unaccounted parent root.
            (0x1074, 0),             // Unaccounted parent counter change.
            (0x100060, 0x0094_0001), // Associated draw not proven.
            (0x4074, 32769),         // Ambiguous local wrap.
        ] {
            let mut m = input();
            m.set_u32(address, value);
            assert!(rebase(&mut m, 0x1000, 0x4000, 2, 0x7ffe, 2).is_err());
            assert_eq!(m.u32(0x100000), 0xabc1_8000);
        }
    }
}
