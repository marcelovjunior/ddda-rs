//! Original-instruction differential vectors; no shard dependency.
use ddnative_base::{
    mem::{Mem, Sparse},
    x86::{Cpu, Entry},
};
use std::io::{Cursor, Read};

#[test]
fn inflate_registration_defaults_on_and_respects_explicit_override() {
    let mut ports = Vec::new();
    ddruntime::archive_inflate::install(&mut ports);
    let disabled = std::env::var("DDRUNTIME_ARCHIVE_INFLATE").as_deref() == Ok("0");
    let adler = std::env::var("DDRUNTIME_ZLIB_ADLER").as_deref() == Ok("1");
    let table = std::env::var("DDRUNTIME_ZLIB_TABLE").as_deref() == Ok("1");
    let outer = std::env::var("DDRUNTIME_ZLIB_PORT").as_deref() == Ok("1");
    let mut addresses: Vec<_> = ports.iter().map(|p| p.0).collect();
    addresses.sort_unstable();
    let mut expected = Vec::new();
    if outer {
        expected.push(0x00cf02e0);
    }
    if !disabled {
        expected.push(0x00cf2cb0);
    }
    if table {
        expected.push(0x00cf30c0);
    }
    if adler {
        expected.push(0x00cf34d0);
    }
    assert_eq!(addresses, expected);
    if !disabled {
        let fast = std::env::var("DDRUNTIME_ZLIB_FAST").as_deref() == Ok("1");
        let expected: ddnative_base::x86::LiftFn =
            if std::env::var("DDRUNTIME_ZLIB_FAST").as_deref() == Ok("2") {
                ddruntime::archive_inflate::inflate_fast_wide
            } else if fast {
                ddruntime::archive_inflate::inflate_fast_pages
            } else {
                ddruntime::archive_inflate::inflate_fast
            };
        assert!(std::ptr::fn_addr_eq(
            ports.iter().find(|p| p.0 == 0x00cf2cb0).unwrap().1,
            expected
        ));
    }
    fn explicit(_: &mut dyn Mem, c: &mut Cpu) {
        c.eax = 42;
    }
    let mut ports = vec![(0x00cf2cb0, explicit as ddnative_base::x86::LiftFn)];
    ddruntime::archive_inflate::install(&mut ports);
    assert_eq!(
        ports.len(),
        1 + usize::from(adler) + usize::from(outer) + usize::from(table)
    );
    let mut c = Cpu::default();
    ports[0].1(&mut Sparse::default(), &mut c);
    assert_eq!(c.eax, 42);
    let mut explicit_outer = vec![(0x00cf02e0, explicit as ddnative_base::x86::LiftFn)];
    ddruntime::archive_inflate::install(&mut explicit_outer);
    assert_eq!(explicit_outer.iter().filter(|p| p.0 == 0x00cf02e0).count(), 1);
    explicit_outer[0].1(&mut Sparse::default(), &mut c);
    assert_eq!(c.eax, 42);
    let mut explicit_table = vec![(0x00cf30c0, explicit as ddnative_base::x86::LiftFn)];
    ddruntime::archive_inflate::install(&mut explicit_table);
    assert_eq!(explicit_table.iter().filter(|p| p.0 == 0x00cf30c0).count(), 1);
    explicit_table.iter().find(|p| p.0 == 0x00cf30c0).unwrap().1(&mut Sparse::default(), &mut c);
    assert_eq!(c.eax, 42);
    // A user-supplied Adler port has the same precedence as the fast decoder.
    let mut explicit_ports = vec![(0x00cf34d0, explicit as ddnative_base::x86::LiftFn)];
    ddruntime::archive_inflate::install(&mut explicit_ports);
    explicit_ports.iter().find(|p| p.0 == 0x00cf34d0).unwrap().1(&mut Sparse::default(), &mut c);
    assert_eq!(c.eax, 42);
}

fn word(c: &mut Cursor<Vec<u8>>) -> u32 {
    let mut bytes = [0; 4];
    c.read_exact(&mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
fn machine(c: &mut Cursor<Vec<u8>>) -> ([u32; 8], [[u32; 4]; 8], Vec<(u32, Vec<u8>)>) {
    let regs = std::array::from_fn(|_| word(c));
    let xmm = std::array::from_fn(|_| std::array::from_fn(|_| word(c)));
    let windows = (0..word(c))
        .map(|_| {
            let addr = word(c);
            let len = word(c) as usize;
            let mut bytes = vec![0; len];
            c.read_exact(&mut bytes).unwrap();
            (addr, bytes)
        })
        .collect();
    (regs, xmm, windows)
}

#[test]
fn original_fast_inflate_registers_and_all_memory() {
    // Production parallel guest access policy, including unaligned fields.
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::var("L109_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L109/vectors/inflate_fast.bin".into());
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => {
            eprintln!("private original vectors absent: {path}");
            return;
        }
    };
    let mut c = Cursor::new(bytes);
    let mut magic = [0; 8];
    c.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"L109IF02");
    let count = word(&mut c);
    for index in 0..count {
        let (r, xmm, input) = machine(&mut c);
        let (expected, out_xmm, output) = machine(&mut c);
        let flags = word(&mut c);
        for implementation in 0..5 {
            let mut m: Box<dyn Mem> = if implementation == 2 || implementation == 4 {
                let mut mapped = ddruntime::memory::GuestMemory::new();
                let mut pages = std::collections::BTreeSet::new();
                for (address, bytes) in &input {
                    for page in (*address & !4095..address + bytes.len() as u32 + 4095 & !4095).step_by(4096)
                    {
                        if pages.insert(page) {
                            mapped
                                .map(page, 4096, ddruntime::memory::READ | ddruntime::memory::WRITE)
                                .unwrap();
                        }
                    }
                }
                Box::new(mapped)
            } else {
                Box::new(Sparse::default())
            };
            for (addr, bytes) in &input {
                for (k, b) in bytes.iter().copied().enumerate() {
                    m.set_u8(addr + k as u32, b);
                }
            }
            let mut cpu = Cpu::from(Entry {
                eax: r[0],
                ecx: r[1],
                edx: r[2],
                ebx: r[3],
                esp: r[4],
                ebp: r[5],
                esi: r[6],
                edi: r[7],
                xmm0: xmm[0][0],
            });
            cpu.xmm = xmm;
            if implementation == 0 {
                ddruntime::archive_inflate::inflate_fast(&mut *m, &mut cpu);
            } else if implementation >= 3 {
                ddruntime::archive_inflate::inflate_fast_wide(&mut *m, &mut cpu);
            } else {
                ddruntime::archive_inflate::inflate_fast_pages(&mut *m, &mut cpu);
            }
            assert_eq!(
                [
                    cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi
                ],
                expected,
                "case {index} registers"
            );
            assert_eq!(cpu.xmm, out_xmm, "case {index} xmm");
            assert_eq!(
                cpu.fl.snapshot().0 & 0x8d5,
                flags & 0x8d5,
                "case {index} arithmetic flags"
            );
            for (addr, bytes) in &output {
                for (k, b) in bytes.iter().copied().enumerate() {
                    assert_eq!(
                        m.u8(addr + k as u32),
                        b,
                        "case {index} memory {:#x}",
                        addr + k as u32
                    );
                }
            }
        }
    }
    assert_eq!(c.position() as usize, c.get_ref().len());
    eprintln!("{count} original fast inflate cases matched in five memory/decoder configurations");
}
