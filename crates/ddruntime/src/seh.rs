//! x86 Windows exception dispatch. ABI: Windows SDK winnt.h, CONTEXT (716
//! bytes), EXCEPTION_RECORD (80 bytes), EXCEPTION_DISPOSITION (excpt.h).
//! The Windows comparison program is research/runtime/seh_oracle.c.
//!
//! This module never guesses an instruction pointer from a stack scan.
//! Software raises have a known API return boundary; instruction faults
//! require an exact instruction checkpoint supplied by the executor.
use crate::services::{arg, ret};
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, Flags},
};

pub const RECORD_SIZE: u32 = 80;
pub const CONTEXT_SIZE: u32 = 716;
pub const NONCONTINUABLE: u32 = 1;
pub const UNWINDING: u32 = 2;
pub const EXIT_UNWIND: u32 = 4;
pub const STACK_INVALID: u32 = 8;
pub const NESTED_CALL: u32 = 16;
pub const END: u32 = u32::MAX;
const CALLBACK: u32 = 0xffff_ffd0;
const NESTED_GUARD: u32 = 0xffff_ffcc;
const UNWIND_GUARD: u32 = 0xffff_ffc8;
// Runtime-reserved TEB range, separate from image/heaps/stacks. Each guest
// thread gets its own page. These are runtime addresses, not game constants.
pub const TEB_BASE: u32 = 0x7e00_0000;

#[derive(Clone, Debug)]
pub struct Exception {
    pub code: u32,
    pub flags: u32,
    pub address: u32,
    pub nested: u32,
    pub parameters: Vec<u32>,
}

impl Exception {
    pub fn new(code: u32, address: u32) -> Self {
        Self {
            code,
            flags: 0,
            address,
            nested: 0,
            parameters: Vec::new(),
        }
    }
    /// Requires the instruction EIP from the executor, never a function
    /// entry inferred from a host backtrace. Windows AV parameter 0 is
    /// access kind (0 read / 1 write), parameter 1 is the fault address.
    pub fn access_violation(fault: &crate::memory::Fault, instruction: u32) -> Self {
        let mut e = Self::new(0xc0000005, instruction);
        e.parameters = vec![u32::from(fault.write), fault.addr];
        e
    }
    fn write(&self, m: &mut dyn Mem, p: u32) {
        assert!(self.parameters.len() <= 15);
        for k in (0..RECORD_SIZE).step_by(4) {
            m.set_u32(p + k, 0);
        }
        for (k, v) in [
            self.code,
            self.flags,
            self.nested,
            self.address,
            self.parameters.len() as u32,
        ]
        .into_iter()
        .enumerate()
        {
            m.set_u32(p + 4 * k as u32, v);
        }
        for (k, &v) in self.parameters.iter().enumerate() {
            m.set_u32(p + 20 + 4 * k as u32, v);
        }
    }
}

/// Precise unsupported states are failures, never a successful continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Unhandled {
        code: u32,
        address: u32,
        flags: u32,
        parameters: Vec<u32>,
    },
    InvalidStack {
        frame: u32,
        low: u32,
        high: u32,
    },
    InvalidDisposition {
        handler: u32,
        value: u32,
    },
    Noncontinuable {
        code: u32,
    },
    UnsupportedContext {
        reason: &'static str,
        address: u32,
    },
    UnavailableContextRead {
        context: u32,
        offset: u32,
    },
    UnsupportedResume {
        address: u32,
        esp: u32,
    },
    BadHandlerReturn {
        handler: u32,
        esp: u32,
    },
    UnsupportedUnwind {
        frame: u32,
        disposition: u32,
    },
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "guest SEH: {self:#x?}")
    }
}

