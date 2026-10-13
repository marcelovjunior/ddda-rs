//! The CRT's memcpy and memset as page-wise bulk copies (L61).
//!
//! ddnative-base's models of them (platform::memcpy, platform::memset) copy
//! byte by byte through the Mem trait: claude-2's sampler found them copying
//! whole video frames (YV12 720p, ~1.4 MB) that way during the title movie.
//! The runtime redirects both models to these services
//! (session::REDIRECTED_MODELS). Same results: memcpy handles overlap like
//! memmove (the source is read whole first), a page that is not writable
//! takes the per-byte path and faults where the original would.

use ddnative_base::mem::{store_at, Mem, ATOMIC_ACCESSES, PAGE, WRITE};
use ddnative_base::x86::Cpu;
use std::sync::atomic::Ordering;

fn reuse_scratch() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("DDRUNTIME_CRT_SCRATCH").as_deref() == Ok("1"))
}

// A host retention policy, not a limit on guest copies. Large/reentrant
// requests keep the original private temporary, including read-before-write
// fault behavior and overlapping memcpy's whole-source snapshot.
fn with_scratch<R>(size: usize, f: impl FnOnce(&mut [u8]) -> R) -> R {
    const RETAIN: usize = 2 << 20;
    thread_local! { static SCRATCH: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) }; }
    SCRATCH.with(|scratch| {
        if size <= RETAIN {
            if let Ok(mut bytes) = scratch.try_borrow_mut() {
                if bytes.len() < size { bytes.resize(size, 0); }
                return f(&mut bytes[..size]);
            }
        }
        f(&mut vec![0; size])
    })
}

/// 0x012EFE20: memset(dst, c, n), cdecl, eax = dst.
pub const MEMSET: u32 = 0x012E_FE20;
/// 0x012EFEA0: memcpy(dst, src, n), cdecl, eax = dst (overlap as memmove).
pub const MEMCPY: u32 = 0x012E_FEA0;

fn arg(m: &dyn Mem, cpu: &Cpu, k: u32) -> u32 {
    m.u32(cpu.esp.wrapping_add(4 + 4 * k))
}

/// `data` at `addr`, page by page.
pub fn write_bulk(m: &mut dyn Mem, addr: u32, data: &[u8]) {
    let atomic = ATOMIC_ACCESSES.load(Ordering::Relaxed);
    let table = m.page_table();
    let mut k = 0usize;
    while k < data.len() {
        let a = addr.wrapping_add(k as u32);
        let (p, off) = ((a / PAGE) as usize, (a % PAGE) as usize);
        let n = (PAGE as usize - off).min(data.len() - k);
        // SAFETY: page_table()'s contract (alive, never moves)
        let writable = table.map(|t| unsafe { t.as_ref() }.perm(p) & WRITE != 0).unwrap_or(false);
        if !writable {
            for j in 0..n {
                m.set_u8(a.wrapping_add(j as u32), data[k + j]); // faults as the original
            }
        } else {
            // SAFETY: a writable page; backed_page gives its 4 KiB (backing a
            // lazy page with zeros first, as a store would)
            let page = unsafe { table.unwrap().as_ref() }.backed_page(p);
            if atomic {
                for j in 0..n {
                    // SAFETY: inside the page
                    unsafe { store_at(page.add(off + j), 1, data[k + j] as u32) };
                }
            } else {
                // SAFETY: off + n <= PAGE inside a backed page
                unsafe { std::ptr::copy_nonoverlapping(data[k..].as_ptr(), page.add(off), n) };
            }
        }
        k += n;
    }
}

pub fn memcpy(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dst, src, n) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let mut copy = |bytes: &mut [u8]| {
        m.read_bytes(src, bytes);
        write_bulk(m, dst, bytes);
    };
    if reuse_scratch() { with_scratch(n as usize, copy); }
    else { copy(&mut vec![0u8; n as usize]); }
    cpu.eax = dst;
    let _ = m.u32(cpu.esp); // the return address
    cpu.esp = cpu.esp.wrapping_add(4);
}

pub fn memset(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dst, c, n) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    if reuse_scratch() {
        let bytes = [c as u8; PAGE as usize];
        let mut offset = 0;
        while offset < n {
            let count = (n - offset).min(PAGE);
            write_bulk(m, dst.wrapping_add(offset), &bytes[..count as usize]);
            offset += count;
        }
    } else {
        write_bulk(m, dst, &vec![c as u8; n as usize]);
    }
    cpu.eax = dst;
    let _ = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
}

#[cfg(test)]
mod scratch_tests {
    use super::*;
    #[test]
    fn nested_and_unwound_scratch_is_independent_and_retention_is_bounded() {
        with_scratch(128, |outer| {
            outer.fill(0x57);
            with_scratch(128, |inner| { inner.fill(0xaa); });
            assert!(outer.iter().all(|&b| b == 0x57));
        });
        let _ = std::panic::catch_unwind(|| with_scratch(4096, |_| panic!("synthetic unwind")));
        let pointer = with_scratch(4096, |b| b.as_ptr());
        with_scratch((2 << 20) + 1, |b| b.fill(9));
        assert_eq!(with_scratch(4096, |b| b.as_ptr()), pointer);
    }
}
