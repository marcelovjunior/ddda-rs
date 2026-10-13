//! Complete original FE9580 replay. Private fixtures include synthetic edge inputs.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};
use ddruntime::{
    fastmem::write_bulk,
    memory::{GuestMemory, READ, WRITE},
};
use std::{
    cell::Cell,
    io::{Cursor, Read},
    rc::Rc,
};

type Windows = Vec<(u32, Vec<u8>)>;
struct State {
    cpu: Cpu,
    flags: u32,
    mxcsr: u32,
    stack: Vec<u8>,
}
struct Call {
    target: u32,
    before: State,
    after: State,
    writes: Windows,
}
struct RestoreMxcsr(u32);
impl Drop for RestoreMxcsr {
    fn drop(&mut self) {
        x86::ldmxcsr(self.0);
    }
}

fn word(reader: &mut Cursor<Vec<u8>>) -> u32 {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
fn blob(reader: &mut Cursor<Vec<u8>>) -> Vec<u8> {
    let mut bytes = vec![0; word(reader) as usize];
    reader.read_exact(&mut bytes).unwrap();
    bytes
}
fn state(reader: &mut Cursor<Vec<u8>>) -> State {
    let registers: [u32; 8] = std::array::from_fn(|_| word(reader));
    let xmm = std::array::from_fn(|_| std::array::from_fn(|_| word(reader)));
    let flags = word(reader);
    let mxcsr = word(reader);
    State {
        cpu: Cpu {
            eax: registers[0],
            ecx: registers[1],
            edx: registers[2],
            ebx: registers[3],
            esp: registers[4],
            ebp: registers[5],
            esi: registers[6],
            edi: registers[7],
            xmm,
            fl: x86::Flags::new(x86::RAW, 0, 0, flags, 32),
            ..Cpu::default()
        },
        flags,
        mxcsr,
        stack: blob(reader),
    }
}
fn windows(reader: &mut Cursor<Vec<u8>>) -> Windows {
    (0..word(reader)).map(|_| (word(reader), blob(reader))).collect()
}
fn check_bytes(memory: &dyn Mem, address: u32, expected: &[u8], label: &str) {
    let mut actual = vec![0; expected.len()];
    memory.read_bytes(address, &mut actual);
    if actual != expected {
        let offset = actual.iter().zip(expected).position(|(a, b)| a != b).unwrap();
        panic!(
            "{label}: byte {:#x}: actual={:02x} expected={:02x}",
            address + offset as u32,
            actual[offset],
            expected[offset]
        );
    }
}
fn check(memory: &dyn Mem, cpu: &Cpu, expected: &State, stack: u32, label: &str) {
    let e = &expected.cpu;
    assert_eq!(
        [
            cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi
        ],
        [e.eax, e.ecx, e.edx, e.ebx, e.esp, e.ebp, e.esi, e.edi],
        "{label}: GPR"
    );
    assert_eq!(cpu.xmm, e.xmm, "{label}: XMM");
    let (flags, valid) = cpu.fl.snapshot();
    assert_eq!(
        flags & valid & 0x8c5,
        expected.flags & valid & 0x8c5,
        "{label}: flags"
    );
    assert_eq!(x86::stmxcsr(), expected.mxcsr, "{label}: MXCSR");
    check_bytes(memory, stack, &expected.stack, label);
    // vectors_geometry executes each original arithmetic/compare SSE
    // instruction on hardware; Unicorn alone is not an MXCSR/NaN oracle.
}
fn map(memory: &mut GuestMemory, address: u32, size: usize) {
    let first = address & !4095;
    let end = (address + size as u32 + 4095) & !4095;
    for page in (first..end).step_by(4096) {
        // Separate static windows may share a page.
        if !memory.is_mapped(page) {
            memory.map(page, 4096, READ | WRITE).unwrap();
        }
    }
}

#[test]
fn original_geometry_machine_and_memory() {
    let path = std::env::var("L109_GEOMETRY_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L109c/vectors/geometry.bin".into());
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(error) => {
            assert!(
                std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_none(),
                "required {path}: {error}"
            );
            eprintln!("skip private original geometry fixtures: {path}: {error}");
            return;
        }
    };
    ddnative_host::install();
    let mut reader = Cursor::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"L109GE01");
    let count = word(&mut reader);
    assert!(count >= 3);
    let mut total_calls = 0;
    for _ in 0..count {
        let seed = word(&mut reader);
        let stack_base = word(&mut reader);
        let before = state(&mut reader);
        let after = state(&mut reader);
        let initial = windows(&mut reader);
        let expected = windows(&mut reader);
        let calls: Vec<Call> = (0..word(&mut reader))
            .map(|_| Call {
                target: word(&mut reader),
                before: state(&mut reader),
                after: state(&mut reader),
                writes: windows(&mut reader),
            })
            .collect();
        let call_count = calls.len();
        total_calls += call_count;
        let mut memory = GuestMemory::new();
        map(&mut memory, stack_base, before.stack.len());
        for (address, bytes) in &initial {
            map(&mut memory, *address, bytes.len());
        }
        for (address, bytes) in &initial {
            write_bulk(&mut memory, *address, bytes);
        }
        write_bulk(&mut memory, stack_base, &before.stack);
        let cursor = Rc::new(Cell::new(0));
        let index = cursor.clone();
        let previous = x86::set_ext_calls(Some(Box::new(move |memory, cpu, target| {
            let k = index.get();
            let call = &calls[k];
            let label = format!("case {seed} call {k} target {target:08x}");
            assert_eq!(target, call.target, "{label}");
            check(memory, cpu, &call.before, stack_base, &label);
            for (address, bytes) in &call.writes {
                for (offset, byte) in bytes.iter().enumerate() {
                    memory.set_u8(*address + offset as u32, *byte);
                }
            }
            *cpu = call.after.cpu.clone();
            for (offset, byte) in call.after.stack.iter().enumerate() {
                memory.set_u8(stack_base + offset as u32, *byte);
            }
            index.set(k + 1);
        })));
        assert!(previous.is_none());
        let _restore_mxcsr = RestoreMxcsr(x86::stmxcsr());
        x86::ldmxcsr(before.mxcsr);
        let mut cpu = before.cpu;
        ddruntime::procedural_geometry::build_geometry(&mut memory, &mut cpu);
        x86::set_ext_calls(None);
        assert_eq!(cursor.get(), call_count);
        check(&memory, &cpu, &after, stack_base, &format!("case {seed} return"));
        for (address, bytes) in expected {
            check_bytes(&memory, address, &bytes, &format!("case {seed} memory"));
        }
    }
    assert_eq!(reader.position(), reader.get_ref().len() as u64);
    eprintln!("{count} original geometry cases, {total_calls} exact call boundaries");
}

#[test]
fn geometry_registration_is_opt_in_and_preserves_explicit_override() {
    fn explicit(_: &mut dyn Mem, _: &mut Cpu) {}
    let mut ports = Vec::new();
    ddruntime::procedural_geometry::install(&mut ports);
    assert_eq!(
        ports.len(),
        usize::from(std::env::var("DDRUNTIME_GEOMETRY_PORT").as_deref() == Ok("1"))
    );
    let mut ports = vec![(0x00fe9580, explicit as x86::LiftFn)];
    ddruntime::procedural_geometry::install(&mut ports);
    assert_eq!(ports.len(), 1);
    assert!(std::ptr::fn_addr_eq(ports[0].1, explicit as x86::LiftFn));
}
