//! The readable ports behind the lifted program's calling convention (the
//! contract of `mtcore::abi`): each `f_XXXXXXXX` has a lifted function's
//! signature and replaces the lifted body at that address.

use ddnative_base::mem::Mem;
use ddnative_base::x86::{Cpu, LiftFn};

/// thiscall `void sRender::execute()` (0x00DAB480): ECX = sRender.
pub fn f_00dab480(m: &mut dyn Mem, cpu: &mut Cpu) {
    crate::executor::execute(m, cpu);
}

/// Every port: (address, adapter, registers checked by the differential
/// test: all eight).
pub const PORTS: &[(u32, LiftFn, u8)] = &[
    (0x00DA_B480, f_00dab480, 0xff),
    (0x00DA_6FE0, crate::apply::targets, 0xff),
    (0x00DA_A5F0, crate::apply::fixed, 0xff),
    (0x00DA_A7C0, crate::apply::streams, 0xff),
];
