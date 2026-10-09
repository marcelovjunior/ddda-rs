//! Synthetic checks of the status build-up (0x007772B0, 0x007781A0, 0x0076E340).
use ddcombat::status::*;

fn thresholds() -> [f32; 20] {
    // memory order of cCharParamEnemy +0x54..+0xA0 (index 10 = frozen, 11 = burning)
    let mut v = [1000.0f32; 20];
    v[10] = 800.0; // 耐氷漬け
    v[11] = 300.0; // 耐延焼
    v[8] = 10000.0; // 耐スキル封印: immune
    v
}

#[test]
fn torch_burns_in_one_hit_and_fire_replaces_ice() {
    let mut p = StatusPools::from_char_param(thresholds());
    assert_eq!((p.threshold[10], p.threshold[11]), (300.0, 800.0));
    let mut hit = StatusHit { element: 1, ..Default::default() };
    hit.element_buildup[1] = 1000.0;
    let fired = build_up(&mut p, &hit, &[1.0; 5], 1.0, 1.0, || 0.5);
    assert_eq!(fired, 1 << 10);
    assert_eq!(p.acc[10], 300.0);
    p.fired |= 1 << 11; // pretend frozen was pending
    build_up(&mut p, &hit, &[1.0; 5], 1.0, 1.0, || 0.5);
    assert_eq!(p.fired & (1 << 11), 0);
    let (slots, frozen) = fire(&mut p);
    assert_eq!((slots, frozen), (vec![10], false));
    assert_eq!(p.acc[10], 0.0);
}

#[test]
fn poison_accumulates_resist_scales_and_immune_slots_stay_empty() {
    let mut p = StatusPools::from_char_param(thresholds());
    let mut hit = StatusHit::default();
    hit.status[0] = 400.0; // mPoison
    hit.status[9] = 5000.0; // mSeal: slot 8 is immune
    for i in 0..2 {
        assert_eq!(build_up(&mut p, &hit, &[1.0; 5], 1.0, 1.0, || 0.0), 0, "hit {i}");
    }
    assert_eq!(p.acc[0], 800.0);
    assert_eq!(build_up(&mut p, &hit, &[1.0; 5], 1.0, 1.0, || 0.0), 1);
    assert_eq!(p.acc[0], 1000.0);
    assert_eq!(p.acc[8], 0.0);
    // a failed roll still accumulates but cannot fire
    let mut q = StatusPools::from_char_param(thresholds());
    q.chance[0] = 0.1;
    hit.status[0] = 2000.0;
    assert_eq!(build_up(&mut q, &hit, &[1.0; 5], 1.0, 1.0, || 0.5), 0);
    assert_eq!(q.acc[0], 2000.0);
}

#[test]
fn drenched_raises_burning_threshold() {
    let mut p = StatusPools::from_char_param(thresholds());
    p.region_bits = 1;
    let mut hit = StatusHit { element: 1, ..Default::default() };
    hit.element_buildup[1] = 1000.0;
    assert_eq!(build_up(&mut p, &hit, &[1.0; 5], 1.0, 1.0, || 0.0), 0); // 300 + 1000 > 1000
    assert_eq!(build_up(&mut p, &hit, &[1.0; 5], 1.0, 1.0, || 0.0), 1 << 10);
}

#[test]
fn decay_after_150_and_450_frames() {
    let mut p = StatusPools::default();
    p.acc[0] = 500.0;
    for _ in 0..150 {
        decay(&mut p, 1.0);
    }
    assert_eq!(p.acc[0], 500.0);
    decay(&mut p, 1.0);
    assert_eq!(p.acc[0], 499.5);
    for _ in 0..299 {
        decay(&mut p, 1.0);
    }
    assert_eq!(p.acc[0], 350.0);
    decay(&mut p, 1.0);
    assert_eq!(p.acc[0], 349.0);
}
