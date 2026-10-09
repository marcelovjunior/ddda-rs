use ddstory::encounter::{EncounterFacts, GroupFacts, UnitFacts};
use ddstory::{Host, NpcIntel, Target};

const ENEMY: Target = Target {
    kind: 2,
    group: 10,
    index: 0,
};

#[test]
fn enemy_checks_distinguish_unloaded_killed_and_zero_hp() {
    let mut host = EncounterFacts::default();
    for flag in 0..4 {
        assert!(!host.enemy_check(380, &ENEMY, flag));
    }
    assert!(host.enemy_check(380, &ENEMY, 0x1000));
    let mut unit = UnitFacts {
        layout_loaded: true,
        present: true,
        hp: Some(80.0),
        max_hp: Some(100.0),
        ..Default::default()
    };
    host.report_unit(380, ENEMY, unit);
    assert!(host.enemy_check(380, &ENEMY, 0));
    assert!(host.enemy_check(380, &ENEMY, 2));
    assert!(!host.enemy_check(380, &ENEMY, 1));
    assert_eq!(host.unit_hp(380, &ENEMY, true), Some(80.0));
    unit.hp = Some(0.0);
    host.report_unit(380, ENEMY, unit);
    assert!(host.enemy_check(380, &ENEMY, 3));
    assert!(
        !host.enemy_check(380, &ENEMY, 1),
        "zero HP is not a persistent kill"
    );
    unit.max_hp = Some(0.0);
    host.report_unit(380, ENEMY, unit);
    assert!(!host.enemy_check(380, &ENEMY, 3));
    unit = UnitFacts {
        killed: true,
        ..Default::default()
    };
    host.report_unit(380, ENEMY, unit);
    assert!(host.enemy_check(380, &ENEMY, 1));
    assert!(host.enemy_check(380, &ENEMY, 3));
    assert!(!host.enemy_check(380, &ENEMY, 0));
    assert!(
        !host.enemy_check(200, &ENEMY, 1),
        "stages never share a placement's death"
    );
    assert_eq!(host.unit_hp(380, &ENEMY, false), None);
}

#[test]
fn script_completion_does_not_follow_elapsed_time_or_unloaded_npcs() {
    let mut host = EncounterFacts::default();
    let npc = Target {
        kind: 5,
        group: 4,
        index: 0,
    };
    assert!(!host.applied_script_finished(100, &npc));
    host.report_unit(
        100,
        npc,
        UnitFacts {
            npc: Some(NpcIntel {
                id: 304,
                hp_positive: true,
            }),
            ..Default::default()
        },
    );
    assert_eq!(host.npc_intel(100, &npc).unwrap().id, 304);
    assert!(!host.npc_unit(304), "intel and active units are separate");
    for _ in 0..10_000 {
        assert!(!host.applied_script_finished(100, &npc));
    }
    host.report_unit(
        100,
        npc,
        UnitFacts {
            present: true,
            applied_script_finished: true,
            ..Default::default()
        },
    );
    assert!(host.applied_script_finished(100, &npc));
    assert!(host.applied_script_finished(
        380,
        &Target {
            kind: 1,
            group: 1,
            index: 0
        }
    ));
    assert!(!host.applied_script_finished(380, &ENEMY));
    host.report_unit(
        380,
        ENEMY,
        UnitFacts {
            killed: true,
            ..Default::default()
        },
    );
    assert!(host.applied_script_finished(380, &ENEMY));
}

#[test]
fn group_completion_requires_an_explicit_stage_scoped_report() {
    let mut host = EncounterFacts::default();
    assert!(!host.enemy_group_done(380, 10));
    host.report_group(
        380,
        2,
        10,
        GroupFacts {
            loaded: true,
            active: true,
            done: false,
        },
    );
    host.report_unit(
        380,
        ENEMY,
        UnitFacts {
            killed: true,
            ..Default::default()
        },
    );
    assert!(!host.enemy_group_done(380, 10));
    assert!(host.enemy_group_active(380, 10));
    assert!(!host.enemy_group_active(200, 10));
}
