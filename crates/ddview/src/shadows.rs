//! Characters' floor shadows: the original's two `uAmbientShadow` units
//! per player body.
//!
//! Traced (player vf05 -> 0x00B565A0): each player creates two
//! `uAmbientShadow` (player +0x58B0, +0x58B4) with the texture
//! `effect\tex\cm\t_cm100_07_GM`, parented to joints 0x10 and 0x14
//! (`mParentNo`, +0x54), `mSize` (+0x94) scaled by the body's scale
//! (player +0x2E54 -> +0xF78), and registers them on move line 0xB.
//! Constructor defaults (0x00E79BC0): `mSize` 50, `mStart` 1, `mEnd` 100,
//! `mStartDecay` 10, `mEndDecay` 50, `mIntensity` 1, `mDir` (0, 0, 1),
//! `mProjectionType` 0, `mBlendType` 0.
//!
//! Drawn here as a quad on the collision ground under each joint, darkening
//! what is under it by the texture's colour (the blob is white in the
//! middle, black at the edges). *Viewer approximation*: the original
//! projects onto the drawn geometry; how it blends ([`DARKEN`]) and how
//! the start/end/decay distances fade it ([`height_fade`]) are not traced.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::character::Playback;
use crate::play::PlayState;
use crate::stage::UNIT;

pub const TEXTURE: &str = r"effect\tex\cm\t_cm100_07_GM";
/// `mParentNo` of the two units.
pub const JOINTS: [u8; 2] = [0x10, 0x14];
/// `mSize` default (cm), the blob's width here (*assumed*).
pub const SIZE: f32 = 50.0;
/// `mEnd` and `mEndDecay` defaults (cm).
pub const END: f32 = 100.0;
pub const END_DECAY: f32 = 50.0;
/// Darkening at the blob's centre at `mIntensity` 1 (*viewer choice*, to be
/// matched against an oracle capture).
pub const DARKEN: f32 = 0.6;

#[derive(Resource)]
pub struct BlobAssets {
    mesh: Handle<Mesh>,
    image: Handle<Image>,
}

/// On a character: its two blobs.
#[derive(Component)]
pub struct Blobs;

/// One blob: its owner and the joint it follows.
#[derive(Component)]
pub struct Blob {
    owner: Entity,
    joint: Entity,
    material: Handle<StandardMaterial>,
}

/// Loads the blob's texture; without it there are no blobs.
pub fn prepare(commands: &mut Commands, vfs: &ddvfs::Vfs, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) {
    let image = match vfs.read(TEXTURE, crate::stage::RTEXTURE).map_err(|e| e.to_string()).and_then(|b| mask_image(&b)) {
        Ok(i) => i,
        Err(e) => {
            warn!("shadows: {TEXTURE}: {e}");
            return;
        }
    };
    let mesh = Plane3d::new(Vec3::Y, Vec2::splat(0.5)).mesh().build();
    commands.insert_resource(BlobAssets { mesh: meshes.add(mesh), image: images.add(image) });
}

