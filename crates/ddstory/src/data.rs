//! Game data the story runtime reads, from the player's install.

use crate::Error;
use ddquest::events::EventTable;
use ddquest::qct::QuestCtrl;
use mtfsm::Graph;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Scenario families in task order (suffix table 0x0163aefc; loader
/// 0x005083c0 loads `scr\stSSS\etc\stSSS_<suffix>`).
pub const SCENARIO_FAMILIES: [(u32, &str); 11] = [
    (1, "flag"),
    (2, "door"),
    (3, "gsf"),
    (4, "fsm"),
    (5, "chp"),
    (6, "ladder"),
    (7, "cam"),
    (8, "mes"),
    (9, "pawn"),
    (10, "etc"),
    (11, "npc"),
];

pub trait Data {
    fn fsm(&self, path: &str) -> Result<Arc<Graph>, Error>;
    /// A quest control table; `None` when the quest has none.
    fn qct(&self, path: &str) -> Result<Option<Arc<QuestCtrl>>, Error>;
    /// rAIFSM paths of the stage archive, in archive order.
    fn stage_scripts(&self, stage: u32) -> Result<Vec<String>, Error>;
    /// The stage's scenario files as (task, XFS), in family order.
    fn scenarios(&self, stage: u32) -> Result<Vec<(u32, Arc<mtxfs::Xfs>)>, Error>;
    fn event_table(&self) -> Option<&EventTable>;
    /// rQuestReward (`etc\questreward\questreward`).
    fn rewards(&self) -> Result<Arc<mtxfs::Xfs>, Error>;
    /// The prologue-end world set-up decoded from DDDA.exe (0x00444c20).
    fn prologue_end(&self) -> Option<&crate::exe::PrologueEnd>;
}

/// Reads from `nativePC` (and `DDDA.exe` for the event table).
pub struct InstallData {
    pub native_pc: PathBuf,
    vfs: ddvfs::Vfs,
    events: Option<EventTable>,
    prologue: Option<crate::exe::PrologueEnd>,
    fsm: Mutex<HashMap<String, Arc<Graph>>>,
}

fn data<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> Error + '_ {
    move |e| Error::Data(format!("{what}: {e}"))
}

impl InstallData {
    /// `exe` is the game's DDDA.exe (packed or unpacked); without it the
    /// Event order cannot resolve rows and fails explicitly.
    pub fn open(native_pc: impl AsRef<Path>, exe: Option<&Path>) -> Result<Self, Error> {
        let native_pc = native_pc.as_ref().to_path_buf();
        let vfs = ddvfs::Vfs::open(&native_pc).map_err(data("nativePC"))?;
        let (events, prologue) = match exe {
            Some(p) => {
                let b = std::fs::read(p).map_err(data("DDDA.exe"))?;
                (
                    Some(EventTable::from_exe(&b).map_err(data("event table"))?),
                    crate::exe::PrologueEnd::from_exe(&b),
                )
            }
            None => (None, None),
        };
        Ok(InstallData { native_pc, vfs, events, prologue, fsm: Mutex::new(HashMap::new()) })
    }

    fn stage_archive(&self, stage: u32) -> PathBuf {
        self.native_pc
            .join("rom")
            .join("stage")
            .join(format!("stage{}", stage / 100 * 100))
            .join(format!("stage{stage}.arc"))
    }
}

impl Data for InstallData {
    fn fsm(&self, path: &str) -> Result<Arc<Graph>, Error> {
        let key = path.to_lowercase();
        if let Some(g) = self.fsm.lock().unwrap().get(&key) {
            return Ok(g.clone());
        }
        let b = self.vfs.read(path, mtarc::type_hash("rAIFSM")).map_err(data(path))?;
        let g = Arc::new(Graph::parse(&b).map_err(data(path))?);
        self.fsm.lock().unwrap().insert(key, g.clone());
        Ok(g)
    }

    fn qct(&self, path: &str) -> Result<Option<Arc<QuestCtrl>>, Error> {
        let t = mtarc::type_hash("rQuestCtrlTbl");
        if self.vfs.locate(path, t).is_none_or(|l| l.is_empty()) {
            return Ok(None);
        }
        let b = self.vfs.read(path, t).map_err(data(path))?;
        Ok(Some(Arc::new(QuestCtrl::parse(&b).map_err(data(path))?)))
    }

    // Translated from 0x005081a0 (DDDA.exe 2017-12-13 Steam), listing part:
    // the stage's own archive, entries in archive order.
    fn stage_scripts(&self, stage: u32) -> Result<Vec<String>, Error> {
        let p = self.stage_archive(stage);
        let (arc, _) = mtarc::Archive::open(&p).map_err(data(&p.display().to_string()))?;
        let t = mtarc::type_hash("rAIFSM");
        Ok(arc.entries.iter().filter(|e| e.type_hash == t).map(|e| e.path.clone()).collect())
    }

    fn scenarios(&self, stage: u32) -> Result<Vec<(u32, Arc<mtxfs::Xfs>)>, Error> {
        let t = mtarc::type_hash("rScenario");
        let mut out = vec![];
        for (task, suffix) in SCENARIO_FAMILIES {
            let path = format!("scr\\st{stage:03}\\etc\\st{stage:03}_{suffix}");
            if self.vfs.locate(&path, t).is_none_or(|l| l.is_empty()) {
                continue;
            }
            let b = self.vfs.read(&path, t).map_err(data(&path))?;
            out.push((task, Arc::new(mtxfs::parse(&b).map_err(data(&path))?)));
        }
        Ok(out)
    }

    fn event_table(&self) -> Option<&EventTable> {
        self.events.as_ref()
    }

    fn prologue_end(&self) -> Option<&crate::exe::PrologueEnd> {
        self.prologue.as_ref()
    }

    fn rewards(&self) -> Result<Arc<mtxfs::Xfs>, Error> {
        let p = r"etc\questreward\questreward";
        let b = self.vfs.read(p, mtarc::type_hash("rQuestReward")).map_err(data(p))?;
        Ok(Arc::new(mtxfs::parse(&b).map_err(data(p))?))
    }
}
