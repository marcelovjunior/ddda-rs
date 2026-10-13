//! Recycled upload buffers (L144). A resource's shadow copy goes to the
//! backend in a fresh `Vec` per upload; a large one (the game re-uploads
//! ~3.5 MB a frame) is a new block of the OS heap each time, its pages
//! faulted in and zeroed: ~520 us of a 2 MB upload's ~860 us on the
//! recording thread. The recorder takes a buffer here, the backend gives
//! it back once written, and the next upload reuses its pages.
use std::sync::Mutex;

/// Smaller buffers come from the allocator's own free lists cheaply.
const MIN: usize = 64 * 1024;
/// What the pool holds at most, in bytes of capacity.
const CAP: usize = 32 << 20;

static POOL: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

/// A buffer of `len` bytes, its contents unspecified (the caller writes them
/// all): a recycled one when one of a fitting capacity is free.
pub fn take(len: usize) -> Vec<u8> {
    if len >= MIN {
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        // the smallest that fits, and not more than twice the need
        let best = pool.iter().enumerate()
            .filter(|(_, v)| v.capacity() >= len && v.capacity() / 2 <= len)
            .min_by_key(|(_, v)| v.capacity())
            .map(|(k, _)| k);
        if let Some(k) = best {
            let mut v = pool.swap_remove(k);
            drop(pool);
            // shrinking keeps the old bytes; growing within the capacity zeroes only the new part
            v.resize(len, 0);
            return v;
        }
    }
    vec![0; len]
}

/// Return an upload's bytes once the backend has written them.
pub fn give(mut v: Vec<u8>) {
    if v.capacity() < MIN {
        return;
    }
    v.clear();
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let held: usize = pool.iter().map(|v| v.capacity()).sum();
    if held + v.capacity() <= CAP {
        pool.push(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A given buffer comes back for a fitting size, at its length; small,
    /// too large and too small ones are not pooled or not chosen.
    #[test]
    fn a_given_buffer_is_taken_again_at_the_asked_length() {
        let v = take(1 << 20);
        assert_eq!(v.len(), 1 << 20);
        let p = v.as_ptr();
        give(v);
        let w = take(900 * 1024);
        assert_eq!((w.as_ptr(), w.len()), (p, 900 * 1024));
        give(w);
        // more than twice the need: not this one
        assert_ne!(take(100 * 1024).as_ptr(), p);
        let w = take(1 << 20);
        assert_eq!(w.as_ptr(), p);
        assert_eq!(take(16).len(), 16);
        give(w);
    }
}
