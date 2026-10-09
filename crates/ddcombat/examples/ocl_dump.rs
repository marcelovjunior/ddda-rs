//! Print an rObjCollision resource: groups, nodes, attacks (non-zero values
//! and set flags) and the sequence index.
//!
//! cargo run -p ddcombat --example ocl_dump -- <nativePC> <resource path>
use ddcombat::ocl::{Value, ATTACK_FIELDS};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: ocl_dump <nativePC> <resource path, e.g. collision\\em\\e01\\e0100\\e0100>");
        std::process::exit(2);
    }
    let vfs = ddvfs::Vfs::open(&args[1]).expect("open nativePC");
    let data = vfs
        .read(&args[2], mtarc::type_hash("rObjCollision"))
        .expect("read rObjCollision");
    let o = ddcombat::ObjCollision::parse(&data).expect("parse");
    println!("resource id {} groups {} attacks {} seq {}", o.resource_id, o.groups.len(), o.attacks.len(), o.seq_index.len());
    for g in &o.groups {
        println!(
            "group {} kind {} atk {} dmg {} scr {} obj {} hang {} hanged {} chk {} ntc {} ntc2 {}",
            g.no, g.kind, g.is_attack as u8, g.is_damage as u8, g.is_scr_adj as u8, g.is_obj_adj as u8,
            g.is_hang as u8, g.is_hanged as u8, g.is_check as u8, g.is_notice as u8, g.is_notice2 as u8
        );
        for n in &g.nodes {
            println!(
                "  node {} shape {} r {} j0 {} {:?} j1 {} {:?} region {} prio {} attr {:#x} eff {:#x} work {}",
                n.no, n.shape, n.radius, n.joint0, n.offset0, n.joint1, n.offset1, n.region_id, n.hit_prio,
                n.node_attr, n.node_effect_attr, n.node_free_work
            );
        }
    }
    for (i, a) in o.attacks.iter().enumerate() {
        let Some(a) = a else { continue };
        let mut s = String::new();
        for (name, _, _) in ATTACK_FIELDS.iter() {
            match a.value(name).unwrap() {
                Value::F32(v) if v != 0.0 => s += &format!(" {name}={v}"),
                Value::U32(v) if v != 0 => s += &format!(" {name}={v}"),
                Value::S32(v) if v != 0 => s += &format!(" {name}={v}"),
                _ => {}
            }
        }
        let flags: Vec<_> = a.set_flags().collect();
        println!("attack[{i}] no {}{s} flags {flags:?}", a.no);
    }
    for q in &o.seq_index {
        println!("seq {} model {} group {} attack {} relation {}", q.no, q.model_id, q.group_no, q.attack_no, q.relation);
    }
}
