//! Per-player input for one host frame.
//!
//! In the original the input singletons read the devices before anything
//! else moves (`sBbsRpgMain::vf06` 0x0041DD60 calls `sQlocInput::vf06`
//! 0x00409EE0, `sKeyboard::vf06` 0x00DF40E0 and `sMouse::vf06`
//! 0x00E04F20 before `sAI::vf06` and the unit lines; docs/FRAME.md). The
//! host does the same: a frame's inputs are fixed before any system runs,
//! so the same inputs give the same frame (the replay property co-op needs).

use crate::player::PlayerId;
use std::collections::BTreeMap;

/// What one Arisen's controls say for one host frame. Buttons are held
/// states; the host derives presses (rising edges) itself, so a repeated
/// or re-sent input never presses twice.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerInput {
    /// Movement, camera yaw, walk, sprint, jump (`ddsim::Input`).
    pub sim: ddsim::Input,
    /// The interact button (Converse / scenario prompts) is held.
    pub interact: bool,
}

/// All players' input for one host frame, ordered by seat.
pub type FrameInputs = BTreeMap<PlayerId, PlayerInput>;

/// Rising-edge detection for buttons the host interprets itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Edges {
    interact_held: BTreeMap<PlayerId, bool>,
}

impl Edges {
    /// Players who pressed interact this frame, in seat order.
    pub(crate) fn interact_presses(&mut self, inputs: &FrameInputs) -> Vec<PlayerId> {
        let mut out = vec![];
        for (&id, i) in inputs {
            let was = self.interact_held.insert(id, i.interact).unwrap_or(false);
            if i.interact && !was {
                out.push(id);
            }
        }
        // A player with no input this frame has released everything.
        for (id, held) in self.interact_held.iter_mut() {
            if !inputs.contains_key(id) {
                *held = false;
            }
        }
        out
    }
}
