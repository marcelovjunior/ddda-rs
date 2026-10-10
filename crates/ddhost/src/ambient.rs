//! Canonical ambient enemy ownership, independent of renderer/door lifetime.
//! Admission uses the ordinary enemy GPL/kill-record path. Behavioural
//! stepping currently supports authored em8300 idle tables; other controllers
//! expose diagnostics until their world inputs and physical movement are connected.

use ddenemyrt::ambient::{Data, Species, ddai};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlacementId {
    pub stage: u32,
    pub group: u32,
    pub cell: (u16, u16),
    pub record: i32,
}

impl From<&ddspawn::Spawn> for PlacementId {
    fn from(spawn: &ddspawn::Spawn) -> Self {
        Self {
            stage: spawn.stage,
            group: spawn.group,
            cell: spawn.cell,
            record: spawn.set_id,
        }
    }
}

pub struct Instance {
    pub id: PlacementId,
    /// Monotonic allocation generation; stale observations cannot update a
    /// new actor which reused the same authored placement.
    pub lifetime: u64,
    pub data: Arc<Data>,
    pub position: [f32; 3],
    pub angle: [f32; 3],
    pub scale: [f32; 3],
    /// Phase92 root-motion proposal awaiting scenery collision resolution.
    pub proposed_position: Option<[f32; 3]>,
    pub think_root: u32,
    pub runtime: ddenemyrt::ambient::Runtime,
    tables: Arc<ddai::thinktbl::Tables>,
    authored_idle: bool,
}

#[derive(Default)]
pub struct Population {
    instances: BTreeMap<PlacementId, Instance>,
    resources: BTreeMap<String, Arc<Data>>,
    lifetime_counter: u64,
    retired_groups: BTreeSet<(u32, u32)>,
    pub diagnostics: BTreeMap<PlacementId, String>,
}

impl Population {
    pub fn iter(&self) -> impl Iterator<Item = &Instance> {
        self.instances.values()
    }

    /// Run existing GPL admission and kill-record filtering for cells whose
    /// layouts have actually loaded. Unknown families are returned to the
    /// caller; they must not silently disappear from enemy-manager coverage.
    #[allow(clippy::too_many_arguments)]
    pub fn admit_cells(
        &mut self,
        vfs: &ddvfs::Vfs,
        exe: &ddexe::Exe<'_>,
        stage: u32,
        groups: &ddspawn::gpl::GroupList,
        cells: &[(u16, u16)],
        world: &ddspawn::World<'_>,
        records: &[ddspawn::respawn::Record],
    ) -> Result<Vec<ddspawn::Spawn>, String> {
        // Preflight every admitted layout so a missing file cannot masquerade
        // as an empty enemy manager. The generic reader's Option callback is
        // intentionally permissive; this canonical owner needs a strict feed.
        let mut layouts = BTreeMap::new();
        for &cell in cells {
            for group in &groups.groups {
                if !group.cells.iter().any(|c| (c.m, c.n) == cell) {
                    continue;
                }
                match ddspawn::gate(group, world) {
                    ddspawn::Gate::Spawn => {
                        let path = ddspawn::layout_path(stage, cell.0, cell.1, 'e', group.slot);
                        let bytes = vfs
                            .read(&path, mtarc::type_hash("rLayout"))
                            .map_err(|e| format!("ambient layout {path}: {e}"))?;
                        layouts.insert(path, bytes);
                    }
                    ddspawn::Gate::Unported(conditions) => {
                        return Err(format!(
                            "ambient group {} has unresolved admission: {conditions:?}",
                            group.slot
                        ));
                    }
                    _ => {}
                }
            }
        }
        let read = |path: &str| layouts.get(path).cloned();
        let mut unsupported = Vec::new();
        for &cell in cells {
            let (_, spawns) = ddspawn::spawns(stage, groups, cell, &read, world, records)?;
            for spawn in spawns {
                if !self.admit(vfs, exe, &spawn)? {
                    unsupported.push(spawn);
                }
            }
        }
        Ok(unsupported)
    }

