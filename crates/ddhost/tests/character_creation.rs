//! S56 profile identity and atomic save restoration, without rendering.
use ddhost::character_creation::TAG_EDIT;
use ddhost::data::NoStoryData;
use ddhost::save::{Owner, SaveFile};
use ddhost::{Control, Host, ProfileId};
use ddpawn::cmc::Edit;
use ddstory::scripted::ScriptedHost;

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.0)
    }
}
type Session = Host<NoStoryData, ScriptedHost>;
fn host() -> Session {
    Host::new(
        ddsim::Sim::new(Default::default()),
        None,
        ScriptedHost::default(),
        Box::new(Flat),
    )
}
fn join(host: &mut Session, profile: u64) -> u8 {
    host.join(ProfileId(profile), Control::Local, [0.0; 3], 0.0)
        .unwrap()
}
fn edit(name: &str, sex: u8) -> Edit {
    let mut edit = Edit {
        name: [0; 25],
        nickname: 345,
        fields: std::array::from_fn(|i| i as u8),
    };
    edit.set("mGender", sex);
    edit.set_name(name.as_bytes());
    edit
}

#[test]
fn confirmed_characters_roundtrip_remap_and_never_follow_a_recycled_seat() {
    let mut original = host();
    let a = join(&mut original, 11);
    let b = join(&mut original, 22);
    original.confirm_character(a, edit("Alba", 1)).unwrap();
    original.confirm_character(b, edit("Beren", 0)).unwrap();
    let file = SaveFile::from_bytes(&original.save().unwrap().to_bytes()).unwrap();
    let mut restored = host();
    let rb = join(&mut restored, 22);
    let ra = join(&mut restored, 11);
    assert_ne!(ra, a);
    restored.load(&file, NoStoryData).unwrap();
    assert_eq!(restored.character_edit(ra), Some(&edit("Alba", 1)));
    assert_eq!(restored.character_name(rb), "Beren");
    restored.leave(ra);
    let c = join(&mut restored, 33);
    assert_eq!(c, ra);
    assert_eq!(restored.character_name(c), "");
    assert!(restored.character_edit(c).is_none());
    let parked = SaveFile::from_bytes(&restored.save().unwrap().to_bytes()).unwrap();
    let mut next = host();
    let nb = join(&mut next, 22);
    join(&mut next, 33);
    next.load(&parked, NoStoryData).unwrap();
    assert_eq!(next.saved_hash(), restored.saved_hash());
    next.leave(nb);
    let returning = join(&mut next, 11);
    assert_eq!(next.character_name(returning), "Alba");
    assert_eq!(next.character_edit(returning), Some(&edit("Alba", 1)));
}

#[test]
fn malformed_edit_rejects_entire_host_load_before_any_mutation() {
    let mut current = host();
    let seat = join(&mut current, 11);
    current.confirm_character(seat, edit("Alba", 1)).unwrap();
    let before = current.save().unwrap().to_bytes();
    let before_hash = current.state_hash();
    let mut incoming = host();
    let incoming_seat = join(&mut incoming, 11);
    incoming
        .confirm_character(incoming_seat, edit("Beren", 0))
        .unwrap();
    incoming.frame = 987;
    incoming.sim.player_mut(incoming_seat).unwrap().position = [123.0, 0.0, 456.0];
    let valid = incoming.save().unwrap();
    let bytes = valid
        .get(TAG_EDIT, Owner::Profile(ProfileId(11)))
        .unwrap()
        .to_vec();
    for bad in [
        vec![0],
        {
            let mut invalid = bytes.clone();
            invalid[33] = 2;
            invalid
        },
        {
            let mut invalid = bytes.clone();
            invalid[8] = 0xff;
            invalid
        },
    ] {
        let mut file = valid.clone();
        // Validate unseated profiles too: a corrupt offline record cannot hide
        // until that profile rejoins and poison an otherwise successful load.
        file.put(TAG_EDIT, Owner::Profile(ProfileId(99)), bad);
        assert!(current.load(&file, NoStoryData).is_err());
        assert_eq!(current.save().unwrap().to_bytes(), before);
        assert_eq!(current.state_hash(), before_hash);
    }
    assert!(current.confirm_character(99, edit("No seat", 0)).is_err());
    assert_eq!(current.save().unwrap().to_bytes(), before);
}
