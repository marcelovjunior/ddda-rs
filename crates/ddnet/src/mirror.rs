//! What the host replicates besides the Arisen bodies (protocol 2):
//! - each seat's look, once at join;
//! - the host's spot, so a guest loads the same world;
//! - per-seat HP;
//! - the fight's enemies.
//!
//! All design decisions (ADR 0002 section 8). The data is what the
//! original keeps (save edit fields, equipped items, region HP).

use ddid::{PlayerId, Reader, Result, Writer};

/// An Arisen's (or pawn's) look as its save holds it: the
/// `cSAVE_DATA_EDIT` fields by name and the equipped item numbers
/// (`mEquipItem`, -1 empty). Each side rebuilds the dressed model from its
/// own install (`ddlook::save::edit_look`); the bulk data never crosses
/// the network.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookData {
    pub edit: Vec<(String, u32)>,
    pub items: Vec<i16>,
}

/// Where the host's session is: stage, origin cell (A, B) and the host
/// Arisen's position (stage frame, cm) and yaw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub stage: u32,
    pub cell: (i32, i32),
    pub pos: [f32; 3],
    pub yaw: f32,
}

/// A seat's hit points (the Arisen and pawns of the fight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vital {
    pub id: PlayerId,
    pub hp: f32,
    pub hp_max: f32,
}

/// One enemy of the fight: its owner id, place, action (state), motion
/// (bank, slot, frame), main HP pool and death.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Enemy {
    pub id: u32,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub action: i32,
    pub bank: u8,
    pub slot: u16,
    pub frame: f32,
    pub hp: f32,
    pub hp_max: f32,
    pub dead: bool,
}

impl Enemy {
    /// Between `a` and `b` at `t` in 0..1: place and yaw blend; the rest
    /// is `a`'s until `t` passes one half.
    pub fn lerp(a: &Enemy, b: &Enemy, t: f32) -> Enemy {
        let mut out = if t < 0.5 { *a } else { *b };
        out.pos = std::array::from_fn(|i| a.pos[i] + (b.pos[i] - a.pos[i]) * t);
        let mut d = (b.yaw - a.yaw) % std::f32::consts::TAU;
        if d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        } else if d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        out.yaw = a.yaw + d * t;
        out
    }
}

pub fn write_look(w: &mut Writer, l: &LookData) {
    w.len(l.edit.len());
    for (k, v) in &l.edit {
        w.bytes(k.as_bytes()).u32(*v);
    }
    w.len(l.items.len());
    for &i in &l.items {
        w.i16(i);
    }
}

pub fn read_look(r: &mut Reader) -> Result<LookData> {
    let n = r.len(8)?;
    let mut edit = Vec::with_capacity(n);
    for _ in 0..n {
        let k = String::from_utf8(r.bytes()?).map_err(|e| e.to_string())?;
        edit.push((k, r.u32()?));
    }
    let n = r.len(2)?;
    let items = (0..n).map(|_| r.i16()).collect::<Result<_>>()?;
    Ok(LookData { edit, items })
}

pub fn write_spot(w: &mut Writer, s: &Spot) {
    w.u32(s.stage).i32(s.cell.0).i32(s.cell.1).f32(s.pos[0]).f32(s.pos[1]).f32(s.pos[2]).f32(s.yaw);
}

pub fn read_spot(r: &mut Reader) -> Result<Spot> {
    Ok(Spot { stage: r.u32()?, cell: (r.i32()?, r.i32()?), pos: [r.f32()?, r.f32()?, r.f32()?], yaw: r.f32()? })
}

pub fn write_vitals(w: &mut Writer, v: &[Vital]) {
    w.len(v.len());
    for x in v {
        w.u8(x.id).f32(x.hp).f32(x.hp_max);
    }
}

pub fn read_vitals(r: &mut Reader) -> Result<Vec<Vital>> {
    let n = r.len(9)?;
    (0..n).map(|_| Ok(Vital { id: r.u8()?, hp: r.f32()?, hp_max: r.f32()? })).collect()
}

/// Enemies, each whole or (when equal to the baseline's) as a one-byte
/// "unchanged".
pub fn write_enemies(w: &mut Writer, e: &[Enemy], base: &[Enemy]) {
    w.len(e.len());
    for x in e {
        w.u32(x.id);
        if base.iter().any(|b| b == x) {
            w.bool(false);
            continue;
        }
        w.bool(true);
        w.f32(x.pos[0]).f32(x.pos[1]).f32(x.pos[2]).f32(x.yaw).i32(x.action);
        w.u8(x.bank).u16(x.slot).f32(x.frame).f32(x.hp).f32(x.hp_max).bool(x.dead);
    }
}

pub fn read_enemies(r: &mut Reader, base: &[Enemy]) -> Result<Vec<Enemy>> {
    let n = r.len(5)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u32()?;
        if !r.bool()? {
            let b = base.iter().find(|b| b.id == id).ok_or_else(|| format!("enemy {id}: unchanged without a baseline"))?;
            out.push(*b);
            continue;
        }
        out.push(Enemy {
            id,
            pos: [r.f32()?, r.f32()?, r.f32()?],
            yaw: r.f32()?,
            action: r.i32()?,
            bank: r.u8()?,
            slot: r.u16()?,
            frame: r.f32()?,
            hp: r.f32()?,
            hp_max: r.f32()?,
            dead: r.bool()?,
        });
    }
    Ok(out)
}

/// The fight's replicated state on the host: every seat's HP (Arisen and
/// pawns, by seat) and every enemy.
pub fn from_fight(f: Option<&ddhost::ddenemyrt::fight::Fight>) -> (Vec<Vital>, Vec<Enemy>) {
    use ddhost::ddenemyrt::combat::Victim;
    let Some(f) = f else { return (vec![], vec![]) };
    let vitals = f
        .arisen
        .iter()
        .map(|(&id, a)| {
            let hp_max = match &a.fighter.victim {
                Victim::Player { hp_max, .. } => *hp_max,
                Victim::Enemy { regions, .. } => regions.first().map_or(0.0, |r| r.hp_max),
            };
            Vital { id, hp: a.fighter.victim.hp(), hp_max }
        })
        .collect();
    let enemies = f
        .goblins
        .iter()
        .map(|g| {
            let hp_max = match &g.fighter.victim {
                Victim::Enemy { regions, .. } => regions.first().map_or(0.0, |r| r.hp_max),
                Victim::Player { hp_max, .. } => *hp_max,
            };
            Enemy {
                id: g.fighter.owner,
                pos: g.pos,
                yaw: g.yaw,
                action: g.action,
                bank: g.motion.bank,
                slot: g.motion.slot,
                frame: g.frame,
                hp: g.fighter.victim.hp(),
                hp_max,
                dead: g.dead(),
            }
        })
        .collect();
    (vitals, enemies)
}
