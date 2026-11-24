//! Table of peers seen through discovery. Pure state, no I/O.

use std::collections::HashMap;
use std::net::SocketAddrV6;
use std::time::{Duration, Instant};

use crate::beacon::NodeId;

/// A peer is forgotten after this long without a beacon.
pub const PEER_TIMEOUT: Duration = Duration::from_secs(15);
/// Maximum number of peers kept in the table.
pub const MAX_PEERS: usize = 256;

/// Result of recording a beacon in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// The peer was not known before.
    New,
    /// The peer was already known; its entry was refreshed.
    Refreshed,
    /// The peer is new but the table is full; it was not recorded.
    TableFull,
}

#[derive(Debug)]
struct Entry {
    addr: SocketAddrV6,
    last_seen: Instant,
}

/// Peers currently considered present on the link.
#[derive(Debug, Default)]
pub struct PeerTable {
    peers: HashMap<NodeId, Entry>,
}

impl PeerTable {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a beacon from `id` received at `now`.
    pub fn observe(&mut self, id: NodeId, addr: SocketAddrV6, now: Instant) -> Observation {
        if let Some(entry) = self.peers.get_mut(&id) {
            entry.addr = addr;
            entry.last_seen = now;
            return Observation::Refreshed;
        }
        if self.peers.len() >= MAX_PEERS {
            return Observation::TableFull;
        }
        self.peers.insert(
            id,
            Entry {
                addr,
                last_seen: now,
            },
        );
        Observation::New
    }

    /// Removes and returns the peers not heard from within `PEER_TIMEOUT`.
    pub fn expire(&mut self, now: Instant) -> Vec<NodeId> {
        let mut lost: Vec<NodeId> = self
            .peers
            .iter()
            .filter(|(_, entry)| now.saturating_duration_since(entry.last_seen) > PEER_TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        lost.sort_unstable();
        for id in &lost {
            self.peers.remove(id);
        }
        lost
    }

    /// Returns the last known address of a peer.
    #[must_use]
    pub fn addr(&self, id: &NodeId) -> Option<SocketAddrV6> {
        self.peers.get(id).map(|entry| entry.addr)
    }

    /// Number of peers in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether the table has no peers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    fn id(n: u16) -> NodeId {
        let mut id = [0u8; 16];
        id[..2].copy_from_slice(&n.to_be_bytes());
        id
    }

    fn addr() -> SocketAddrV6 {
        SocketAddrV6::new(Ipv6Addr::LOCALHOST, 1, 0, 0)
    }

    #[test]
    fn first_beacon_is_new_then_refreshed() {
        let mut table = PeerTable::new();
        let now = Instant::now();
        assert_eq!(table.observe(id(1), addr(), now), Observation::New);
        assert_eq!(table.observe(id(1), addr(), now), Observation::Refreshed);
        assert_eq!(table.len(), 1);
        assert_eq!(table.addr(&id(1)), Some(addr()));
    }

    #[test]
    fn peer_expires_only_after_timeout() {
        let mut table = PeerTable::new();
        let start = Instant::now();
        table.observe(id(1), addr(), start);
        assert!(table.expire(start + PEER_TIMEOUT).is_empty());
        let lost = table.expire(start + PEER_TIMEOUT + Duration::from_secs(1));
        assert_eq!(lost, vec![id(1)]);
        assert!(table.is_empty());
    }

    #[test]
    fn refuses_new_peers_when_full() {
        let mut table = PeerTable::new();
        let now = Instant::now();
        for n in 0..MAX_PEERS as u16 {
            assert_eq!(table.observe(id(n), addr(), now), Observation::New);
        }
        assert_eq!(
            table.observe(id(MAX_PEERS as u16), addr(), now),
            Observation::TableFull
        );
        assert_eq!(table.observe(id(0), addr(), now), Observation::Refreshed);
        assert_eq!(table.len(), MAX_PEERS);
    }
}
