use ddnpc::{schedule_path, schedule_time, Ledger, MessageSerial, MessageSet};

fn header(magic: &[u8; 4], version: u32, count: u32) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend(version.to_le_bytes());
    out.extend(count.to_le_bytes());
    out
}
fn ledger() -> Vec<u8> {
    let mut d = header(b"nnl\0", 8, 1);
    for n in [11i16, 203, 555, 1, 33, -1, 3, 20] {
        d.extend(n.to_le_bytes());
    }
    d.extend([4, 1, 255, 2, 8, 1]);
    d.extend(b"Synthetic\0\xff\xfe\0");
    d
}
fn serial() -> Vec<u8> {
    let mut d = header(b"msl\0", 0x10, 3);
    for n in [0u16, 65535, 1000] {
        d.extend(n.to_le_bytes());
    }
    d
}
fn message_set() -> Vec<u8> {
    let mut d = header(b"mss\0", 0x20, 1);
    d.extend(71i32.to_le_bytes());
    d.extend((-2i16).to_le_bytes());
    for n in [5i32, 8, -1, 29, 31, 33] {
        d.extend(n.to_le_bytes());
    }
    for n in [-1i16, 11, 12, 13, 14, 15] {
        d.extend(n.to_le_bytes());
    }
    for n in [2i32, 4, -1, 88, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
        d.extend(n.to_le_bytes());
    }
    for n in 100i16..115 {
        d.extend(n.to_le_bytes());
    }
    d.extend((0..15).map(|n| (n - 1) as u8));
    for n in [-1i16, 99, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1] {
        d.extend(n.to_le_bytes());
    }
    for n in 200i16..215 {
        d.extend(n.to_le_bytes());
    }
    for n in 300i16..315 {
        d.extend(n.to_le_bytes());
    }
    d.extend([100, 255, 2]);
    for n in [-1i16, 40, 550, 750, 77] {
        d.extend(n.to_le_bytes());
    }
    assert_eq!(d.len(), 262);
    d
}

#[test]
fn ledger_variable_strings_signed_sentinels_and_field_order() {
    let d = ledger();
    let f = Ledger::parse(&d).unwrap();
    let n = &f.entries[0];
    assert_eq!((n.id, n.stage, n.initial_friendship), (11, 203, 555));
    assert_eq!(
        (n.model_type, n.model_index, n.stray_id, n.shop_type, n.voice_type),
        (1, 33, -1, 3, 20)
    );
    assert_eq!(
        (n.flag, n.child, n.tp, n.tp_type, n.like_item, n.civilian),
        (4, 1, -1, 2, 8, 1)
    );
    assert_eq!(n.name, b"Synthetic");
    assert_eq!(n.name_j, b"\xff\xfe");
}

#[test]
fn serial_preserves_unsigned_numbers() {
    assert_eq!(MessageSerial::parse(&serial()).unwrap().numbers, [0, 65535, 1000]);
}

