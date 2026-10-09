//! One peer-to-peer link over datagrams (ADR 0002 section 8.2).
//!
//! Every packet carries a header and two channels.
//!
//! Header: `magic "DDCO", version u8, connection id u32, sequence u32,
//! newest sequence received u32, 32 ack bits for the ones before it, next
//! reliable id expected u32`.
//!
//! Channels:
//! - **Reliable, ordered.** Each message has an id. The sender repeats
//!   every message the peer has not acknowledged in every packet, until
//!   the peer's "next expected" passes it. The receiver delivers in id
//!   order and holds early ones.
//! - **Unreliable, newest-wins.** At most one payload per packet (inputs,
//!   snapshots). A payload older than one already received is dropped.
//!
//! There is no fragmentation. Reliable messages are packed up to
//! [`MAX_RELIABLE_BYTES`] per packet, with at least one message per
//! packet.

use ddid::{Reader, Result, Writer};
use std::collections::{BTreeMap, VecDeque};

pub const MAGIC: &[u8; 4] = b"DDCO";
pub const VERSION: u8 = 1;
/// Reliable bytes packed into one packet (the first message always goes).
pub const MAX_RELIABLE_BYTES: usize = 1000;

#[derive(Debug, Clone)]
pub struct Link {
    pub conn: u32,
    next_seq: u32,
    /// Newest sequence received and the 32 before it (bit i = newest-1-i).
    recv_newest: Option<u32>,
    recv_bits: u32,
    /// Newest sequence whose unreliable payload was delivered.
    unreliable_newest: Option<u32>,
    out: VecDeque<(u32, Vec<u8>)>,
    next_rel: u32,
    in_next: u32,
    in_early: BTreeMap<u32, Vec<u8>>,
    sent_at: VecDeque<(u32, u64)>,
    /// Smoothed round trip, milliseconds.
    pub rtt_ms: f32,
    pub last_recv_ms: u64,
    /// Packets received, and those whose sequence was not newer (late or
    /// duplicated).
    pub received: u64,
    pub stale: u64,
}

/// What one packet delivered.
#[derive(Debug, Default)]
pub struct Delivery {
    pub reliable: Vec<Vec<u8>>,
    pub unreliable: Option<Vec<u8>>,
}

impl Link {
    pub fn new(conn: u32, now: u64) -> Link {
        Link {
            conn,
            next_seq: 0,
            recv_newest: None,
            recv_bits: 0,
            unreliable_newest: None,
            out: VecDeque::new(),
            next_rel: 0,
            in_next: 0,
            in_early: BTreeMap::new(),
            sent_at: VecDeque::new(),
            rtt_ms: 0.0,
            last_recv_ms: now,
            received: 0,
            stale: 0,
        }
    }

    /// Queues a reliable message.
    pub fn send_reliable(&mut self, msg: Vec<u8>) {
        self.out.push_back((self.next_rel, msg));
        self.next_rel = self.next_rel.wrapping_add(1);
    }

    /// Reliable messages not yet acknowledged.
    pub fn unacked(&self) -> usize {
        self.out.len()
    }

    /// Builds the next packet with every unacknowledged reliable message
    /// that fits, and `unreliable` if given.
    pub fn packet(&mut self, unreliable: Option<&[u8]>, now: u64) -> Vec<u8> {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.sent_at.push_back((seq, now));
        if self.sent_at.len() > 64 {
            self.sent_at.pop_front();
        }
        let mut w = Writer::new();
        w.u8(MAGIC[0]).u8(MAGIC[1]).u8(MAGIC[2]).u8(MAGIC[3]).u8(VERSION);
        w.u32(self.conn).u32(seq);
        w.bool(self.recv_newest.is_some()).u32(self.recv_newest.unwrap_or(0)).u32(self.recv_bits);
        w.u32(self.in_next);
        let mut size = 0;
        let take: Vec<&(u32, Vec<u8>)> = self
            .out
            .iter()
            .take_while(|(_, m)| {
                let fits = size == 0 || size + m.len() <= MAX_RELIABLE_BYTES;
                size += m.len();
                fits
            })
            .collect();
        w.u16(take.len() as u16);
        for (id, m) in take {
            w.u32(*id).bytes(m);
        }
        w.bool(unreliable.is_some());
        if let Some(u) = unreliable {
            w.bytes(u);
        }
        w.finish()
    }