/// ContextFlags advertises only supported groups. Floating/debug/segment
/// state is deliberately not advertised as valid zero-filled state.
pub fn write_context(m: &mut dyn Mem, p: u32, cpu: &Cpu, ip: u32) -> Result<(), Stop> {
    let (flags, valid) = cpu.fl.snapshot();
    if valid & x86::ARITH_FLAGS != x86::ARITH_FLAGS {
        return Err(Stop::UnsupportedContext {
            reason: "undefined arithmetic flags have no recorded hardware value",
            address: ip,
        });
    }
    for k in (0..CONTEXT_SIZE).step_by(4) {
        m.set_u32(p + k, 0);
    }
    m.set_u32(p, 0x10003); // CONTEXT_i386 | CONTROL | INTEGER
    for (off, v) in [
        (156, cpu.edi),
        (160, cpu.esi),
        (164, cpu.ebx),
        (168, cpu.edx),
        (172, cpu.ecx),
        (176, cpu.eax),
        (180, cpu.ebp),
        (184, ip),
        (188, 0x23),
        (192, flags | 0x202 | (u32::from(cpu.df) << 10)),
        (196, cpu.esp),
        (200, 0x2b),
    ] {
        m.set_u32(p + off, v);
    }
    Ok(())
}

pub fn read_context(m: &dyn Mem, p: u32, cpu: &mut Cpu) -> Result<u32, Stop> {
    let ip = m.u32(p + 184);
    if m.u32(p) != 0x10003 || m.u32(p + 188) != 0x23 || m.u32(p + 200) != 0x2b {
        return Err(Stop::UnsupportedContext {
            reason: "context groups or selectors changed",
            address: ip,
        });
    }
    cpu.edi = m.u32(p + 156);
    cpu.esi = m.u32(p + 160);
    cpu.ebx = m.u32(p + 164);
    cpu.edx = m.u32(p + 168);
    cpu.ecx = m.u32(p + 172);
    cpu.eax = m.u32(p + 176);
    cpu.ebp = m.u32(p + 180);
    cpu.esp = m.u32(p + 196);
    let flags = m.u32(p + 192);
    if flags & !(x86::ARITH_FLAGS | 0x602) != 0 || flags & 0x202 != 0x202 {
        return Err(Stop::UnsupportedContext {
            reason: "unsupported EFLAGS control change",
            address: ip,
        });
    }
    cpu.fl = Flags::new(x86::RAW, 0, 0, flags, 32);
    cpu.df = flags & 0x400 != 0;
    Ok(ip)
}

/// NT_TIB first seven words. The thread id selects a separate reserved page.
/// The one thread information block per guest thread, shared by SEH
/// dispatch and the exe's own FS users: ExceptionList +0x00 starts as the
/// end marker, StackBase +0x04, StackLimit +0x08, Self +0x18 (winnt.h).
/// The exe reads FS only at offset 0 (53 instructions, all SEH chain links:
/// `__SEH_prolog4`, `__EH_prolog3`, compiler-inlined frames; Ghidra search).
pub fn initialize_teb(m: &mut crate::memory::GuestMemory, tid: u32, low: u32, high: u32) -> u32 {
    let slot = tid
        .checked_sub(crate::services::MAIN_THREAD_ID)
        .expect("guest thread id")
        / 4;
    let p = TEB_BASE
        .checked_add(slot.checked_mul(0x1000).expect("TEB id overflow"))
        .expect("TEB overflow");
    assert!(
        p < crate::session::STACK_TOP - crate::session::STACK_SIZE,
        "TEB range exhausted"
    );
    m.map(p, 0x1000, crate::memory::READ | crate::memory::WRITE)
        .expect("unique TEB");
    m.set_u32(p, END);
    m.set_u32(p + 4, high);
    m.set_u32(p + 8, low);
    m.set_u32(p + 24, p);
    p
}

fn bounds(m: &dyn Mem, cpu: &Cpu) -> Result<(u32, u32), Stop> {
    if cpu.fs_base == 0 {
        return Err(Stop::UnsupportedContext {
            reason: "thread has no TEB",
            address: 0,
        });
    }
    let (low, high) = (m.u32(cpu.fs_base + 8), m.u32(cpu.fs_base + 4));
    if low >= high || cpu.esp < low || cpu.esp > high {
        return Err(Stop::InvalidStack {
            frame: cpu.esp,
            low,
            high,
        });
    }
    Ok((low, high))
}
fn valid_frame(frame: u32, low: u32, high: u32) -> Result<(), Stop> {
    if frame & 3 != 0 || frame < low || frame.checked_add(8).is_none_or(|p| p > high) {
        return Err(Stop::InvalidStack { frame, low, high });
    }
    Ok(())
}

