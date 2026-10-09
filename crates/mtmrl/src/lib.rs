//! Reader for MT Framework `rMaterial` files (`.mrl`, version 0x20) as used
//! by Dragon's Dogma: Dark Arisen on PC.
//!
//! Field names follow Albam's `mrl.ksy` (MIT). Checked on all 3,972
//! distinct MRL files of the Steam install (`ddtool mrl-scan`):
//!
//! ```text
//! 0x00 "MRL\0", u32 version 0x20, u32 materials, u32 textures,
//!      u32 shader-package hash (0xB46006D5 in every file), u32 texture
//!      table offset (0x1C), u32 material table offset
//! textures  (0x4C each): u32 type hash (rTexture or rRenderTargetTexture),
//!           u32, u32 (always 0), char[64] path without extension
//! materials (0x3C each): u32 material class (JAMCRC of an MtDTI name, e.g.
//!           nDraw::MaterialStdEst), u32 JAMCRC of the material name (as
//!           listed in the MOD), u32 command bytes, u32 blend / depth-stencil /
//!           rasterizer state ids, u32 bits (command count = low 12), u32 bits,
//!           f32[4] blend factor, u32 animation bytes, u32 command offset,
//!           u32 animation offset
//! commands  (12 each): u32 (type = low 4 bits, entry index = high 12 bits),
//!           u32 value, u32 shader-object id (see `mtshader`)
//! ```
//!
//! Command types: 0 feature switch (value = id of the chosen variant, e.g.
//! `FAlbedo` = `FAlbedoMap`), 1 constant buffer (value = byte offset of its
//! data from the command list start), 2 sampler state (value = id),
//! 3 texture (value = 1-based index into the texture table, 0 = none).
//!
//! Constant-buffer data follows the command list, each buffer 16-byte
//! aligned and laid out as its MFX `CB...` entry says (`mtshader`): checked
//! on all 48,114 buffer commands of the 17,787 materials (one MRL per
//! resource path), which tile the command block exactly. `shading` resolves a material into
//! features, textures, constants and render states.

use std::fmt;

pub mod shading;

