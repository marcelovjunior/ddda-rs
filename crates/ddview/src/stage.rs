//! Converts stage cells (MOD + MRL + TEX from the user's install) into
//! Bevy meshes, materials and images.
//!
//! Placement follows `docs/WORLD.md`: cell `st<S>_<A>m<B>n` sits at
//! X = (B - B0) * 10000, Z = (A - A0) * 10000 game units from the origin
//! cell (A0, B0). Game units are taken as centimetres and scaled by 0.01
//! (*unresolved:* unit not yet confirmed). Axes are used as stored (Y up);
//! *unresolved:* whether the original is left-handed (a mirror flip).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use ddvfs::Vfs;
use mtshader::ShaderPackage;

pub const RMODEL: u32 = 0x58A1_5856;
pub const RMATERIAL: u32 = 0x2749_C8A8;
pub const RTEXTURE: u32 = 0x241F_5DEB;
pub const UNIT: f32 = 0.01;

#[derive(Default)]
pub struct Stats {
    pub cells: usize,
    pub meshes: usize,
    pub triangles: usize,
    pub textures: usize,
    pub missing: Vec<String>,
}

pub struct Loader<'a> {
    pub vfs: &'a Vfs,
    pub mfx: &'a ShaderPackage,
    pub textures: HashMap<String, Option<Handle<Image>>>,
    pub stats: Stats,
}

/// Builds a Bevy image from a TEX file: base colour, so sRGB (*assumed*:
/// the exe's sRGB rule is not traced yet).
pub fn tex_to_image(bytes: &[u8]) -> Result<Image, String> {
    let t = mttex::parse(bytes).map_err(|e| e.to_string())?;
    let h = &t.header;
    if h.shape != mttex::Shape::Flat {
        return Err(format!("{:?} textures not supported in the viewer", h.shape));
    }
    let format = match h.format {
        mttex::D3dFormat::Dxt1 => TextureFormat::Bc1RgbaUnormSrgb,
        mttex::D3dFormat::Dxt3 => TextureFormat::Bc2RgbaUnormSrgb,
        mttex::D3dFormat::Dxt5 => TextureFormat::Bc3RgbaUnormSrgb,
        mttex::D3dFormat::A8R8G8B8 | mttex::D3dFormat::X8R8G8B8 => TextureFormat::Bgra8UnormSrgb,
        f => return Err(format!("format {f:?} not supported in the viewer")),
    };
    if format != TextureFormat::Bgra8UnormSrgb && (h.width % 4 != 0 || h.height % 4 != 0) {
        return Err(format!(
            "{}x{} not a multiple of the block size",
            h.width, h.height
        ));
    }
    let data: Vec<u8> = t.surfaces.iter().flat_map(|s| s.data.iter().copied()).collect();
    let mut img = Image::new_uninit(
        Extent3d {
            width: h.width,
            height: h.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = h.mip_count.max(1);
    img.data = Some(data);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    Ok(img)
}

impl Loader<'_> {
    fn texture(&mut self, path: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(h) = self.textures.get(path) {
            return h.clone();
        }
        let h = match self.vfs.read(path, RTEXTURE).and_then(|b| tex_to_image(&b)) {
            Ok(img) => {
                self.stats.textures += 1;
                Some(images.add(img))
            }
            Err(e) => {
                self.stats.missing.push(format!("texture {path}: {e}"));
                None
            }
        };
        self.textures.insert(path.to_string(), h.clone());
        h
    }

    /// Spawns one cell model with its materials. Returns false if absent.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_cell(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        stage: u32,
        a: i32,
        b: i32,
        offset: Vec3,
    ) -> bool {
        let mfx = self.mfx;
        // The folder is the cell's A (m) number rounded down to tens, e.g.
        // scr\st100\model\m50\st100_58m49n (checked with ddtool arc-ls).
        let path = format!("scr\\st{stage}\\model\\m{}\\st{stage}_{a}m{b}n", a / 10 * 10);
        let Ok(mod_bytes) = self.vfs.read(&path, RMODEL) else {
            return false;
        };
        let model = match mtmod::parse(&mod_bytes) {
            Ok(m) => m,
            Err(e) => {
                self.stats.missing.push(format!("{path}: {e}"));
                return false;
            }
        };
        let mrl_bytes = self.vfs.read(&path, RMATERIAL).ok();
        let mrl = mrl_bytes.as_deref().and_then(|b| mtmrl::parse(b).ok());
        self.stats.cells += 1;
        let mut mat_cache: HashMap<u16, Handle<StandardMaterial>> = HashMap::new();
        for m in &model.meshes {
            // LOD byte: bit 0 = highest detail (*assumed*), 255 = all.
            if m.lod & 1 == 0 {
                continue;
            }
            let Some(layout) = mfx.layout(m.vertex_format) else {
                continue;
            };
            let Ok(v) = model.decode(m, layout) else { continue };
            let Ok(tris) = model.triangles(m) else { continue };
            if v.uvs[0].is_empty() || tris.is_empty() {
                continue;
            }
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                v.positions
                    .iter()
                    .map(|p| [p[0] * UNIT, p[1] * UNIT, p[2] * UNIT])
                    .collect::<Vec<_>>(),
            );
            if !v.normals.is_empty() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, v.normals.clone());
            }
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, v.uvs[0].clone());
            mesh.insert_indices(Indices::U32(
                tris.iter().flatten().map(|&i| u32::from(i)).collect(),
            ));
            self.stats.meshes += 1;
            self.stats.triangles += tris.len();
            let material = mat_cache
                .entry(m.material)
                .or_insert_with(|| {
                    let name = model
                        .material_names
                        .get(m.material as usize)
                        .cloned()
                        .unwrap_or_default();
                    let mut sm = StandardMaterial {
                        perceptual_roughness: 0.9,
                        metallic: 0.0,
                        reflectance: 0.2,
                        ..default()
                    };
                    if let Some(mat) = mrl.as_ref().and_then(|l| l.by_name(&name)) {
                        for c in &mat.commands {
                            let obj = mfx.object(c.object).map(|o| o.name.as_str());
                            match (c.kind, obj) {
                                (mtmrl::CommandKind::Texture, Some("tAlbedoMap")) => {
                                    if let Some(t) = mrl.as_ref().and_then(|l| l.texture(c)) {
                                        let p = t.path.clone();
                                        sm.base_color_texture = self.texture(&p, images);
                                    }
                                }
                                (mtmrl::CommandKind::Feature, Some("FTransparency")) => {
                                    let v = mfx.object(c.value).map(|o| o.name.as_str());
                                    if matches!(v, Some("FTransparencyAlpha") | Some("FTransparencyMap")) {
                                        // Alpha test vs blend not yet traced; cut-out
                                        // at 0.5 is a placeholder.
                                        sm.alpha_mode = AlphaMode::Mask(0.5);
                                        sm.double_sided = true;
                                        sm.cull_mode = None;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    materials.add(sm)
                })
                .clone();
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::from_translation(offset),
                Name::new(format!("st{stage}_{a}m{b}n")),
            ));
        }
        true
    }
}