    /// Materialize an already admitted generic enemy placement. A renderer
    /// or another player's cell request cannot allocate it a second time.
    pub fn admit(
        &mut self,
        vfs: &ddvfs::Vfs,
        exe: &ddexe::Exe<'_>,
        spawn: &ddspawn::Spawn,
    ) -> Result<bool, String> {
        let Some(species) = Species::from_name(&spawn.name) else {
            return Ok(false);
        };
        let id = PlacementId::from(spawn);
        if self.retired_groups.contains(&(id.stage, id.group)) || self.instances.contains_key(&id) {
            return Ok(true);
        }
        let first = match spawn.record.get("mUseFirstSetThinkTbl") {
            Some(mtlot::Value::Int(0)) => None,
            Some(mtlot::Value::Int(_)) => match spawn.record.get("mStartTableNo") {
                Some(mtlot::Value::Int(index)) => {
                    Some(i32::try_from(*index).map_err(|_| "ambient first table overflows i32")?)
                }
                _ => return Err("ambient first table missing".into()),
            },
            _ => return Err("ambient first-table enable missing".into()),
        };
        let think_root = species.think_root(exe, first)?;
        let placement = spawn
            .record
            .placement()
            .ok_or("ambient placement has no transform")?;
        let data = match self.resources.get(&spawn.name) {
            Some(data) => Arc::clone(data),
            None => {
                let data = Arc::new(Data::load(vfs, exe, species)?);
                self.resources.insert(spawn.name.clone(), Arc::clone(&data));
                data
            }
        };
        let tables = Arc::new(
            ddai::thinktbl::Tables::read_roots(exe, vec![think_root], &[]).map_err(|e| e.to_string())?,
        );
        let authored_idle = species == Species::Em8300 && matches!(first, Some(5 | 6));
        let mut runtime = ddenemyrt::ambient::Runtime::new(&data, think_root)?;
        // uEm8300::vf33 0x00a9be40 locks explicitly authored first tables.
        runtime.think.lock = species == Species::Em8300 && first.is_some();
        self.lifetime_counter = self
            .lifetime_counter
            .checked_add(1)
            .ok_or("ambient lifetime exhausted")?;
        self.instances.insert(
            id,
            Instance {
                id,
                lifetime: self.lifetime_counter,
                data,
                position: spawn.position,
                angle: spawn.angle,
                scale: placement.scale,
                proposed_position: None,
                think_root,
                runtime,
                tables,
                authored_idle,
            },
        );
        Ok(true)
    }

