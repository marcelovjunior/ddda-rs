//! L62: x87 state kept live in the host FPU gives the same results as
//! running each state alone, whatever else uses the FPU in between.
use ddnative_base::x87::{park, X87};

fn load(st: &mut X87, v: f32) {
    let mut d = [0u8; 10];
    d[..4].copy_from_slice(&v.to_bits().to_le_bytes());
    st.exec::<0x00d9>(&mut d); // fld m32real
}

fn store64(st: &mut X87) -> u64 {
    let mut d = [0u8; 10];
    st.exec::<0x18dd>(&mut d); // fstp m64real
    u64::from_le_bytes(d[..8].try_into().unwrap())
}

/// (a + b) / 3, (a + b) * pi with a 24-bit precision control word.
fn program(st: &mut X87, a: f32, b: f32, between: &mut dyn FnMut()) -> (u64, u64, u16) {
    let mut cw = [0u8; 10];
    cw[..2].copy_from_slice(&0x007fu16.to_le_bytes()); // single precision
    st.exec::<0x28d9>(&mut cw); // fldcw
    load(st, a);
    between();
    load(st, b);
    st.register::<0xc1de>(); // faddp
    between();
    st.register::<0xc0d9>(); // fld st0
    load(st, 3.0);
    between();
    st.register::<0xf9de>(); // fdivp
    let q = store64(st);
    st.register::<0xebd9>(); // fldpi
    st.register::<0xc9de>(); // fmulp
    between();
    let status = st.status();
    (q, store64(st), status)
}

#[test]
fn interleaved_states_equal_separate_runs() {
    ddnative_host::install();
    let alone_a = program(&mut X87::default(), 1.5, 2.25, &mut || {});
    let alone_b = program(&mut X87::default(), -7.0, 0.1, &mut || {});
    assert_ne!(alone_a, alone_b);

    // another state executes between every step: displaced and reloaded
    let mut other = X87::default();
    let mut k = 0.0f32;
    let mut a = X87::default();
    let got = program(&mut a, 1.5, 2.25, &mut || {
        k += 1.0;
        load(&mut other, k);
        store64(&mut other);
    });
    assert_eq!(got, alone_a);

    // parked between every step (a service call)
    assert_eq!(program(&mut X87::default(), -7.0, 0.1, &mut park), alone_b);
}

#[test]
fn snapshots_while_live_change_nothing() {
    ddnative_host::install();
    let mut st = X87::default();
    load(&mut st, 2.5);
    load(&mut st, 4.0);
    let copy = st.clone();
    assert_eq!(st.depth(), 2);
    assert_eq!(st.top_f32(), Some(4.0f32.to_bits()));
    assert_eq!(copy.top_f32(), Some(4.0f32.to_bits()));
    assert_eq!(st.control(), 0x037f);
    // the clone is independent of the live original
    let mut copy = copy;
    copy.register::<0xc1de>(); // faddp: 6.5
    assert_eq!(copy.top_f32(), Some(6.5f32.to_bits()));
    assert_eq!(st.depth(), 2);
    st.register::<0xc9de>(); // fmulp: 10
    assert_eq!(f64::from_bits(store64(&mut st)), 10.0);
    assert!(st.is_empty());
    assert_eq!(copy.depth(), 1);
}

#[test]
fn fnsave_frstor_round_trip_while_live() {
    ddnative_host::install();
    struct Buf([u8; 256]);
    impl ddnative_base::mem::Mem for Buf {
        fn u8(&self, a: u32) -> u8 { self.0[a as usize] }
        fn set_u8(&mut self, a: u32, v: u8) { self.0[a as usize] = v }
    }
    let mut m = Buf([0; 256]);
    let mut st = X87::default();
    load(&mut st, 3.0);
    load(&mut st, 0.75);
    st.fnsave(&mut m, 16, 0, 0, 0);
    assert!(st.is_empty());
    assert_eq!(st.depth(), 0);
    load(&mut st, 100.0); // live again, with a different value
    st.frstor(&m, 16);
    assert_eq!(st.depth(), 2);
    st.register::<0xc1de>(); // faddp
    assert_eq!(f64::from_bits(store64(&mut st)), 3.75);
}

#[test]
fn a_state_moved_to_another_thread_after_parking_is_refused() {
    ddnative_host::install();
    let mut st = X87::default();
    load(&mut st, 1.0);
    let r = std::thread::spawn(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| st.depth())).is_err()
    }).join().unwrap();
    assert!(r, "a live state read on another thread must stop, not read stale bytes");
}
