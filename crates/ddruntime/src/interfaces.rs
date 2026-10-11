//! Interface objects the runtime gives the guest: C++ objects (Steam's
//! `ISteam*`, thiscall) and COM objects (Direct3D, DirectInput: stdcall
//! with `this` on the stack) whose methods are runtime services.
//!
//! An object is a guest heap block whose first word points to a vtable of
//! method tokens; each token is a dispatcher address named
//! `Interface::slotN`. Implemented methods are bound to their service; the
//! others stay unbound, so a call to one stops with its name instead of
//! doing something made up.
use crate::dispatch::{Binding, Conflict, Registry};
use crate::services::with_state;
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// Method tokens live on their own reserved pages (never executed).
pub const METHOD_TOKENS: u32 = 0x7FE0_0000;
const TOKEN_STRIDE: u32 = 0x10;

pub type Method = fn(&mut dyn Mem, &mut Cpu);

pub struct Spec {
    pub name: &'static str,
    /// Number of vtable slots to provide (at least the highest one called).
    pub slots: u32,
    /// (slot index, service)
    pub methods: &'static [(u32, Method)],
}

/// Allocates method tokens across all interfaces.
#[derive(Default)]
pub struct Tokens {
    next: u32,
}

impl Tokens {
    fn take(&mut self) -> u32 {
        let t = METHOD_TOKENS + TOKEN_STRIDE * self.next;
        self.next += 1;
        t
    }
    /// The highest token address handed out (for mapping the token pages).
    pub fn span(&self) -> u32 {
        (self.next * TOKEN_STRIDE).max(1)
    }
}

/// Create one object for `spec`: its vtable and the object on the guest
/// heap, its tokens named and bound. Returns the object's address.
pub fn create(m: &mut dyn Mem, registry: &mut Registry, tokens: &mut Tokens, spec: &Spec) -> Result<u32, Conflict> {
    let vtable = with_state(|s| s.heap.alloc(4 * spec.slots)).expect("guest heap");
    let object = with_state(|s| s.heap.alloc(4)).expect("guest heap");
    m.set_u32(object, vtable);
    for k in 0..spec.slots {
        let t = tokens.take();
        m.set_u32(vtable + 4 * k, t);
        registry.name(t, format!("{}::slot{k}", spec.name));
        if let Some((_, f)) = spec.methods.iter().find(|(slot, _)| *slot == k) {
            registry.insert(t, Binding::Service(*f))?;
        }
    }
    Ok(object)
}
