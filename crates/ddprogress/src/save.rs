//! Progression's save parts (the host's `PROG` chunks, ADR 0002 section 6).
//!
//! - **Per seat:** the record of that seat's Arisen.
//! - **Shared:** every other character (the main pawn, hired pawns), the
//!   character order, and the party with its roles.
//!
//! In the shared part, an Arisen is a reference to its seat. Seats can
//! differ between sessions, so [`Progression::load`] takes a seat map from
//! the host. An Arisen whose player is absent is left out, together with
//! its party slot.
//!
//! Our own format, not `cPlayerInfo`.

use crate::character::{Bits, Character, DisciplineGear, Learned};
use crate::party::{CharId, Member, PlayerId, Progression, Role};
use ddid::{Parts, Reader, Result, Writer};
use ddskill::growth::{Discipline, Progress, Stats};
use ddskill::vocation::Vocation;
use std::collections::BTreeMap;

const VERSION: u8 = 1;

fn bits(w: &mut Writer, b: &Bits) {
    w.len(b.words().len());
    for &x in b.words() {
        w.u32(x);
    }
}

fn read_bits(r: &mut Reader) -> Result<Bits> {
    let n = r.len(4)?;
    Ok(Bits::from_words((0..n).map(|_| r.u32()).collect::<Result<_>>()?))
}

fn character(w: &mut Writer, c: &Character) {
    w.u8(c.vocation as u8);
    let p = &c.progress;
    w.u32(p.level).u32(p.exp).u32(p.next_exp);
    let s = &p.stats;
    for v in [s.hp_max, s.stamina_lv, s.attack, s.defend, s.magic_attack, s.magic_defend] {
        w.f32(v);
    }
    let d = &c.discipline;
    w.i32(d.points);
    for a in [&d.rank, &d.exp, &d.next] {
        for &v in a.iter() {
            w.u32(v);
        }
    }
    for b in [&c.learned.level1, &c.learned.level2, &c.learned.level3, &c.learned.augments] {
        bits(w, b);
    }
    for &a in &c.augments {
        w.opt_u16(a);
    }
    for set in &c.weapon_sets {
        for &s in set {
            w.opt_u16(s);
        }
    }
    for &a in &c.active {
        w.opt_u16(a);
    }
    w.i32(c.rift_crystals);
    for &p in &c.paid {
        w.bool(p);
    }
    w.u32(c.changes).bool(c.gear.asura_armor).bool(c.gear.perseverance_rings[0]).bool(c.gear.perseverance_rings[1]);
    w.bool(c.is_pawn);
}

fn read_character(r: &mut Reader) -> Result<Character> {
    let v = r.u8()?;
    let vocation = Vocation::from_id(v as u32).ok_or_else(|| format!("PROG: vocation {v}"))?;
    let (level, exp, next_exp) = (r.u32()?, r.u32()?, r.u32()?);
    let stats = Stats {
        hp_max: r.f32()?,
        stamina_lv: r.f32()?,
        attack: r.f32()?,
        defend: r.f32()?,
        magic_attack: r.f32()?,
        magic_defend: r.f32()?,
    };
    let progress = Progress { level, exp, next_exp, stats };
    let mut discipline = Discipline { points: r.i32()?, ..Default::default() };
    for a in [&mut discipline.rank, &mut discipline.exp, &mut discipline.next] {
        for v in a.iter_mut() {
            *v = r.u32()?;
        }
    }
    let learned = Learned { level1: read_bits(r)?, level2: read_bits(r)?, level3: read_bits(r)?, augments: read_bits(r)? };
    let mut augments = [None; 6];
    for a in &mut augments {
        *a = r.opt_u16()?;
    }
    let mut weapon_sets = [[None; 6]; 13];
    for set in &mut weapon_sets {
        for s in set.iter_mut() {
            *s = r.opt_u16()?;
        }
    }
    let mut active = [None; 6];
    for a in &mut active {
        *a = r.opt_u16()?;
    }
    let rift_crystals = r.i32()?;
    let mut paid = [false; 10];
    for p in &mut paid {
        *p = r.bool()?;
    }
    let changes = r.u32()?;
    let gear = DisciplineGear { asura_armor: r.bool()?, perseverance_rings: [r.bool()?, r.bool()?] };
    let is_pawn = r.bool()?;
    Ok(Character {
        vocation,
        progress,
        discipline,
        learned,
        augments,
        weapon_sets,
        active,
        rift_crystals,
        paid,
        changes,
        gear,
        is_pawn,
    })
}

