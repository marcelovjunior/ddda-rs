//! Anodyne's native message receipt. Admission into a HealingSpot and the
//! recipient's ability to apply HP are separate decisions; zero HP is not
//! an actor death-marker provider.

/// An authoritative +31EA observation, not a health test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Known(bool),
    Unknown,
}

pub struct Markers {
    pub global_delta: f32,
    pub actors: std::collections::BTreeMap<u8, Marker>,
}

impl Markers {
    /// Head8865's narrow accepted S53 lifecycle provider. Other action states
    /// remain Unknown; a Wait actor is not proof of the native marker value.
    // Native fatal vf72 0x00B7C566 sets +31EA; 0x00B735C9 latches +31ED
    // before fatal requests. 0x00B6D57A clears +31EA before PawnReturn81.
    // DDDA.exe 2017-12-13 Steam.
    pub fn accepted_death_phases(sim: &ddsim::Sim, global_delta: f32) -> Self {
        use ddsim::{DeathPhase, State};
        Self {
            global_delta,
            actors: sim.players.iter().map(|p| {
                let marker = match p.state {
                    State::Death { phase: DeathPhase::PawnReturn } => Marker::Known(false),
                    State::Death { phase: DeathPhase::Collapse { .. } | DeathPhase::ArisenDead
                        | DeathPhase::PawnDownStart | DeathPhase::PawnDown } => Marker::Known(true),
                    _ => Marker::Unknown,
                };
                (p.id, marker)
            }).collect(),
        }
    }
}

/// Values retained from the owner's installed HealingSpot/HealingUnit, never
/// a copied spell table. This reader does not itself spawn a gameplay field.
#[derive(Debug, Clone)]
pub struct Data {
    pub offset: [f32; 3],
    pub joint: i32,
    pub spot_lifetime: f32,
    pub target_count: usize,
    pub limit_id: i32,
    pub reset_child: bool,
    pub clear_target_without_child: bool,
    pub value: f32,
    pub value_end: f32,
    pub end_frame: f32,
    pub one_hit: bool,
    pub generator_rows: Vec<(f32, usize)>,
}

