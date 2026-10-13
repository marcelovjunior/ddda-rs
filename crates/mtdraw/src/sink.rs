//! Where the executor's commands can go instead of the Direct3D 9 device
//! (L119, step N2 of the native renderer).
//!
//! A [`CommandSink`] installed with [`set_sink`] is offered every record
//! the executor dispatches (after a pending merged draw was decided). A
//! sink that handles a record does the whole command (the native path);
//! the executor then skips the record's apply calls and device calls and
//! goes on with the next entry. A record the sink leaves runs exactly as
//! without a sink (the default path: the lifted applies and the device's
//! vtable), so a sink can take over command by command.
//!
//! Why this keeps the D3D9 state right for code that still calls the
//! device directly (213 sites, docs/NDRAW.md): a handled command changes
//! neither the device nor the executor context's record of what is applied
//! (the apply functions' identity/hash suppression), so the two stay
//! consistent: the next default command applies against the true device
//! state, and direct calls see the state the last default command left.
//! A sink must therefore leave the guest's D3D9 objects alone, and must
//! make its own output reach the backend in order with the D3D9 commands
//! (the same deferred backend queue).
//!
//! Without a sink the executor makes one atomic load per frame and is the
//! exact port (crates/mtdraw/tests/differential.rs).

use ddnative_base::mem::Mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// What a sink did with a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// The executor runs the command (the default path).
    Default,
    /// The sink did the command; the executor skips it.
    Handled,
}

/// A frame's command stream, offered to a native implementation.
pub trait CommandSink: Send + Sync {
    /// The executor begins a frame: `render` = sRender, `context` = the
    /// executor's context (0xE28 bytes in its frame; built by 0x00DA6DA0,
    /// read by the apply functions). After BeginScene.
    fn frame_begin(&self, _m: &mut dyn Mem, _render: u32, _context: u32) {}

    /// A record of type `kind` (the low nibble at +2; 0..15) is about to
    /// run. Nested lists (6) are only offered; the executor always walks
    /// them itself.
    fn command(&self, m: &mut dyn Mem, context: u32, record: u32, kind: u32) -> Taken;

    /// The frame's commands are done: before EndScene.
    fn frame_end(&self, _m: &mut dyn Mem, _render: u32, _context: u32) {}
}

static INSTALLED: AtomicBool = AtomicBool::new(false);
static SINK: RwLock<Option<Arc<dyn CommandSink>>> = RwLock::new(None);

/// Install `sink` (None: the default path for everything).
pub fn set_sink(sink: Option<Arc<dyn CommandSink>>) {
    let mut s = SINK.write().unwrap_or_else(|e| e.into_inner());
    INSTALLED.store(sink.is_some(), Ordering::Release);
    *s = sink;
}

/// The installed sink, for one frame.
pub(crate) fn sink() -> Option<Arc<dyn CommandSink>> {
    if !INSTALLED.load(Ordering::Acquire) {
        return None;
    }
    SINK.read().unwrap_or_else(|e| e.into_inner()).clone()
}