fn role(w: &mut Writer, r: Role) {
    match r {
        Role::Arisen(p) => w.u8(0).u8(p),
        Role::MainPawn { owner } => w.u8(1).u8(owner),
        Role::Support { owner } => w.u8(2).u8(owner),
    };
}

fn read_role(r: &mut Reader) -> Result<(u8, PlayerId)> {
    let k = r.u8()?;
    if k > 2 {
        return Err(format!("PROG: role kind {k}"));
    }
    Ok((k, r.u8()?))
}

impl Progression {
    /// The seat whose Arisen is character `c`, if any.
    fn seat_of(&self, c: CharId) -> Option<PlayerId> {
        self.party.iter().flatten().find_map(|m| match m.role {
            Role::Arisen(p) if m.character == c => Some(p),
            _ => None,
        })
    }

    pub fn save(&self) -> Parts {
        let mut w = Writer::new();
        let mut players = BTreeMap::new();
        w.u8(VERSION).len(self.characters.len());
        for (i, c) in self.characters.iter().enumerate() {
            match self.seat_of(i) {
                Some(p) => {
                    w.u8(1).u8(p);
                    let mut pw = Writer::new();
                    pw.u8(VERSION);
                    character(&mut pw, c);
                    players.insert(p, pw.finish());
                }
                None => {
                    w.u8(0);
                    character(&mut w, c);
                }
            }
        }
        for m in &self.party {
            w.bool(m.is_some());
            if let Some(m) = m {
                w.len(m.character);
                role(&mut w, m.role);
            }
        }
        Parts { shared: w.finish(), players }
    }

    /// Rebuilds from [`Progression::save`]'s parts. `seat` maps a seat of
    /// the saved session to the seat of the same profile now (`None`: that
    /// player is absent); `parts.players` is keyed by the new seats.
    pub fn load(parts: &Parts, seat: &dyn Fn(PlayerId) -> Option<PlayerId>) -> Result<Progression> {
        let mut r = Reader::new(&parts.shared);
        r.version("PROG", VERSION)?;
        let n = r.len(1)?;
        // Old character index -> new index (None: an absent Arisen).
        let mut index: Vec<Option<CharId>> = Vec::with_capacity(n);
        let mut characters = Vec::with_capacity(n);
        for _ in 0..n {
            let c = match r.u8()? {
                0 => Some(read_character(&mut r)?),
                1 => {
                    let old = r.u8()?;
                    match seat(old).and_then(|p| parts.players.get(&p)) {
                        Some(b) => {
                            let mut pr = Reader::new(b);
                            pr.version("PROG player", VERSION)?;
                            let c = read_character(&mut pr)?;
                            pr.end()?;
                            Some(c)
                        }
                        None => None,
                    }
                }
                k => return Err(format!("PROG: character entry kind {k}")),
            };
            index.push(c.as_ref().map(|_| characters.len()));
            characters.extend(c);
        }
        let mut party = [None; 4];
        for slot in &mut party {
            if !r.bool()? {
                continue;
            }
            let old = r.u32()? as usize;
            let (kind, p) = read_role(&mut r)?;
            let Some(character) = *index.get(old).ok_or_else(|| format!("PROG: character {old}"))? else { continue };
            // A pawn whose owner is absent stays, owned by the host.
            let owner = seat(p).unwrap_or(0);
            let role = match kind {
                0 => match seat(p) {
                    Some(q) => Role::Arisen(q),
                    None => continue,
                },
                1 => Role::MainPawn { owner },
                _ => Role::Support { owner },
            };
            *slot = Some(Member { character, role });
        }
        r.end()?;
        Ok(Progression { characters, party })
    }
}
