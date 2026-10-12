//! Instruction boundary for resumable guest code (L27b).
//!
//! This is a producer contract, not an adapter for existing whole-function
//! lifts. Each callback executes ONE restartable instruction (one iteration
//! for REP), and returns its actual successor. CALL pushes a guest return
//! address; RET reads it and returns `Flow::Return`. No Rust caller stack is
//! retained. Generated instruction resolvers use the same boundary.
//!
//! Instructions must not call services, allocate, change mappings, release
//! the guest lock, or publish side effects outside Mem/Cpu. Such effects
//! need separate executor boundaries. GuestMemory access remains serialized
//! by the runtime guest lock, including preflight and commit.
use crate::{memory::Fault, seh};
use ddnative_base::{mem::Mem, x86::Cpu};
use std::{
    collections::BTreeMap,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
};

pub use ddnative_base::execution::{divide, Flow};
pub type Instruction = fn(&mut dyn Mem, &mut Cpu) -> Flow;

#[derive(Debug)]
struct Access(Fault);

/// A write buffer for one restartable instruction. Reads see earlier writes
/// of that instruction, but no write becomes visible before it succeeds.
struct Transaction<'a> {
    memory: &'a mut dyn Mem,
    writes: BTreeMap<u32, u8>,
}
impl Transaction<'_> {
    fn check(&self, addr: u32, width: u32, write: bool) {
        if let Err(bad) = self.memory.probe(addr, width, write) {
            std::panic::panic_any(Access(Fault {
                addr: bad,
                width,
                write,
                reason: "instruction access",
            }));
        }
    }
    fn load(&self, addr: u32, width: u32) -> u32 {
        self.check(addr, width, false);
        (0..width).fold(0, |v, k| {
            let a = addr.wrapping_add(k);
            let b = self.writes.get(&a).copied().unwrap_or_else(|| self.memory.u8(a));
            v | ((b as u32) << (8 * k))
        })
    }
    fn store(&mut self, addr: u32, width: u32, value: u32) {
        self.check(addr, width, true);
        for k in 0..width {
            self.writes.insert(addr.wrapping_add(k), (value >> (8 * k)) as u8);
        }
    }
    fn commit(self) {
        // Permissions cannot change during an instruction. All bytes were
        // checked before staging; commit cannot expose half a faulting store.
        for (a, v) in self.writes {
            self.memory.set_u8(a, v);
        }
    }
}
impl Mem for Transaction<'_> {
    fn u8(&self, a: u32) -> u8 {
        self.load(a, 1) as u8
    }
    fn u16(&self, a: u32) -> u16 {
        self.load(a, 2) as u16
    }
    fn u32(&self, a: u32) -> u32 {
        self.load(a, 4)
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.store(a, 1, v as u32);
    }
    fn set_u16(&mut self, a: u32, v: u16) {
        self.store(a, 2, v as u32);
    }
    fn set_u32(&mut self, a: u32, v: u32) {
        self.store(a, 4, v);
    }
}

