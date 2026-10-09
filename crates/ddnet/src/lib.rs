//! LAN co-op for ddda-rs (M3; docs/adr/0002-game-host.md section 8).
//!
//! The host runs the only simulation (`ddhost::Host`, inside a
//! [`server::Server`]). Clients send their inputs and draw what the host
//! sends back ([`client::Client`]). Everything here is a design decision:
//! the original has no LAN co-op.
//!
//! - [`transport`]: UDP, and a wrapper adding latency, jitter and loss for
//!   tests.
//! - [`link`]: one connection: acks, round trip, a reliable ordered
//!   channel, an unreliable newest-wins one.
//! - [`proto`]: join, refusal, inputs, snapshots, events.
//! - [`body`]: an Arisen's replicated state and its delta encoding.

pub mod body;
pub mod client;
pub mod link;
pub mod mirror;
pub mod proto;
pub mod server;
pub mod transport;

pub use body::Body;
pub use mirror::{Enemy, LookData, Spot, Vital};
pub use client::Client;
pub use server::{Server, ServerConfig};
pub use transport::{Lossy, Transport, Udp};

/// Hash of replicated bodies (FNV over their exact text, as
/// `ddhost::hash`).
pub fn bodies_hash(bodies: &[Body]) -> u64 {
    let mut h = ddhost::hash::StateHasher::default();
    for b in bodies {
        h.debug(b);
    }
    h.finish()
}

/// Hash of everything a snapshot replicates: bodies, HP, enemies (frame
/// and delta fields excluded).
pub fn snapshot_hash(s: &proto::Snapshot) -> u64 {
    let mut h = ddhost::hash::StateHasher::default();
    h.u64(bodies_hash(&s.bodies)).debug(&s.vitals).debug(&s.enemies);
    h.finish()
}

/// A fingerprint of an install: every archive under `native_pc` by
/// relative path and size, in path order. Two machines must agree before
/// they play together (ADR 0002 section 8.3).
pub fn data_fingerprint(native_pc: &std::path::Path) -> std::io::Result<u64> {
    let mut files = vec![];
    let mut stack = vec![native_pc.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let p = e.path();
            if e.file_type()?.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("arc")) {
                let rel = p.strip_prefix(native_pc).unwrap_or(&p).to_string_lossy().to_lowercase().replace('/', "\\");
                files.push((rel, e.metadata()?.len()));
            }
        }
    }
    files.sort();
    let mut h = ddhost::hash::StateHasher::default();
    for (p, n) in &files {
        h.bytes(p.as_bytes()).u64(*n);
    }
    Ok(h.finish())
}
