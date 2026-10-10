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

/// An NPC where the host's schedule put it (`Host::npcs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcBody {
    pub id: u32,
    pub pos: [f32; 3],
}

/// The NPCs of a host's runtime and those parked off it (`Host::npc_parked`),
/// in id order.
pub fn npcs_of(
    n: &ddhost::ddnpcrt::NpcRuntime,
    parked: &std::collections::BTreeMap<u32, [f32; 3]>,
) -> Vec<NpcBody> {
    let mut v: Vec<NpcBody> = n
        .npcs
        .keys()
        .filter_map(|&id| {
            Some(NpcBody {
                id,
                pos: n.position(id)?,
            })
        })
        .collect();
    v.extend(
        parked
            .iter()
            .filter(|(id, _)| !n.npcs.contains_key(id))
            .map(|(&id, &pos)| NpcBody { id, pos }),
    );
    v.sort_by_key(|b| b.id);
    v
}

pub fn write_npcs(w: &mut Writer, v: &[NpcBody]) {
    w.len(v.len());
    for x in v {
        w.u32(x.id).f32(x.pos[0]).f32(x.pos[1]).f32(x.pos[2]);
    }
}

pub fn read_npcs(r: &mut Reader) -> Result<Vec<NpcBody>> {
    let n = r.len(16)?;
    (0..n)
        .map(|_| {
            Ok(NpcBody {
                id: r.u32()?,
                pos: [r.f32()?, r.f32()?, r.f32()?],
            })
        })
        .collect()
}

/// A seat's hit points (the Arisen and pawns of the fight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vital {
    pub id: PlayerId,
    pub hp: f32,
    pub hp_max: f32,
}

/// Family dispatch tags are co-op protocol decisions, not DDDA enemy ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyFamily {
    Goblin,
    Dragon,
    Unknown(u16),
}

impl EnemyFamily {
    fn tag(self) -> u16 {
        match self {
            Self::Goblin => 0,
            Self::Dragon => 1,
            Self::Unknown(n) => n,
        }
    }
    fn from_tag(n: u16) -> Self {
        match n {
            0 => Self::Goblin,
            1 => Self::Dragon,
            n => Self::Unknown(n),
        }
    }
}

/// Only resource names cross the network. The guest loads its own model
/// and indexed motion banks; animation/model bytes stay in its install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyIdentity {
    pub family: EnemyFamily,
    pub model: String,
    pub banks: Vec<(u8, String)>,
}

/// Grounded actors keep the four-byte yaw. Flying actors preserve every
/// rotation component (quaternion x,y,z,w). This is a wire design choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnemyRotation {
    Yaw(f32),
    Quaternion([f32; 4]),
}

impl EnemyRotation {
    pub fn quaternion(self) -> [f32; 4] {
        match self {
            Self::Quaternion(q) => q,
            Self::Yaw(y) => [0.0, (y * 0.5).sin(), 0.0, (y * 0.5).cos()],
        }
    }
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        if let (Self::Yaw(a), Self::Yaw(b)) = (a, b) {
            let mut d = (b - a) % std::f32::consts::TAU;
            if d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            } else if d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            return Self::Yaw(a + d * t);
        }
        // Shortest-arc normalized linear interpolation, a presentation
        // decision. q and -q describe the same rotation.
        let (a, mut b) = (a.quaternion(), b.quaternion());
        if a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() < 0.0 {
            b = b.map(|x| -x);
        }
        let mut q = std::array::from_fn::<_, 4, _>(|i| a[i] + (b[i] - a[i]) * t);
        let norm = q.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > f32::EPSILON {
            q = q.map(|x| x / norm);
        }
        Self::Quaternion(q)
    }
}

/// One host enemy's identity, pose, action, motion and main HP pool.
#[derive(Debug, Clone, PartialEq)]
pub struct Enemy {
    pub id: u32,
    pub identity: EnemyIdentity,
    pub pos: [f32; 3],
    pub rotation: EnemyRotation,
    pub scale: [f32; 3],
    pub action: i32,
    pub bank: u8,
    pub slot: u16,
    pub frame: f32,
    pub hp: f32,
    pub hp_max: f32,
    pub dead: bool,
}

