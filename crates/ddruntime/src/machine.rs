//! The machine's answers to CPUID and RDTSC (ddnative_base::platform::
//! Machine), installed on every guest thread with the call dispatcher.
//!
//! The game asks the processor directly in the system-information
//! collector 0x00DBD170 (CPUID wrapper 0x00DBD140, ID-flag test 0x00DBD110:
//! leaves 0 and 1 for the vendor and the SSE features, 0x80000002..4 for
//! the brand string) and reads the time-stamp counter in 0x011BA3C0
//! against sAI's clock rate (registry `~MHz` x 1,000,000, crate::registry).
//!
//! The answers are those of the one recorded machine the registry
//! describes (crate::registry: name, vendor, identifier, clock), so a
//! session does not depend on the host. Measured on that machine
//! (2026-10-11, Windows 11 26200) by research/native/probe_machine.rs in a
//! 32-bit process pinned to processor 0, matching GetSystemInfo's one
//! processor (crate::win32): leaves 0..=0xD and 0x80000000..=0x80000023,
//! subleaves 0..=5 of the subleaf-indexed leaves (Intel SDM vol. 2A and AMD
//! APM vol. 3, CPUID), and the first leaf above each range. Leaf 1 still
//! reports the processor's 16 logical processors (EBX bits 16-23). The
//! hypervisor leaf 0x40000000 ("Microsoft Hv") is what Windows presents on
//! that machine. Anything else is not modelled and panics.
//!
//! RDTSC counts at the registry clock (`~MHz`) from the session's start:
//! the counter's rate is what the game compares against; its origin is not
//! architectural.
use ddnative_base::platform::Machine;
use std::time::Instant;