/// The texture's top level as a black image whose alpha is the colour's
/// brightness (the shape lives in the colour; alpha is about 1 throughout).
fn mask_image(bytes: &[u8]) -> Result<Image, String> {
    let t = mttex::parse(bytes).map_err(|e| e.to_string())?;
    if t.header.format != mttex::D3dFormat::Dxt5 {
        return Err(format!("{:?}, expected DXT5", t.header.format));
    }
    let s = &t.surfaces[0];
    let (w, h) = (s.width as usize, s.height as usize);
    let mut data = vec![0u8; w * h * 4];
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            let block = &s.data[(by * (w / 4) + bx) * 16..][..16];
            let colours = dxt_colours(block[8..16].try_into().unwrap());
            let bits = u32::from_le_bytes(block[12..16].try_into().unwrap());
            for i in 0..16 {
                let c = colours[((bits >> (2 * i)) & 3) as usize];
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                data[(y * w + x) * 4 + 3] = ((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3) as u8;
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    Ok(img)
}

/// The four colours of a DXT1/DXT5 colour block (8 bytes), as stored
/// (sRGB-encoded bytes).
fn dxt_colours(b: [u8; 8]) -> [[u8; 3]; 4] {
    let rgb = |v: u16| {
        let (r, g, bl) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
        [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (bl * 255 / 31) as u8]
    };
    let (c0, c1) = (rgb(u16::from_le_bytes([b[0], b[1]])), rgb(u16::from_le_bytes([b[2], b[3]])));
    let mix = |a: u8, b: u8| ((2 * a as u32 + b as u32) / 3) as u8;
    // DXT5 colour blocks always use the four-colour mode
    [c0, c1, [mix(c0[0], c1[0]), mix(c0[1], c1[1]), mix(c0[2], c1[2])], [mix(c1[0], c0[0]), mix(c1[1], c0[1]), mix(c1[2], c0[2])]]
}

/// Strength of a blob whose joint is `height` cm above the ground: full up
/// to `mEnd - mEndDecay`, gone at `mEnd` (*inferred* from the field names).
pub fn height_fade(height: f32) -> f32 {
    (1.0 - (height - (END - END_DECAY)) / END_DECAY).clamp(0.0, 1.0)
}

/// Gives the play characters (Arisen and pawns) their blobs and keeps the
/// blobs on the ground under their joints.
#[allow(clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    assets: Option<Res<BlobAssets>>,
    state: Option<Res<PlayState>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    chars: Query<(&Playback, Has<Blobs>)>,
    owners: Query<&InheritedVisibility>,
    joints: Query<&GlobalTransform>,
    mut blobs: Query<(Entity, &Blob, &mut Transform, &mut Visibility)>,
) {
    let (Some(assets), Some(st)) = (assets, state) else { return };
    for &e in st.entities.values() {
        let Ok((playback, false)) = chars.get(e) else { continue };
        for id in JOINTS {
            let Some(&joint) = playback.joints.get(&id) else { continue };
            let material = materials.add(StandardMaterial {
                base_color: Color::BLACK.with_alpha(DARKEN),
                base_color_texture: Some(assets.image.clone()),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                depth_bias: 50.0,
                ..default()
            });
            commands.spawn((
                Name::new("ambient shadow"),
                Blob { owner: e, joint, material: material.clone() },
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_scale(Vec3::splat(SIZE * UNIT)),
                Visibility::Hidden,
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
        info!("shadows: blobs for {e:?} (joints {:?})", JOINTS.map(|j| playback.joints.contains_key(&j)));
        commands.entity(e).insert(Blobs);
    }
    use ddsim::Ground;
    for (blob_entity, blob, mut tr, mut vis) in &mut blobs {
        let Ok(owner_vis) = owners.get(blob.owner) else {
            commands.entity(blob_entity).despawn();
            continue;
        };
        let Ok(joint) = joints.get(blob.joint) else { continue };
        let at = joint.translation() / UNIT + st.origin;
        let ground = st.terrain.height(at.x, at.z, at.y + 50.0);
        let fade = ground.map_or(0.0, |g| height_fade(at.y - g));
        let shown = owner_vis.get() && fade > 0.0;
        vis.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        let Some(g) = ground.filter(|_| shown) else { continue };
        tr.translation = (Vec3::new(at.x, g, at.z) - st.origin) * UNIT;
        if let Some(mut m) = materials.get_mut(&blob.material) {
            let a = DARKEN * fade;
            if (m.base_color.alpha() - a).abs() > 1e-3 {
                m.base_color.set_alpha(a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_blob_fades_out_with_height() {
        assert_eq!(super::height_fade(10.0), 1.0);
        assert_eq!(super::height_fade(75.0), 0.5);
        assert_eq!(super::height_fade(120.0), 0.0);
    }

    #[test]
    fn the_blob_is_bright_in_the_middle_and_dark_at_the_corners() {
        let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        let Ok(vfs) = ddvfs::Vfs::open(native) else { return };
        let img = super::mask_image(&vfs.read(super::TEXTURE, crate::stage::RTEXTURE).unwrap()).unwrap();
        let data = img.data.unwrap();
        let alpha = |x: usize, y: usize| data[(y * 32 + x) * 4 + 3];
        assert!(alpha(16, 16) > 200, "{}", alpha(16, 16));
        assert!(alpha(0, 0) < 30, "{}", alpha(0, 0));
    }
}