/// Unadvertised context groups are unavailable, not synthetic zero state.
/// Keep the underlying memory's whole-access checks and allocation contract.
struct ContextMem<'a> {
    inner: &'a mut dyn Mem,
    context: u32,
}
impl ContextMem<'_> {
    fn check_read(&self, addr: u32, width: u32) {
        for k in 0..width {
            let offset = addr.wrapping_add(k).wrapping_sub(self.context);
            if offset < CONTEXT_SIZE && !(offset < 4 || (156..204).contains(&offset)) {
                std::panic::panic_any(Stop::UnavailableContextRead {
                    context: self.context,
                    offset,
                });
            }
        }
    }
}
impl Mem for ContextMem<'_> {
    fn u8(&self, a: u32) -> u8 {
        self.check_read(a, 1);
        self.inner.u8(a)
    }
    fn u16(&self, a: u32) -> u16 {
        self.check_read(a, 2);
        self.inner.u16(a)
    }
    fn u32(&self, a: u32) -> u32 {
        self.check_read(a, 4);
        self.inner.u32(a)
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.inner.set_u8(a, v)
    }
    fn set_u16(&mut self, a: u32, v: u16) {
        self.inner.set_u16(a, v)
    }
    fn set_u32(&mut self, a: u32, v: u32) {
        self.inner.set_u32(a, v)
    }
    fn alloc(&mut self, n: u32) -> u32 {
        self.inner.alloc(n)
    }
}

/// Windows handlers are cdecl with four arguments, not stdcall callbacks.
fn handler(m: &mut dyn Mem, cpu: &Cpu, stack: u32, target: u32, args: [u32; 4]) -> Result<u32, Stop> {
    let mut hc = cpu.clone();
    // Windows clears DF for handler execution but preserves the fault's DF
    // in CONTEXT (measured by seh_oracle's STD/divide case).
    hc.df = false;
    hc.esp = stack - 20;
    m.set_u32(hc.esp, CALLBACK);
    for (i, v) in args.into_iter().enumerate() {
        m.set_u32(hc.esp + 4 + i as u32 * 4, v);
    }
    x86::call_ext(
        &mut ContextMem {
            inner: m,
            context: args[2],
        },
        &mut hc,
        target,
    );
    if hc.esp != stack - 16 {
        return Err(Stop::BadHandlerReturn {
            handler: target,
            esp: hc.esp,
        });
    }
    Ok(hc.eax)
}

// The OS protects a handler invocation with a nested-dispatch registration.
// Model that registration explicitly, so guest handlers installed by the
// callback itself are searched BEFORE EXCEPTION_NESTED_CALL is introduced.
fn protected_handler(
    m: &mut dyn Mem,
    cpu: &Cpu,
    stack: u32,
    target: u32,
    args: [u32; 4],
    guard: u32,
) -> Result<u32, Stop> {
    let node = stack - 12;
    let old = m.u32(cpu.fs_base);
    m.set_u32(node, old);
    m.set_u32(node + 4, guard);
    m.set_u32(node + 8, args[1]);
    m.set_u32(cpu.fs_base, node);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handler(m, cpu, node, target, args)
    }));
    if m.u32(cpu.fs_base) == node {
        m.set_u32(cpu.fs_base, old);
    }
    match result {
        Ok(r) => r,
        Err(e) => std::panic::resume_unwind(e),
    }
}

