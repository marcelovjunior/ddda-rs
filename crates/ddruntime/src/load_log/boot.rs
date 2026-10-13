//! Sparse, read-only boot observations; requires LOAD_LOG and BOOT_DETAIL=1.
//! Addresses/offsets sourced from DDDA.exe 2017-12-13 Steam:
//! 00531800/00531A20 (GUI), 005325E0/0052FC30 (storage), 00530F90
//! (dialog timer), 0072A650/0072AED0 (GUI states). No game logic replaced.
use ddnative_base::mem::Mem;
use std::{cell::Cell, sync::OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Detail {
    Gui { pointer: u32, vtable: u32, state: u32, sequence_done: u8, result: u32, callback: u32, sequence: Option<u32>, choice: Option<[u32; 3]> },
    Storage { state: u32, dialog: u32, mode: u32, timer_positive: bool },
    Other,
}

fn snapshot(m: &dyn Mem, object: u32, callback: u32) -> (Detail, Option<u32>) {
    match callback {
        0x00531800 | 0x00531a20 if m.u8(object.wrapping_add(5)) == 1 => {
            let pointer = m.u32(object.wrapping_add(0x34));
            if pointer == 0 { return (Detail::Other, None); }
            let vtable = m.u32(pointer);
            // Only read fields of the two source-identified GUI classes.
            if !matches!(vtable, 0x0158a5a8 | 0x0158a758) { return (Detail::Other, None); }
            // 00E0A620 installs this rGUI sequence; 00412C90 returns it.
            let sequence = m.u32(pointer.wrapping_add(0x13c));
            (Detail::Gui {
                pointer, vtable,
                state: m.u32(pointer.wrapping_add(0x1d0)),
                sequence_done: m.u8(pointer.wrapping_add(0x19c)),
                result: m.u32(pointer.wrapping_add(0x1c8)),
                callback: if vtable == 0x0158a758 { m.u32(pointer.wrapping_add(0x1d8)) } else { 0x0072a650 },
                sequence: (sequence != 0).then(|| m.u32(sequence)),
                choice: (vtable == 0x0158a758).then(|| [0x284, 0x294, 0x298].map(|offset| m.u32(pointer.wrapping_add(offset)))),
            }, Some(m.u32(pointer.wrapping_add(0x134))))
        }
        0x005325e0 => {
            let storage = object.wrapping_add(0x58);
            let timer = m.u32(storage.wrapping_add(0x24));
            (Detail::Storage {
                state: m.u32(storage), dialog: m.u32(storage.wrapping_add(4)),
                mode: m.u32(storage.wrapping_add(0x20)),
                // Integer classification avoids changing host FP status for NaN.
                timer_positive: timer > 0 && timer <= 0x7f80_0000,
            }, Some(timer))
        }
        _ => (Detail::Other, None),
    }
}

pub(super) fn observe(m: &dyn Mem, object: u32) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if !super::enabled() || !*ENABLED.get_or_init(|| std::env::var("DDRUNTIME_BOOT_DETAIL").as_deref() == Ok("1")) {
        return;
    }
    let callback = m.u32(object.wrapping_add(0xf0));
    let (detail, timer) = snapshot(m, object, callback);
    thread_local! { static LAST: Cell<Option<(u32, u32, Detail)>> = const { Cell::new(None) }; }
    let key = Some((object, callback, detail));
    if LAST.with(|last| last.replace(key)) == key { return; }
    // Timestamp formatting runs with guest FP state live at this boundary.
    struct Restore(u32);
    impl Drop for Restore { fn drop(&mut self) { ddnative_base::x86::ldmxcsr(self.0); } }
    let _restore = Restore(ddnative_base::x86::stmxcsr());
    ddnative_base::x87::park();
    ddnative_base::x86::ldmxcsr(ddnative_base::x86::MXCSR_DEFAULT);
    // Deliberately not timeline::transition: preserve existing interval costs.
    // Timer bits are reported only when a substate or its sign changes.
    // 0072D5C0 consumes this region setting before title entry; it is not
    // itself the selected logo-list index (0/3 -> 0, 1 -> 1, otherwise 2).
    let region = m.u32(0x018750f0);
    let prefix = format_args!("object={object:#x} callback={callback:#010x} logo_region={region} boundary=pre_dispatch_not_display");
    match detail {
        Detail::Gui { pointer, vtable, state, sequence_done, result, callback, sequence, choice } =>
            super::event("boot_detail", format_args!("{prefix} kind=gui gui={pointer:#x} vtable={vtable:#x} gui_state={state} sequence_done={sequence_done} result={result} gui_callback={callback:#010x} sequence={sequence:?} choice={choice:?} sequence_time_bits={:#010x}", timer.unwrap())),
        Detail::Storage { state, dialog, mode, timer_positive } =>
            super::event("boot_detail", format_args!("{prefix} kind=storage storage_state={state:#x} dialog={dialog:#x} mode={mode} timer_positive={timer_positive} timer_bits={:#010x}", timer.unwrap())),
        Detail::Other => super::event("boot_detail", format_args!("{prefix} kind=other")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddnative_base::mem::Sparse;

    #[test]
    fn boot_observation_preserves_guest_mxcsr() {
        let mut m = Sparse::default();
        m.set_u8(0x4005, 0);
        m.set_u32(0x40f0, 0x00531800);
        m.set_u32(0x018750f0, 1);
        let original = ddnative_base::x86::stmxcsr();
        ddnative_base::x86::ldmxcsr(0x3fa1);
        observe(&m, 0x4000);
        let after = ddnative_base::x86::stmxcsr();
        ddnative_base::x86::ldmxcsr(original);
        assert_eq!(after, 0x3fa1);
    }

    #[test]
    fn storage_countdown_ticks_do_not_change_key_but_expiry_does() {
        let mut m = Sparse::default();
        let object = 0x1000;
        for offset in [0x58, 0x5c, 0x78] { m.set_u32(object + offset, 0); }
        m.set_u32(object + 0x7c, 120f32.to_bits());
        let first = snapshot(&m, object, 0x005325e0);
        m.set_u32(object + 0x7c, 119f32.to_bits());
        let second = snapshot(&m, object, 0x005325e0);
        assert_eq!(first.0, second.0);
        assert_ne!(first.1, second.1);
        m.set_u32(object + 0x7c, (-1f32).to_bits());
        assert_ne!(first.0, snapshot(&m, object, 0x005325e0).0);
    }

    #[test]
    fn gui_initialization_null_and_other_classes_are_not_dereferenced() {
        let mut m = Sparse::default();
        let object = 0x1000;
        m.set_u8(object + 5, 0);
        assert_eq!(snapshot(&m, object, 0x00531800).0, Detail::Other);
        m.set_u8(object + 5, 1);
        m.set_u32(object + 0x34, 0);
        assert_eq!(snapshot(&m, object, 0x00531800).0, Detail::Other);
        m.set_u32(object + 0x34, 0x2000);
        m.set_u32(0x2000, 123);
        assert_eq!(snapshot(&m, object, 0x00531800).0, Detail::Other);
        m.set_u32(0x2000, 0x0158a5a8);
        for offset in [0x134, 0x13c, 0x1c8] { m.set_u32(0x2000 + offset, 0); }
        m.set_u32(0x21d0, 2);
        m.set_u8(0x219c, 1);
        assert_eq!(snapshot(&m, object, 0x00531800).0, Detail::Gui {
            pointer: 0x2000, vtable: 0x0158a5a8, state: 2,
            sequence_done: 1, result: 0, callback: 0x0072a650,
            sequence: None, choice: None,
        });
        // Observations must not publish any guest state.
        assert_eq!((m.u32(0x21d0), m.u8(0x219c)), (2, 1));
    }

    #[test]
    fn menu_sequence_and_choice_changes_are_observable_without_tick_spam() {
        let mut m = Sparse::default();
        m.set_u8(0x1005, 1);
        m.set_u32(0x1034, 0x2000);
        m.set_u32(0x2000, 0x0158a758);
        for offset in [0x134, 0x1c8, 0x1d0, 0x1d8, 0x284, 0x294, 0x298] { m.set_u32(0x2000 + offset, 0); }
        m.set_u8(0x219c, 0);
        m.set_u32(0x213c, 0x3000);
        m.set_u32(0x3000, 31);
        let first = snapshot(&m, 0x1000, 0x00531a20);
        m.set_u32(0x2134, 1f32.to_bits());
        assert_eq!(first.0, snapshot(&m, 0x1000, 0x00531a20).0);
        m.set_u32(0x2284, 6);
        assert_ne!(first.0, snapshot(&m, 0x1000, 0x00531a20).0);
        m.set_u32(0x2284, 0);
        m.set_u32(0x3000, 36);
        assert_ne!(first.0, snapshot(&m, 0x1000, 0x00531a20).0);
    }
}
