//! Exact WGSL frontend output, cached locally across launches. The key includes
//! the complete generated source (therefore every translation option). wgpu
//! still validates the IR with the actual device capabilities on every load.
use bincode::Options;
use std::path::{Path, PathBuf};

// Naga's serde representation is not a stable interchange format. Keep the
// dependency pinned and bump this namespace whenever its version/features change.
const VERSION: &str = "naga-29.0.4-bincode1-v1";
const LIMIT: u64 = 64 * 1024 * 1024;

/// Preserve the existing WGSL path when caching is disabled/unavailable.
/// Cache I/O errors are harmless: the freshly parsed module is still usable.
pub fn source(wgsl: String) -> Result<wgpu::ShaderSource<'static>, String> {
    source_at(wgsl, default_dir().as_deref())
}

fn source_at(wgsl: String, dir: Option<&Path>) -> Result<wgpu::ShaderSource<'static>, String> {
    let Some(dir) = dir else {
        return Ok(wgpu::ShaderSource::Wgsl(wgsl.into()));
    };
    let module = match load(dir, &wgsl) {
        Some(module) => module,
        None => {
            let module = naga::front::wgsl::parse_str(&wgsl).map_err(|e| e.emit_to_string(&wgsl))?;
            let _ = save(dir, &wgsl, &module);
            module
        }
    };
    Ok(wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)))
}

pub fn default_dir() -> Option<PathBuf> {
    if std::env::var_os("DDGPU_SHADER_CACHE_DISABLE").is_some() {
        return None;
    }
    std::env::var_os("DDGPU_SHADER_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("ddda-rs").join("shader-modules"))
        })
}

fn path(dir: &Path, source: &str) -> PathBuf {
    dir.join(VERSION)
        .join(format!("{}.bin", blake3::hash(source.as_bytes()).to_hex()))
}

pub fn load(dir: &Path, source: &str) -> Option<naga::Module> {
    let path = path(dir, source);
    if std::fs::metadata(&path).ok()?.len() > LIMIT {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let (source_digest, bytes) = bytes.split_at_checked(32)?;
    if source_digest != blake3::hash(source.as_bytes()).as_bytes() {
        return None;
    }
    let (digest, payload) = bytes.split_at_checked(32)?;
    if digest != blake3::hash(payload).as_bytes() {
        return None;
    }
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(LIMIT)
        .reject_trailing_bytes()
        .deserialize(payload)
        .ok()
}

pub fn save(dir: &Path, source: &str, module: &naga::Module) -> std::io::Result<()> {
    use std::io::Write;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = path(dir, source);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let payload = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(LIMIT)
        .serialize(module)
        .map_err(std::io::Error::other)?;
    let tmp = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(blake3::hash(source.as_bytes()).as_bytes())?;
        file.write_all(blake3::hash(&payload).as_bytes())?;
        file.write_all(&payload)?;
        drop(file);
        std::fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_roundtrip_and_corruption_miss() {
        let dir = std::env::temp_dir().join(format!("ddgpu-module-test-{}", std::process::id()));
        let source = "@compute @workgroup_size(1) fn main() {}";
        let module = naga::front::wgsl::parse_str(source).unwrap();
        save(&dir, source, &module).unwrap();
        let restored = load(&dir, source).unwrap();
        assert_eq!(format!("{module:?}"), format!("{restored:?}"));
        assert!(load(&dir, &format!("{source} ")).is_none());
        let other = format!("{source} ");
        std::fs::copy(path(&dir, source), path(&dir, &other)).unwrap();
        assert!(load(&dir, &other).is_none());
        let mut corrupt = std::fs::read(path(&dir, source)).unwrap();
        *corrupt.last_mut().unwrap() ^= 1;
        std::fs::write(path(&dir, source), corrupt).unwrap();
        assert!(load(&dir, source).is_none());
        std::fs::write(path(&dir, source), b"truncated").unwrap();
        assert!(load(&dir, source).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn miss_hit_disabled_and_unwritable_keep_the_same_shader() {
        let dir = std::env::temp_dir().join(format!("ddgpu-source-test-{}", std::process::id()));
        let wgsl = "@compute @workgroup_size(1) fn main() {}";
        let expected = bincode::serialize(&naga::front::wgsl::parse_str(wgsl).unwrap()).unwrap();
        for _ in 0..2 {
            let wgpu::ShaderSource::Naga(module) = source_at(wgsl.into(), Some(&dir)).unwrap() else {
                panic!("IR expected")
            };
            assert_eq!(bincode::serialize(module.as_ref()).unwrap(), expected);
        }
        assert!(matches!(
            source_at(wgsl.into(), None).unwrap(),
            wgpu::ShaderSource::Wgsl(_)
        ));
        std::fs::remove_dir_all(&dir).unwrap();
        // A file in place of the directory makes persistence fail.
        std::fs::write(&dir, b"not a directory").unwrap();
        let wgpu::ShaderSource::Naga(module) = source_at(wgsl.into(), Some(&dir)).unwrap() else {
            panic!("IR expected")
        };
        assert_eq!(bincode::serialize(module.as_ref()).unwrap(), expected);
        assert!(source_at("invalid WGSL".into(), Some(&dir)).is_err());
        std::fs::remove_file(dir).unwrap();
    }
}