pub const MAGIC: [u8; 4] = *b"MRL\0";
pub const VERSION: u32 = 0x20;
pub const TEXTURE_SIZE: usize = 0x4C;
pub const MATERIAL_SIZE: usize = 0x3C;
pub const COMMAND_SIZE: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadMagic,
    BadVersion(u32),
    Truncated {
        what: &'static str,
        needed: usize,
        have: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadMagic => write!(f, "not an MRL file"),
            Error::BadVersion(v) => write!(f, "unsupported MRL version {v:#x}"),
            Error::Truncated { what, needed, have } => {
                write!(f, "{what}: needs {needed} bytes, file has {have}")
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureRef {
    pub type_hash: u32,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Feature,
    ConstantBuffer,
    Sampler,
    Texture,
    Other(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub kind: CommandKind,
    /// Shader object this command sets (`mtshader` id).
    pub object: u32,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material<'a> {
    pub class_hash: u32,
    pub name_hash: u32,
    pub blend_state: u32,
    pub depth_stencil_state: u32,
    pub rasterizer_state: u32,
    pub bits: [u32; 2],
    pub blend_factor: [f32; 4],
    pub commands: Vec<Command>,
    /// Command list plus constant-buffer data (offsets in commands are
    /// relative to its start).
    pub command_block: &'a [u8],
    pub anim_data: &'a [u8],
}

impl Material<'_> {
    /// Constant-buffer bytes for a command of kind `ConstantBuffer`.
    pub fn constant_buffer(&self, c: &Command, len: usize) -> Option<&[u8]> {
        let s = c.value as usize;
        self.command_block.get(s..s + len)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialList<'a> {
    pub shader_package_hash: u32,
    pub textures: Vec<TextureRef>,
    pub materials: Vec<Material<'a>>,
}

impl MaterialList<'_> {
    /// Material by its name as listed in the model (matched by JAMCRC).
    pub fn by_name(&self, name: &str) -> Option<&Material<'_>> {
        let h = jamcrc(name.as_bytes());
        self.materials.iter().find(|m| m.name_hash == h)
    }

    /// Texture referenced by a `Texture` command (1-based; 0 = none).
    pub fn texture(&self, c: &Command) -> Option<&TextureRef> {
        (c.value as usize)
            .checked_sub(1)
            .and_then(|i| self.textures.get(i))
    }
}

pub fn jamcrc(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub fn parse(b: &[u8]) -> Result<MaterialList<'_>, Error> {
    let get = |what: &'static str, o: usize, n: usize| {
        b.get(o..o + n).ok_or(Error::Truncated {
            what,
            needed: o + n,
            have: b.len(),
        })
    };
    let u32_at = |what, o| get(what, o, 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
    if get("magic", 0, 4)? != MAGIC {
        return Err(Error::BadMagic);
    }
    let version = u32_at("header", 4)?;
    if version != VERSION {
        return Err(Error::BadVersion(version));
    }
    let n_mat = u32_at("header", 8)? as usize;
    let n_tex = u32_at("header", 12)? as usize;
    let shader_package_hash = u32_at("header", 16)?;
    let o_tex = u32_at("header", 20)? as usize;
    let o_mat = u32_at("header", 24)? as usize;
    let mut textures = Vec::with_capacity(n_tex);
    for i in 0..n_tex {
        let o = o_tex + i * TEXTURE_SIZE;
        let path = get("textures", o + 12, 64)?;
        let end = path.iter().position(|&c| c == 0).unwrap_or(64);
        textures.push(TextureRef {
            type_hash: u32_at("textures", o)?,
            path: String::from_utf8_lossy(&path[..end]).into_owned(),
        });
    }
    let mut materials = Vec::with_capacity(n_mat);
    for i in 0..n_mat {
        let o = o_mat + i * MATERIAL_SIZE;
        let f = |k: usize| u32_at("materials", o + 4 * k);
        let cmd_size = f(2)? as usize;
        let bits = [f(6)?, f(7)?];
        let anim_size = f(12)? as usize;
        let o_cmd = f(13)? as usize;
        let o_anim = f(14)? as usize;
        let command_block = get("commands", o_cmd, cmd_size)?;
        let n_cmd = (bits[0] & 0xFFF) as usize;
        let mut commands = Vec::with_capacity(n_cmd);
        for k in 0..n_cmd {
            let c = o_cmd + k * COMMAND_SIZE;
            let w0 = u32_at("commands", c)?;
            commands.push(Command {
                kind: match w0 & 0xF {
                    0 => CommandKind::Feature,
                    1 => CommandKind::ConstantBuffer,
                    2 => CommandKind::Sampler,
                    3 => CommandKind::Texture,
                    k => CommandKind::Other(k as u8),
                },
                value: u32_at("commands", c + 4)?,
                object: u32_at("commands", c + 8)?,
            });
        }
        let mut blend_factor = [0f32; 4];
        for (k, x) in blend_factor.iter_mut().enumerate() {
            *x = f32::from_bits(f(8 + k)?);
        }
        materials.push(Material {
            class_hash: f(0)?,
            name_hash: f(1)?,
            blend_state: f(3)?,
            depth_stencil_state: f(4)?,
            rasterizer_state: f(5)?,
            bits,
            blend_factor,
            commands,
            command_block,
            anim_data: if anim_size == 0 {
                &[]
            } else {
                get("animation", o_anim, anim_size)?
            },
        });
    }
    Ok(MaterialList {
        shader_package_hash,
        textures,
        materials,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jamcrc_matches_dti_type_hash() {
        // rTexture's ARC type hash is JAMCRC("rTexture") & 0x7FFFFFFF.
        assert_eq!(jamcrc(b"rTexture") & 0x7FFF_FFFF, 0x241F_5DEB);
    }

    #[test]
    fn parses_synthetic_list() {
        let mut b = Vec::new();
        b.extend(MAGIC);
        for v in [VERSION, 1, 1, 0xB460_06D5, 0x1C, 0x1C + TEXTURE_SIZE as u32] {
            b.extend(v.to_le_bytes());
        }
        b.extend(0x241F_5DEBu32.to_le_bytes());
        b.extend([0u8; 8]);
        let mut path = b"scr\\x_BM".to_vec();
        path.resize(64, 0);
        b.extend(path);
        let o_cmd = (b.len() + MATERIAL_SIZE) as u32;
        let words = [7, jamcrc(b"mat0"), 12, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, o_cmd, 0];
        for w in words {
            b.extend(w.to_le_bytes());
        }
        b.extend(3u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(0xABCD_E001u32.to_le_bytes());
        let l = parse(&b).unwrap();
        let m = l.by_name("mat0").unwrap();
        assert_eq!(m.commands[0].kind, CommandKind::Texture);
        assert_eq!(l.texture(&m.commands[0]).unwrap().path, "scr\\x_BM");
    }
}
