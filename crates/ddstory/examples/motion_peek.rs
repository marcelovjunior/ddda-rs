//! Print the root-level tracks of one motion of a motion list (debugging
//! event actors).
//!
//! cargo run -p ddstory --example motion_peek -- <nativePC> <rMotionList path> <slot> [frame]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v = ddvfs::Vfs::open(&a[1]).unwrap();
    let b = v.read(&a[2], mtarc::type_hash("rMotionList")).unwrap();
    let l = mtlmt::parse(&b).unwrap();
    println!("{} slots: {:?}", a[2], l.motions.iter().map(|m| m.is_some() as u8).collect::<Vec<_>>());
    let slot: usize = a[3].parse().unwrap();
    let f: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let Some(Some(m)) = l.motions.get(slot) else { return };
    println!("frames {} loop {} end_pos {:?} end_rot {:?}", m.frame_count, m.loop_frame, m.end_position, m.end_rotation);
    for t in &m.tracks {
        if t.joint == 255 || t.joint <= 3 {
            println!("joint {:3} {:?} bone_type {} keys {} @{f}: {:?} ref {:?}", t.joint, t.usage, t.bone_type, t.keys.len(), t.sample(f), t.reference);
        }
    }
}