#[test]
fn message_set_field_order_and_runtime_overwrite() {
    let f = MessageSet::parse(&message_set()).unwrap();
    let c = &f.conversations[0];
    assert_eq!((c.number, c.npc_id), (71, -2));
    assert_eq!(c.active_choices(), [5, 8]);
    assert_eq!(c.choices[3], 29);
    assert_eq!(c.choice_jumps, [-1, 11, 12, 13, 14, 15]);
    assert_eq!(c.active_messages(), [2, 4]);
    assert_eq!(c.motions, std::array::from_fn(|i| 100 + i as i16));
    assert_eq!(c.faces, std::array::from_fn(|i| i as i8 - 1));
    assert_eq!(c.quest_numbers, std::array::from_fn(|i| 200 + i as i16));
    assert_eq!(c.loaded_quest_numbers(), &std::array::from_fn(|i| 300 + i as i16));
    assert_eq!(
        (c.season, c.season_sequence, c.season_sequence_type),
        (100, -1, 2)
    );
    assert_eq!(
        (
            c.quest_number,
            c.quest_flag,
            c.friendship,
            c.friendship_max,
            c.global_flag
        ),
        (-1, 40, 550, 750, 77)
    );
    // Serialized quest conditions are present but never consulted after load.
    assert_eq!(
        f.loaded_message_index(71, 0, |_| panic!("no GSF condition")),
        Some(2)
    );
    assert_eq!(
        f.loaded_message_index(71, 1, |flag| {
            assert_eq!(flag, 99);
            false
        }),
        None
    );
    assert_eq!(f.loaded_message_index(71, 1, |flag| flag == 99), Some(4));
    assert_eq!(f.loaded_message_index(71, 2, |_| true), None);
    assert_eq!(f.loaded_message_index(71, 15, |_| true), None);
    assert_eq!(f.loaded_message_index(999, 0, |_| true), None);
    // Accessor uses requested slot, not the derived active-prefix length.
    assert_eq!(f.loaded_message_index(71, 3, |_| true), Some(88));
    let mut duplicated = f.clone();
    let mut second = f.conversations[0].clone();
    second.messages[0] = 99;
    duplicated.conversations.push(second);
    assert_eq!(duplicated.loaded_message_index(71, 0, |_| true), Some(2));
}

#[test]
fn rejects_every_truncation_and_malformed_header() {
    for (kind, original) in [ledger(), serial(), message_set()].into_iter().enumerate() {
        let valid = |d: &[u8]| match kind {
            0 => Ledger::parse(d).is_ok(),
            1 => MessageSerial::parse(d).is_ok(),
            _ => MessageSet::parse(d).is_ok(),
        };
        for end in 0..original.len() {
            assert!(!valid(&original[..end]), "kind {kind} truncation {end}");
        }
        for offset in [0, 4, 8] {
            let mut d = original.clone();
            d[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(!valid(&d));
        }
        let mut d = original;
        d.push(0);
        assert!(!valid(&d));
    }
}

#[test]
fn empty_resources_are_valid() {
    assert!(Ledger::parse(&header(b"nnl\0", 8, 0)).unwrap().entries.is_empty());
    assert!(MessageSet::parse(&header(b"mss\0", 0x20, 0))
        .unwrap()
        .conversations
        .is_empty());
    assert!(MessageSerial::parse(&header(b"msl\0", 0x10, 0))
        .unwrap()
        .numbers
        .is_empty());
}

#[test]
fn schedule_selector_boundaries_and_gate() {
    for (stage, scenario, suffix) in [
        (200, 3599, "_00"),
        (200, 3600, ""),
        (200, 7799, ""),
        (200, 7800, "_01"),
        (240, 3599, ""),
        (240, 7799, ""),
        (240, 7800, "_01"),
        (100, 7800, ""),
    ] {
        assert_eq!(
            schedule_path(stage, scenario, 2, 1).unwrap(),
            format!("AI\\FSM\\Npc\\st{stage:03}\\st{stage:03}_n002{suffix}")
        );
    }
    for id in [-1, 900, i32::MAX] {
        assert!(schedule_path(200, 0, id, 1).is_none());
    }
    for enabled in [0, 2, 255] {
        assert!(schedule_path(200, 0, 2, enabled).is_none());
    }
    assert!(schedule_path(100, 0, 0, 1).is_some());
    assert!(schedule_path(100, 0, 899, 1).is_some());
}

#[test]
fn schedule_ten_minute_clock_boundaries() {
    for (hour, minute, bucket) in [
        (0, 0, 0),
        (5, 59, 35),
        (6, 0, 36),
        (9, 29, 56),
        (9, 30, 57),
        (11, 59, 71),
        (12, 0, 72),
        (18, 59, 113),
        (19, 0, 114),
        (23, 59, 143),
    ] {
        assert_eq!(schedule_time(hour, minute), Some(bucket));
    }
    assert_eq!(schedule_time(24, 0), None);
    assert_eq!(schedule_time(0, 60), None);
}
