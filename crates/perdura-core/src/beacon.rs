//! Discovery beacon wire format (protocol version 0).
//!
//! A beacon is a CBOR array in deterministic encoding (RFC 8949, section 4.2):
//! `[version, message_type, node_id, port]`.
//!
//! An announce (type 1) is sent to the multicast group. A reply (type 4) is
//! sent directly to a node whose announce was received.

/// Protocol version implemented by this crate.
pub const PROTOCOL_VERSION: u64 = 0;
/// Message type of a discovery announce.
pub const MESSAGE_TYPE_BEACON: u64 = 1;
/// Message type of a direct discovery reply.
pub const MESSAGE_TYPE_BEACON_REPLY: u64 = 4;
/// Maximum size of a discovery datagram, in bytes.
pub const MAX_DATAGRAM_SIZE: usize = 1200;
/// Length of a node identifier, in bytes.
pub const NODE_ID_LEN: usize = 16;

/// Ephemeral node identifier, regenerated on every start.
pub type NodeId = [u8; NODE_ID_LEN];

pub(crate) const MAJOR_UINT: u8 = 0;
pub(crate) const MAJOR_BYTES: u8 = 2;
pub(crate) const MAJOR_ARRAY: u8 = 4;
const BEACON_ITEMS: u64 = 4;

/// How a beacon was meant to be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeaconKind {
    /// Sent to the multicast group. Receivers answer with a `Reply`.
    Announce,
    /// Sent directly to one node. Never answered.
    Reply,
}

/// A decoded discovery beacon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beacon {
    /// Whether this is an announce or a direct reply.
    pub kind: BeaconKind,
    /// Ephemeral identifier of the sending node.
    pub node_id: NodeId,
    /// Contact port advertised by the sending node.
    pub port: u16,
}

/// Reasons a datagram is not accepted as a beacon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The datagram is larger than `MAX_DATAGRAM_SIZE`.
    Oversized,
    /// The datagram is not a well-formed, deterministic version 0 beacon.
    Malformed,
    /// The datagram announces a protocol version this crate does not know.
    UnsupportedVersion(u64),
    /// The datagram carries a message type that is not a beacon.
    UnknownType(u64),
}

impl Beacon {
    /// Encodes the beacon into its wire representation.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let message_type = match self.kind {
            BeaconKind::Announce => MESSAGE_TYPE_BEACON,
            BeaconKind::Reply => MESSAGE_TYPE_BEACON_REPLY,
        };
        let mut out = Vec::with_capacity(32);
        put_head(&mut out, MAJOR_ARRAY, BEACON_ITEMS);
        put_head(&mut out, MAJOR_UINT, PROTOCOL_VERSION);
        put_head(&mut out, MAJOR_UINT, message_type);
        put_head(&mut out, MAJOR_BYTES, NODE_ID_LEN as u64);
        out.extend_from_slice(&self.node_id);
        put_head(&mut out, MAJOR_UINT, u64::from(self.port));
        out
    }

    /// Decodes a datagram, accepting only the exact deterministic encoding.
    ///
    /// # Errors
    ///
    /// Returns a [`DecodeError`] describing why the datagram was rejected.
    pub fn decode(input: &[u8]) -> Result<Self, DecodeError> {
        if input.len() > MAX_DATAGRAM_SIZE {
            return Err(DecodeError::Oversized);
        }
        let mut reader = Reader { input, pos: 0 };

        let items = reader.head(MAJOR_ARRAY)?;
        if items < 2 {
            return Err(DecodeError::Malformed);
        }
        let version = reader.head(MAJOR_UINT)?;
        if version != PROTOCOL_VERSION {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        let kind = match reader.head(MAJOR_UINT)? {
            MESSAGE_TYPE_BEACON => BeaconKind::Announce,
            MESSAGE_TYPE_BEACON_REPLY => BeaconKind::Reply,
            other => return Err(DecodeError::UnknownType(other)),
        };
        if items != BEACON_ITEMS {
            return Err(DecodeError::Malformed);
        }

        let id_len = reader.head(MAJOR_BYTES)?;
        if id_len != NODE_ID_LEN as u64 {
            return Err(DecodeError::Malformed);
        }
        let mut node_id = [0u8; NODE_ID_LEN];
        node_id.copy_from_slice(reader.take(NODE_ID_LEN)?);

        let port = u16::try_from(reader.head(MAJOR_UINT)?).map_err(|_| DecodeError::Malformed)?;

        if reader.pos != input.len() {
            return Err(DecodeError::Malformed);
        }
        Ok(Self {
            kind,
            node_id,
            port,
        })
    }
}

