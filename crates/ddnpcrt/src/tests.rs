//! Synthetic tests: the chooser, shop menus, markup, actions and a talk
//! session with the lock rule.

use super::*;
use ddnpc::Conversation;
use talk::*;

fn conv(no: i32, season: i8, seq: i8, ty: i8, lines: &[i32]) -> Conversation {
    let mut messages = [-1; 15];
    messages[..lines.len()].copy_from_slice(lines);
    Conversation {
        number: no,
        npc_id: 2,
        choices: [-1; 6],
        choice_jumps: [-1; 6],
        messages,
        motions: [-1; 15],
        faces: [-1; 15],
        global_flags: [-1; 15],
        quest_numbers: [-1; 15],
        quest_flags: [-1; 15],
        season,
        season_sequence: seq,
        season_sequence_type: ty,
        quest_number: -1,
        quest_flag: -1,
        friendship: -1,
        friendship_max: -1,
        global_flag: -1,
    }
}

struct St {
    quest: bool,
    gsf: bool,
    friend: i32,
}

impl TalkState for St {
    fn quest_flag(&self, _q: i32, _f: i32) -> bool {
        self.quest
    }
    fn gsf(&self, _f: u32) -> bool {
        self.gsf
    }
    fn affinity(&self, _npc: i32) -> i32 {
        self.friend
    }
}

fn quina_band2() -> MessageSet {
    let mut c22 = conv(22, 2, 4, 1, &[41, 42]);
    c22.quest_number = 52;
    c22.quest_flag = 94;
    MessageSet {
        conversations: vec![conv(20, 2, 1, 0, &[1]), conv(18, 2, 2, 0, &[2]), conv(21, 2, 3, 0, &[3]), c22, conv(6, 100, -1, 0, &[0])],
    }
}

#[test]
fn chooser_runs_the_band_in_sequence_and_waits_for_flags() {
    let set = quina_band2();
    let off = St { quest: false, gsf: false, friend: 500 };
    let on = St { quest: true, gsf: false, friend: 500 };
    let mut m = TalkMemory::default();
    let picks: Vec<_> = (0..5).map(|_| choose(&set, &mut m, 2, 2, &off)).collect();
    assert_eq!(picks, vec![Some(20), Some(18), Some(21), Some(21), Some(21)]);
    assert_eq!((m.last, m.seq[2]), (21, 3));
    assert_eq!(choose(&set, &mut m, 2, 2, &on), Some(22));
    // Type 1 marks it once-only; as the last said it may still repeat.
    assert!(m.once[0] & (0x8000_0000 >> 22) != 0);
    assert_eq!(choose(&set, &mut m, 2, 2, &on), Some(22));
    // An empty band falls back to the band below; band 1 has nothing here.
    let mut fresh = TalkMemory::default();
    assert_eq!(choose(&set, &mut fresh, 2, 3, &off), Some(20));
    assert_eq!(choose(&set, &mut TalkMemory::default(), 2, 1, &off), None);
    // The plain-talk entry falls back to conversation 6.
    assert_eq!(entry(&set, &mut TalkMemory::default(), 2, 1, &off), Some(6));
}

#[test]
fn affinity_window_and_gsf() {
    let mut c = conv(30, 5, 1, 0, &[1]);
    c.friendship = 700;
    c.friendship_max = 900;
    let mut g = conv(31, 5, 2, 0, &[2]);
    g.global_flag = 567;
    let set = MessageSet { conversations: vec![c, g] };
    let low = St { quest: false, gsf: true, friend: 650 };
    let high = St { quest: false, gsf: true, friend: 800 };
    assert_eq!(choose(&set, &mut TalkMemory::default(), 2, 5, &low), Some(31));
    assert_eq!(choose(&set, &mut TalkMemory::default(), 2, 5, &high), Some(30));
    let nogsf = St { quest: false, gsf: false, friend: 650 };
    assert_eq!(choose(&set, &mut TalkMemory::default(), 2, 5, &nogsf), None);
}

