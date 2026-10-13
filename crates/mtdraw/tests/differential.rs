//! The readable executor against the lifted one (crates/ddlift/lift_hot_00
//! `f_00dab480`, the 1:1 translation of 0x00DAB480): both run on the same
//! generated frames (command lists with every command type, nested lists,
//! merged draws, Clear colours that do and do not round trip, StretchRects
//! that coalesce at a list's end) with every callee mocked identically.
//! They must make the same calls (target, return address, all eight
//! registers, the stack arguments), and leave the same registers, xmm, x87
//! state and memory.
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu, LiftFn};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

// --- memory -----------------------------------------------------------------------

/// Guest memory for the test: unwritten bytes read as zero; writes kept.
#[derive(Clone, Default)]
struct TestMem {
    bytes: HashMap<u32, u8>,
    written: BTreeMap<u32, u8>,
}

impl Mem for TestMem {
    fn u8(&self, addr: u32) -> u8 {
        self.bytes.get(&addr).copied().unwrap_or(0)
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        self.bytes.insert(addr, v);
        self.written.insert(addr, v);
    }
}

// --- the layout of a generated frame ---------------------------------------------------

const RENDER: u32 = 0x0100_0000;
const DEVICE: u32 = 0x0200_0000;
const VTABLE: u32 = 0x0201_0000;
/// method tokens: VTABLE slot k -> METHODS + 4k
const METHODS: u32 = 0x7100_0000;
/// command records, lists and objects are allocated from here
const DATA: u32 = 0x0300_0000;
const STACK_TOP: u32 = 0x0080_0000;

const BEGIN_CONTEXT: u32 = 0x00DA_6DA0;
const END_CONTEXT: u32 = 0x00DA_6EA0;
const APPLY_TARGETS: u32 = 0x00DA_6FE0;
const APPLY_STATES: u32 = 0x00DA_A5F0;
const APPLY_STREAMS: u32 = 0x00DA_A7C0;
const APPLY_SHADERS: u32 = 0x00DA_A8B0;
const CHECK_RESULT: u32 = 0x00DA_89B0;
const RECT_EQUAL: u32 = 0x0067_FB40;
const SPAN_BEGIN: u32 = 0x0114_81B0;
const SPAN_END: u32 = 0x0114_81F0;

// --- the mocked callees -------------------------------------------------------------------

/// One call: target, return address, registers at the call, stack arguments.
#[derive(Debug, Clone, PartialEq)]
struct Call {
    target: u32,
    return_to: u32,
    regs: [u32; 8],
    args: Vec<u32>,
}

thread_local! {
    static TRACE: RefCell<Vec<Call>> = const { RefCell::new(Vec::new()) };
}

fn regs(cpu: &Cpu) -> [u32; 8] {
    [cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi]
}

/// The callee's stack arguments (count) and the bytes its `ret` pops.
fn arity(target: u32) -> (u32, u32) {
    match target {
        BEGIN_CONTEXT | END_CONTEXT | RECT_EQUAL => (1, 4),
        APPLY_TARGETS | APPLY_STATES | APPLY_STREAMS | APPLY_SHADERS | CHECK_RESULT => (2, 8),
        SPAN_BEGIN | SPAN_END => (0, 0),
        t if (METHODS..METHODS + 0x400).contains(&t) => {
            let n = match t - METHODS {
                0x88 => 6,  // StretchRect
                0xA4 | 0xA8 => 1,
                0xAC => 7,  // Clear
                0x144 => 4, // DrawPrimitive
                0x148 => 7, // DrawIndexedPrimitive
                0x190 => 5, // SetStreamSource
                0x198 => 3, // SetStreamSourceFreq
                o => panic!("unexpected device method +{o:#x}"),
            };
            (n, 4 * n)
        }
        t => panic!("unexpected call to {t:#010x}"),
    }
}