/// (leaf, subleaf, [eax, ebx, ecx, edx]) as measured on processor 0.
const CPUID: &[(u32, u32, [u32; 4])] = &[
    (0x00000000, 0, [0x0000000d, 0x68747541, 0x444d4163, 0x69746e65]),
    (0x00000001, 0, [0x00a20f12, 0x00100800, 0xfed8320b, 0x178bfbff]),
    (0x00000002, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000003, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000004, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000004, 1, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000004, 2, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000004, 3, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000004, 4, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000004, 5, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000005, 0, [0x00000040, 0x00000040, 0x00000003, 0x00000011]),
    (0x00000006, 0, [0x00000000, 0x00000000, 0x00000001, 0x00000000]),
    (0x00000007, 0, [0x00000000, 0x219c97a9, 0x00400684, 0x00000010]),
    (0x00000007, 1, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000007, 2, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000007, 3, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000007, 4, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000007, 5, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000008, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x00000009, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x0000000a, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x0000000b, 0, [0x00000001, 0x00000002, 0x00000100, 0x00000000]),
    (0x0000000b, 1, [0x00000004, 0x00000010, 0x00000201, 0x00000000]),
    (0x0000000b, 2, [0x00000000, 0x00000000, 0x00000002, 0x00000000]),
    (0x0000000b, 3, [0x00000000, 0x00000000, 0x00000003, 0x00000000]),
    (0x0000000b, 4, [0x00000000, 0x00000000, 0x00000004, 0x00000000]),
    (0x0000000b, 5, [0x00000000, 0x00000000, 0x00000005, 0x00000000]),
    (0x0000000c, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x0000000d, 0, [0x00000007, 0x00000340, 0x00000340, 0x00000000]),
    (0x0000000d, 1, [0x0000000f, 0x00000350, 0x00001800, 0x00000000]),
    (0x0000000d, 2, [0x00000100, 0x00000240, 0x00000000, 0x00000000]),
    (0x0000000d, 3, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x0000000d, 4, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x0000000d, 5, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000000, 0, [0x80000023, 0x68747541, 0x444d4163, 0x69746e65]),
    (0x80000001, 0, [0x00a20f12, 0x20000000, 0x20c023f3, 0x2fd3fbff]),
    (0x80000002, 0, [0x20444d41, 0x657a7952, 0x2037206e, 0x30303735]),
    (0x80000003, 0, [0x20443358, 0x6f432d38, 0x50206572, 0x65636f72]),
    (0x80000004, 0, [0x726f7373, 0x20202020, 0x20202020, 0x00202020]),
    (0x80000005, 0, [0xff40ff40, 0xff40ff40, 0x20080140, 0x20080140]),
    (0x80000006, 0, [0x48002200, 0x68004200, 0x02006140, 0x03009140]),
    (0x80000007, 0, [0x00000000, 0x0000001b, 0x00000000, 0x00006789]),
    (0x80000008, 0, [0x00003030, 0x3112d215, 0x0000400f, 0x00010000]),
    (0x80000009, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000000a, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000000b, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000000c, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000000d, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000000e, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000000f, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000010, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000011, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000012, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000013, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000014, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000015, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000016, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000017, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000018, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000019, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000001a, 0, [0x00000006, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000001b, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000001c, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000001d, 0, [0x00004121, 0x01c0003f, 0x0000003f, 0x00000000]),
    (0x8000001d, 1, [0x00004122, 0x01c0003f, 0x0000003f, 0x00000000]),
    (0x8000001d, 2, [0x00004143, 0x01c0003f, 0x000003ff, 0x00000002]),
    (0x8000001d, 3, [0x0003c163, 0x03c0003f, 0x00017fff, 0x00000001]),
    (0x8000001d, 4, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000001d, 5, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x8000001e, 0, [0x00000000, 0x00000100, 0x00000000, 0x00000000]),
    (0x8000001f, 0, [0x0001780f, 0x00000173, 0x000001fd, 0x00000001]),
    (0x80000020, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000020, 1, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000020, 2, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000020, 3, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000020, 4, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000020, 5, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000021, 0, [0x00000001, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000022, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000023, 0, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
];

/// Leaves above the measured ranges that were probed (subleaf 0).
const BEYOND: &[(u32, [u32; 4])] = &[
    (0x0000000e, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x80000024, [0x00000000, 0x00000000, 0x00000000, 0x00000000]),
    (0x40000000, [0x4000000c, 0x7263694d, 0x666f736f, 0x76482074]),
];

/// Leaves whose answer depends on the subleaf in ECX.
const SUBLEAF_INDEXED: &[u32] = &[4, 7, 0xb, 0xd, 0xf, 0x10, 0x8000_001d, 0x8000_0020];

/// The recorded machine.
pub struct Recorded {
    epoch: Instant,
}

impl Recorded {
    pub fn new() -> Self {
        Recorded { epoch: session_epoch() }
    }
}

impl Default for Recorded {
    fn default() -> Self {
        Self::new()
    }
}

/// One counter origin for every guest thread.
fn session_epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// The recorded answer for (leaf, subleaf), if the probe measured it.
pub fn cpuid(leaf: u32, subleaf: u32) -> Option<[u32; 4]> {
    let indexed = SUBLEAF_INDEXED.contains(&leaf);
    let sub = if indexed { subleaf } else { 0 };
    CPUID.iter().find(|e| e.0 == leaf && e.1 == sub).map(|e| e.2)
        .or_else(|| BEYOND.iter().find(|e| e.0 == leaf && sub == 0).map(|e| e.1))
}

impl Machine for Recorded {
    fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4] {
        cpuid(leaf, subleaf)
            .unwrap_or_else(|| panic!("CPUID leaf {leaf:#x} subleaf {subleaf:#x} is not modelled (crate::machine)"))
    }

    fn rdtsc(&self) -> u64 {
        let ns = self.epoch.elapsed().as_nanos();
        (ns * crate::registry::CPU_MHZ as u128 / 1000) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{CPU_MHZ, IDENTIFIER, PROCESSOR_NAME, VENDOR};

    fn text(leaves: &[u32], order: &[usize]) -> String {
        let bytes: Vec<u8> = leaves.iter().flat_map(|&l| {
            let r = cpuid(l, 0).unwrap();
            order.iter().flat_map(move |&k| r[k].to_le_bytes())
        }).collect();
        String::from_utf8(bytes).unwrap().trim_end_matches('\0').to_string()
    }

    #[test]
    fn cpuid_describes_the_registry_processor() {
        assert_eq!(text(&[0x8000_0002, 0x8000_0003, 0x8000_0004], &[0, 1, 2, 3]), PROCESSOR_NAME);
        assert_eq!(text(&[0], &[1, 3, 2]), VENDOR);
        let signature = cpuid(1, 0).unwrap()[0];
        let family = ((signature >> 8) & 0xf) + ((signature >> 20) & 0xff);
        let model = ((signature >> 4) & 0xf) | ((signature >> 12) & 0xf0);
        assert_eq!(format!("AMD64 Family {family} Model {model} Stepping {}", signature & 0xf), IDENTIFIER);
        assert_eq!(cpuid(1, 0).unwrap()[1] >> 24, 0, "processor 0: initial APIC id 0");
        assert_eq!(cpuid(1, 0).unwrap()[3] & 0x0600_0000, 0x0600_0000, "SSE and SSE2");
    }

    #[test]
    fn subleaves_and_unmodelled_leaves() {
        assert_eq!(cpuid(1, 7), cpuid(1, 0));
        assert_ne!(cpuid(0xb, 1), cpuid(0xb, 0));
        assert_eq!(cpuid(0xe, 0), Some([0; 4]));
        assert_eq!(cpuid(0xb, 9), None);
        assert_eq!(cpuid(0x4000_0001, 0), None);
    }

    #[test]
    fn rdtsc_counts_at_the_registry_clock() {
        let m = Recorded::new();
        let (t0, i0) = (m.rdtsc(), Instant::now());
        std::thread::sleep(std::time::Duration::from_millis(50));
        let (t1, i1) = (m.rdtsc(), Instant::now());
        let expected = (i1 - i0).as_nanos() as f64 * CPU_MHZ as f64 / 1000.0;
        let ratio = (t1 - t0) as f64 / expected;
        assert!((0.95..1.05).contains(&ratio), "{ratio}");
    }
}
