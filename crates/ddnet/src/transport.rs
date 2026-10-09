//! Datagram transports: real UDP, and a wrapper that delays and drops
//! packets for tests (simulated latency, jitter and loss).

use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, UdpSocket};

/// Sends and receives datagrams. `now` is the caller's clock in ms; real
/// sockets ignore it, the lossy wrapper schedules by it.
pub trait Transport {
    fn send(&mut self, to: SocketAddr, bytes: &[u8], now: u64);
    fn recv(&mut self, now: u64) -> Option<(SocketAddr, Vec<u8>)>;
    fn local_addr(&self) -> SocketAddr;
}

/// A non-blocking UDP socket.
pub struct Udp {
    sock: UdpSocket,
    buf: Vec<u8>,
}

impl Udp {
    pub fn bind(addr: &str) -> io::Result<Udp> {
        let sock = UdpSocket::bind(addr)?;
        sock.set_nonblocking(true)?;
        Ok(Udp { sock, buf: vec![0; 65536] })
    }
}

impl Transport for Udp {
    fn send(&mut self, to: SocketAddr, bytes: &[u8], _: u64) {
        // A full socket buffer or an unreachable peer is a lost packet.
        let _ = self.sock.send_to(bytes, to);
    }
    fn recv(&mut self, _: u64) -> Option<(SocketAddr, Vec<u8>)> {
        loop {
            match self.sock.recv_from(&mut self.buf) {
                Ok((n, from)) => return Some((from, self.buf[..n].to_vec())),
                // Windows reports a peer's closed port as an error on the
                // next receive (WSAECONNRESET): skip it.
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(_) => return None,
            }
        }
    }
    fn local_addr(&self) -> SocketAddr {
        self.sock.local_addr().expect("bound socket")
    }
}

/// Delays every outgoing packet by `latency_ms` plus up to `jitter_ms`,
/// and drops a fraction `loss` of them, from a seeded generator (the same
/// seed gives the same drops and delays).
pub struct Lossy<T: Transport> {
    pub inner: T,
    pub latency_ms: u64,
    pub jitter_ms: u64,
    pub loss: f64,
    rng: u64,
    queue: VecDeque<(u64, SocketAddr, Vec<u8>)>,
    pub dropped: u64,
    pub sent: u64,
}

impl<T: Transport> Lossy<T> {
    pub fn new(inner: T, latency_ms: u64, jitter_ms: u64, loss: f64, seed: u64) -> Self {
        Lossy { inner, latency_ms, jitter_ms, loss, rng: seed | 1, queue: VecDeque::new(), dropped: 0, sent: 0 }
    }

    fn next(&mut self) -> u64 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn flush(&mut self, now: u64) {
        // Jitter can reorder packets: send whatever is due, in any order.
        let mut i = 0;
        while i < self.queue.len() {
            if self.queue[i].0 <= now {
                let (_, to, b) = self.queue.remove(i).unwrap();
                self.inner.send(to, &b, now);
            } else {
                i += 1;
            }
        }
    }
}

impl<T: Transport> Transport for Lossy<T> {
    fn send(&mut self, to: SocketAddr, bytes: &[u8], now: u64) {
        self.sent += 1;
        let roll = (self.next() % 1_000_000) as f64 / 1_000_000.0;
        if roll < self.loss {
            self.dropped += 1;
            return;
        }
        let jitter = if self.jitter_ms > 0 { self.next() % (self.jitter_ms + 1) } else { 0 };
        self.queue.push_back((now + self.latency_ms + jitter, to, bytes.to_vec()));
        self.flush(now);
    }
    fn recv(&mut self, now: u64) -> Option<(SocketAddr, Vec<u8>)> {
        self.flush(now);
        self.inner.recv(now)
    }
    fn local_addr(&self) -> SocketAddr {
        self.inner.local_addr()
    }
}
