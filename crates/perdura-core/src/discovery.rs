//! LAN peer discovery over IPv6 link-local multicast.
//!
//! Each node announces itself to a multicast group. A node that receives an
//! announce answers with a direct reply, so two nodes find each other even
//! when only one of them can receive multicast.
//!
//! Discovery is a hint, not authentication: any host on the link can forge
//! beacons. Peers are authenticated later, when a connection is made.

use std::io;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket};
use std::time::{Duration, Instant};

use crate::beacon::{Beacon, BeaconKind, DecodeError, MAX_DATAGRAM_SIZE, NODE_ID_LEN, NodeId};
use crate::peers::{Observation, PeerTable};

/// Multicast group used for discovery (transient, link-local scope). Provisional.
pub const DISCOVERY_GROUP: Ipv6Addr = Ipv6Addr::new(0xff12, 0, 0, 0, 0, 0, 0x7065, 0x7264);
/// UDP port used for discovery. Provisional.
pub const DISCOVERY_PORT: u16 = 29170;
/// Nominal time between two announces.
pub const BEACON_INTERVAL: Duration = Duration::from_secs(5);

const BEACON_JITTER_MS: u64 = 500;
const SEND_RETRY: Duration = Duration::from_millis(500);
const MAX_WAIT: Duration = Duration::from_secs(1);
const MIN_WAIT: Duration = Duration::from_millis(10);

/// Something that happened on the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A peer was seen for the first time.
    PeerFound {
        /// Ephemeral identifier of the peer.
        id: NodeId,
        /// Link-local address and advertised port of the peer.
        addr: SocketAddrV6,
    },
    /// A peer stopped sending beacons.
    PeerLost {
        /// Ephemeral identifier of the peer.
        id: NodeId,
    },
}

/// Local counters. Never sent anywhere.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Announces sent to the multicast group.
    pub beacons_sent: u64,
    /// Failed attempts to send an announce or a reply.
    pub send_errors: u64,
    /// Valid beacons received from other nodes, announces and replies.
    pub beacons_received: u64,
    /// Direct replies sent.
    pub replies_sent: u64,
    /// Direct replies received. Also counted in `beacons_received`.
    pub replies_received: u64,
    /// Datagrams dropped for being malformed.
    pub dropped_malformed: u64,
    /// Datagrams dropped for an unknown protocol version.
    pub dropped_unsupported_version: u64,
    /// Datagrams dropped for carrying a type that is not a beacon.
    pub dropped_unknown_type: u64,
    /// Datagrams dropped for exceeding the size limit.
    pub dropped_oversized: u64,
    /// Datagrams dropped because the source was not link-local.
    pub dropped_not_link_local: u64,
    /// Beacons dropped because the peer table was full.
    pub dropped_table_full: u64,
}

/// Generates a fresh random node identifier.
///
/// # Errors
///
/// Fails when the system random source is unavailable.
pub fn new_node_id() -> io::Result<NodeId> {
    let mut id = [0u8; NODE_ID_LEN];
    getrandom::fill(&mut id).map_err(|_| io::Error::other("system random source unavailable"))?;
    Ok(id)
}