impl Enemy {
    /// Pose blends between matching lifetimes; discrete state switches
    /// halfway. A replacement identity never blends with the old body.
    pub fn lerp(a: &Enemy, b: &Enemy, t: f32) -> Enemy {
        let mut out = if t < 0.5 { a.clone() } else { b.clone() };
        if a.id != b.id || a.identity != b.identity {
            return out;
        }
        out.pos = std::array::from_fn(|i| a.pos[i] + (b.pos[i] - a.pos[i]) * t);
        out.rotation = EnemyRotation::lerp(a.rotation, b.rotation, t);
        out.scale = std::array::from_fn(|i| a.scale[i] + (b.scale[i] - a.scale[i]) * t);
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
    w.u32(s.stage)
        .i32(s.cell.0)
        .i32(s.cell.1)
        .f32(s.pos[0])
        .f32(s.pos[1])
        .f32(s.pos[2])
        .f32(s.yaw);
}

pub fn read_spot(r: &mut Reader) -> Result<Spot> {
    Ok(Spot {
        stage: r.u32()?,
        cell: (r.i32()?, r.i32()?),
        pos: [r.f32()?, r.f32()?, r.f32()?],
        yaw: r.f32()?,
    })
}

pub fn write_vitals(w: &mut Writer, v: &[Vital]) {
    w.len(v.len());
    for x in v {
        w.u8(x.id).f32(x.hp).f32(x.hp_max);
    }
}

pub fn read_vitals(r: &mut Reader) -> Result<Vec<Vital>> {
    let n = r.len(9)?;
    (0..n)
        .map(|_| {
            Ok(Vital {
                id: r.u8()?,
                hp: r.f32()?,
                hp_max: r.f32()?,
            })
        })
        .collect()
}

// Delta groups: resource identity, position, rotation, scale, animation,
// HP/death. An unchanged enemy remains five bytes; a moving goblin sends
// only the changed groups, never its model or bank paths again.
const IDENTITY: u8 = 1;
const POSITION: u8 = 2;
const ROTATION: u8 = 4;
const SCALE: u8 = 8;
const ANIMATION: u8 = 16;
const HEALTH: u8 = 32;
const ALL: u8 = 63;

fn write_identity(w: &mut Writer, x: &EnemyIdentity) {
    w.u16(x.family.tag()).bytes(x.model.as_bytes()).len(x.banks.len());
    for (bank, path) in &x.banks {
        w.u8(*bank).bytes(path.as_bytes());
    }
}
fn read_identity(r: &mut Reader) -> Result<EnemyIdentity> {
    let family = EnemyFamily::from_tag(r.u16()?);
    let model = String::from_utf8(r.bytes()?).map_err(|e| e.to_string())?;
    let n = r.len(5)?;
    let banks = (0..n)
        .map(|_| Ok((r.u8()?, String::from_utf8(r.bytes()?).map_err(|e| e.to_string())?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(EnemyIdentity { family, model, banks })
}
fn write_rotation(w: &mut Writer, x: EnemyRotation) {
    match x {
        EnemyRotation::Yaw(y) => {
            w.u8(0).f32(y);
        }
        EnemyRotation::Quaternion(q) => {
            w.u8(1);
            for x in q {
                w.f32(x);
            }
        }
    }
}
fn read_rotation(r: &mut Reader) -> Result<EnemyRotation> {
    match r.u8()? {
        0 => Ok(EnemyRotation::Yaw(r.f32()?)),
        1 => Ok(EnemyRotation::Quaternion([
            r.f32()?,
            r.f32()?,
            r.f32()?,
            r.f32()?,
        ])),
        n => Err(format!("enemy rotation kind {n}")),
    }
}
fn vec3(w: &mut Writer, x: [f32; 3]) {
    for v in x {
        w.f32(v);
    }
}
fn read_vec3(r: &mut Reader) -> Result<[f32; 3]> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

pub fn write_enemies(w: &mut Writer, e: &[Enemy], base: &[Enemy]) {
    w.len(e.len());
    for x in e {
        let b = base.iter().find(|b| b.id == x.id);
        let mask = b.map_or(ALL, |b| {
            (if x.identity != b.identity { IDENTITY } else { 0 })
                | (if x.pos != b.pos { POSITION } else { 0 })
                | (if x.rotation != b.rotation { ROTATION } else { 0 })
                | (if x.scale != b.scale { SCALE } else { 0 })
                | (if (x.action, x.bank, x.slot, x.frame) != (b.action, b.bank, b.slot, b.frame) {
                    ANIMATION
                } else {
                    0
                })
                | (if (x.hp, x.hp_max, x.dead) != (b.hp, b.hp_max, b.dead) {
                    HEALTH
                } else {
                    0
                })
        });
        w.u32(x.id).u8(mask);
        if mask & IDENTITY != 0 {
            write_identity(w, &x.identity);
        }
        if mask & POSITION != 0 {
            vec3(w, x.pos);
        }
        if mask & ROTATION != 0 {
            write_rotation(w, x.rotation);
        }
        if mask & SCALE != 0 {
            vec3(w, x.scale);
        }
        if mask & ANIMATION != 0 {
            w.i32(x.action).u8(x.bank).u16(x.slot).f32(x.frame);
        }
        if mask & HEALTH != 0 {
            w.f32(x.hp).f32(x.hp_max).bool(x.dead);
        }
    }
}

pub fn read_enemies(r: &mut Reader, base: &[Enemy]) -> Result<Vec<Enemy>> {
    let n = r.len(5)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u32()?;
        let mask = r.u8()?;
        if mask & !ALL != 0 {
            return Err(format!("enemy {id}: invalid delta mask {mask}"));
        }
        let mut x = match base.iter().find(|b| b.id == id) {
            Some(b) => b.clone(),
            None if mask == ALL => Enemy {
                id,
                identity: EnemyIdentity {
                    family: EnemyFamily::Unknown(u16::MAX),
                    model: String::new(),
                    banks: vec![],
                },
                pos: [0.0; 3],
                rotation: EnemyRotation::Yaw(0.0),
                scale: [1.0; 3],
                action: -1,
                bank: 0,
                slot: 0,
                frame: 0.0,
                hp: 0.0,
                hp_max: 0.0,
                dead: false,
            },
            None => return Err(format!("enemy {id}: delta without a baseline")),
        };
        if mask & IDENTITY != 0 {
            x.identity = read_identity(r)?;
        }
        if mask & POSITION != 0 {
            x.pos = read_vec3(r)?;
        }
        if mask & ROTATION != 0 {
            x.rotation = read_rotation(r)?;
        }
        if mask & SCALE != 0 {
            x.scale = read_vec3(r)?;
        }
        if mask & ANIMATION != 0 {
            x.action = r.i32()?;
            x.bank = r.u8()?;
            x.slot = r.u16()?;
            x.frame = r.f32()?;
        }
        if mask & HEALTH != 0 {
            x.hp = r.f32()?;
            x.hp_max = r.f32()?;
            x.dead = r.bool()?;
        }
        out.push(x);
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
                Victim::Npc { region, .. } => region.hp_max,
            };
            Vital {
                id,
                hp: a.fighter.victim.hp(),
                hp_max,
            }
        })
        .collect();
    let mut enemies: Vec<Enemy> = f
        .goblins
        .iter()
        .map(|g| {
            let hp_max = match &g.fighter.victim {
                Victim::Enemy { regions, .. } => regions.first().map_or(0.0, |r| r.hp_max),
                Victim::Npc { region, .. } => region.hp_max,
                Victim::Player { hp_max, .. } => *hp_max,
            };
            Enemy {
                id: g.fighter.owner,
                pos: g.pos,
                identity: EnemyIdentity {
                    family: EnemyFamily::Goblin,
                    model: f.data.goblin.model.clone(),
                    banks: f
                        .data
                        .goblin
                        .body
                        .banks
                        .iter()
                        .enumerate()
                        .filter_map(|(i, b)| b.as_ref().map(|b| (i as u8, b.path.clone())))
                        .collect(),
                },
                rotation: EnemyRotation::Yaw(g.yaw),
                scale: [f.data.goblin.body.scale; 3],
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
    if let Some(d) = &f.dragon {
        // Admission and the action bind run on the host. An actor that
        // has not bound a motion yet has no drawable pose to replicate.
        if let Some(motion) = d.actor.motion() {
            let model = d
                .data
                .ai
                .resources
                .iter()
                .find(|r| r.class == "rModel" && r.flags & 1 != 0)
                .expect("loaded dragon body has a bound rModel");
            let hp_max = match &d.actor.fighter.victim {
                Victim::Enemy { regions, .. } => regions.first().map_or(0.0, |r| r.hp_max),
                Victim::Npc { region, .. } => region.hp_max,
                Victim::Player { hp_max, .. } => *hp_max,
            };
            enemies.push(Enemy {
                id: d.actor.fighter.owner,
                identity: EnemyIdentity {
                    family: EnemyFamily::Dragon,
                    model: model.path.clone(),
                    banks: d
                        .data
                        .body
                        .banks
                        .iter()
                        .enumerate()
                        .filter_map(|(i, b)| b.as_ref().map(|b| (i as u8, b.path.clone())))
                        .collect(),
                },
                pos: d.actor.pos,
                rotation: EnemyRotation::Quaternion(d.actor.rotation),
                scale: d.scale,
                action: d.actor.current_action().map_or(-1, |a| a as i32),
                bank: motion.bank,
                slot: motion.slot,
                frame: d.actor.frame,
                hp: d.actor.fighter.victim.hp(),
                hp_max,
                dead: d.actor.fighter.victim.dead(),
            });
        }
    }
    enemies.sort_by_key(|e| e.id);
    (vitals, enemies)
}