/// What a mocked callee does: record the call, change what the executor
/// reads back (the context, the targets), clobber EAX/ECX/EDX like a real
/// callee, and return.
fn mock(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
    if !(METHODS..METHODS + 0x400).contains(&target) && ![BEGIN_CONTEXT, END_CONTEXT, APPLY_TARGETS, APPLY_STATES, APPLY_STREAMS, APPLY_SHADERS, CHECK_RESULT, SPAN_BEGIN, SPAN_END].contains(&target) {
        panic!("unexpected call to {target:#010x} returning to {:#010x} (ecx {:#x} eax {:#x} edx {:#x} edi {:#x})", m.u32(cpu.esp), cpu.ecx, cpu.eax, cpu.edx, cpu.edi);
    }
    let (n, pop) = arity(target);
    let return_to = m.u32(cpu.esp);
    let args: Vec<u32> = (0..n).map(|k| m.u32(cpu.esp + 4 + 4 * k)).collect();
    let index = TRACE.with(|t| {
        let mut t = t.borrow_mut();
        t.push(Call { target, return_to, regs: regs(cpu), args: args.clone() });
        t.len() as u32
    });
    match target {
        BEGIN_CONTEXT => {
            // the original's: zero the context, the device, the root list
            let ctx = args[0];
            for k in 0..0xE28 {
                m.set_u8(ctx + k, 0);
            }
            m.set_u32(ctx + 4, m.u32(cpu.ecx + 0x98));
            let list = m.u32(cpu.ecx + 0x0043_3DD8);
            m.set_u32(ctx + 0x220, list);
            m.set_u32(ctx + 0x224, list + 8 * m.u32(cpu.ecx + 0x0043_3DE0));
        }
        APPLY_TARGETS => {
            // the four render targets the context then holds
            let (ctx, obj) = (args[0], args[1]);
            for k in 0..4 {
                m.set_u32(ctx + 0x88 + 4 * k, target_object(obj, k));
            }
        }
        APPLY_STREAMS => {
            // the vertex count of the bound streams
            m.set_u32(args[0] + 0xE20, 0x100 + (args[1] & 0xFF));
        }
        _ => {}
    }
    cpu.eax = match target {
        // D3D_OK mostly; a failed StretchRect now and then
        t if t == METHODS + 0x88 => if index % 3 == 0 { 0x8876_086C } else { 0 },
        t if t >= METHODS => 0,
        _ => 0x5150_0000 | index,
    };
    cpu.ecx = 0xC1C1_0000 ^ index;
    cpu.edx = 0xD2D2_0000 ^ index;
    cpu.esp += 4 + pop;
}

/// `bool equal(const RECT *)` (0x0067FB40), thiscall: ECX = this rectangle.
fn rect_equal(m: &mut dyn Mem, cpu: &mut Cpu) {
    let other = m.u32(cpu.esp + 4);
    TRACE.with(|t| t.borrow_mut().push(Call { target: RECT_EQUAL, return_to: m.u32(cpu.esp), regs: regs(cpu), args: vec![other] }));
    let equal = (0..4).all(|k| m.u32(cpu.ecx + 4 * k) == m.u32(other + 4 * k));
    cpu.eax = if equal { 0x0012_3401 } else { 0x0012_3400 };
    cpu.esp += 8;
}

fn lookup(addr: u32) -> Option<LiftFn> {
    (addr == RECT_EQUAL).then_some(rect_equal as LiftFn)
}

/// A target object of the targets object `obj`: +0x14 its handle.
fn target_object(obj: u32, k: u32) -> u32 {
    if obj == 0 { 0 } else { DATA + 0x00F0_0000 + (obj & 0xFFF) * 0x40 + k * 0x10 }
}

// --- generated frames -----------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
}

struct Frame {
    m: TestMem,
    at: u32,
    rng: Rng,
}

impl Frame {
    fn alloc(&mut self, size: u32) -> u32 {
        let p = self.at;
        self.at += size.next_multiple_of(16);
        p
    }

    /// A shader binding: its first word's bits 21..28 = the primitive code.
    fn shader(&mut self) -> u32 {
        let p = self.alloc(16);
        let code = self.rng.below(7);
        let v = (self.rng.next() & !(0xFF << 21)) | code << 21;
        self.m.set_u32(p, v);
        p
    }

