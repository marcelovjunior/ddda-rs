//! A story data source with nothing in it, for sessions and tests that
//! run without an install: no scripts, no scenarios, no event table.

use ddstory::data::Data;
use ddstory::Error;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct NoStoryData;

impl Data for NoStoryData {
    fn fsm(&self, path: &str) -> Result<Arc<mtfsm::Graph>, Error> {
        Err(Error::Data(format!("{path}: no story data")))
    }
    fn qct(&self, _: &str) -> Result<Option<Arc<ddquest::qct::QuestCtrl>>, Error> {
        Ok(None)
    }
    fn stage_scripts(&self, _: u32) -> Result<Vec<String>, Error> {
        Ok(vec![])
    }
    fn scenarios(&self, _: u32) -> Result<Vec<(u32, Arc<mtxfs::Xfs>)>, Error> {
        Ok(vec![])
    }
    fn event_table(&self) -> Option<&ddquest::events::EventTable> {
        None
    }
    fn rewards(&self) -> Result<Arc<mtxfs::Xfs>, Error> {
        Err(Error::Data("no story data".into()))
    }
    fn prologue_end(&self) -> Option<&ddstory::exe::PrologueEnd> {
        None
    }
}