#[test]
fn shop_menus_markup_and_actions() {
    assert_eq!(shop_menu(MenuKind::Open, 149, 220, false, false), Some(100));
    assert_eq!(shop_menu(MenuKind::Open, 149, 220, true, false), Some(110));
    assert_eq!(shop_menu(MenuKind::AfterBuy, 149, 220, false, false), Some(105));
    assert_eq!(shop_menu(MenuKind::AfterSell, 149, 220, false, false), Some(106));
    assert_eq!(shop_menu(MenuKind::Open, 0x30f, 200, false, false), None);
    assert_eq!(shop_menu(MenuKind::Open, 0x310, 400, false, false), Some(102));
    assert_eq!(line_handler("<UPGR>They're masterworks, all."), Some(Handler(7)));
    assert_eq!(line_handler("<EQUG>x"), Some(Handler(11)));
    assert_eq!(line_handler("<FUNC 0>Rest?"), Some(Handler(0)));
    assert_eq!(line_handler("<ICON 3>no command"), None);
    assert_eq!(action(Handler(7), Some(0)), Action::Shop { mode: 0 });
    assert_eq!(action(Handler(7), Some(2)), Action::Shop { mode: 2 });
    assert_eq!(action(Handler(7), Some(3)), Action::Nothing);
    assert_eq!(action(Handler(4), Some(1)), Action::Shop { mode: 1 });
    assert_eq!(action(Handler(6), None), Action::Shop { mode: 0 });
    assert_eq!(action(Handler(11), Some(3)), Action::Shop { mode: 4 });
    assert_eq!(action(Handler(15), Some(1)), Action::Nothing);
    assert_eq!(action(Handler(17), None), Action::Shop { mode: 3 });
    assert_eq!(action(Handler(3), None), Action::Other(3));
}

struct Data(Arc<TalkSet>);

impl schedule::Graphs for Data {
    fn graph(&self, path: &str) -> Result<Arc<mtfsm::Graph>, Error> {
        Err(Error::Data(path.into()))
    }
}

impl NpcData for Data {
    fn talk_set(&self, _npc: u32) -> Result<Arc<TalkSet>, Error> {
        Ok(self.0.clone())
    }
}

fn caxton() -> Data {
    let mut menu = conv(100, 100, -1, 0, &[1]);
    menu.choices[..4].copy_from_slice(&[2, 3, 4, 5]);
    menu.choice_jumps[3] = 104;
    let set = MessageSet {
        conversations: vec![menu, conv(104, 100, -1, 0, &[6]), conv(27, 1, 1, 0, &[0])],
    };
    let text = ["Chatter.", "<UPGR>They're masterworks, all.", "Buy", "Sell", "Enhance", "Never mind", "Come again."]
        .iter()
        .map(|s| s.to_string())
        .collect();
    Data(Arc::new(TalkSet { set, text }))
}

#[test]
fn shop_talk_opens_the_menu_and_one_player_per_npc() {
    let data = caxton();
    let world = ddstory::World::default();
    let story = Story { world: &world, equip_menus: false };
    let mut rt = NpcRuntime::default();
    let ev = rt.talk(&data, &story, 0, 149, true).unwrap();
    assert!(matches!(&ev[0], TalkEvent::Line { conversation: 100, text, .. } if text.starts_with("<UPGR>")));
    // The second Arisen finds Caxton busy.
    assert_eq!(rt.talk(&data, &story, 1, 149, true), Err(TalkError::Busy(0)));
    let ev = rt.talk_next(0).unwrap();
    assert!(matches!(&ev[0], TalkEvent::Choices { options, .. } if options.len() == 4 && options[2] == "Enhance"));
    let ev = rt.talk_choose(&story, 0, 2).unwrap();
    assert_eq!(
        ev,
        vec![
            TalkEvent::Action { player: 0, npc: 149, action: Action::Shop { mode: 2 } },
            TalkEvent::End { player: 0, npc: 149 }
        ]
    );
    assert_eq!(rt.talking_to(149), None);
    // "Never mind" jumps to 104 and ends without an action.
    rt.talk(&data, &story, 1, 149, true).unwrap();
    rt.talk_next(1).unwrap();
    let ev = rt.talk_choose(&story, 1, 3).unwrap();
    assert!(matches!(&ev[0], TalkEvent::Line { conversation: 104, .. }));
    assert_eq!(rt.talk_next(1).unwrap(), vec![TalkEvent::End { player: 1, npc: 149 }]);
}