    /// A stream table of an instanced draw: four streams (+0), divisor
    /// bytes (+0x10), offsets (+0x14), instance-data flags (+0x24).
    fn streams(&mut self) -> u32 {
        let p = self.alloc(0x30);
        for k in 0..4 {
            let stream = if self.rng.chance(60) {
                let s = self.alloc(0x30);
                self.m.set_u32(s + 0x20, self.rng.next());
                s
            } else {
                0
            };
            self.m.set_u32(p + 4 * k, stream);
            self.m.set_u8(p + 0x10 + k, self.rng.next() as u8);
            self.m.set_u32(p + 0x14 + 4 * k, self.rng.next() & 0xFFFF);
            self.m.set_u8(p + 0x24 + k, self.rng.chance(40) as u8);
        }
        p
    }

    /// A small object pointer (apply arguments): some shared, so identity
    /// matters.
    fn object(&mut self) -> u32 {
        DATA + 0x00E0_0000 + self.rng.below(6) * 0x40
    }

    fn draw_record(&mut self, kind: u32) -> u32 {
        let r = self.alloc(0x40);
        let half = (self.rng.next() as u16 & 0xFFF0) | kind as u16;
        self.m.set_u16(r + 2, half);
        let targets = self.object();
        self.m.set_u32(r + 4, targets);
        let states = self.object();
        self.m.set_u32(r + 8, states);
        let shader = self.shader();
        self.m.set_u32(r + 0xC, shader);
        let streams = if kind == 2 || kind == 3 { self.streams() } else { self.object() };
        self.m.set_u32(r + 0x10, streams);
        self.m.set_u32(r + 0x14, self.rng.below(600));
        self.m.set_u32(r + 0x18, self.rng.below(2000));
        self.m.set_u32(r + 0x1C, self.rng.below(300));
        self.m.set_u32(r + 0x20, self.rng.below(50));
        self.m.set_u32(r + 0x24, self.rng.below(50));
        r
    }

    /// Indexed draws over consecutive index ranges of one draw (merged when
    /// sRender+0x30 is set), sometimes broken.
    fn merged_run(&mut self, out: &mut Vec<u32>) {
        let first = self.draw_record(1);
        out.push(first);
        let (mut start, mut count) = (self.m.u32(first + 0x18), self.m.u32(first + 0x14));
        for _ in 0..self.rng.below(5) {
            let r = self.alloc(0x40);
            for off in (0..0x40).step_by(4) {
                let v = self.m.u32(first + off);
                self.m.set_u32(r + off, v);
            }
            let next = if self.rng.chance(80) { (count & 1) + count + start + 2 } else { start + count };
            count = self.rng.below(300);
            self.m.set_u32(r + 0x18, next);
            self.m.set_u32(r + 0x14, count);
            if self.rng.chance(10) {
                let o = self.object();
                self.m.set_u32(r + 8, o); // a different state breaks the run
            }
            start = next;
            out.push(r);
        }
    }

    fn colour_channel(&mut self) -> u32 {
        match self.rng.below(7) {
            0 | 1 | 2 => (self.rng.below(256) as f32 * (1.0f32 / 255.0)).to_bits(), // byte / 255 as the original computes it
            3 => (self.rng.below(256) as f32 / 255.0).to_bits(),
            4 => [1.0f32, 0.0, 1.5, -0.25, 2.0][self.rng.below(5) as usize].to_bits(),
            5 => f32::NAN.to_bits(),
            _ => self.rng.next() & 0x3F7F_FFFF,
        }
    }

    fn clear_record(&mut self) -> u32 {
        let r = self.alloc(0x40);
        self.m.set_u16(r + 2, (self.rng.next() as u16 & 0xFFF0) | 4);
        let mask = match self.rng.below(4) {
            0 => 0x30,
            1 => 0x0F,
            2 => 0x3F,
            _ => self.rng.next() & 0x3F,
        };
        self.m.set_u32(r + 4, mask);
        self.m.set_u32(r + 8, (self.rng.below(100) as f32 / 100.0).to_bits());
        self.m.set_u32(r + 0xC, self.rng.below(256));
        for k in 0..4 {
            let c = self.colour_channel();
            self.m.set_u32(r + 0x10 + 4 * k, c);
        }
        // the fallback draw; its targets object's +0x14: channel count
        let fallback = self.draw_record(0);
        let targets = self.alloc(0x20);
        self.m.set_u32(targets + 0x14, self.rng.below(8));
        self.m.set_u32(fallback + 4, targets);
        self.m.set_u32(r + 0x20, fallback);
        r
    }

