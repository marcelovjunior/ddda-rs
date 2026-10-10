use ddnpcrt::demo_reaction::{Inputs, Motion, Phase, Reaction, Request};

fn terminal() -> Inputs {
    Inputs {
        layer_flags: Some(1),
        ..Inputs::default()
    }
}
fn launched() -> Reaction {
    let (mut reaction, _) = Reaction::start(4).unwrap();
    reaction
        .step(Inputs {
            blow_adjustment: Some([12.0, 23.0, -4.0]),
            ..Inputs::default()
        })
        .unwrap();
    reaction
}
fn motion(number: u16, force: bool, flags: u32, interpolation: f32) -> Request {
    Request::Motion(Motion {
        number,
        force,
        flags,
        interpolation,
        start_frame: 0.0,
        rate: 1.0,
    })
}

#[test]
fn extra_preserves_native_entry_arguments_and_requires_real_terminal_flags() {
    let (mut reaction, entry) = Reaction::start(3).unwrap();
    assert_eq!(entry, [Request::DamageBaseEntry, motion(0x115, false, 0, 0.0)]);
    for flags in [None, Some(0), Some(2), Some(8)] {
        for _ in 0..1000 {
            assert!(reaction
                .step(Inputs {
                    layer_flags: flags,
                    ..Inputs::default()
                })
                .unwrap()
                .is_empty());
        }
        assert!(!reaction.done());
    }
    assert_eq!(
        reaction
            .step(Inputs {
                layer_flags: Some(4),
                ..Inputs::default()
            })
            .unwrap(),
        [Request::BodyFlags {
            mask: 0x10,
            value: false
        }]
    );
    assert!(reaction.done());
    assert!(!reaction.down_ready());
    assert!(reaction.interface_done(false));
}

#[test]
fn blow_requires_runtime_adjustments_and_actual_ground_exactly_one() {
    let (mut reaction, entry) = Reaction::start(4).unwrap();
    assert_eq!(entry, [Request::DamageBaseEntry, motion(0x102, true, 1, 0.0)]);
    assert!(reaction.step(Inputs::default()).is_err());
    assert_eq!(reaction.phase(), Phase::Launch);
    assert!(reaction
        .step(Inputs {
            blow_adjustment: Some([f32::NAN, 1.0, 2.0]),
            ..Inputs::default()
        })
        .is_err());
    assert_eq!(
        reaction
            .step(Inputs {
                blow_adjustment: Some([12.0, 23.0, -4.0]),
                ..Inputs::default()
            })
            .unwrap(),
        [
            Request::Launch {
                horizontal_speed: 12.0,
                vertical_speed: 23.0,
                vertical_acceleration: -4.0
            },
            Request::BodyFlags {
                mask: 1,
                value: false
            },
            Request::UpdateHorizontalVelocity
        ]
    );
    assert_eq!(reaction.phase(), Phase::InitialMotion);
    assert_eq!(
        reaction.step(terminal()).unwrap(),
        [motion(0x10A, true, 1, 0.0), Request::UpdateHorizontalVelocity]
    );
    for ground_byte in [None, Some(0), Some(2), Some(255)] {
        for _ in 0..1000 {
            assert_eq!(
                reaction
                    .step(Inputs {
                        ground_byte,
                        ..terminal()
                    })
                    .unwrap(),
                [Request::UpdateHorizontalVelocity]
            );
        }
        assert_eq!(reaction.phase(), Phase::Airborne);
        assert!(!reaction.interface_done(true));
    }
    assert_eq!(
        reaction
            .step(Inputs {
                ground_byte: Some(1),
                ..Inputs::default()
            })
            .unwrap(),
        [
            Request::StopVelocity,
            motion(0x10B, true, 0, 4.0),
            Request::BodyFlags { mask: 1, value: true },
            Request::ClearLandingByte,
            Request::UpdateHorizontalVelocity
        ]
    );
    assert_eq!(reaction.phase(), Phase::LandingMotion);
}

#[test]
fn blow_down_endpoint_and_recovery_keep_native_completion_bytes_distinct() {
    for down in [false, true] {
        let mut reaction = launched();
        reaction.step(terminal()).unwrap();
        reaction
            .step(Inputs {
                ground_byte: Some(1),
                ..Inputs::default()
            })
            .unwrap();
        assert!(reaction.step(terminal()).is_err(), "no assumed body Down default");
        assert_eq!(reaction.phase(), Phase::LandingMotion);
        let requests = reaction
            .step(Inputs {
                down: Some(down),
                ..terminal()
            })
            .unwrap();
        assert_eq!(requests.first(), Some(&Request::MarkLandingMotionFinished));
        assert!(!reaction.done());
        if down {
            assert_eq!(
                requests,
                [
                    Request::MarkLandingMotionFinished,
                    Request::UpdateHorizontalVelocity
                ]
            );
            assert_eq!(reaction.phase(), Phase::Down);
            assert!(reaction.down_ready());
            assert!(reaction.interface_done(true));
            assert!(!reaction.interface_done(false));
            assert_eq!(
                reaction.step(terminal()).unwrap(),
                [Request::UpdateHorizontalVelocity]
            );
            assert!(!reaction.done());
        } else {
            assert_eq!(
                requests,
                [
                    Request::MarkLandingMotionFinished,
                    motion(0x103, true, 0, 10.0),
                    Request::UpdateHorizontalVelocity
                ]
            );
            assert_eq!(reaction.phase(), Phase::RecoveryMotion);
            assert!(!reaction.down_ready());
            assert!(!reaction.interface_done(true));
            assert_eq!(
                reaction.step(terminal()).unwrap(),
                [
                    Request::BodyFlags {
                        mask: 0x10,
                        value: false
                    },
                    Request::UpdateHorizontalVelocity
                ]
            );
            assert!(reaction.done());
            assert!(reaction.down_ready());
            assert_eq!(reaction.phase(), Phase::Finished);
        }
    }
    assert!(
        Reaction::start(2).is_err(),
        "unsupported reactions cannot borrow another kind's completion"
    );
}
