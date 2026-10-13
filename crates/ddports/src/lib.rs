//! The readable ports (crates/mtmath, mtcore, mtframe, mtdraw) and mods, registered
//! by address over their lifted bodies (L60, docs/NATIVE_PIPELINE.md,
//! "Overrides").
//!
//! The shards call each other and the platform models through overridable
//! dispatch (`x86::call_lifted`, `x86::call_local`); an override registered
//! here before the program runs takes the lifted body's place for every
//! caller. Each port crate's `abi::PORTS` lists its adapters; its
//! differential test replays them against the lifted vectors.
use ddnative_base::x86::{self, LiftFn};

/// Every verified readable port: (address, adapter).
pub fn ports() -> Vec<(u32, LiftFn)> {
    let mut ports: Vec<(u32, LiftFn)> = mtmath::abi::PORTS.iter().chain(mtcore::abi::PORTS).chain(mtcore::hot::PORTS).chain(mtframe::abi::PORTS).chain(mtdraw::abi::PORTS)
        .map(|&(a, f, _)| (a, f))
        .collect();
    // L158: exact inner dependency hash. Keep opt-in until the combined
    // native gate and head's matched per-job capture have accepted it.
    if std::env::var("DDRUNTIME_JOB_HASH").as_deref() == Ok("1") {
        ports.push((0x00e0e5e0, mtcore::job_hash::dependency_hash));
    }
    // L169: preserve the original serial/queued decision and callback order.
    if std::env::var("DDRUNTIME_SHADOW_TRAVERSAL").as_deref() == Ok("1") {
        ports.push((0x00db63e0, mtcore::shadow_traversal::traverse));
    }
    if std::env::var("DDRUNTIME_SHADOW_PASS").as_deref() == Ok("1") {
        ports.push((0x00efd240, mtcore::shadow_pass::setup));
    }
    ports
}

/// The ports with `mods` over them: a mod at a ported address replaces the
/// port.
pub fn with_mods(mods: &[(u32, LiftFn)]) -> Vec<(u32, LiftFn)> {
    let mut v: Vec<(u32, LiftFn)> = ports().into_iter().filter(|(a, _)| !mods.iter().any(|m| m.0 == *a)).collect();
    v.extend_from_slice(mods);
    v
}

/// Register the ports and `mods`, once, before the lifted program runs.
pub fn install(mods: &[(u32, LiftFn)]) {
    x86::set_overrides(&with_mods(mods));
}

static MODS: std::sync::OnceLock<Vec<(u32, LiftFn)>> = std::sync::OnceLock::new();

/// A mod's overrides, set before the program starts (before [`ensure`]);
/// false if mods were already set.
pub fn set_mods(mods: Vec<(u32, LiftFn)>) -> bool {
    MODS.set(mods).is_ok()
}

/// Register the ports and the mods set so far, once per process (the
/// runtime calls this before it lists the lifted program).
pub fn ensure() {
    ensure_with(|_| {});
}

/// Compose diagnostic wrappers with ports/mods before dispatch freezes.
/// Called once; a wrapper must retain and call the body it replaces.
pub fn ensure_with(wrap: impl FnOnce(&mut Vec<(u32, LiftFn)>)) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut v = with_mods(MODS.get().map(|v| &v[..]).unwrap_or(&[]));
        wrap(&mut v);
        x86::set_overrides(&v);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddnative_base::mem::Mem;
    use ddnative_base::x86::Cpu;

    fn a_mod(_: &mut dyn Mem, cpu: &mut Cpu) {
        cpu.eax = 7;
    }

    #[test]
    fn ports_are_unique_and_a_mod_replaces_a_port() {
        let p = ports();
        let mut addrs: Vec<u32> = p.iter().map(|e| e.0).collect();
        addrs.sort_unstable();
        addrs.dedup();
        assert_eq!(addrs.len(), p.len(), "an address ported twice");
        let first = p[0].0;
        let v = with_mods(&[(first, a_mod as LiftFn)]);
        assert_eq!(v.len(), p.len());
        assert!(v.iter().any(|&(a, f)| a == first && std::ptr::fn_addr_eq(f, a_mod as LiftFn)));
    }
}
