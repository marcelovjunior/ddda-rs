//! What a material does when drawn: its MFX feature switches, textures,
//! constant-buffer values and render states, resolved through the shader
//! package (`mtshader`), plus the rules the original pixel shaders apply
//! to them. See `docs/SHADING.md` for the evidence.
//!
//! The rules come from the compiled D3D9 shaders of the stage shader
//! package (`sc\dx9\stage100`, an `rShaderPackage`), disassembled with
//! D3DCompiler. Every material-pass pixel shader there that samples
//! `tAlbedoMap`, reads the light-accumulation buffer and has a
//! vertex-alpha input computes:
//!
//! ```text
//! colour = albedo.rgb * fAlbedoColor * (SH ambient + light buffer + sun)
//!          * fDiffuseColor * occlusion
//! alpha  = albedo.a * vertex alpha [* transparency map .g] * fTransparency
//!          [* fGlobalTransparency]
//! ```
//!
//! *Not traced:* which shader of the package a given feature set selects
//! (the package's permutation lookup), so the rules are those shared by
//! every variant with the inputs above, not of one proven permutation.

use std::collections::HashMap;

use mtshader::{BlendFactor, BlendState, DepthStencilState, RasterizerState, ShaderPackage};

use crate::{AlphaTest, CommandKind, Material, MaterialList};

/// Where a material's alpha comes from (MFX feature `FTransparency`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlphaSource {
    /// `FTransparency` (the default variant): alpha is not used by the
    /// material (the shader writes the global transparency only).
    None,
    /// `FTransparencyAlpha`: albedo alpha x vertex alpha x `fTransparency`.
    Albedo,
    /// `FTransparencyMap`: as `Albedo`, times one channel of
    /// `tTransparencyMap`, sampled with its own UV set.
    Map {
        /// `tTransparencyMap` texture path.
        texture: String,
        /// Layout alias of the UV set (`UV_Primary`, `UV_Extend`, ...),
        /// from `FUVTransparencyMap`.
        uv: String,
        /// Channel index (0 = R). The only variant in the data,
        /// `FChannelTransparencyMap`, reads G in the compiled shaders.
        channel: u8,
    },
    /// Another variant (`FTransparencyVolume`, ...): not traced.
    Other(String),
}

/// The drawable description of one MRL material.
#[derive(Debug, Clone, PartialEq)]
pub struct Shading {
    /// Feature switch name -> chosen variant name (`FAlbedo` -> `FAlbedoMap`).
    pub features: HashMap<String, String>,
    /// Texture slot name -> texture path (`tAlbedoMap` -> `scr\...\x_BM`).
    pub textures: HashMap<String, String>,
    /// Layout alias of the UV set the albedo map uses (`FUVAlbedoMap`).
    pub albedo_uv: String,
    /// Multiplier on the albedo texture: `$Globals.fAlbedoColor` when
    /// `FAlbedo` is `FAlbedoMap`, `FAlbedoMapAdd` or
    /// `FAlbedoMapBlendMaxAlpha` (the variants whose pixel shaders were
    /// read), else 1. Linear RGB.
    pub albedo_color: [f32; 3],
    /// `$Globals.fAlbedoBlendColor` when `FAlbedo` is `FAlbedoMapAdd`, else
    /// 0: the albedo becomes `albedo * fAlbedoColor + blend.rgb * blend.a *
    /// rgb * w`, with `blend` = `tAlbedoBlendMap` (character pixel shaders
    /// at file offsets 0x31BEA8, 0x40C494 of the root shader package).
    pub albedo_blend_color: [f32; 4],
    /// `$Globals.fSpecularColor` (default 1).
    pub specular_color: [f32; 3],
    /// `$Globals.fShininess`, the specular exponent (default 16).
    pub shininess: f32,
    /// Multiplier on the lit diffuse colour: `CBMaterial.fDiffuseColor`.
    pub diffuse_color: [f32; 3],
    /// `CBMaterial.fTransparency`, a factor on the alpha.
    pub transparency: f32,
    /// `CBMaterial.fReflectiveColor`, the tint of the environment
    /// reflection (`FReflect*` variants).
    pub reflective_color: [f32; 3],
    /// `$Globals.fFresnelSchlick`, the reflection's Fresnel F0
    /// (`FFresnelSchlick`).
    pub fresnel: f32,
    /// `CBMaterial.fUVTransform` (a row-major float4x2): row 0 gives
    /// `u' = r.x*u + r.y*v + r.w`, row 1 gives `v'` (z unused), as the
    /// vertex shaders apply it. Set only when `FUVTransformPrimary` is
    /// `FUVTransformOffset`: 156 of the 176 non-identity transforms in the
    /// data have that variant, and 13,452 of the 13,471 default-variant
    /// materials hold the identity (*assumed* the default ignores it).
    pub uv_transform: Option<[[f32; 4]; 2]>,
    pub alpha: AlphaSource,
    pub blend: Option<BlendState>,
    pub depth: Option<DepthStencilState>,
    pub raster: Option<RasterizerState>,
    /// Alpha test from the material header (`Material::alpha_test`).
    pub alpha_test: Option<AlphaTest>,
}