/// Writes a CBOR item head using the shortest form for `value`.
pub(crate) fn put_head(out: &mut Vec<u8>, major: u8, value: u64) {
    let high = major << 5;
    if value < 24 {
        out.push(high | (value as u8));
    } else if let Ok(v) = u8::try_from(value) {
        out.push(high | 24);
        out.push(v);
    } else if let Ok(v) = u16::try_from(value) {
        out.push(high | 25);
        out.extend_from_slice(&v.to_be_bytes());
    } else if let Ok(v) = u32::try_from(value) {
        out.push(high | 26);
        out.extend_from_slice(&v.to_be_bytes());
    } else {
        out.push(high | 27);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

pub(crate) struct Reader<'a> {
    pub(crate) input: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(len).ok_or(DecodeError::Malformed)?;
        let slice = self
            .input
            .get(self.pos..end)
            .ok_or(DecodeError::Malformed)?;
        self.pos = end;
        Ok(slice)
    }

    /// Reads one item head of the expected major type and returns its argument.
    /// Rejects indefinite lengths and integers not in shortest form.
    pub(crate) fn head(&mut self, expected_major: u8) -> Result<u64, DecodeError> {
        let first = self.take(1)?[0];
        if first >> 5 != expected_major {
            return Err(DecodeError::Malformed);
        }
        let info = first & 0x1f;
        let (value, minimum): (u64, u64) = match info {
            0..=23 => (u64::from(info), 0),
            24 => (u64::from(self.take(1)?[0]), 24),
            25 => {
                let b = self.take(2)?;
                (u64::from(u16::from_be_bytes([b[0], b[1]])), 0x100)
            }
            26 => {
                let b = self.take(4)?;
                (
                    u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
                    0x1_0000,
                )
            }
            27 => {
                let mut raw = [0u8; 8];
                raw.copy_from_slice(self.take(8)?);
                (u64::from_be_bytes(raw), 0x1_0000_0000)
            }
            _ => return Err(DecodeError::Malformed),
        };
        if value < minimum {
            return Err(DecodeError::Malformed);
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: Beacon = Beacon {
        kind: BeaconKind::Announce,
        node_id: [0x11; NODE_ID_LEN],
        port: 29170,
    };

    const SAMPLE_REPLY: Beacon = Beacon {
        kind: BeaconKind::Reply,
        node_id: [0x11; NODE_ID_LEN],
        port: 29170,
    };

    /// Bytes of `SAMPLE`, written by hand from the specification.
    fn sample_bytes() -> Vec<u8> {
        let mut bytes = vec![0x84, 0x00, 0x01, 0x50];
        bytes.extend_from_slice(&[0x11; NODE_ID_LEN]);
        bytes.extend_from_slice(&[0x19, 0x71, 0xf2]);
        bytes
    }

    /// Bytes of `SAMPLE_REPLY`: the same beacon with message type 4.
    fn sample_reply_bytes() -> Vec<u8> {
        let mut bytes = sample_bytes();
        bytes[2] = 0x04;
        bytes
    }

    /// `sample_bytes` with the port item replaced by `tail`.
    fn with_port_tail(tail: &[u8]) -> Vec<u8> {
        let mut bytes = sample_bytes();
        bytes.truncate(20);
        bytes.extend_from_slice(tail);
        bytes
    }

    #[test]
    fn encodes_to_known_bytes() {
        assert_eq!(SAMPLE.encode(), sample_bytes());
    }

    #[test]
    fn decodes_known_bytes() {
        assert_eq!(Beacon::decode(&sample_bytes()), Ok(SAMPLE));
    }

    #[test]
    fn reply_encodes_to_known_bytes() {
        assert_eq!(SAMPLE_REPLY.encode(), sample_reply_bytes());
    }

    #[test]
    fn reply_decodes_known_bytes() {
        assert_eq!(Beacon::decode(&sample_reply_bytes()), Ok(SAMPLE_REPLY));
    }

    #[test]
    fn rejects_every_truncation() {
        let bytes = sample_bytes();
        for len in 0..bytes.len() {
            assert_eq!(
                Beacon::decode(&bytes[..len]),
                Err(DecodeError::Malformed),
                "length {len}"
            );
        }
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut bytes = sample_bytes();
        bytes.push(0x00);
        assert_eq!(Beacon::decode(&bytes), Err(DecodeError::Malformed));
    }

    #[test]
    fn rejects_integer_not_in_shortest_form() {
        let bytes = with_port_tail(&[0x1a, 0x00, 0x00, 0x71, 0xf2]);
        assert_eq!(Beacon::decode(&bytes), Err(DecodeError::Malformed));
    }

    #[test]
    fn rejects_port_out_of_range() {
        let bytes = with_port_tail(&[0x1a, 0x00, 0x01, 0x00, 0x00]);
        assert_eq!(Beacon::decode(&bytes), Err(DecodeError::Malformed));
    }

    #[test]
    fn rejects_indefinite_length_array() {
        let mut bytes = sample_bytes();
        bytes[0] = 0x9f;
        assert_eq!(Beacon::decode(&bytes), Err(DecodeError::Malformed));
    }

    #[test]
    fn rejects_wrong_item_count() {
        let mut bytes = sample_bytes();
        bytes[0] = 0x85;
        assert_eq!(Beacon::decode(&bytes), Err(DecodeError::Malformed));
    }

    #[test]
    fn rejects_short_node_id() {
        let mut bytes = vec![0x84, 0x00, 0x01, 0x4f];
        bytes.extend_from_slice(&[0x11; 15]);
        bytes.extend_from_slice(&[0x19, 0x71, 0xf2]);
        assert_eq!(Beacon::decode(&bytes), Err(DecodeError::Malformed));
    }

    #[test]
    fn reports_unsupported_version() {
        let mut bytes = sample_bytes();
        bytes[1] = 0x01;
        assert_eq!(
            Beacon::decode(&bytes),
            Err(DecodeError::UnsupportedVersion(1))
        );
    }

    #[test]
    fn reports_types_that_are_not_beacons() {
        for other in [0x00u8, 0x02, 0x03, 0x05] {
            let mut bytes = sample_bytes();
            bytes[2] = other;
            assert_eq!(
                Beacon::decode(&bytes),
                Err(DecodeError::UnknownType(u64::from(other)))
            );
        }
    }

    #[test]
    fn rejects_oversized_input() {
        let bytes = vec![0u8; MAX_DATAGRAM_SIZE + 1];
        assert_eq!(Beacon::decode(&bytes), Err(DecodeError::Oversized));
    }
}
