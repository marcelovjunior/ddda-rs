//! Ordinary `uEm5000` cyclops data and think initialization.
//! This is not yet an action driver: weapon children, armor and clinging
//! callbacks require their own runtime observations before combat is enabled.

/// Enemy kind written by constructor 0x009780d0 (2017-12-13 Steam).
pub const ENEMY_KIND: u8 = 0x3f;

#[derive(Debug, Clone)]
pub struct Catalogue {
    pub actions: Vec<crate::chimera::ActionRecord>,
    pub resources: Vec<crate::chimera::ResourceRequest>,
    /// Hit-region to life-pool identity, not an array index.
    pub hit_to_pool: Vec<u32>,
}

impl Catalogue {
    /// Read the installed executable's bulk descriptors. Initializer
    /// 0x00978820 registers actions at 0x01847d38 and 44 resource slots at
    /// 0x01849cf0; the null resource slot is skipped by 0x00889de0.
    pub fn read(exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let names = exe.class_names();
        let class = |dti: u32| {
            names
                .get(&dti)
                .cloned()
                .ok_or_else(|| format!("cyclops: unknown DTI {dti:#x}"))
        };
        let mut actions = Vec::new();
        // 0x007f19c0 reads 0x44-byte descriptors until a null DTI.
        for index in 0..115 {
            let address = 0x0184_7d38 + index * 0x44;
            actions.push(crate::chimera::ActionRecord {
                class: class(exe.u32(address)?)?,
                argument: exe.u32(address + 4)?,
            });
        }
        if exe.u32(0x0184_9bc4)? != 0 {
            return Err("cyclops action terminator is missing".into());
        }
        let mut resources = Vec::new();
        for index in 0..44 {
            let address = 0x0184_9cf0 + index * 12;
            let path = exe.u32(address)?;
            if path != 0 {
                resources.push(crate::chimera::ResourceRequest {
                    path: exe.cstr(path)?.to_owned(),
                    class: class(exe.u32(address + 4)?)?,
                    flags: initialized_resource_flags(index, exe.u32(address + 8)?),
                });
            }
        }
        Ok(Self {
            actions,
            resources,
            hit_to_pool: exe.u32s(0x014f_f680, 13)?,
        })
    }
}

// Translated from 0x01357a00..0x01357cfa (DDDA.exe 2017-12-13 Steam).
fn initialized_resource_flags(index: u32, flags: u32) -> u32 {
    if index <= 20 || index == 37 || index >= 39 {
        (flags & !4) | 3
    } else {
        (flags & !5) | 2
    }
}

#[derive(Debug, Clone)]
pub struct ThinkInitialization {
    pub tables: ddai::thinktbl::Tables,
    pub current: u32,
    pub default: u32,
    pub base: u32,
    pub lock: bool,
}

impl ThinkInitialization {
    /// `controller` is present only when actor +0x5c14 enables the index
    /// at +0x5c18. No target, motion or family flags are fabricated here.
    // Translated from 0x00978820 (DDDA.exe 2017-12-13 Steam).
    pub fn read(exe: &ddexe::Exe<'_>, controller: Option<u32>) -> Result<Self, String> {
        let default = 0x0173_4b48;
        let current = match controller {
            Some(index) => {
                let address = index
                    .checked_mul(4)
                    .and_then(|offset| 0x0173_6ee8u32.checked_add(offset))
                    .ok_or("cyclops controller index overflow")?;
                exe.u32(address)?
            }
            None => default,
        };
        if exe.section_of(current) != Some(".data") {
            return Err(format!(
                "cyclops controller root {current:#x} is not executable data"
            ));
        }
        let tables = ddai::thinktbl::Tables::read_roots(exe, vec![current, default], &[])
            .map_err(|error| error.to_string())?;
        // +0x789c is overwritten with fallback after the enabled branch;
        // +0x78a0 remains the selected base, unlike the chimera initializer.
        Ok(Self {
            tables,
            current,
            default,
            base: current,
            lock: controller.is_some(),
        })
    }

    pub fn manager(&self, scale: f32) -> ddai::thinktbl::ThinkMgr {
        let mut manager = ddai::thinktbl::ThinkMgr::new();
        manager.table = self.current;
        manager.root = self.current;
        manager.default_table = self.default;
        manager.base_table = self.base;
        manager.lock = self.lock;
        manager.scale_on = scale > 0.0 && scale != 1.0;
        manager.scale = if manager.scale_on { scale } else { 1.0 };
        manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_fixups_preserve_high_bits_and_separate_child_requests() {
        for index in 0..44 {
            let bound = index <= 20 || index == 37 || index >= 39;
            assert_eq!(
                initialized_resource_flags(index, 0xffff_ffff),
                if bound { 0xffff_fffb } else { 0xffff_fffa }
            );
            assert_eq!(initialized_resource_flags(index, 0), if bound { 3 } else { 2 });
        }
    }
}