impl Shading {
    pub fn feature(&self, switch: &str) -> Option<&str> {
        self.features.get(switch).map(String::as_str)
    }

    /// Ordinary tangent-space normal map on the primary UV set. Other
    /// bump variants (detail/water/edit blends) need their own shader rules.
    /// Decode/basis: sc/DX9/Stage200 PS 0x6171f8, VS 0x1381d4.
    pub fn primary_normal_map(&self) -> Option<&str> {
        (self.feature("FBump") == Some("FBumpNormalMap")
            && self.feature("FUVNormalMap") == Some("FUVPrimary")
            && self.albedo_uv == "UV_Primary")
            .then(|| self.textures.get("tNormalMap").map(String::as_str)).flatten()
    }

    /// Standard alpha blending (`BSBlendAlpha`: SrcAlpha, InvSrcAlpha).
    pub fn blends_alpha(&self) -> bool {
        self.blend
            .is_some_and(|b| b.enable && b.src == BlendFactor::SrcAlpha && b.dst == BlendFactor::InvSrcAlpha)
    }

    /// Whether the alpha (and so the vertex alpha) is used at all.
    pub fn uses_alpha(&self) -> bool {
        matches!(self.alpha, AlphaSource::Albedo | AlphaSource::Map { .. })
    }
}

/// UV alias read by a `FUV...` feature variant (`FUVExtend` -> `UV_Extend`).
pub fn uv_alias(variant: &str) -> String {
    match variant.strip_prefix("FUV") {
        Some(rest) => format!("UV_{rest}"),
        None => "UV_Primary".to_string(),
    }
}

