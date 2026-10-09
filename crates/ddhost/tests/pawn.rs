//! The host steps the fight's pawns: `Host::tick` asks the fight for the
//! pawns' pads and moves them with the seats (play-from-save). Skipped
//! without the install.

use ddenemyrt::fight::{Fight, FightData};
use ddhost::data::NoStoryData;
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddsim::{Ground, Sim};
use ddstory::scripted::ScriptedHost;
use std::collections::HashMap;
use std::sync::Arc;

struct Flat;

impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

fn data() -> Option<Arc<FightData>> {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).ok()?;
    Some(Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap()))
}

/// A seated Arisen standing still, its pawn (a sim player outside the
/// roster) and a goblin 6 m away: the pawn goes for the goblin and hits it.
#[test]
fn the_host_moves_the_pawn_and_it_fights() {
    let Some(data) = data() else { return };
    let mut h = Host::new(
        Sim::new(HashMap::new()),
        None::<ddstory::Story<NoStoryData>>,
        ScriptedHost::default(),
        Box::new(Flat),
    );
    assert_eq!(h.join(ProfileId(1), Control::Local, [0.0; 3], 0.0), Some(0));
    let mut f = Fight::new(data);
    f.add_arisen(0);
    f.equip(&mut h.sim, 0);
    f.add_pawn(&mut h.sim, 4, 0, [150.0, 0.0, 0.0], 0.0, [500.0; 9], 7);
    f.add_goblin([0.0, 0.0, 600.0], std::f32::consts::PI);
    f.volumes(&h.sim);
    h.fight = Some(f);
    let start = h.sim.player(4).unwrap().position;
    let mut pawn_hits = 0;
    for _ in 0..900 {
        let mut inputs = FrameInputs::new();
        inputs.insert(0, PlayerInput::default());
        h.tick(&inputs).unwrap();
        let f = h.fight.as_ref().unwrap();
        pawn_hits += f.events.iter().filter(|e| e.attacker == 4).count();
    }
    let end = h.sim.player(4).unwrap().position;
    let moved = ((end[0] - start[0]).powi(2) + (end[2] - start[2]).powi(2)).sqrt();
    assert!(moved > 100.0, "the pawn did not move: {start:?} -> {end:?}");
    assert!(pawn_hits > 0, "the pawn never hit");
}