/// Runs discovery on the interface `ifindex` until `deadline`, or forever
/// when `deadline` is `None`. Calls `on_event` for every peer found or lost.
///
/// # Errors
///
/// Fails when the socket cannot be set up or stops working.
pub fn run(
    ifindex: u32,
    node_id: NodeId,
    deadline: Option<Instant>,
    on_event: &mut dyn FnMut(Event),
) -> io::Result<Stats> {
    let socket = UdpSocket::bind(SocketAddrV6::new(
        Ipv6Addr::UNSPECIFIED,
        DISCOVERY_PORT,
        0,
        0,
    ))?;
    socket.join_multicast_v6(&DISCOVERY_GROUP, ifindex)?;

    let destination = SocketAddrV6::new(DISCOVERY_GROUP, DISCOVERY_PORT, 0, ifindex);
    let announce = Beacon {
        kind: BeaconKind::Announce,
        node_id,
        port: DISCOVERY_PORT,
    }
    .encode();
    let reply = Beacon {
        kind: BeaconKind::Reply,
        node_id,
        port: DISCOVERY_PORT,
    }
    .encode();

    let mut table = PeerTable::new();
    let mut stats = Stats::default();
    let mut buffer = [0u8; MAX_DATAGRAM_SIZE + 1];
    let mut next_beacon = Instant::now();

    loop {
        let now = Instant::now();
        if deadline.is_some_and(|limit| now >= limit) {
            break;
        }

        if now >= next_beacon {
            if socket.send_to(&announce, destination).is_ok() {
                stats.beacons_sent += 1;
                next_beacon = now + next_interval();
            } else {
                stats.send_errors += 1;
                next_beacon = now + SEND_RETRY;
            }
        }

        for id in table.expire(now) {
            on_event(Event::PeerLost { id });
        }

        let wait = next_beacon
            .saturating_duration_since(now)
            .clamp(MIN_WAIT, MAX_WAIT);
        socket.set_read_timeout(Some(wait))?;

        match socket.recv_from(&mut buffer) {
            Ok((len, SocketAddr::V6(source))) => {
                let reply_due = handle_datagram(
                    &buffer[..len],
                    source,
                    node_id,
                    &mut table,
                    &mut stats,
                    on_event,
                );
                if reply_due {
                    if socket.send_to(&reply, source).is_ok() {
                        stats.replies_sent += 1;
                    } else {
                        stats.send_errors += 1;
                    }
                }
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(stats)
}

/// Processes one received datagram. Returns whether a direct reply must be
/// sent to `source`.
fn handle_datagram(
    datagram: &[u8],
    source: SocketAddrV6,
    own_id: NodeId,
    table: &mut PeerTable,
    stats: &mut Stats,
    on_event: &mut dyn FnMut(Event),
) -> bool {
    if (source.ip().segments()[0] & 0xffc0) != 0xfe80 {
        stats.dropped_not_link_local += 1;
        return false;
    }

    let beacon = match Beacon::decode(datagram) {
        Ok(beacon) => beacon,
        Err(DecodeError::Oversized) => {
            stats.dropped_oversized += 1;
            return false;
        }
        Err(DecodeError::Malformed) => {
            stats.dropped_malformed += 1;
            return false;
        }
        Err(DecodeError::UnsupportedVersion(_)) => {
            stats.dropped_unsupported_version += 1;
            return false;
        }
        Err(DecodeError::UnknownType(_)) => {
            stats.dropped_unknown_type += 1;
            return false;
        }
    };

    if beacon.node_id == own_id {
        return false;
    }
    stats.beacons_received += 1;
    if beacon.kind == BeaconKind::Reply {
        stats.replies_received += 1;
    }

    let now = Instant::now();
    let addr = SocketAddrV6::new(*source.ip(), beacon.port, 0, source.scope_id());
    match table.observe(beacon.node_id, addr, now) {
        Observation::New => on_event(Event::PeerFound {
            id: beacon.node_id,
            addr,
        }),
        Observation::Refreshed => {}
        Observation::TableFull => {
            stats.dropped_table_full += 1;
            return false;
        }
    }

    // A reply is never answered, which rules out loops.
    beacon.kind == BeaconKind::Announce && table.reply_due(&beacon.node_id, now)
}

/// Beacon interval with random jitter, so nodes do not stay synchronized.
fn next_interval() -> Duration {
    let mut raw = [0u8; 2];
    let offset_ms = if getrandom::fill(&mut raw).is_ok() {
        u64::from(u16::from_be_bytes(raw)) % (2 * BEACON_JITTER_MS + 1)
    } else {
        BEACON_JITTER_MS
    };
    BEACON_INTERVAL - Duration::from_millis(BEACON_JITTER_MS) + Duration::from_millis(offset_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN_ID: NodeId = [0xaa; NODE_ID_LEN];
    const PEER_ID: NodeId = [0xbb; NODE_ID_LEN];

    struct Outcome {
        reply_due: bool,
        events: Vec<Event>,
        stats: Stats,
    }

    fn link_local_source() -> SocketAddrV6 {
        SocketAddrV6::new(
            Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1),
            DISCOVERY_PORT,
            0,
            3,
        )
    }

    fn datagram(kind: BeaconKind, node_id: NodeId) -> Vec<u8> {
        Beacon {
            kind,
            node_id,
            port: DISCOVERY_PORT,
        }
        .encode()
    }

    fn feed(table: &mut PeerTable, bytes: &[u8], source: SocketAddrV6) -> Outcome {
        let mut events = Vec::new();
        let mut stats = Stats::default();
        let reply_due = handle_datagram(bytes, source, OWN_ID, table, &mut stats, &mut |event| {
            events.push(event);
        });
        Outcome {
            reply_due,
            events,
            stats,
        }
    }

    #[test]
    fn announce_from_a_new_peer_is_recorded_and_answered() {
        let mut table = PeerTable::new();
        let outcome = feed(
            &mut table,
            &datagram(BeaconKind::Announce, PEER_ID),
            link_local_source(),
        );
        assert!(outcome.reply_due);
        assert_eq!(
            outcome.events,
            vec![Event::PeerFound {
                id: PEER_ID,
                addr: link_local_source(),
            }]
        );
        assert_eq!(outcome.stats.beacons_received, 1);
        assert_eq!(outcome.stats.replies_received, 0);
    }

    #[test]
    fn repeated_announce_is_not_answered_again_at_once() {
        let mut table = PeerTable::new();
        let bytes = datagram(BeaconKind::Announce, PEER_ID);
        assert!(feed(&mut table, &bytes, link_local_source()).reply_due);
        let second = feed(&mut table, &bytes, link_local_source());
        assert!(!second.reply_due);
        assert!(second.events.is_empty());
        assert_eq!(second.stats.beacons_received, 1);
    }

    #[test]
    fn reply_is_recorded_but_never_answered() {
        let mut table = PeerTable::new();
        let outcome = feed(
            &mut table,
            &datagram(BeaconKind::Reply, PEER_ID),
            link_local_source(),
        );
        assert!(!outcome.reply_due);
        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.stats.beacons_received, 1);
        assert_eq!(outcome.stats.replies_received, 1);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn own_beacon_is_ignored() {
        let mut table = PeerTable::new();
        let outcome = feed(
            &mut table,
            &datagram(BeaconKind::Announce, OWN_ID),
            link_local_source(),
        );
        assert!(!outcome.reply_due);
        assert!(outcome.events.is_empty());
        assert_eq!(outcome.stats.beacons_received, 0);
        assert!(table.is_empty());
    }

    #[test]
    fn beacon_from_outside_the_link_is_dropped() {
        let mut table = PeerTable::new();
        let source = SocketAddrV6::new(
            Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
            DISCOVERY_PORT,
            0,
            0,
        );
        let outcome = feed(&mut table, &datagram(BeaconKind::Announce, PEER_ID), source);
        assert!(!outcome.reply_due);
        assert!(outcome.events.is_empty());
        assert_eq!(outcome.stats.dropped_not_link_local, 1);
        assert!(table.is_empty());
    }

    #[test]
    fn malformed_datagram_is_counted_and_dropped() {
        let mut table = PeerTable::new();
        let outcome = feed(&mut table, &[0xff], link_local_source());
        assert!(!outcome.reply_due);
        assert_eq!(outcome.stats.dropped_malformed, 1);
        assert!(table.is_empty());
    }
}