    fn stretch_record(&mut self) -> u32 {
        let r = self.alloc(0x40);
        self.m.set_u16(r + 2, (self.rng.next() as u16 & 0xFFF0) | 5);
        // the owner: +0x44 and +0x48 surface tables (objects with a handle at +0x14)
        let owner = self.alloc(0x50);
        for table in [0x44, 0x48] {
            let t = self.alloc(0x20);
            for k in 0..4 {
                let s = self.alloc(0x20);
                self.m.set_u32(s + 0x14, self.rng.next());
                self.m.set_u32(t + 4 * k, s);
            }
            self.m.set_u32(owner + table, t);
        }
        self.m.set_u32(r + 4, owner);
        self.m.set_u32(r + 8, self.rng.below(4));
        self.m.set_u32(r + 0xC, (self.rng.next() & !0xF) | self.rng.below(6));
        for k in 0..4 {
            self.m.set_u32(r + 0x10 + 4 * k, self.rng.below(512));
        }
        self.m.set_u32(r + 0x20, self.rng.below(512));
        self.m.set_u32(r + 0x24, self.rng.below(512));
        let targets = self.object();
        self.m.set_u32(r + 0x28, targets);
        r
    }

    /// A copy of `r` (a StretchRect the executor merges with it), sometimes
    /// with one field changed.
    fn copy_of(&mut self, r: u32) -> u32 {
        let c = self.alloc(0x40);
        for off in (0..0x40).step_by(4) {
            let v = self.m.u32(r + off);
            self.m.set_u32(c + off, v);
        }
        if self.rng.chance(30) {
            let off = [4, 0xC, 0x10, 0x1C, 0x20, 0x24][self.rng.below(6) as usize];
            let v = self.rng.next() & 0xFF;
            self.m.set_u32(c + off, v);
        }
        c
    }

    /// A command list of `n` records; `depth` nested lists below it.
    fn list(&mut self, n: u32, depth: u32) -> (u32, u32) {
        let mut records = Vec::new();
        while (records.len() as u32) < n {
            match self.rng.below(12) {
                0 | 1 => records.push(self.draw_record(0)),
                2 | 3 => self.merged_run(&mut records),
                4 => records.push(self.draw_record(2)),
                5 => records.push(self.draw_record(3)),
                6 => records.push(self.clear_record()),
                7 => {
                    let r = self.stretch_record();
                    records.push(r);
                    if self.rng.chance(60) {
                        let c = self.copy_of(r);
                        records.push(c);
                    }
                }
                8 if depth > 0 => {
                    let k = self.rng.below(6);
                    let (list, count) = self.list(k, depth - 1);
                    let r = self.alloc(0x10);
                    self.m.set_u16(r + 2, (self.rng.next() as u16 & 0xFFF0) | 6);
                    self.m.set_u32(r + 4, self.rng.next());
                    self.m.set_u32(r + 8, list);
                    self.m.set_u32(r + 0xC, count);
                    records.push(r);
                }
                9 => {
                    let r = self.alloc(0x10);
                    self.m.set_u16(r + 2, (self.rng.next() as u16 & 0xFFF0) | 7);
                    self.m.set_u32(r + 4, self.rng.below(2));
                    let o = if self.rng.chance(80) { self.object() } else { 0 };
                    self.m.set_u32(r + 8, o);
                    records.push(r);
                }
                _ => {
                    let r = self.alloc(0x10);
                    let kind = 8 + self.rng.below(8);
                    self.m.set_u16(r + 2, (self.rng.next() as u16 & 0xFFF0) | kind as u16);
                    records.push(r);
                }
            }
        }
        // 8-byte entries; a null record now and then ends the frame early
        let list = self.alloc(8 * records.len() as u32 + 8);
        for (k, r) in records.iter().enumerate() {
            let e = list + 8 * k as u32;
            self.m.set_u32(e, self.rng.next());
            let r = if self.rng.chance(1) { 0 } else { *r };
            self.m.set_u32(e + 4, r);
        }
        (list, records.len() as u32)
    }
}

