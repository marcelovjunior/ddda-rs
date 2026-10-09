use ddparam::{stamina::*, StaminaEntry};
fn row(stamina: f32, frame: f32) -> StaminaEntry {
    StaminaEntry {
        stamina,
        frame,
        motion_speed: 1.0,
    }
}
#[test]
fn continuous_clock_scaling_and_one_shot_are_distinct() {
    let timing = Timing {
        work_rate_0: 0.5,
        work_rate_1: 0.25,
        main_delta: 2.0,
    };
    assert_eq!(timing.delta(dash_rate(&row(3.0, 2.0), 2.0, false)), -0.75);
    assert_eq!(dash_jump_cost(&row(80.0, 20.0)), -80.0);
    assert_eq!(dash_rate(&row(3.0, 2.0), 2.0, true), 0.0);
}
#[test]
fn recorded_case_candidate_rates_are_not_universal_constants() {
    // Shipped RecoverNormal body row 1 / load row 5 and Dash row 1.
    // The CSV supports these effective rates, but does not record selected rows.
    let recovery = recovery_base(&row(1.625, 1.0), &row(0.0, 0.0));
    let drain = dash_rate(&row(1.5, 1.0), 1.0, false);
    assert_eq!(recovery, 1.625);
    assert_eq!(drain, -1.5);
    assert_eq!(recovery_base(&row(1.625, 1.0), &row(0.0, 0.4)), 1.625 / 1.4);
    let timing = Timing {
        work_rate_0: 1.0,
        work_rate_1: 1.0,
        main_delta: 0.25,
    };
    let mut recovered = 0.0;
    let mut spent = 0.0;
    for _ in 0..120 {
        recovered += timing.delta(recovery);
        spent += timing.delta(drain);
    }
    assert_eq!(recovered, 48.75);
    assert_eq!(spent, -45.0);
}
#[test]
fn recovery_gates_and_debt_override_follow_original_order() {
    let debt = RecoveryDebt {
        remaining: 4.0,
        initial: 8.0,
        override_rate: 0.25,
    };
    assert_eq!(recovery_rate(2.0, true, false, RecoveryDebt::default(), 1.0), 0.0);
    assert_eq!(recovery_rate(2.0, true, true, RecoveryDebt::default(), 1.5), 3.0);
    // Debt override wins even over the ordinary suppression gate.
    assert_eq!(recovery_rate(2.0, true, false, debt, 2.0), 0.5);
    assert_eq!(
        recovery_rate(
            2.0,
            false,
            false,
            RecoveryDebt {
                override_rate: 0.0,
                ..debt
            },
            2.0
        ),
        0.0
    );
}
#[test]
fn debt_uses_unit_delta_and_clears_all_fields_at_boundary() {
    let mut debt = RecoveryDebt {
        remaining: 1.0,
        initial: 3.0,
        override_rate: 0.5,
    };
    debt.advance(0.5, 0.5);
    assert_eq!(debt.remaining, 0.75);
    debt.advance(1.5, 0.5);
    assert_eq!(debt, RecoveryDebt::default());
    debt = RecoveryDebt {
        remaining: 0.1,
        initial: 3.0,
        override_rate: 0.5,
    };
    debt.advance(1.0, 0.5);
    assert_eq!(debt, RecoveryDebt::default());
}
#[test]
fn zero_debt_override_does_not_invent_a_countdown() {
    let mut debt = RecoveryDebt {
        remaining: 1.0,
        initial: 1.0,
        override_rate: 0.0,
    };
    let rate = recovery_rate(1.625, false, false, debt, 1.0);
    debt.advance(30.0, rate);
    assert_eq!(debt.remaining, 1.0);
}
#[test]
fn row_adjustment_truncates_and_only_lighter_status_clamps() {
    assert_eq!(locomotion_row(2, -0.9, false), Some(2));
    assert_eq!(locomotion_row(2, -1.9, false), Some(1));
    assert_eq!(locomotion_row(0, 0.0, true), Some(0));
    assert_eq!(locomotion_row(0, -1.0, false), None);
    assert_eq!(locomotion_row(1, f32::NAN, false), None);
}
#[test]
fn exhaustion_crossing_block_and_forced_recovery() {
    let mut state = StaminaState {
        current: 2.0,
        exhausted: false,
    };
    state.apply_resolved_delta(-2.0, 640.0, false);
    assert_eq!(
        state,
        StaminaState {
            current: 0.0,
            exhausted: true
        }
    );
    state.apply_resolved_delta(100.0, 640.0, false);
    assert_eq!(state.current, 0.0);
    state.apply_resolved_delta(100.0, 640.0, true);
    assert_eq!(state.current, 100.0);
    assert!(state.exhausted); // Tired action owns clearing, not this arithmetic tail.
    let mut empty = StaminaState {
        current: 0.0,
        exhausted: false,
    };
    empty.apply_resolved_delta(-1.0, 640.0, false);
    assert!(!empty.exhausted);
}
#[test]
fn cap_and_recovery_before_drain_order() {
    let mut state = StaminaState {
        current: 640.0,
        exhausted: false,
    };
    state.apply_resolved_delta(2.0, 640.0, false);
    state.apply_resolved_delta(-1.5, 640.0, false);
    assert_eq!(state.current, 638.5);
    let cap = status_cap(640.0, true);
    assert_eq!(cap, 384.0);
    state.apply_resolved_delta(0.0, cap, false);
    assert_eq!(state.current, 384.0);
    assert_eq!(status_cap(640.0, false), 640.0);
}
#[test]
fn actors_have_independent_exhaustion_and_debt() {
    let mut a = StaminaState {
        current: 1.0,
        exhausted: false,
    };
    let mut b = StaminaState {
        current: 10.0,
        exhausted: false,
    };
    a.apply_resolved_delta(-5.0, 100.0, false);
    b.apply_resolved_delta(5.0, 100.0, false);
    assert!(a.exhausted);
    assert_eq!(b.current, 15.0);
    assert!(!b.exhausted);
}