/// Resolves one material of `list` through `pkg`.
pub fn describe(m: &Material<'_>, list: &MaterialList<'_>, pkg: &ShaderPackage) -> Shading {
    let name = |id: u32| pkg.object(id).map(|o| o.name.clone());
    let mut features = HashMap::new();
    let mut textures = HashMap::new();
    let mut buffers: HashMap<String, (&mtshader::ConstantBuffer, &[u8])> = HashMap::new();
    for c in &m.commands {
        let Some(obj) = name(c.object) else { continue };
        match c.kind {
            CommandKind::Feature => {
                if let Some(v) = name(c.value) {
                    features.insert(obj, v);
                }
            }
            CommandKind::Texture => {
                if let Some(t) = list.texture(c) {
                    textures.insert(obj, t.path.clone());
                }
            }
            CommandKind::ConstantBuffer => {
                if let Some(cb) = pkg.constant_buffer(c.object) {
                    if let Some(d) = m.constant_buffer(c, cb.byte_size()) {
                        buffers.insert(obj, (cb, d));
                    }
                }
            }
            _ => {}
        }
    }
    let read = |cb: &str, param: &str| -> Option<Vec<f32>> {
        let (layout, data) = buffers.get(cb)?;
        layout.read(param, data)
    };
    let rgb = |v: Option<Vec<f32>>| v.and_then(|v| v.get(..3).map(|s| [s[0], s[1], s[2]]));
    let feature = |f: &str| features.get(f).map(String::as_str);
    let albedo_color = if matches!(
        feature("FAlbedo"),
        Some("FAlbedoMap") | Some("FAlbedoMapAdd") | Some("FAlbedoMapBlendMaxAlpha")
    ) {
        rgb(read("$Globals", "fAlbedoColor")).unwrap_or([1.0; 3])
    } else {
        [1.0; 3]
    };
    let albedo_blend_color = match (feature("FAlbedo"), read("$Globals", "fAlbedoBlendColor")) {
        (Some("FAlbedoMapAdd"), Some(v)) if v.len() == 4 => [v[0], v[1], v[2], v[3]],
        _ => [0.0; 4],
    };
    let specular_color = rgb(read("$Globals", "fSpecularColor")).unwrap_or([1.0; 3]);
    let shininess = read("$Globals", "fShininess")
        .and_then(|v| v.first().copied())
        .unwrap_or(16.0);
    let diffuse_color = rgb(read("CBMaterial", "fDiffuseColor")).unwrap_or([1.0; 3]);
    let reflective_color = rgb(read("CBMaterial", "fReflectiveColor")).unwrap_or([1.0; 3]);
    let fresnel = read("$Globals", "fFresnelSchlick")
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0);
    let transparency = read("CBMaterial", "fTransparency")
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0);
    let uv_transform = if feature("FUVTransformPrimary") == Some("FUVTransformOffset") {
        read("CBMaterial", "fUVTransform")
            .filter(|v| v.len() == 8)
            .map(|v| [[v[0], v[1], v[2], v[3]], [v[4], v[5], v[6], v[7]]])
    } else {
        None
    };
    let alpha = match feature("FTransparency") {
        None | Some("FTransparency") => AlphaSource::None,
        Some("FTransparencyAlpha") => AlphaSource::Albedo,
        Some("FTransparencyMap") => match textures.get("tTransparencyMap") {
            Some(t) => AlphaSource::Map {
                texture: t.clone(),
                uv: uv_alias(feature("FUVTransparencyMap").unwrap_or("FUVPrimary")),
                channel: 1,
            },
            None => AlphaSource::Albedo,
        },
        Some(v) => AlphaSource::Other(v.to_string()),
    };
    let albedo_uv = uv_alias(feature("FUVAlbedoMap").unwrap_or("FUVPrimary"));
    Shading {
        albedo_uv,
        albedo_color,
        albedo_blend_color,
        specular_color,
        shininess,
        diffuse_color,
        transparency,
        reflective_color,
        fresnel,
        uv_transform,
        alpha,
        blend: pkg.blend_state(m.blend_state).copied(),
        depth: pkg.depth_stencil_state(m.depth_stencil_state).copied(),
        raster: pkg.rasterizer_state(m.rasterizer_state).copied(),
        alpha_test: m.alpha_test(),
        features,
        textures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, TextureRef};
    use mtshader::{object_id, ConstantBuffer, Entry, Param, ParamType};

    fn entry(pkg: &mut ShaderPackage, name: &str, index: u16, kind: u8) -> u32 {
        pkg.entries.insert(
            index,
            Entry {
                index,
                kind,
                name: name.to_string(),
                kind_name: String::new(),
            },
        );
        object_id(name, index)
    }

    fn param(name: &str, columns: u8, rows: u16, offset: u16) -> Param {
        Param {
            name: name.to_string(),
            ty: ParamType::Float,
            columns,
            rows,
            array: 1,
            offset,
            type_word: 0,
            bits: 0,
            class: 0,
        }
    }

    /// A terrain overlay as in `st100_58m50n` (`XfB_W_Y_0__m00_1`):
    /// FAlbedoMap, FTransparencyAlpha, BSBlendAlpha / DSZTest / RSMesh,
    /// CBMaterial with defaults except a UV offset.
    #[test]
    fn describes_terrain_overlay() {
        let mut pkg = ShaderPackage::default();
        let f_albedo = entry(&mut pkg, "FAlbedo", 1, 2);
        let f_albedo_map = entry(&mut pkg, "FAlbedoMap", 2, 2);
        let f_tr = entry(&mut pkg, "FTransparency", 3, 2);
        let f_tr_alpha = entry(&mut pkg, "FTransparencyAlpha", 4, 2);
        let f_uvt = entry(&mut pkg, "FUVTransformPrimary", 5, 2);
        let f_uvt_off = entry(&mut pkg, "FUVTransformOffset", 6, 2);
        let t_albedo = entry(&mut pkg, "tAlbedoMap", 7, 1);
        let cb_mat = entry(&mut pkg, "CBMaterial", 8, 0);
        let bs = entry(&mut pkg, "BSBlendAlpha", 9, 4);
        let ds = entry(&mut pkg, "DSZTest", 10, 5);
        let rs = entry(&mut pkg, "RSMesh", 11, 6);
        pkg.constant_buffers.insert(
            8,
            ConstantBuffer {
                name: "CBMaterial".into(),
                index: 8,
                size: 16,
                params: vec![
                    param("fDiffuseColor", 3, 1, 0),
                    param("fTransparency", 1, 1, 3),
                    param("fUVTransform", 4, 2, 8),
                ],
                defaults: vec![],
            },
        );
        pkg.blend_states
            .insert(9, BlendState::from_words([0x000A_0802, 1, 0x0F0F_0F0F, 0x0F0F_0F0F]));
        pkg.depth_stencil_states
            .insert(10, DepthStencilState::from_words([0x007F_FF8D, 0x0070_0070]));
        pkg.rasterizer_states
            .insert(11, RasterizerState::from_words([0x291, 0]));
        let cmd = |kind, object, value| Command { kind, object, value };
        let commands = vec![
            cmd(CommandKind::Feature, f_albedo, f_albedo_map),
            cmd(CommandKind::Texture, t_albedo, 1),
            cmd(CommandKind::Feature, f_tr, f_tr_alpha),
            cmd(CommandKind::Feature, f_uvt, f_uvt_off),
            cmd(CommandKind::ConstantBuffer, cb_mat, 0x40),
        ];
        let mut block = vec![0u8; 0x40];
        for f in [0.5f32, 0.5, 0.5, 0.75, 0., 0., 0., 0., 2., 0., 0., 0.25, 0., 1., 0., 0.5] {
            block.extend(f.to_le_bytes());
        }
        let m = Material {
            class_hash: 0,
            name_hash: 0,
            blend_state: bs,
            depth_stencil_state: ds,
            rasterizer_state: rs,
            bits: [0; 2],
            blend_factor: [0.0; 4],
            commands,
            command_block: &block,
            anim_data: &[],
        };
        let list = MaterialList {
            shader_package_hash: 0,
            textures: vec![TextureRef {
                type_hash: 0,
                path: "scr\\x_BM".into(),
            }],
            materials: vec![],
        };
        let s = describe(&m, &list, &pkg);
        assert_eq!(s.textures["tAlbedoMap"], "scr\\x_BM");
        assert_eq!(s.alpha, AlphaSource::Albedo);
        assert!(s.blends_alpha() && s.uses_alpha());
        assert_eq!(s.diffuse_color, [0.5; 3]);
        assert_eq!(s.transparency, 0.75);
        // No $Globals: albedo colour defaults to 1.
        assert_eq!(s.albedo_color, [1.0; 3]);
        assert_eq!(s.uv_transform, Some([[2.0, 0.0, 0.0, 0.25], [0.0, 1.0, 0.0, 0.5]]));
        assert_eq!(s.albedo_uv, "UV_Primary");
        let d = s.depth.unwrap();
        assert!(d.depth_test && !d.depth_write);
        let mut s = s;
        s.textures.insert("tNormalMap".into(), "scr\\x_NM".into());
        assert_eq!(s.primary_normal_map(), None, "a bound map alone must not enable bump mapping");
        s.features.insert("FBump".into(), "FBumpNormalMap".into());
        s.features.insert("FUVNormalMap".into(), "FUVPrimary".into());
        assert_eq!(s.primary_normal_map(), Some("scr\\x_NM"));
        s.features.insert("FUVNormalMap".into(), "FUVSecondary".into());
        assert_eq!(s.primary_normal_map(), None, "unhandled UV sets cannot silently use albedo UV");
        s.features.insert("FUVNormalMap".into(), "FUVPrimary".into());
        s.features.insert("FBump".into(), "FBumpDetailNormalMap".into());
        assert_eq!(s.primary_normal_map(), None, "detail blend needs its own shader path");
    }

    /// A character material as in the `model\pl` lists: FAlbedoMapAdd
    /// takes fAlbedoColor and fAlbedoBlendColor from $Globals.
    #[test]
    fn describes_albedo_add() {
        let mut pkg = ShaderPackage::default();
        let f_albedo = entry(&mut pkg, "FAlbedo", 1, 2);
        let f_add = entry(&mut pkg, "FAlbedoMapAdd", 2, 2);
        let globals = entry(&mut pkg, "$Globals", 3, 0);
        pkg.constant_buffers.insert(
            3,
            ConstantBuffer {
                name: "$Globals".into(),
                index: 3,
                size: 48,
                params: vec![
                    param("fAlbedoColor", 3, 1, 0),
                    param("fAlbedoBlendColor", 4, 1, 4),
                    param("fSpecularColor", 3, 1, 36),
                    param("fShininess", 1, 1, 39),
                ],
                defaults: vec![],
            },
        );
        let commands = vec![
            Command { kind: CommandKind::Feature, object: f_albedo, value: f_add },
            Command { kind: CommandKind::ConstantBuffer, object: globals, value: 0 },
        ];
        let mut vals = [0.0f32; 48];
        vals[..3].copy_from_slice(&[1.139, 1.035, 0.932]);
        vals[4..8].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
        vals[36..40].copy_from_slice(&[0.5, 0.5, 0.5, 30.0]);
        let block: Vec<u8> = vals.iter().flat_map(|f| f.to_le_bytes()).collect();
        let m = Material {
            class_hash: 0,
            name_hash: 0,
            blend_state: 0,
            depth_stencil_state: 0,
            rasterizer_state: 0,
            bits: [0; 2],
            blend_factor: [0.0; 4],
            commands,
            command_block: &block,
            anim_data: &[],
        };
        let list = MaterialList { shader_package_hash: 0, textures: vec![], materials: vec![] };
        let s = describe(&m, &list, &pkg);
        assert_eq!(s.albedo_color, [1.139, 1.035, 0.932]);
        assert_eq!(s.albedo_blend_color, [1.0; 4]);
        assert_eq!(s.specular_color, [0.5; 3]);
        assert_eq!(s.shininess, 30.0);
    }

    #[test]
    fn uv_aliases() {
        assert_eq!(uv_alias("FUVExtend"), "UV_Extend");
        assert_eq!(uv_alias("FUVPrimary"), "UV_Primary");
    }
}
