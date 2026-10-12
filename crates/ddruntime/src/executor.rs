//! Instruction boundary for resumable guest code (L27b).
//!
//! This is a producer contract, not an adapter for existing whole-function
//! lifts. Each callback executes ONE restartable instruction (one iteration
//! for REP), and returns its actual successor. CALL pushes a guest return
//! address; RET reads it and returns `Flow::Return`. No Rust caller stack is
//! retained. The production lifter has not yet been wired to this contract.
//!
//! Instructions must not call services, allocate, change mappings, release
//! the guest lock, or publish side effects outside Mem/Cpu. Such effects
//! need separate executor boundaries. GuestMemory access remains serialized
//! by the runtime guest lock, including preflight and commit.
use crate::{
    memory::{Fault, GuestMemory},
    seh,
};
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};
use std::{
    collections::BTreeMap,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Fallthrough, branch, or CALL after the instruction pushed its return.
    Next(u32),
    /// RET after it read its guest target and performed stack cleanup.
    Return(u32),
}
pub type Instruction = fn(&mut dyn Mem, &mut Cpu) -> Flow;

#[derive(Debug)]
enum Trap {
    Access(Fault),
    Divide,
}

/// x86 #DE covers both zero divisor and quotient overflow. The helper leaves
/// output registers untouched on failure; the instruction boundary restores
/// the complete pre-instruction Cpu in either case.
pub fn divide(low: u32, high: u32, divisor: u32, bits: u32, signed: bool) -> (u32, u32) {
    x86::divide(low, high, divisor, bits, signed).unwrap_or_else(|| std::panic::panic_any(Trap::Divide))
}

/// A write buffer for one restartable instruction. Reads see earlier writes
/// of that instruction, but no write becomes visible before it succeeds.
struct Transaction<'a> {
    memory: &'a mut GuestMemory,
    writes: BTreeMap<u32, u8>,
}
impl Transaction<'_> {
    fn check(&self, addr: u32, width: u32, write: bool) {
        if self.memory.check(addr, width, write).is_err() {
            // The checked-memory report names the whole access's start.
            // SEH needs the inaccessible byte (CR2), including split pages.
            for k in 0..width {
                if let Err(mut f) = self.memory.check(addr.wrapping_add(k), 1, write) {
                    f.width = width;
                    std::panic::panic_any(Trap::Access(f));
                }
            }
            unreachable!("mapping changed during an instruction");
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
    memory: &mut GuestMemory,
    cpu: &mut Cpu,
    ip: u32,
    step: Instruction,
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
            match payload.downcast::<Trap>() {
                Ok(trap) => Err(match *trap {
                    Trap::Access(f) => seh::Exception::access_violation(&f, ip),
                    Trap::Divide => seh::Exception::new(0xc0000094, ip),
                }),
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
        memory: &mut GuestMemory,
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
                Ok(Flow::Next(target) | Flow::Return(target)) => ip = target,
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
