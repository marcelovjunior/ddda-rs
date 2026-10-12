//! Guest memory's fast paths (brief L25) behave like the checked byte-wise
//! accesses: loads and stores within a page, and `Mem::read_bytes`, give the
//! same values and the same faults as `check` + `read`/`poke` on random
//! accesses over read-write, read-only, lazily backed, unmapped and
//! wrapping pages.
use ddnative_base::mem::Mem;
use ddruntime::memory::{GuestMemory, PAGE, READ, WRITE};

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

/// Pages 0x10..0x18: rw, ro, lazy rw, unmapped, rw, lazy ro, rw, unmapped;
/// plus the last and first pages of the address space (wrapping).
fn memory() -> GuestMemory {
    let mut m = GuestMemory::new();
    let at = |p: u32| p * PAGE;
    m.map(at(0x10), PAGE, READ | WRITE).unwrap();
    m.map(at(0x11), PAGE, READ).unwrap();
    m.map_lazy(at(0x12), PAGE, READ | WRITE).unwrap();
    m.map(at(0x14), PAGE, READ | WRITE).unwrap();
    m.map_lazy(at(0x15), PAGE, READ).unwrap();
    m.map(at(0x16), PAGE, READ | WRITE).unwrap();
    m.map(0xFFFF_F000, PAGE, READ | WRITE).unwrap();
    m.map(0, PAGE, READ | WRITE).unwrap();
    // some contents (poke ignores permissions; the lazy pages stay unbacked)
    let mut rng = Rng(7);
    for p in [0x10, 0x11, 0x14, 0x16] {
        let bytes: Vec<u8> = (0..PAGE).map(|_| rng.next() as u8).collect();
        m.poke(at(p), &bytes).unwrap();
    }
    m
}

fn address(rng: &mut Rng) -> u32 {
    match rng.next() % 8 {
        0 => 0xFFFF_FFFCu32.wrapping_add(rng.next() % 8), // around the wrap
        1 => 0x10000 + (rng.next() % 7 + 1) * PAGE - 1 - rng.next() % 4, // page ends
        _ => 0x10000 + rng.next() % (8 * PAGE),
    }
}

/// Silence the expected guest faults; anything else (a failed assertion)
/// still prints.
fn quiet_faults() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.payload().downcast_ref::<String>().map(String::as_str).unwrap_or("");
        if !msg.starts_with("guest ") {
            default(info);
        }
    }));
}

fn outcome<T: std::fmt::Debug>(f: impl FnOnce() -> T) -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => format!("{v:?}"),
        Err(e) => e.downcast_ref::<String>().cloned().unwrap_or_else(|| "panic".into()),
    }
}

#[test]
fn loads_and_stores_match_the_checked_accesses() {
    quiet_faults();
    let (mut fast, mut checked) = (memory(), memory());
    let mut rng = Rng(0x1234_5678);
    for _ in 0..200_000 {
        let a = address(&mut rng);
        let width = [1u32, 2, 4][rng.next() as usize % 3];
        if rng.next() % 2 == 0 {
            let got = outcome(|| match width {
                1 => fast.u8(a) as u32,
                2 => fast.u16(a) as u32,
                _ => fast.u32(a),
            });
            let want = match checked.read(a, width) {
                Ok(b) => format!("{:?}", b.iter().rev().fold(0u32, |v, &x| v << 8 | x as u32)),
                Err(f) => f.to_string(),
            };
            assert_eq!(got, want, "load of {width} at {a:#x}");
        } else {
            let v = rng.next();
            let got = outcome(|| match width {
                1 => fast.set_u8(a, v as u8),
                2 => fast.set_u16(a, v as u16),
                _ => fast.set_u32(a, v),
            });
            let want = match checked.check(a, width, true) {
                Ok(()) => {
                    checked.poke(a, &v.to_le_bytes()[..width as usize]).unwrap();
                    "()".to_string()
                }
                Err(f) => f.to_string(),
            };
            assert_eq!(got, want, "store of {width} at {a:#x}");
        }
    }
    // the same contents everywhere afterwards
    for p in (0x10..0x18).chain([0xFFFFF, 0]) {
        let a = p * PAGE;
        assert_eq!(fast.read(a, PAGE).ok(), checked.read(a, PAGE).ok(), "page {a:#x}");
    }
    // and only the pages written got memory behind them
    assert_eq!(fast.backed_pages(), checked.backed_pages());
}

#[test]
fn read_bytes_reads_like_each_byte() {
    quiet_faults();
    let m = memory();
    let mut rng = Rng(0xBEEF_0001);
    for _ in 0..20_000 {
        let a = address(&mut rng);
        let len = [0usize, 1, 3, 16, 100, 5000, 9000][rng.next() as usize % 7];
        let got = outcome(|| {
            let mut v = vec![0xAAu8; len];
            m.read_bytes(a, &mut v);
            v
        });
        let want = outcome(|| (0..len as u32).map(|k| m.u8(a.wrapping_add(k))).collect::<Vec<u8>>());
        assert_eq!(got, want, "{len} bytes at {a:#x}");
    }
}