    /// Supported ordinary actor pass, after player movement. The first-table
    /// 5/6 subgraphs contain no world-flag tests, callbacks or random draws.
    /// Other roots remain diagnosed rather than receiving fabricated inputs.
    pub fn step(&mut self, delta: f32, players: &[ddsim::Player], ground: &dyn ddsim::Ground) {
        self.diagnostics.clear();
        for actor in self.instances.values_mut() {
            let result = (|| -> Result<(), String> {
                if !actor.authored_idle {
                    return Err("ambient world signals/flight controller not connected".into());
                }
                let mut runtime = actor.runtime.clone();
                runtime.think.tick(delta);
                // Co-op extension: consider every player and use the closest
                // as slot0, with stable player id breaking distance ties. The
                // original fills party slots0..3 (0x00a9c7d0). These authored
                // idle rows have no range/angle preference among valid slots.
                let distance = |p: &ddsim::Player| {
                    p.position
                        .iter()
                        .zip(actor.position)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f32>()
                };
                runtime.think.slots[0] = players
                    .iter()
                    .min_by(|a, b| distance(a).total_cmp(&distance(b)).then_with(|| a.id.cmp(&b.id)))
                    .map_or_else(ddai::thinktbl::Slot::default, |p| {
                        ddai::thinktbl::Slot::measure(
                            actor.position,
                            actor.angle[1],
                            p.position,
                            0.0,
                            p.id.into(),
                        )
                    });
                let mut callbacks = ClosedIdle::default();
                runtime.select(&actor.data, &actor.tables, &mut callbacks)?;
                if callbacks.random_requested {
                    return Err("unexpected random request in authored ambient idle".into());
                }
                runtime.step_action(&actor.data)?;
                // Start each attempt from the accepted position: failed ground
                // queries must not accumulate travel across unloaded scenery.
                let mut position = actor.position;
                runtime.advance(&actor.data, &mut position, actor.angle[1], delta)?;
                actor.runtime = runtime;
                actor.proposed_position = Some(position);
                // Provisional simple ground snap (assumed 50 cm query margin).
                // The original creatures' scenery collision is not traced;
                // this does not provide wall collision or slope traversal.
                let height = ground
                    .height(position[0], position[2], position[1] + 50.0)
                    .ok_or("ambient root travel unresolved: no ground below proposal")?;
                position[1] = height;
                actor.position = position;
                actor.proposed_position = None;
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics.insert(actor.id, error);
            }
        }
    }

    /// Stage controller destruction also discards its group-removal latches.
    pub fn retire_stage(&mut self, stage: u32) {
        self.instances.retain(|id, _| id.stage != stage);
        self.retired_groups.retain(|(s, _)| *s != stage);
        self.diagnostics.retain(|id, _| id.stage != stage);
    }

    /// Explicit streaming retirement; request sets alone are not evidence
    /// that the original unloaded a cell.
    pub fn retire_cell(&mut self, stage: u32, cell: (u16, u16)) {
        self.instances
            .retain(|id, _| id.stage != stage || id.cell != cell);
    }

    /// Apply a position produced by this actor's controller/physics pass.
    /// Generation validation prevents a late result for a retired actor
    /// from moving its replacement at the same authored placement.
    pub fn set_position(&mut self, id: PlacementId, lifetime: u64, position: [f32; 3]) -> bool {
        let Some(actor) = self.instances.get_mut(&id) else {
            return false;
        };
        if actor.lifetime != lifetime {
            return false;
        }
        actor.position = position;
        actor.proposed_position = None;
        true
    }

    /// The controller's enabled group-removal tests, separate from initial
    /// admission. Returns the placements whose actor lifetimes ended.
    pub fn remove_groups(
        &mut self,
        stage: u32,
        groups: &ddspawn::gpl::GroupList,
        world: &ddspawn::World<'_>,
    ) -> Vec<PlacementId> {
        let removed: Vec<_> = self
            .instances
            .keys()
            .copied()
            .filter(|id| {
                id.stage == stage
                    && groups
                        .group(id.group)
                        .is_some_and(|g| ddspawn::should_despawn(g, world))
            })
            .collect();
        for id in &removed {
            // cLayoutSetCharaBase::vf11 0x0075f860 latches its +0x43
            // state after retiring a group; admission must not recreate it.
            self.retired_groups.insert((id.stage, id.group));
            self.instances.remove(id);
        }
        removed
    }

    pub fn digest(&self) -> u64 {
        let mut hash = crate::hash::StateHasher::default();
        hash.u64(self.lifetime_counter);
        hash.debug(&self.retired_groups);
        for actor in self.iter() {
            hash.debug(&(
                actor.id,
                actor.lifetime,
                actor.data.species,
                actor.position,
                actor.angle,
                actor.scale,
                actor.proposed_position,
                actor.think_root,
                &actor.runtime,
            ));
        }
        hash.finish()
    }
}

#[derive(Default)]
struct ClosedIdle {
    random_requested: bool,
}
impl ddai::thinktbl::Host for ClosedIdle {
    fn check(&mut self, id: u32, _: u32) -> Result<bool, ddai::Error> {
        Err(ddai::Error::Unimplemented(format!("ambient check {id}")))
    }
    fn set(&mut self, id: u32, _: u32) -> Result<u32, ddai::Error> {
        Err(ddai::Error::Unimplemented(format!("ambient commit {id}")))
    }
    fn rand(&mut self) -> u32 {
        self.random_requested = true;
        0
    }
}
