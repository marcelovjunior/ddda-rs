//! Device-facing adapter for the legacy --play fixture. Opening gameplay bags,
//! grants, saving and use requests live in ddhost; no opening inventory state
//! or new-game item generation remains in this presentation module.
use bevy::prelude::Resource;
use ddhost::ddenemyrt::fight::Fight;
use ddsim::{PlayerId, Sim};

#[derive(Resource)]
pub struct QuickItems(ddhost::items::ImportedItems);

impl QuickItems {
    pub fn load(vfs: &ddvfs::Vfs, xml: &str, locals: &[PlayerId]) -> Result<Self, String> {
        ddhost::items::ImportedItems::load(vfs, xml, locals).map(Self)
    }
    pub fn use_next(&mut self, seat: PlayerId, sim: &mut Sim, fight: &mut Fight) -> Result<i16, String> {
        self.0.use_next(seat, sim, fight)
    }
}