impl Data {
    pub fn load(vfs: &ddvfs::Vfs, magic: &ddskill::magic::Magic) -> Result<Self, String> {
        use mtxfs::{Object, Value};
        if magic.manual_type != 0 || magic.levels.is_empty() {
            return Err("unsupported healing manual type/level".into());
        }
        let spec = &magic.levels[0].shells[0];
        if spec.position_type != 0 || spec.joint != -1 || spec.delay != 0.0 {
            return Err("unsupported healing spawn position/delay".into());
        }
        let bytes = crate::body::load::read(vfs, crate::shell::SHELL_LIST, "rShlParamList")?;
        let list = ddenemy::ShellParamList::parse(&bytes)?;
        let group = list
            .groups
            .get(usize::try_from(spec.group).map_err(|_| "negative healing group")?)
            .and_then(Option::as_ref)
            .ok_or("missing healing group")?;
        let shell = |index: usize| {
            group
                .shells
                .get(index)
                .and_then(Option::as_ref)
                .ok_or_else(|| format!("missing healing shell {index}"))
        };
        let object = |number| {
            list.source
                .walk()
                .into_iter()
                .find(|o| o.number == number)
                .ok_or_else(|| format!("missing healing object {number}"))
        };
        let integer = |o: &Object, name: &str| -> Result<i32, String> {
            match list.source.get(o, name).and_then(|v| v.first()) {
                Some(Value::S(v)) => i32::try_from(*v).map_err(|_| format!("invalid healing {name}")),
                Some(Value::U(v)) => i32::try_from(*v).map_err(|_| format!("invalid healing {name}")),
                _ => Err(format!("missing healing {name}")),
            }
        };
        let float = |o: &Object, name: &str| -> Result<f32, String> {
            match list.source.get(o, name).and_then(|v| v.first()) {
                Some(Value::F(v)) if v.len() == 1 && v[0].is_finite() => Ok(v[0] as f32),
                _ => Err(format!("missing finite healing {name}")),
            }
        };
        let flag = |o: &Object, name: &str| -> Result<bool, String> {
            match list.source.get(o, name).and_then(|v| v.first()) {
                Some(Value::Bool(v)) => Ok(*v),
                _ => Err(format!("missing healing {name}")),
            }
        };
        let generator = shell(usize::try_from(spec.index).map_err(|_| "negative healing generator")?)?;
        if generator.class_hash != mtarc::type_hash("cShlParamGenerator") {
            return Err("unsupported healing generator class".into());
        }
        let generator_object = object(generator.object_number)?;
        let shoot_list = match list
            .source
            .get(generator_object, "shlList")
            .and_then(|v| v.first())
        {
            Some(Value::Object(Some(o))) => o,
            _ => return Err("missing healing generator list".into()),
        };
        let mut generator_rows = Vec::new();
        let mut spot = None;
        for row in list
            .source
            .get(shoot_list, "mpArray")
            .ok_or("missing healing generator rows")?
        {
            let Value::Object(Some(row)) = row else {
                return Err("null healing generator row".into());
            };
            let index = usize::try_from(integer(row, "shlNo")?).map_err(|_| "negative healing child")?;
            let child = shell(index)?;
            generator_rows.push((float(row, "waitFrame")?, index));
            if child.class_hash == mtarc::type_hash("cShlParamHealingSpot") {
                if spot.replace(child).is_some() {
                    return Err("multiple healing spots unsupported".into());
                }
            }
        }
        let spot = spot.ok_or("generator has no healing spot")?;
        let spot_object = object(spot.object_number)?;
        let unit = shell(
            usize::try_from(integer(spot_object, "shlNoHealing")?).map_err(|_| "negative healing unit")?,
        )?;
        if unit.class_hash != mtarc::type_hash("cShlParamHealingUnit")
            || !spot.is_time_die
            || unit.is_time_die
        {
            return Err("unsupported healing class/lifetime".into());
        }
        let unit_object = object(unit.object_number)?;
        if integer(unit_object, "type")? != 0 {
            return Err("non-HP healing unsupported".into());
        }
        let target_count = usize::try_from(integer(spot_object, "targetNum")?)
            .map_err(|_| "negative healing target count")?;
        if target_count > 10 {
            return Err("healing target count exceeds native slots".into());
        }
        for o in [spot_object, unit_object] {
            for name in ["at_0", "at_1", "at_2"] {
                if integer(o, name)? >= 0 {
                    return Err("healing support must not invent attack damage".into());
                }
            }
        }
        Ok(Self {
            offset: spec.offset,
            joint: spec.joint,
            spot_lifetime: spot.die_frame,
            target_count,
            limit_id: integer(spot_object, "limitID")?,
            reset_child: flag(spot_object, "isResetShlDie")?,
            clear_target_without_child: flag(spot_object, "isShlDieNoHit")?,
            value: float(unit_object, "value")?,
            value_end: float(unit_object, "valueEnd")?,
            end_frame: float(unit_object, "endFrame")?,
            one_hit: flag(unit_object, "isOneHit")?,
            generator_rows,
        })
    }
}

/// Resolve the initial generator position, not a stand-in final field center.
/// The provider must implement the native ray query's scene/triangle filters;
/// a missing joint is an unsupported spawn, never an offset from the feet.
// Translated from 0x00B4A324..0x00B4A3F5 and 0x00B4B330
// (DDDA.exe 2017-12-13 Steam), ordinary manual0/position0/joint-1 case.
pub fn generator_position(
    pose: &dyn ddcombat::volumes::Pose,
    offset: [f32; 3],
    ray: &mut dyn FnMut([f32; 3], [f32; 3]) -> Option<[f32; 3]>,
) -> Result<[f32; 3], String> {
    let mut position = ddcombat::volumes::transform_point(&pose.model_matrix(), offset);
    let joint = pose.joint_matrix(4).ok_or("healing spawn has no native joint4")?;
    let start = [joint[3][0], joint[3][1], joint[3][2]];
    let delta: [f32; 3] = std::array::from_fn(|i| start[i] - position[i]);
    let distance_sq = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
    // Embedded B4B3F7 threshold at 0x0152CF28.
    if distance_sq >= 0.01 {
        if let Some(hit) = ray(start, position) {
            // Native segment-hit backoff at 0x0153E46C: 120cm.
            let scale = 120.0 / distance_sq.sqrt();
            position = std::array::from_fn(|i| hit[i] + delta[i] * scale);
        }
    }
    // Embedded down-ray endpoints at 0x014EA160/0x01520BA8.
    let from = [position[0], position[1] + 30.0, position[2]];
    let to = [position[0], position[1] - 10000.0, position[2]];
    if let Some(hit) = ray(from, to) {
        // B4A3B6 supplies offsetY + 0x014F7270 (5cm) to B4B5DA.
        position[1] = hit[1] + offset[1] + 5.0;
    }
    Ok(position)
}

