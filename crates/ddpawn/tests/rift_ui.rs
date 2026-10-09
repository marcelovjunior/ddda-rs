use ddpawn::rift_ui::*;

fn row(vocation: u32, gender: u32, id: i32) -> ListRow {
    ListRow {
        state: 0x400,
        pawn: Some(PawnFields {
            vocation,
            gender,
            equipped_skills: [id, -1, -1, -1, -1, -1],
        }),
    }
}
fn category(id: i32) -> Option<SkillCategory> {
    match id {
        0 => Some(SkillCategory {
            action: 1,
            text_flags: Some(1),
        }),
        1 => Some(SkillCategory {
            action: 1,
            text_flags: Some(1 << 11),
        }),
        2 => Some(SkillCategory {
            action: u32::MAX,
            text_flags: Some(1),
        }),
        3 => Some(SkillCategory {
            action: 1,
            text_flags: None,
        }),
        4 => Some(SkillCategory {
            action: 1,
            text_flags: Some(0xc000),
        }),
        429 => Some(SkillCategory {
            action: 1,
            text_flags: Some(1),
        }),
        _ => None,
    }
}

#[test]
fn restrictions_and_choices_or_include_all_six_slots() {
    let mut selected = [false; 12];
    selected[0] = true;
    selected[11] = true;
    let viewer = Viewer {
        filters: Filters {
            vocations: Some([true, false, false, false, false, true]),
            skill_categories: Some(selected),
            genders: Some([true, false]),
        },
        ..Viewer::default()
    };
    let mut sixth = row(1, 0, -1);
    sixth.pawn.as_mut().unwrap().equipped_skills[5] = 1;
    let rows = [
        row(1, 0, 0),
        row(9, 0, 1),
        row(2, 0, 0),
        row(1, 1, 0),
        row(1, 0, -1),
        sixth,
    ];
    let p = project(&rows, &viewer, category).unwrap();
    assert_eq!(p.excluded, [false, false, true, true, true, false]);
    assert_eq!(p.rows.iter().map(|r| r.raw_index).collect::<Vec<_>>(), [0, 1, 5]);
}

#[test]
fn every_vocation_mapping_and_nonzero_gender() {
    for (job, expected) in [
        (1, 0),
        (2, 1),
        (3, 2),
        (4, 3),
        (5, 4),
        (6, 5),
        (7, 3),
        (8, 4),
        (9, 5),
    ] {
        for index in 0..6 {
            let mut picks = [false; 6];
            picks[index] = true;
            let viewer = Viewer {
                filters: Filters {
                    vocations: Some(picks),
                    genders: Some([false, true]),
                    ..Filters::default()
                },
                ..Viewer::default()
            };
            assert_eq!(
                project(&[row(job, 123, -1)], &viewer, category)
                    .unwrap()
                    .visible_count,
                usize::from(index == expected)
            );
        }
    }
}

#[test]
fn skill_bounds_invalid_action_and_no_text() {
    let viewer = Viewer {
        filters: Filters {
            skill_categories: Some([true; 12]),
            ..Filters::default()
        },
        ..Viewer::default()
    };
    let rows: Vec<_> = [-2, -1, 0, 2, 3, 4, 429, 430]
        .into_iter()
        .map(|i| row(1, 0, i))
        .collect();
    assert_eq!(
        project(&rows, &viewer, category).unwrap().excluded,
        [true, true, false, true, true, true, false, true]
    );
    let empty = Viewer {
        filters: Filters {
            skill_categories: Some([false; 12]),
            ..Filters::default()
        },
        ..Viewer::default()
    };
    assert_eq!(
        project(&[row(1, 0, 0)], &empty, category).unwrap().visible_count,
        0
    );
    for bit in 0..12 {
        let mut selected = [false; 12];
        selected[bit] = true;
        let viewer = Viewer {
            filters: Filters {
                skill_categories: Some(selected),
                ..Filters::default()
            },
            ..Viewer::default()
        };
        for flags_bit in 0..16 {
            assert_eq!(
                project(&[row(1, 0, 0)], &viewer, |_| Some(SkillCategory {
                    action: 0,
                    text_flags: Some(1 << flags_bit)
                }))
                .unwrap()
                .visible_count,
                usize::from(bit == flags_bit)
            );
        }
    }
}

