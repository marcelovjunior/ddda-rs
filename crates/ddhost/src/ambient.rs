//! Canonical ambient enemy ownership, independent of renderer/door lifetime.
//! Admission uses the ordinary enemy GPL/kill-record path. Behavioural
//! stepping is not yet connected; these actors currently retain spawn poses.

use ddenemyrt::ambient::{Data, Species};
use std::collections::BTreeMap;
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
    pub think_root: u32,
    pub runtime: ddenemyrt::ambient::Runtime,
}

#[derive(Default)]
pub struct Population {
    instances: BTreeMap<PlacementId, Instance>,
    resources: BTreeMap<String, Arc<Data>>,
    lifetime_counter: u64,
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
        if self.instances.contains_key(&id) {
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
                think_root,
                runtime,
            },
        );
        Ok(true)
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
            self.instances.remove(id);
        }
        removed
    }

    pub fn digest(&self) -> u64 {
        let mut hash = crate::hash::StateHasher::default();
        hash.u64(self.lifetime_counter);
        for actor in self.iter() {
            hash.debug(&(
                actor.id,
                actor.lifetime,
                actor.data.species,
                actor.position,
                actor.angle,
                actor.scale,
                actor.think_root,
                &actor.runtime,
            ));
        }
        hash.finish()
    }
}