/// Dispatch a known machine checkpoint. Returns the handler-selected EIP.
/// Caller owns transferring execution there; this routine never calls a
/// function entry with a fabricated call frame to emulate an interior jump.
pub fn dispatch(m: &mut dyn Mem, cpu: &mut Cpu, exception: &Exception, ip: u32) -> Result<u32, Stop> {
    let (low, high) = bounds(m, cpu)?;
    let scratch = cpu
        .esp
        .checked_sub(CONTEXT_SIZE + RECORD_SIZE + 64)
        .map(|p| p & !15)
        .ok_or(Stop::InvalidStack {
            frame: cpu.esp,
            low,
            high,
        })?;
    valid_frame(scratch.checked_sub(48).unwrap_or(0), low, high)?;
    let record = scratch;
    let context = record + RECORD_SIZE;
    let dispatcher = context + CONTEXT_SIZE;
    exception.write(m, record);
    write_context(m, context, cpu, ip)?;
    let mut nested_frame = None;
    let mut frame = m.u32(cpu.fs_base);
    let mut flags = exception.flags;
    while frame != END {
        valid_frame(frame, low, high)?;
        let next = m.u32(frame);
        let target = m.u32(frame + 4);
        m.set_u32(record + 4, flags);
        m.set_u32(dispatcher, 0);
        let disposition = if target == UNWIND_GUARD {
            return Err(Stop::UnsupportedUnwind {
                frame,
                disposition: 3,
            });
        } else if target == NESTED_GUARD {
            m.set_u32(dispatcher, m.u32(frame + 8));
            2
        } else {
            protected_handler(
                m,
                cpu,
                scratch,
                target,
                [record, frame, context, dispatcher],
                NESTED_GUARD,
            )?
        };
        flags |= m.u32(record + 4) & NONCONTINUABLE;
        match disposition {
            0 => {
                if flags & NONCONTINUABLE != 0 {
                    return Err(Stop::Noncontinuable { code: exception.code });
                }
                return read_context(m, context, cpu);
            }
            1 => (),
            2 => {
                let boundary = m.u32(dispatcher);
                valid_frame(boundary, low, high)?;
                flags |= NESTED_CALL;
                nested_frame = Some(nested_frame.map_or(boundary, |f: u32| f.max(boundary)));
            }
            value => {
                return Err(Stop::InvalidDisposition {
                    handler: target,
                    value,
                })
            }
        }
        if nested_frame == Some(frame) {
            flags &= !NESTED_CALL;
        }
        if next != END && next <= frame {
            return Err(Stop::InvalidStack {
                frame: next,
                low,
                high,
            });
        }
        frame = next;
    }
    let filter = crate::services::with_state(|s| s.exception_filter);
    if filter != 0 {
        // EXCEPTION_POINTERS follows the dispatcher scratch word. No state
        // borrow is held across the reentrant guest callback.
        let pointers = dispatcher + 4;
        m.set_u32(record + 4, flags);
        m.set_u32(pointers, record);
        m.set_u32(pointers + 4, context);
        let mut callback = cpu.clone();
        callback.df = false;
        callback.esp = scratch;
        let result = crate::services::call_guest(
            &mut ContextMem { inner: m, context },
            &mut callback,
            filter,
            &[pointers],
        );
        if result == u32::MAX {
            if flags & NONCONTINUABLE != 0 {
                return Err(Stop::Noncontinuable { code: exception.code });
            }
            return read_context(m, context, cpu);
        }
    }
    Err(Stop::Unhandled {
        code: exception.code,
        address: exception.address,
        flags,
        parameters: exception.parameters.clone(),
    })
}

pub fn raise_exception(m: &mut dyn Mem, cpu: &mut Cpu) {
    let mut e = Exception::new(arg(m, cpu, 0), m.u32(cpu.esp));
    e.flags = arg(m, cpu, 1) & NONCONTINUABLE;
    let (n, p) = (arg(m, cpu, 2).min(15), arg(m, cpu, 3));
    if p != 0 {
        e.parameters = (0..n).map(|i| m.u32(p + 4 * i)).collect();
    }
    raise_at_api(m, cpu, e, 16);
}

pub fn debug_break(m: &mut dyn Mem, cpu: &mut Cpu) {
    let ip = m.u32(cpu.esp);
    raise_at_api(m, cpu, Exception::new(0x80000003, ip), 0);
}

fn raise_at_api(m: &mut dyn Mem, cpu: &mut Cpu, e: Exception, argument_bytes: u32) {
    // Capture a caller-side software-raise boundary, after the stdcall ret.
    // Hardware faults must instead supply their actual instruction EIP.
    let mut continuation = cpu.clone();
    continuation.esp += 4 + argument_bytes;
    let expected_esp = continuation.esp;
    match dispatch(m, &mut continuation, &e, e.address) {
        Ok(ip) if ip == e.address && continuation.esp == expected_esp => *cpu = continuation,
        Ok(ip) => std::panic::panic_any(Stop::UnsupportedResume {
            address: ip,
            esp: continuation.esp,
        }),
        Err(e) => std::panic::panic_any(e),
    }
}