#[test]
fn load_failure_bypass_combinations_and_state_equality() {
    let failed = ListRow {
        state: 0x8000,
        pawn: None,
    };
    for bypass_bits in 0..8 {
        let viewer = Viewer {
            filters: Filters {
                vocations: (bypass_bits & 1 == 0).then_some([true; 6]),
                skill_categories: (bypass_bits & 2 == 0).then_some([true; 12]),
                genders: (bypass_bits & 4 == 0).then_some([true; 2]),
            },
            ..Viewer::default()
        };
        assert_eq!(
            project(&[failed], &viewer, |_| panic!(
                "failed row must not resolve skills"
            ))
            .unwrap()
            .visible_count,
            usize::from(bypass_bits == 7)
        );
    }
    let states = [0x1000, 0x8000, 0x2000, 0xa000, 0x4000];
    let rows: Vec<_> = states
        .into_iter()
        .map(|state| ListRow {
            state,
            ..row(1, 0, 0)
        })
        .collect();
    let p = project(&rows, &Viewer::default(), category).unwrap();
    assert_eq!(
        p.rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [
            RowKind::Loading,
            RowKind::LoadFailed,
            RowKind::OfflineNetwork,
            RowKind::Pawn,
            RowKind::Pawn
        ]
    );
    assert_eq!(
        project(
            &rows,
            &Viewer {
                online: true,
                ..Viewer::default()
            },
            category
        )
        .unwrap()
        .rows[2]
            .kind,
        RowKind::Pawn
    );
}

#[test]
fn independent_viewers_filtered_pagination_and_empty_edges() {
    let rows: Vec<_> = (0..25).map(|i| row(1, i % 2, 0)).collect();
    let first = Viewer {
        filters: Filters {
            genders: Some([true, false]),
            ..Filters::default()
        },
        ..Viewer::default()
    };
    let second = Viewer {
        first_visible: 7,
        ..first.clone()
    };
    let a = project(&rows, &first, category).unwrap();
    let b = project(&rows, &second, category).unwrap();
    assert_eq!(a.visible_count, 13);
    assert_eq!(
        a.rows.iter().map(|r| r.raw_index).collect::<Vec<_>>(),
        [0, 2, 4, 6, 8, 10, 12]
    );
    assert_eq!(
        b.rows.iter().map(|r| r.raw_index).collect::<Vec<_>>(),
        [14, 16, 18, 20, 22, 24]
    );
    assert_eq!(project(&rows, &first, category).unwrap(), a);
    assert!(project(
        &rows,
        &Viewer {
            first_visible: usize::MAX,
            ..first
        },
        category
    )
    .unwrap()
    .rows
    .is_empty());
    assert!(project(&[], &Viewer::default(), category)
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn invalid_inputs_are_explicit_without_dereferencing_absent_data() {
    let absent = ListRow { state: 0, pawn: None };
    assert_eq!(
        project(&[absent], &Viewer::default(), category).unwrap().rows[0].kind,
        RowKind::MissingData
    );
    let viewer = Viewer {
        filters: Filters {
            vocations: Some([true; 6]),
            ..Filters::default()
        },
        ..Viewer::default()
    };
    assert_eq!(
        project(&[absent], &viewer, category),
        Err(Error::MissingPawn { raw_index: 0 })
    );
    for vocation in [0, 10, u32::MAX] {
        assert_eq!(
            project(&[row(vocation, 0, 0)], &viewer, category),
            Err(Error::InvalidVocation {
                raw_index: 0,
                vocation
            })
        );
    }
}
