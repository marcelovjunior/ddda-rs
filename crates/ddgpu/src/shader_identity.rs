//! Stable shader keys: shared draws do not hash bytecode again.
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(crate) struct ShaderIdentities {
    // Retain each allocation while its pointer is a key: a released guest
    // shader cannot have its address recycled into an old pipeline key.
    pointers: HashMap<usize, (Arc<[u32]>, u64)>,
    // Deduplicate independently created objects once, preserving the old
    // cache's sharing of identical shader programs without hash collisions.
    programs: HashMap<Arc<[u32]>, u64>,
}

impl ShaderIdentities {
    pub fn key(&mut self, tokens: &Arc<[u32]>) -> u64 {
        let pointer = tokens.as_ptr() as usize;
        if let Some((_, key)) = self.pointers.get(&pointer) { return *key; }
        let next = self.programs.len() as u64;
        let key = *self.programs.entry(tokens.clone()).or_insert(next);
        self.pointers.insert(pointer, (tokens.clone(), key));
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_are_retained_and_equal_programs_share_keys() {
        let mut ids = ShaderIdentities::default();
        let a: Arc<[u32]> = vec![1, 2, 3].into();
        let b: Arc<[u32]> = vec![1, 2, 3].into();
        let c: Arc<[u32]> = vec![1, 2, 4].into();
        let weak = Arc::downgrade(&a);
        let key = ids.key(&a);
        assert_eq!(ids.key(&a), key);
        assert_eq!(ids.key(&b), key);
        assert_ne!(ids.key(&c), key);
        drop(a);
        assert!(weak.upgrade().is_some());
        assert_eq!(ids.pointers.len(), 3);
        assert_eq!(ids.programs.len(), 2);
    }
}
