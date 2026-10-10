//! Shared dynamic h00 collision, over an existing world. Geometry is
//! stored once; host and camera overlays share only its Arc and enable bit.
use std::{collections::BTreeMap, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use ddsim::{Ground, RayHit};
use crate::placement::Id;

pub struct Leaf {
    pub terrain: ddworld::Terrain,
    pub enabled: AtomicBool,
}

#[derive(Clone, Default)]
pub struct Set(pub Arc<BTreeMap<Id, Leaf>>);
impl Set {
    pub fn enable(&self, id: Id, enabled: bool) {
        if let Some(leaf) = self.0.get(&id) { leaf.enabled.store(enabled, Ordering::Relaxed); }
    }
    pub fn over<'a>(&'a self, base: &'a dyn Ground) -> Overlay<'a> { Overlay { base, doors: self } }
    pub fn wrap(&self, base: Box<dyn Ground + Send>) -> Box<dyn Ground + Send> {
        Box::new(Owned { base, doors: self.clone() })
    }
    fn active(&self) -> impl Iterator<Item = &ddworld::Terrain> {
        self.0.values().filter(|l| l.enabled.load(Ordering::Relaxed)).map(|l| &l.terrain)
    }
}

/// h00 is the same player/camera channel already used by stage_collision.
/// Original 0x00c52f80 registers it with channel1; e00 is separately
/// registered as channel2 by 0x00c52860, not duplicated into this channel.
/// `matrix` is the evaluated column-major model transform, in stage cm.
pub fn load(vfs: &ddvfs::Vfs, path: &str, matrix: [f32;16]) -> Result<Leaf, String> {
    let bytes = vfs.read(path, mtarc::type_hash("rCollision"))?;
    let collision = mtsbc::parse(&bytes).map_err(|e| e.to_string())?;
    let mut terrain = ddworld::Terrain::empty();
    for (triangle, metadata) in collision.world_triangles() {
        let transformed = triangle.map(|p| std::array::from_fn(|i|
            matrix[i]*p[0] + matrix[4+i]*p[1] + matrix[8+i]*p[2] + matrix[12+i]));
        if let Some(&material) = collision.materials.get(usize::from(metadata.material)) {
            terrain.add_material_with_attribute(transformed, material, metadata.attribute);
        } else { terrain.add(transformed); }
    }
    Ok(Leaf { terrain, enabled: AtomicBool::new(true) })
}

pub struct Overlay<'a> { base: &'a dyn Ground, doors: &'a Set }
fn distance_squared(a: [f32;3], b: [f32;3]) -> f32 {
    a.iter().zip(b).map(|(a,b)| (a-b)*(a-b)).sum()
}
impl Ground for Overlay<'_> {
    fn height(&self, x:f32, z:f32, below:f32) -> Option<f32> {
        self.doors.active().filter_map(|g| g.height(x,z,below)).chain(self.base.height(x,z,below))
            .max_by(f32::total_cmp)
    }
    fn wall(&self, p:[f32;3], radius:f32, low:f32, high:f32) -> Option<[f32;2]> {
        self.base.wall(p,radius,low,high).or_else(|| self.doors.active().find_map(|g|g.wall(p,radius,low,high)))
    }
    fn ray(&self, from:[f32;3], to:[f32;3]) -> Option<RayHit> { self.ray_with_flags(from,to).map(|h|h.0) }
    fn ray_with_flags(&self, from:[f32;3], to:[f32;3]) -> Option<(RayHit,u32)> {
        self.base.ray_with_flags(from,to).into_iter()
            .chain(self.doors.active().filter_map(|g|g.ray_with_flags(from,to)))
            .min_by(|a,b|distance_squared(from,a.0.point).total_cmp(&distance_squared(from,b.0.point)))
    }
    fn sphere(&self, center:[f32;3], radius:f32) -> bool {
        self.base.sphere(center,radius) || self.doors.active().any(|g|g.sphere(center,radius))
    }
    fn triangles_in(&self,lo:[f32;3],hi:[f32;3],out:&mut Vec<ddsim::sweep::Face>) -> bool {
        let mut found = self.base.triangles_in(lo,hi,out);
        for g in self.doors.active() { found |= g.triangles_in(lo,hi,out); }
        found
    }
}
struct Owned { base: Box<dyn Ground + Send>, doors: Set }
impl Ground for Owned {
    fn height(&self,x:f32,z:f32,below:f32)->Option<f32>{self.doors.over(self.base.as_ref()).height(x,z,below)}
    fn wall(&self,p:[f32;3],r:f32,l:f32,h:f32)->Option<[f32;2]>{self.doors.over(self.base.as_ref()).wall(p,r,l,h)}
    fn ray(&self,f:[f32;3],t:[f32;3])->Option<RayHit>{self.doors.over(self.base.as_ref()).ray(f,t)}
    fn ray_with_flags(&self,f:[f32;3],t:[f32;3])->Option<(RayHit,u32)>{self.doors.over(self.base.as_ref()).ray_with_flags(f,t)}
    fn sphere(&self,c:[f32;3],r:f32)->bool{self.doors.over(self.base.as_ref()).sphere(c,r)}
    fn triangles_in(&self,l:[f32;3],h:[f32;3],o:&mut Vec<ddsim::sweep::Face>)->bool{self.doors.over(self.base.as_ref()).triangles_in(l,h,o)}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_enable_controls_camera_and_character_geometry() {
        let id = Id { stage:200,group:18,record:2 };
        let mut terrain = ddworld::Terrain::empty();
        terrain.add_flagged([[0.0,0.0,0.0],[0.0,200.0,0.0],[0.0,0.0,200.0]], 0x10);
        let set = Set(Arc::new(BTreeMap::from([(id,Leaf{terrain,enabled:AtomicBool::new(true)})])));
        let camera = set.clone();
        let base = ddworld::Terrain::empty();
        let host = set.wrap(Box::new(ddworld::Terrain::empty()));
        let from=[-20.0,30.0,30.0]; let to=[20.0,30.0,30.0];
        assert_eq!(host.ray_with_flags(from,to).unwrap().1,0x10);
        assert!(camera.over(&base).ray(from,to).is_some());
        set.enable(id,false);
        assert!(host.ray(from,to).is_none());
        assert!(camera.over(&base).ray(from,to).is_none());
        let mut faces=Vec::new();
        host.triangles_in([-50.0;3],[250.0;3],&mut faces);
        assert!(faces.is_empty());
    }
}
