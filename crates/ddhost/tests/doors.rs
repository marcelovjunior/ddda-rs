use dddoor::{placement, policy::Observed, runtime::{Occupant,OccupantKind,Phase}};
use ddhost::doors::Doors;

#[test]
fn installed_door_second_actor_holds_open_then_authored_close_restores_collision() {
    let root=std::env::var("DDDA_NATIVE_PC").unwrap_or_else(|_|"E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !std::path::Path::new(&root).exists(){return;}
    let vfs=ddvfs::Vfs::open(&root).unwrap();
    let exe=ddexe::ExeFile::next_to(std::path::Path::new(&root)).unwrap();
    let families=placement::Families::read(&exe.image().unwrap()).unwrap();
    let groups=placement::groups(&vfs,200).unwrap();
    let world=ddspawn::World{scene:1400,hour:12,flag:&|_|false};
    let candidates=placement::candidates(&vfs,&families,200,&groups,&[(0,0)],&world).unwrap();
    let placement=candidates.doors.into_iter().find(|d|d.id.group==18 && d.id.record==2).unwrap();
    let id=placement.id; let position=placement.transform.position;
    let mut doors=Doors::load(&vfs,200,vec![placement],|_|Observed::fresh(12)).unwrap();
    assert!(doors.diagnostics.is_empty(),"{:?}",doors.diagnostics);
    assert!(doors.states[&id].collision);
    let actor=Occupant{position,kind:OccupantKind::Player};
    doors.request_open(id,Some(position));
    doors.step(1.0,&[actor],12);
    assert_eq!(doors.states[&id].phase,Phase::Opening);
    assert!(!doors.states[&id].collision);
    for _ in 0..100{doors.step(1.0,&[actor],12);}
    assert_eq!(doors.states[&id].phase,Phase::Open);
    // The requesting seat has left; another seat occupies the doorway.
    let second=Occupant{position:[position[0]+10.0,position[1],position[2]],kind:OccupantKind::Player};
    for _ in 0..100{doors.step(1.0,&[second],12);}
    assert_eq!(doors.states[&id].phase,Phase::Open);
    for _ in 0..100{doors.step(1.0,&[],12);}
    assert_eq!(doors.states[&id].phase,Phase::Closed);
    assert!(doors.states[&id].collision);
    doors.entries.get_mut(&id).unwrap().observed.permanently_open=None;
    doors.request_open(id,Some(position)); doors.step(1.0,&[],12);
    assert_eq!(doors.states[&id].phase,Phase::Closed);
}