    /// The connection id of a packet, if it is one of ours.
    pub fn peek_conn(b: &[u8]) -> Option<u32> {
        (b.len() >= 9 && &b[..4] == MAGIC && b[4] == VERSION).then(|| u32::from_le_bytes(b[5..9].try_into().unwrap()))
    }

    /// Takes one received packet.
    pub fn receive(&mut self, b: &[u8], now: u64) -> Result<Delivery> {
        let mut r = Reader::new(b);
        if r.take(4)? != MAGIC || r.u8()? != VERSION {
            return Err("not a DDCO packet of this version".into());
        }
        if r.u32()? != self.conn {
            return Err("packet of another connection".into());
        }
        let seq = r.u32()?;
        let (has_ack, ack, ack_bits) = (r.bool()?, r.u32()?, r.u32()?);
        let peer_next = r.u32()?;
        let n = r.u16()?;
        let mut rel = Vec::with_capacity(n as usize);
        for _ in 0..n {
            rel.push((r.u32()?, r.bytes()?));
        }
        let unreliable = if r.bool()? { Some(r.bytes()?) } else { None };
        r.end()?;

        self.received += 1;
        self.last_recv_ms = now;
        // Our sequences the peer has seen: round trip from the newest.
        if has_ack {
            if let Some(&(_, at)) = self.sent_at.iter().find(|(s, _)| *s == ack) {
                let sample = now.saturating_sub(at) as f32;
                self.rtt_ms = if self.rtt_ms == 0.0 { sample } else { self.rtt_ms * 0.9 + sample * 0.1 };
            }
            let _ = ack_bits;
        }
        // Reliable messages the peer has: drop them.
        while self.out.front().is_some_and(|(id, _)| (id.wrapping_sub(peer_next) as i32) < 0) {
            self.out.pop_front();
        }
        // Our record of the peer's sequences.
        let newer = match self.recv_newest {
            None => true,
            Some(n) => (seq.wrapping_sub(n) as i32) > 0,
        };
        if newer {
            if let Some(n) = self.recv_newest {
                let shift = seq.wrapping_sub(n);
                self.recv_bits = if shift >= 32 { 0 } else { (self.recv_bits << shift) | (1 << (shift - 1)) };
            }
            self.recv_newest = Some(seq);
        } else {
            self.stale += 1;
            if let Some(n) = self.recv_newest {
                let back = n.wrapping_sub(seq);
                if (1..=32).contains(&back) {
                    self.recv_bits |= 1 << (back - 1);
                }
            }
        }
        // Reliable delivery in id order.
        for (id, m) in rel {
            if (id.wrapping_sub(self.in_next) as i32) >= 0 {
                self.in_early.entry(id).or_insert(m);
            }
        }
        let mut d = Delivery::default();
        while let Some(m) = self.in_early.remove(&self.in_next) {
            d.reliable.push(m);
            self.in_next = self.in_next.wrapping_add(1);
        }
        // Unreliable: newest wins.
        if let Some(u) = unreliable {
            let fresh = match self.unreliable_newest {
                None => true,
                Some(n) => (seq.wrapping_sub(n) as i32) > 0,
            };
            if fresh {
                self.unreliable_newest = Some(seq);
                d.unreliable = Some(u);
            }
        }
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reliable_survives_loss_and_reorder() {
        let (mut a, mut b) = (Link::new(7, 0), Link::new(7, 0));
        for i in 0..5u8 {
            a.send_reliable(vec![i; 3]);
        }
        let p1 = a.packet(Some(b"u1"), 0);
        let p2 = a.packet(Some(b"u2"), 10);
        // p1 is lost; p2 carries everything again.
        let d = b.receive(&p2, 20).unwrap();
        assert_eq!(d.reliable.len(), 5);
        assert_eq!(d.unreliable.as_deref(), Some(&b"u2"[..]));
        // The late p1 delivers nothing new and its old unreliable is dropped.
        let d = b.receive(&p1, 25).unwrap();
        assert!(d.reliable.is_empty() && d.unreliable.is_none());
        assert_eq!(b.stale, 1);
        // b's packet acknowledges them; a stops resending.
        let back = b.packet(None, 30);
        a.receive(&back, 40).unwrap();
        assert_eq!(a.unacked(), 0);
        assert!(a.rtt_ms > 0.0);
        assert_eq!(Link::peek_conn(&back), Some(7));
    }
}