/// Measured on Windows x86: RtlUnwind removes younger registrations and
/// returns to its caller, setting EAX=ReturnValue. TargetIp is ignored on
/// this architecture (seh_oracle scenario 3); the CRT transfers afterward.
pub fn rtl_unwind(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (target, record, value) = (arg(m, cpu, 0), arg(m, cpu, 2), arg(m, cpu, 3));
    let result = (|| -> Result<(), Stop> {
        let (low, high) = bounds(m, cpu)?;
        if target != 0 {
            valid_frame(target, low, high)?;
        }
        let scratch = cpu
            .esp
            .checked_sub(CONTEXT_SIZE + RECORD_SIZE + 64)
            .map(|p| p & !15)
            .ok_or(Stop::InvalidStack {
                frame: cpu.esp,
                low,
                high,
            })?;
        valid_frame(scratch.saturating_sub(48), low, high)?;
        let rp = if record == 0 {
            Exception::new(0xc0000027, m.u32(cpu.esp)).write(m, scratch);
            scratch
        } else {
            record
        };
        let cp = scratch + RECORD_SIZE;
        let dp = cp + CONTEXT_SIZE;
        let mut context = cpu.clone();
        context.eax = value;
        context.esp += 20;
        let return_ip = m.u32(cpu.esp);
        write_context(m, cp, &context, return_ip)?;
        m.set_u32(
            rp + 4,
            m.u32(rp + 4) | UNWINDING | if target == 0 { EXIT_UNWIND } else { 0 },
        );
        let mut frame = m.u32(cpu.fs_base);
        while frame != END && frame != target {
            valid_frame(frame, low, high)?;
            if target != 0 && frame > target {
                return Err(Stop::InvalidStack {
                    frame: target,
                    low,
                    high,
                });
            }
            let h = m.u32(frame + 4);
            m.set_u32(dp, 0);
            let d = if h == UNWIND_GUARD {
                return Err(Stop::UnsupportedUnwind {
                    frame,
                    disposition: 3,
                });
            } else if h == NESTED_GUARD {
                1
            } else {
                protected_handler(m, cpu, scratch, h, [rp, frame, cp, dp], UNWIND_GUARD)?
            };
            if d == 3 {
                // The handler identifies the registration whose unwind
                // has already been completed. Windows resumes at its Next,
                // without calling skipped handlers again (oracle case 5).
                let collided = m.u32(dp);
                valid_frame(collided, low, high)?;
                if collided < frame || (target != 0 && collided >= target) {
                    return Err(Stop::UnsupportedUnwind {
                        frame: collided,
                        disposition: d,
                    });
                }
                frame = collided;
            } else if d != 1 {
                return Err(Stop::UnsupportedUnwind {
                    frame,
                    disposition: d,
                });
            }
            let next = m.u32(frame);
            if next != END && next <= frame {
                return Err(Stop::InvalidStack {
                    frame: next,
                    low,
                    high,
                });
            }
            m.set_u32(cpu.fs_base, next);
            frame = next;
        }
        if frame == END {
            return Err(Stop::UnsupportedUnwind {
                frame: target,
                disposition: END,
            });
        }
        let ip = read_context(m, cp, &mut context)?;
        if ip != return_ip || context.esp != cpu.esp + 20 {
            return Err(Stop::UnsupportedResume {
                address: ip,
                esp: context.esp,
            });
        }
        *cpu = context;
        Ok(())
    })();
    if let Err(e) = result {
        std::panic::panic_any(e);
    }
}

pub fn unhandled_exception_filter(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let filter = crate::services::with_state(|s| s.exception_filter);
    cpu.eax = if filter == 0 {
        1
    } else {
        crate::services::call_guest(m, cpu, filter, &[p])
    };
    ret(m, cpu, 4);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("KERNEL32.dll", "RtlUnwind", rtl_unwind),
    (
        "KERNEL32.dll",
        "UnhandledExceptionFilter",
        unhandled_exception_filter,
    ),
];
