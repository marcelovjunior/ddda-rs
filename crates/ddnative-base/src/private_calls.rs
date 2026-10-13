//! Scoped, thread-owned resolution for isolated replay. While set, every
//! cross-shard call and every globally overridden local call must use this
//! resolver instead of the process-global override/cache. An isolated
//! resolver must reject unknown targets; falling back would escape isolation.
use super::{Cpu, LiftFn};
use crate::mem::Mem;
use std::{cell::Cell, marker::PhantomData, rc::Rc};

pub type Resolver = fn(u32) -> LiftFn;

thread_local! {
    static RESOLVER: Cell<Option<Resolver>> = const { Cell::new(None) };
}

/// Restores the previous resolver on normal return or unwind. !Send: this
/// guard changes only the thread that created it and must be dropped there.
pub struct Scope {
    previous: Option<Resolver>,
    thread_owned: PhantomData<Rc<()>>,
}

pub fn enter(resolver: Resolver) -> Scope {
    Scope {
        previous: RESOLVER.with(|slot| slot.replace(Some(resolver))),
        thread_owned: PhantomData,
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        RESOLVER.with(|slot| slot.set(self.previous));
    }
}

/// Default path: one const TLS lookup, no allocation or cache invalidation.
#[inline]
pub fn dispatch(memory: &mut dyn Mem, cpu: &mut Cpu, target: u32) -> bool {
    let resolver = RESOLVER.with(Cell::get);
    if let Some(resolver) = resolver {
        resolver(target)(memory, cpu);
        true
    } else {
        false
    }
}
