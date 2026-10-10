//! Installed basic-arrow data and distance-dependent collision bindings.
//! This is the arrow module gate; the S26 Sim/Fight input gate is separate.

use ddenemyrt::arrow::{Arrow, ArrowAim, ArrowData};

fn data() -> Option<ArrowData> {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !path.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    Some(ArrowData::load(&ddvfs::Vfs::open(path).unwrap()).unwrap())
}

#[test]
fn installed_basic_arrow_reads_its_own_collision_and_falloff() {
    let Some(data) = data() else {
        return;
    };
    let p = data.param;
    assert_eq!(data.collision.body.resource_id, 6);
    assert!(p.speed > p.speed_max && p.acceleration < 0.0);
    assert!(p.gravity_acceleration < 0.0 && p.gravity_min < 0.0);
    assert!(p.death_distance > p.collision_distances[1]);
    let aim = ArrowAim {
        bow_origin: [17.0, 91.0, 38.0],
        target: [17.0, 91.0, 1038.0],
        accuracy: 0.25,
        cone_random: 0x80000000,
        polar_random: 0x80000000,
        steady: false,
    };
    let mut arrow = Arrow::aimed(&p, 500, 1, aim, 1.0).unwrap();
    assert_eq!(arrow.position, aim.bow_origin);
    assert_eq!(arrow.direction, [0.0, 0.0, 1.0]);
    let near = arrow.volumes(&data);
    assert_eq!(near.len(), 1);
    assert_eq!(near[0].side.owner, 500);
    assert_eq!(near[0].side.kind, 5);
    let raw = data.collision.attacks.as_ref().unwrap().attacks[1]
        .as_ref()
        .unwrap();
    assert_eq!(near[0].row.attack, raw.attack() + data.supplemental[0].attack);
    let mut attacks = vec![near[0].row];
    for distance in p.collision_distances {
        arrow.distance = distance;
        let volume = arrow.volumes(&data);
        assert_eq!(volume.len(), 1);
        attacks.push(volume[0].row);
    }
    assert!(attacks.windows(2).all(|rows| rows[0].attack > rows[1].attack));
    arrow.distance = p.repelled_distance;
    let repelled = arrow.volumes(&data);
    assert_eq!(repelled[0].row.attack, 0.0);
    assert_eq!((repelled[0].row.shrink_type, repelled[0].row.blow_type), (0, 3));
    arrow.distance = 0.0;
    assert!(arrow.step(&p, 1.0));
    assert_eq!(
        arrow.position,
        [
            aim.bow_origin[0],
            aim.bow_origin[1] + p.gravity,
            aim.bow_origin[2] + p.speed
        ]
    );
    assert_eq!(arrow.speed, p.speed + p.acceleration);
    assert_eq!(arrow.gravity, p.gravity + p.gravity_acceleration);
    arrow.contact(&p, true, false);
    assert!(!arrow.alive && arrow.volumes(&data).is_empty());
}