/// Execute exactly one producer-supplied instruction. A guest trap restores
/// all Cpu state and discards pending writes. Arbitrary Rust panics propagate
/// as host failures: panic text is never interpreted as an access violation.
pub fn instruction(
    memory: &mut dyn Mem,
    cpu: &mut Cpu,
    ip: u32,
    step: impl FnOnce(&mut dyn Mem, &mut Cpu) -> Flow,
) -> Result<Flow, seh::Exception> {
    let before = cpu.clone();
    let mut tx = Transaction {
        memory,
        writes: BTreeMap::new(),
    };
    match catch_unwind(AssertUnwindSafe(|| step(&mut tx, cpu))) {
        Ok(flow) => {
            tx.commit();
            Ok(flow)
        }
        Err(payload) => {
            *cpu = before;
            if payload.is::<ddnative_base::execution::DivideError>() {
                return Err(seh::Exception::new(0xc0000094, ip));
            }
            match payload.downcast::<Access>() {
                Ok(trap) => Err(seh::Exception::access_violation(&trap.0, ip)),
                Err(other) => resume_unwind(other),
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    MissingInstruction { ip: u32 },
    Budget { ip: u32 },
    Exception(seh::Stop),
}

#[derive(Clone, Copy)]
struct Boundary {
    id: u64,
    slot: u32,
    target: u32,
}
thread_local! {
    static BOUNDARIES: std::cell::RefCell<Vec<Boundary>> = const { std::cell::RefCell::new(Vec::new()) };
    static SERIAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
struct Scope(u64);
impl Drop for Scope {
    fn drop(&mut self) {
        BOUNDARIES.with(|b| assert_eq!(b.borrow_mut().pop().unwrap().id, self.0));
    }
}
#[derive(Debug)]
struct Transfer {
    owner: u64,
    cpu: Cpu,
    ip: Option<u32>,
}

/// On the Windows downward-growing guest stack, restoring ESP past a
/// callback's return slot abandons that callback. This is a stack boundary,
/// never an address-specific exception-handler heuristic. The outermost
/// execution owns arbitrary stack pivots; a callback may retain its slot.
fn owner(esp: u32) -> Option<u64> {
    BOUNDARIES.with(|b| {
        let b = b.borrow();
        b.iter().rev().find(|b| esp <= b.slot).or(b.first()).map(|b| b.id)
    })
}

/// Transfer from a software exception API to a CONTEXT-selected instruction.
/// The typed unwind drops host callback/service frames before re-entry.
pub(crate) fn resume(cpu: Cpu, ip: u32) -> ! {
    match owner(cpu.esp) {
        Some(owner) => resume_unwind(Box::new(Transfer {
            owner,
            cpu,
            ip: Some(ip),
        })),
        None => std::panic::panic_any(seh::Stop::UnsupportedResume {
            address: ip,
            esp: cpu.esp,
        }),
    }
}

/// Registry/Session execution. Services run outside instruction transactions;
/// nested call_guest/SEH callbacks open distinct return-slot boundaries even
/// when they use the same sentinel address. No generated guest call nests Rust.
pub(crate) fn run(registry: &crate::dispatch::Registry, m: &mut dyn Mem, cpu: &mut Cpu, entry: u32) {
    use crate::dispatch::Binding;
    let id = SERIAL.with(|s| {
        let n = s.get().wrapping_add(1);
        s.set(n);
        n
    });
    let boundary = Boundary {
        id,
        slot: cpu.esp,
        target: m.u32(cpu.esp),
    };
    BOUNDARIES.with(|b| b.borrow_mut().push(boundary));
    let _scope = Scope(id);
    let mut ip = entry;
    loop {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let slot = cpu.esp;
            let flow = if let Some(Binding::Service(f)) = registry.get(ip) {
                let target = m.u32(slot);
                crate::threads::maybe_yield();
                f(m, cpu);
                Flow::Return(target)
            } else if let Some(step) = registry.instruction(ip) {
                match instruction(m, cpu, ip, |m, cpu| step(m, cpu, ip)) {
                    Ok(flow) => flow,
                    Err(e) => {
                        let next = seh::dispatch(m, cpu, &e, ip).unwrap_or_else(|e| std::panic::panic_any(e));
                        if owner(cpu.esp) != Some(id) {
                            resume(cpu.clone(), next);
                        }
                        return Some(next);
                    }
                }
            } else if let Some(Binding::Lifted(f)) = registry.get(ip) {
                // L47: steps exist only where execution can resume mid-body
                // (lift.resumable); any other function entered here runs as
                // its whole-function lift and returns like a call. Faults
                // inside it are not restartable instructions.
                let target = m.u32(slot);
                f(m, cpu);
                Flow::Return(target)
            } else {
                std::panic::panic_any(Stop::MissingInstruction { ip });
            };
            if let Flow::Return(target) | Flow::Jump(target) = flow {
                let returned = BOUNDARIES.with(|b| {
                    b.borrow()
                        .iter()
                        .rev()
                        .filter(|b| {
                            b.target == target
                                && match flow {
                                    Flow::Return(_) => b.slot == slot,
                                    // A naked epilogue may POP its return address and
                                    // JMP through a register instead of using RET.
                                    Flow::Jump(_) => cpu.esp > b.slot,
                                    _ => false,
                                }
                        })
                        .max_by_key(|b| b.slot)
                        .copied()
                });
                if let Some(b) = returned {
                    if b.id == id {
                        return None;
                    }
                    resume_unwind(Box::new(Transfer {
                        owner: b.id,
                        cpu: cpu.clone(),
                        ip: None,
                    }));
                }
                // Do not infer abandonment from transient ESP arithmetic
                // alone (e.g. POP/PUSH of a return address in an epilogue).
                if owner(cpu.esp) != Some(id) {
                    resume(cpu.clone(), target);
                }
            }
            let (Flow::Next(target) | Flow::Jump(target) | Flow::Return(target)) = flow;
            Some(target)
        }));
        match result {
            Ok(Some(next)) => ip = next,
            Ok(None) => return,
            Err(e) => match e.downcast::<Transfer>() {
                Ok(t) if t.owner == id => {
                    *cpu = t.cpu;
                    match t.ip {
                        Some(next) => ip = next,
                        None => return,
                    }
                }
                Ok(t) => resume_unwind(t),
                Err(e) => resume_unwind(e),
            },
        }
    }
}

/// Flat guest control flow. Interior entries are ordinary instruction keys;
/// an unknown key never falls back to calling a whole function entry. The
/// root's return token is explicit so guest RET targets are never discarded.
#[derive(Default)]
pub struct Executor {
    instructions: BTreeMap<u32, Instruction>,
}
impl Executor {
    pub fn insert(&mut self, ip: u32, step: Instruction) {
        assert!(
            !self.instructions.contains_key(&ip),
            "duplicate instruction {ip:#x}"
        );
        self.instructions.insert(ip, step);
    }
    pub fn run(
        &self,
        memory: &mut dyn Mem,
        cpu: &mut Cpu,
        entry: u32,
        return_token: u32,
        budget: usize,
    ) -> Result<(), Stop> {
        let mut ip = entry;
        for _ in 0..budget {
            let step = self
                .instructions
                .get(&ip)
                .copied()
                .ok_or(Stop::MissingInstruction { ip })?;
            match instruction(memory, cpu, ip, step) {
                Ok(Flow::Return(target)) if target == return_token => return Ok(()),
                Ok(Flow::Next(target) | Flow::Jump(target) | Flow::Return(target)) => ip = target,
                Err(exception) => {
                    // Cpu is the PRE-instruction checkpoint, including ESP
                    // before a failed PUSH/POP. SEH can select any known
                    // instruction and any register/stack state.
                    ip = seh::dispatch(memory, cpu, &exception, ip).map_err(Stop::Exception)?;
                }
            }
        }
        Err(Stop::Budget { ip })
    }
}