/// The executor's jump tables (DDDA.exe 2017-12-13 Steam, 0x00DAC000-
/// 0x00DAC083): the lifted body jumps through them, as the original does
/// (the port's `match` is their content). Code addresses.
const JUMP_TABLES: [u32; 33] = [
    // 0x00DAC000: the merged draw's primitive codes 1-4
    0x00DA_B65F, 0x00DA_B66B, 0x00DA_B64C, 0x00DA_B63F,
    // 0x00DAC010: the command types 0-8
    0x00DA_B796, 0x00DA_B6C3, 0x00DA_B816, 0x00DA_B980, 0x00DA_BAF0, 0x00DA_BD91, 0x00DA_BF93, 0x00DA_BFD8, 0x00DA_B504,
    // 0x00DAC034, 0x00DAC044, 0x00DAC054, 0x00DAC064, 0x00DAC074: the
    // primitive codes 1-4 of the type 1, 0, 2, 3 and Clear-fallback draws
    0x00DA_B761, 0x00DA_B76A, 0x00DA_B74F, 0x00DA_B745,
    0x00DA_BD6C, 0x00DA_BD75, 0x00DA_B801, 0x00DA_BD62,
    0x00DA_B90A, 0x00DA_B913, 0x00DA_B8F8, 0x00DA_B8EE,
    0x00DA_BA6E, 0x00DA_BA77, 0x00DA_BA5C, 0x00DA_BA52,
    0x00DA_BD6C, 0x00DA_BD75, 0x00DA_B801, 0x00DA_BD62,
];

/// A frame: sRender, the device and its vtable, the command lists; and the
/// machine at the call (ECX = sRender, a return address pushed).
fn frame(seed: u64) -> (TestMem, Cpu) {
    let mut f = Frame { m: TestMem::default(), at: DATA, rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1) };
    f.m.set_u32(RENDER + 0x98, DEVICE);
    let merge = f.rng.chance(60) as u8;
    f.m.set_u8(RENDER + 0x30, merge);
    f.m.set_u32(DEVICE, VTABLE);
    for k in 0..0x100 {
        f.m.set_u32(VTABLE + 4 * k, METHODS + 4 * k);
    }
    let n = 1 + f.rng.below(40);
    let (list, count) = f.list(n, 2);
    f.m.set_u32(RENDER + 0x0043_3DD8, list);
    f.m.set_u32(RENDER + 0x0043_3DE0, count);
    for (addr, bits) in [(0x0140_6CE0u32, 1.0f32.to_bits()), (0x0161_C298, 255.0f32.to_bits()), (0x0161_C29C, (1.0f32 / 255.0).to_bits())] {
        f.m.set_u32(addr, bits);
    }
    // the targets' handles
    for o in 0..0x1000u32 {
        if f.rng.chance(1) {
            for k in 0..4 {
                let t = DATA + 0x00F0_0000 + o * 0x40 + k * 0x10;
                f.m.set_u32(t + 0x14, f.rng.next());
            }
        }
    }
    f.m.set_u32(0x018D_08F4, RENDER);
    for (k, &target) in JUMP_TABLES.iter().enumerate() {
        f.m.set_u32(0x00DA_C000 + 4 * k as u32, target);
    }
    let mut cpu = Cpu::default();
    cpu.esp = STACK_TOP - 4 * f.rng.below(4);
    cpu.esp -= 4;
    f.m.set_u32(cpu.esp, 0x00DA_C0FB); // the render worker's return address
    cpu.ecx = RENDER;
    (cpu.eax, cpu.edx, cpu.ebx, cpu.ebp, cpu.esi, cpu.edi) = (f.rng.next(), f.rng.next(), f.rng.next(), f.rng.next(), f.rng.next(), f.rng.next());
    for k in 0..8 {
        cpu.xmm[k] = [f.rng.next(), f.rng.next(), f.rng.next(), f.rng.next()];
    }
    f.m.written.clear();
    (f.m, cpu)
}