/// Actor-local +3D14/+3D1C state, retained for this actor lifetime only.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Receipt {
    pub pending: bool,
    pub amount: f32,
}

impl Receipt {
    /// Pass-C msg10 accumulates its first float, even on a dead recipient.
    // Translated from 0x00B79FC0 msg0x10 (DDDA.exe 2017-12-13 Steam).
    pub fn receive(&mut self, amount: f32) {
        self.pending = true;
        self.amount += amount;
    }

    /// The recipient's next player update. `dead_marker` is +31EA/vf117
    /// (0x00B53160), independent of HP and knockdown. The caller provides
    /// global WorkRate delta, not an animation substep or target slow rate.
    /// Ordinary msg10 has no white-HP gain and does not clear the dead marker.
    // Translated from 0x00B5C3B0..0x00B5C648 and 0x00B7A960
    // (DDDA.exe 2017-12-13 Steam).
    pub fn apply(
        &mut self,
        dead_marker: bool,
        global_delta: f32,
        hp: &mut f32,
        white_hp: f32,
        maximum_hp: f32,
    ) -> f32 {
        let mut applied = 0.0;
        if self.pending && !dead_marker {
            let requested = self.amount * global_delta;
            // B7A960's positive white cap; Anodyne's ordinary gain is positive.
            let allowed = if *hp + requested > white_hp {
                (white_hp - *hp).max(0.0)
            } else {
                requested
            };
            let before = *hp;
            *hp = (*hp + allowed).min(maximum_hp);
            applied = *hp - before;
            self.amount = 0.0;
        }
        // Dead skips B5C489's scalar clear but still reaches B5C637's
        // pending-flag clear. A later msg10 can reactivate that inert scalar.
        self.pending = false;
        applied
    }
}

/// Native HealingUnit out-of-range/parent-link timer (+2A14). Position is
/// the unit's actual previous world position, not a guessed actor center.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitExit {
    pub elapsed: f32,
}

impl UnitExit {
    // Translated from 0x00C04ADA..0x00C04B72 (DDDA.exe 2017-12-13 Steam).
    pub fn update(
        &mut self,
        position: [f32; 3],
        original_center: [f32; 3],
        parent_alive: bool,
        unit_delta: f32,
        end_frame: f32,
    ) -> bool {
        let distance = ((position[0] - original_center[0]).powi(2)
            + (position[1] - original_center[1]).powi(2)
            + (position[2] - original_center[2]).powi(2))
        .sqrt();
        // Embedded radius loaded at 0x00C0456E from 0x014F70DC: 400cm.
        if distance > 400.0 || !parent_alive {
            self.elapsed += unit_delta;
            self.elapsed < end_frame
        } else {
            self.elapsed = 0.0;
            true
        }
    }

    /// Resource values are read by the caller, never copied spell data.
    // Translated from 0x00C04ED9..0x00C04F09 (DDDA.exe 2017-12-13 Steam).
    pub fn gain(&self, value: f32, value_end: f32, one_hit: bool, modifier: f32) -> f32 {
        let value = if self.elapsed > 0.0 { value_end } else { value };
        if one_hit {
            value
        } else {
            value * modifier
        }
    }
}

/// Native party-distance admission. Target lifetime/order and the native
/// ten-slot spot budget remain the caller's responsibility; HP is not a gate.
// Translated from 0x00C0429E..0x00C0431A (DDDA.exe 2017-12-13 Steam).
pub fn admits(center: [f32; 3], actor_world: [f32; 3]) -> bool {
    let x = actor_world[0] - center[0];
    let y = actor_world[1] - center[1];
    let z = actor_world[2] - center[2];
    // Embedded squared radius at 0x0161CDB4, not sensorTargetRange.
    x * x + y * y + z * z < 122_500.0
}
