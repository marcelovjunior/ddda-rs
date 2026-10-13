//! Bounded read-ahead for immutable install archives. Host policy only;
//! guest byte counts, file position and APC delivery remain in files.rs.
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
};

const WINDOW: usize = 256 * 1024;
const BUDGET: usize = 8 * 1024 * 1024;

struct Block {
    start: u64,
    data: Arc<Vec<u8>>,
    used: u64,
}
#[derive(Default)]
pub(crate) struct ReadCache {
    blocks: HashMap<u32, Block>,
    bytes: usize,
    clock: u64,
    pub hits: u64,
    pub misses: u64,
    pub host_bytes: u64,
}
pub(crate) type Shared = Arc<Mutex<ReadCache>>;

impl ReadCache {
    pub fn remove(&mut self, handle: u32) {
        if let Some(block) = self.blocks.remove(&handle) {
            self.bytes -= block.data.capacity();
        }
    }
    fn get(&mut self, handle: u32, start: u64, count: usize) -> Option<(Arc<Vec<u8>>, usize)> {
        self.clock += 1;
        let block = self.blocks.get_mut(&handle)?;
        let offset = usize::try_from(start.checked_sub(block.start)?).ok()?;
        if offset.checked_add(count)? > block.data.len() {
            return None;
        }
        block.used = self.clock;
        self.hits += 1;
        Some((block.data.clone(), offset))
    }
    fn buffer(&mut self, handle: u32) -> Vec<u8> {
        let Some(block) = self.blocks.remove(&handle) else {
            return Vec::new();
        };
        self.bytes -= block.data.capacity();
        Arc::try_unwrap(block.data).unwrap_or_default()
    }
    fn insert(&mut self, handle: u32, start: u64, data: Arc<Vec<u8>>) {
        self.remove(handle);
        while self.bytes + data.capacity() > BUDGET {
            let Some((&oldest, _)) = self.blocks.iter().min_by_key(|(_, block)| block.used) else {
                break;
            };
            self.remove(oldest);
        }
        self.clock += 1;
        self.bytes += data.capacity();
        self.blocks.insert(
            handle,
            Block {
                start,
                data,
                used: self.clock,
            },
        );
    }
}

/// Read the requested range, retaining at most one forward window per
/// handle and eight MiB per runtime. Disk I/O never holds the cache mutex.
/// Only called for immutable .arc handles opened from the install itself.
pub(crate) fn read(
    cache: &Shared,
    file: &mut File,
    handle: u32,
    offset: u64,
    size: u64,
    out: &mut [u8],
) -> io::Result<()> {
    if out.is_empty() {
        file.seek(SeekFrom::Start(offset))?;
        return Ok(());
    }
    if out.len() > WINDOW {
        file.seek(SeekFrom::Start(offset))?;
        return file.read_exact(out);
    }
    let hit = cache.lock().unwrap().get(handle, offset, out.len());
    if let Some((data, start)) = hit {
        // Preserve the existing runtime's shared host-handle position.
        file.seek(SeekFrom::Start(offset + out.len() as u64))?;
        out.copy_from_slice(&data[start..start + out.len()]);
        crate::load_log::cache_read(true, 0);
        return Ok(());
    }
    let count = (size.saturating_sub(offset)).min(WINDOW as u64) as usize;
    let mut data = cache.lock().unwrap().buffer(handle);
    data.resize(count, 0);
    file.seek(SeekFrom::Start(offset))?;
    // A speculative read error must not turn an otherwise successful
    // requested read into a failure: retry just the original range.
    if file.read_exact(&mut data).is_err() {
        file.seek(SeekFrom::Start(offset))?;
        return file.read_exact(out);
    }
    file.seek(SeekFrom::Start(offset + out.len() as u64))?;
    out.copy_from_slice(&data[..out.len()]);
    let mut cache = cache.lock().unwrap();
    cache.misses += 1;
    cache.host_bytes += data.len() as u64;
    cache.insert(handle, offset, Arc::new(data));
    crate::load_log::cache_read(false, count as u64);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit CPU-only warm-file benchmark"]
    fn read_ahead_cost() {
        let path = std::env::temp_dir().join(format!("ddruntime-cache-cost-{}.bin", std::process::id()));
        std::fs::write(&path, vec![0x5a; 1024 * 1024]).unwrap();
        let mut file = File::open(&path).unwrap();
        for (label, request, repeat) in [
            ("sequential128k", 128 * 1024, false),
            ("sequential1k", 1024, false),
            ("repeated128k", 128 * 1024, true),
        ] {
            let mut out = vec![0; request];
            for sample in 0..5 {
                for mode in if sample % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let cache = Shared::default();
                    let start = std::time::Instant::now();
                    for _ in 0..100 {
                        for block in 0..8 {
                            let offset = if repeat { 0 } else { block * request as u64 };
                            if mode {
                                read(&cache, &mut file, 4, offset, 1024 * 1024, &mut out).unwrap();
                            } else {
                                file.seek(SeekFrom::Start(offset)).unwrap();
                                file.read_exact(&mut out).unwrap();
                            }
                            std::hint::black_box(&out);
                        }
                    }
                    let elapsed = start.elapsed().as_nanos() / 800;
                    let cache = cache.lock().unwrap();
                    eprintln!(
                        "cache-cost pattern={label} sample={sample} cache={mode} ns_per_read={elapsed} hits={} misses={} host_bytes={}",
                        cache.hits, cache.misses, cache.host_bytes
                    );
                }
            }
        }
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn ranges_tail_seek_and_bounded_eviction() {
        let path = std::env::temp_dir().join(format!("ddruntime-cache-{}.bin", std::process::id()));
        let bytes: Vec<_> = (0..WINDOW * 2 + 19)
            .map(|i| (i.wrapping_mul(31) >> 8) as u8)
            .collect();
        std::fs::write(&path, &bytes).unwrap();
        let mut file = File::open(&path).unwrap();
        let cache = Shared::default();
        for (offset, count) in [
            (0, 128 * 1024),
            (128 * 1024, 128 * 1024),
            (17, 99),
            (WINDOW, 128 * 1024),
            (WINDOW * 2, 19),
            (WINDOW * 2 + 19, 0),
            (0, WINDOW + 1),
        ] {
            let mut out = vec![0xcc; count];
            read(&cache, &mut file, 4, offset as u64, bytes.len() as u64, &mut out).unwrap();
            assert_eq!(out, bytes[offset..offset + count]);
            assert_eq!(file.stream_position().unwrap(), (offset + count) as u64);
            let guard = cache.lock().unwrap();
            assert_eq!(guard.bytes, guard.blocks.values().map(|b| b.data.capacity()).sum::<usize>());
            assert!(guard.bytes <= BUDGET);
        }
        assert_eq!(cache.lock().unwrap().hits, 2);
        for handle in 8..48 {
            let mut out = [0; 1];
            read(&cache, &mut file, handle, 0, bytes.len() as u64, &mut out).unwrap();
            assert!(cache.lock().unwrap().bytes <= BUDGET);
        }
        let mut guard = cache.lock().unwrap();
        assert!(!guard.blocks.contains_key(&4));
        assert_eq!(guard.blocks.len(), BUDGET / WINDOW);
        guard.remove(47);
        assert_eq!(guard.bytes, BUDGET - WINDOW);
        drop(guard);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