/// Run `body` on a copy of the frame: (memory after, cpu after, calls).
fn run(body: LiftFn, m: &TestMem, cpu: &Cpu) -> (TestMem, Cpu, Vec<Call>) {
    let (mut m, mut cpu) = (m.clone(), cpu.clone());
    TRACE.with(|t| t.borrow_mut().clear());
    body(&mut m, &mut cpu);
    (m, cpu, TRACE.with(|t| std::mem::take(&mut *t.borrow_mut())))
}

#[test]
fn the_port_executes_frames_as_the_lifted_executor_does() {
    ddnative_host::install();
    x86::set_lifted_lookup(lookup);
    x86::set_ext_calls(Some(Box::new(mock)));
    let frames = std::env::var("MTDRAW_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(400u64);
    let mut calls = 0usize;
    let mut by_target: BTreeMap<u32, usize> = BTreeMap::new();
    for seed in 1..=frames {
        let (m, cpu) = frame(seed);
        let (lm, lcpu, lcalls) = run(ddlift_hot_00::f_00dab480, &m, &cpu);
        let (pm, pcpu, pcalls) = run(mtdraw::abi::f_00dab480, &m, &cpu);
        for (k, (a, b)) in lcalls.iter().zip(&pcalls).enumerate() {
            assert_eq!(a, b, "frame {seed}: call {k} differs");
        }
        assert_eq!(lcalls.len(), pcalls.len(), "frame {seed}: call count");
        assert_eq!(regs(&lcpu), regs(&pcpu), "frame {seed}: registers");
        assert_eq!(lcpu.xmm, pcpu.xmm, "frame {seed}: xmm");
        assert_eq!((lcpu.st.status(), lcpu.st.control(), lcpu.st.depth()), (pcpu.st.status(), pcpu.st.control(), pcpu.st.depth()), "frame {seed}: x87");
        if lm.written != pm.written {
            let diff: Vec<_> = lm.written.iter().filter(|(a, v)| pm.written.get(a) != Some(v)).take(8).collect();
            let extra: Vec<_> = pm.written.iter().filter(|(a, _)| !lm.written.contains_key(a)).take(8).collect();
            panic!("frame {seed}: memory differs: lifted {diff:x?}, port only {extra:x?}");
        }
        calls += lcalls.len();
        for c in &lcalls {
            *by_target.entry(c.target).or_default() += 1;
        }
    }
    eprintln!("{frames} frames, {calls} calls equal");
    let name = |t: u32| match t {
        BEGIN_CONTEXT => "begin context".to_string(),
        END_CONTEXT => "end context".into(),
        APPLY_TARGETS => "apply targets".into(),
        APPLY_STATES => "apply states".into(),
        APPLY_STREAMS => "apply streams".into(),
        APPLY_SHADERS => "apply shaders".into(),
        CHECK_RESULT => "check result".into(),
        RECT_EQUAL => "rect equal".into(),
        SPAN_BEGIN => "span begin".into(),
        SPAN_END => "span end".into(),
        t => format!("device +{:#x}", t - METHODS),
    };
    for (t, n) in &by_target {
        eprintln!("  {:>16} {n}", name(*t));
    }
    // every path the executor has: the device's draws, clears and copies,
    // the instanced streams, the merge check, the query spans
    for t in [0xAC, 0x88, 0x144, 0x148, 0x190, 0x198] {
        assert!(by_target.get(&(METHODS + t)).copied().unwrap_or(0) > 0, "device +{t:#x} never called");
    }
    for t in [RECT_EQUAL, SPAN_BEGIN, SPAN_END, CHECK_RESULT] {
        assert!(by_target.get(&t).copied().unwrap_or(0) > 0, "{} never called", name(t));
    }
}

// --- L119: the command sink ------------------------------------------------------------

/// A sink that records what it is offered and takes the kinds `take` says.
struct Taker {
    take: fn(u32) -> bool,
    offered: std::sync::Mutex<Vec<(u32, u32)>>,
    taken: std::sync::Mutex<Vec<u32>>,
    frames: std::sync::atomic::AtomicU32,
}

impl mtdraw::sink::CommandSink for Taker {
    fn frame_begin(&self, _m: &mut dyn Mem, render: u32, _context: u32) {
        assert_eq!(render, RENDER);
        self.frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn command(&self, _m: &mut dyn Mem, _context: u32, record: u32, kind: u32) -> mtdraw::sink::Taken {
        assert_ne!(kind, 6, "nested lists are not offered");
        self.offered.lock().unwrap().push((record, kind));
        if (self.take)(kind) {
            self.taken.lock().unwrap().push(record);
            mtdraw::sink::Taken::Handled
        } else {
            mtdraw::sink::Taken::Default
        }
    }
    fn frame_end(&self, _m: &mut dyn Mem, render: u32, _context: u32) {
        assert_eq!(render, RENDER);
    }
}

fn taker(take: fn(u32) -> bool) -> std::sync::Arc<Taker> {
    std::sync::Arc::new(Taker { take, offered: Default::default(), taken: Default::default(), frames: Default::default() })
}

/// The executor with a sink: (b) a sink taking nothing changes nothing (the
/// lifted result); (c) a sink taking the DrawPrimitive and instanced
/// DrawPrimitive records gives exactly the lifted result on the same frame
/// with those records turned into no-ops (type 8): their applies and
/// device calls, and nothing else, are gone.
#[test]
fn a_sink_takes_whole_commands_and_nothing_else() {
    ddnative_host::install();
    x86::set_lifted_lookup(lookup);
    x86::set_ext_calls(Some(Box::new(mock)));
    let frames = std::env::var("MTDRAW_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(400u64);
    let (mut offered, mut taken) = (0usize, 0usize);
    for seed in 1..=frames {
        let (m, cpu) = frame(seed);
        // (b)
        let watch = taker(|_| false);
        mtdraw::sink::set_sink(Some(watch.clone()));
        let (pm, pcpu, pcalls) = run(mtdraw::abi::f_00dab480, &m, &cpu);
        mtdraw::sink::set_sink(None);
        let (lm, lcpu, lcalls) = run(ddlift_hot_00::f_00dab480, &m, &cpu);
        assert_eq!(lcalls, pcalls, "frame {seed}: a sink taking nothing changed the calls");
        assert_eq!((regs(&lcpu), lcpu.xmm), (regs(&pcpu), pcpu.xmm), "frame {seed}: registers");
        assert!(lm.written == pm.written, "frame {seed}: memory");
        assert_eq!(watch.frames.load(std::sync::atomic::Ordering::Relaxed), 1);
        offered += watch.offered.lock().unwrap().len();
        // (c)
        let draws = taker(|kind| kind == 0 || kind == 2);
        mtdraw::sink::set_sink(Some(draws.clone()));
        let (pm, pcpu, pcalls) = run(mtdraw::abi::f_00dab480, &m, &cpu);
        mtdraw::sink::set_sink(None);
        let mut skipped = m.clone();
        for &r in draws.taken.lock().unwrap().iter() {
            let half = skipped.u16(r + 2);
            skipped.set_u16(r + 2, (half & !0xF) | 8);
        }
        skipped.written.clear();
        let (lm, lcpu, lcalls) = run(ddlift_hot_00::f_00dab480, &skipped, &cpu);
        for (k, (a, b)) in lcalls.iter().zip(&pcalls).enumerate() {
            assert_eq!(a, b, "frame {seed}: call {k} differs with draws taken");
        }
        assert_eq!(lcalls.len(), pcalls.len(), "frame {seed}: call count with draws taken");
        assert_eq!((regs(&lcpu), lcpu.xmm), (regs(&pcpu), pcpu.xmm), "frame {seed}: registers with draws taken");
        assert_eq!((lcpu.st.status(), lcpu.st.depth()), (pcpu.st.status(), pcpu.st.depth()), "frame {seed}: x87");
        assert!(lm.written == pm.written, "frame {seed}: memory with draws taken");
        taken += draws.taken.lock().unwrap().len();
    }
    eprintln!("{frames} frames: {offered} commands offered; {taken} draws taken by the sink, the rest equal");
    assert!(taken > 0);
}
