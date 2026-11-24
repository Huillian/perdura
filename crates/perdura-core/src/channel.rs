//! Authenticated, encrypted channel between two nodes.
//!
//! The handshake is `Noise_XX_25519_ChaChaPoly_SHA256`. Each side proves its
//! identity by signing its Noise static key with its Ed25519 identity key.
//!
//! `Handshake` and `Session` do no I/O. `Channel` runs them over any
//! reliable, ordered byte stream.
//!
//! After any error the channel must be dropped. It cannot be reused.

use std::fmt;
use std::io::{self, Read, Write};

use zeroize::Zeroize;

use crate::beacon::{MAJOR_ARRAY, MAJOR_BYTES, MAJOR_UINT, PROTOCOL_VERSION, Reader, put_head};
use crate::identity::{self, Identity, PUBLIC_KEY_LEN, PublicKey, SIGNATURE_LEN};

/// TCP port on which a node accepts channels. Provisional.
pub const CONTACT_PORT: u16 = 29170;
/// Maximum size of one application payload, in bytes.
pub const MAX_DATA_LEN: usize = 16 * 1024;

const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
const PROLOGUE: &[u8] = b"perdura/0";
const STATIC_KEY_CONTEXT: &[u8] = b"perdura-noise-static-v0";
const MAX_NOISE_MESSAGE: usize = 65535;
const TAG_LEN: usize = 16;
const MESSAGE_TYPE_PROOF: u64 = 2;
const MESSAGE_TYPE_DATA: u64 = 3;
const HANDSHAKE_STEPS: u8 = 3;

/// Reasons a handshake or a message is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelError {
    /// A message arrived out of order or does not follow the specification.
    Protocol,
    /// A cryptographic operation failed: the message was altered, replayed,
    /// reordered, or encrypted under another key.
    Crypto,
    /// The peer did not prove ownership of the identity it presented.
    Authentication,
    /// The payload exceeds `MAX_DATA_LEN`.
    TooLarge,
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Protocol => "message violates the protocol",
            Self::Crypto => "message failed cryptographic verification",
            Self::Authentication => "peer failed to prove its identity",
            Self::TooLarge => "payload is too large",
        })
    }
}

impl std::error::Error for ChannelError {}

/// Handshake state machine. Does no I/O.
///
/// The initiator calls `write_message`, `read_message`, `write_message`.
/// The responder calls `read_message`, `write_message`, `read_message`.
/// Both then call `finish`.
pub struct Handshake {
    state: snow::HandshakeState,
    initiator: bool,
    proof: Vec<u8>,
    peer: Option<PublicKey>,
    step: u8,
}

impl Handshake {
    /// Starts a handshake as the side that sends the first message.
    ///
    /// # Errors
    ///
    /// Fails when the cryptographic state cannot be created.
    pub fn initiator(identity: &Identity) -> Result<Self, ChannelError> {
        Self::new(identity, true)
    }

    /// Starts a handshake as the side that receives the first message.
    ///
    /// # Errors
    ///
    /// Fails when the cryptographic state cannot be created.
    pub fn responder(identity: &Identity) -> Result<Self, ChannelError> {
        Self::new(identity, false)
    }

    fn new(identity: &Identity, initiator: bool) -> Result<Self, ChannelError> {
        let mut keypair = snow::Builder::new(noise_params()?)
            .generate_keypair()
            .map_err(|_| ChannelError::Crypto)?;

        let mut signed = Vec::with_capacity(STATIC_KEY_CONTEXT.len() + keypair.public.len());
        signed.extend_from_slice(STATIC_KEY_CONTEXT);
        signed.extend_from_slice(&keypair.public);
        let proof = encode_proof(&identity.public_key(), &identity.sign(&signed));

        let state = build_state(&keypair.private, initiator);
        keypair.private.zeroize();

        Ok(Self {
            state: state?,
            initiator,
            proof,
            peer: None,
            step: 0,
        })
    }

    /// Produces the next handshake message to send.
    ///
    /// # Errors
    ///
    /// Fails when it is not this side's turn to write.
    pub fn write_message(&mut self) -> Result<Vec<u8>, ChannelError> {
        let payload: &[u8] = match (self.initiator, self.step) {
            (true, 0) => &[],
            (true, 2) | (false, 1) => &self.proof,
            _ => return Err(ChannelError::Protocol),
        };
        let mut buffer = vec![0u8; MAX_NOISE_MESSAGE];
        let len = self
            .state
            .write_message(payload, &mut buffer)
            .map_err(|_| ChannelError::Crypto)?;
        buffer.truncate(len);
        self.step += 1;
        Ok(buffer)
    }

    /// Consumes a handshake message received from the peer.
    ///
    /// # Errors
    ///
    /// Fails when it is not this side's turn to read, when the message was
    /// altered, or when the peer does not prove its identity.
    pub fn read_message(&mut self, message: &[u8]) -> Result<(), ChannelError> {
        let expects_proof = match (self.initiator, self.step) {
            (false, 0) => false,
            (true, 1) | (false, 2) => true,
            _ => return Err(ChannelError::Protocol),
        };
        if message.len() > MAX_NOISE_MESSAGE {
            return Err(ChannelError::Protocol);
        }

        let mut buffer = vec![0u8; MAX_NOISE_MESSAGE];
        let len = self
            .state
            .read_message(message, &mut buffer)
            .map_err(|_| ChannelError::Crypto)?;
        let payload = &buffer[..len];

        if expects_proof {
            let remote_static = self
                .state
                .get_remote_static()
                .ok_or(ChannelError::Protocol)?;
            self.peer = Some(verify_proof(payload, remote_static)?);
        } else if !payload.is_empty() {
            return Err(ChannelError::Protocol);
        }

        self.step += 1;
        Ok(())
    }

    /// Completes the handshake and returns the encrypted session.
    ///
    /// # Errors
    ///
    /// Fails when the handshake is not complete or the peer is not verified.
    pub fn finish(self) -> Result<Session, ChannelError> {
        if self.step != HANDSHAKE_STEPS {
            return Err(ChannelError::Protocol);
        }
        let peer = self.peer.ok_or(ChannelError::Protocol)?;
        let transport = self
            .state
            .into_transport_mode()
            .map_err(|_| ChannelError::Crypto)?;
        Ok(Session { transport, peer })
    }
}

/// An established session. Does no I/O.
pub struct Session {
    transport: snow::TransportState,
    peer: PublicKey,
}

impl Session {
    /// Identity public key the peer proved during the handshake.
    #[must_use]
    pub fn peer(&self) -> PublicKey {
        self.peer
    }

    /// Encrypts one application payload into a message for the peer.
    ///
    /// # Errors
    ///
    /// Fails when the payload is too large or encryption fails.
    pub fn seal(&mut self, payload: &[u8]) -> Result<Vec<u8>, ChannelError> {
        if payload.len() > MAX_DATA_LEN {
            return Err(ChannelError::TooLarge);
        }
        let plaintext = encode_data(payload);
        let mut buffer = vec![0u8; plaintext.len() + TAG_LEN];
        let len = self
            .transport
            .write_message(&plaintext, &mut buffer)
            .map_err(|_| ChannelError::Crypto)?;
        buffer.truncate(len);
        Ok(buffer)
    }

    /// Decrypts one message from the peer into its application payload.
    ///
    /// # Errors
    ///
    /// Fails when the message was altered, replayed, reordered or malformed.
    /// After an error the session must be dropped.
    pub fn open(&mut self, message: &[u8]) -> Result<Vec<u8>, ChannelError> {
        if message.len() > MAX_NOISE_MESSAGE {
            return Err(ChannelError::Protocol);
        }
        let mut buffer = vec![0u8; message.len()];
        let len = self
            .transport
            .read_message(message, &mut buffer)
            .map_err(|_| ChannelError::Crypto)?;
        decode_data(&buffer[..len]).ok_or(ChannelError::Protocol)
    }
}

/// A session running over a reliable, ordered byte stream.
pub struct Channel<S> {
    stream: S,
    session: Session,
}

impl<S: Read + Write> Channel<S> {
    /// Performs the handshake as initiator over `stream`.
    ///
    /// # Errors
    ///
    /// Fails on I/O errors or when the handshake is rejected.
    pub fn initiate(identity: &Identity, mut stream: S) -> io::Result<Self> {
        let mut handshake = Handshake::initiator(identity).map_err(to_io)?;
        write_frame(&mut stream, &handshake.write_message().map_err(to_io)?)?;
        handshake
            .read_message(&read_frame(&mut stream)?)
            .map_err(to_io)?;
        write_frame(&mut stream, &handshake.write_message().map_err(to_io)?)?;
        let session = handshake.finish().map_err(to_io)?;
        Ok(Self { stream, session })
    }

    /// Performs the handshake as responder over `stream`.
    ///
    /// # Errors
    ///
    /// Fails on I/O errors or when the handshake is rejected.
    pub fn respond(identity: &Identity, mut stream: S) -> io::Result<Self> {
        let mut handshake = Handshake::responder(identity).map_err(to_io)?;
        handshake
            .read_message(&read_frame(&mut stream)?)
            .map_err(to_io)?;
        write_frame(&mut stream, &handshake.write_message().map_err(to_io)?)?;
        handshake
            .read_message(&read_frame(&mut stream)?)
            .map_err(to_io)?;
        let session = handshake.finish().map_err(to_io)?;
        Ok(Self { stream, session })
    }

    /// Identity public key the peer proved during the handshake.
    #[must_use]
    pub fn peer(&self) -> PublicKey {
        self.session.peer()
    }

    /// Sends one application payload.
    ///
    /// # Errors
    ///
    /// Fails on I/O errors or when the payload is too large.
    pub fn send(&mut self, payload: &[u8]) -> io::Result<()> {
        let message = self.session.seal(payload).map_err(to_io)?;
        write_frame(&mut self.stream, &message)
    }

    /// Receives one application payload.
    ///
    /// # Errors
    ///
    /// Fails on I/O errors or when the message is rejected. After an error
    /// the channel must be dropped.
    pub fn receive(&mut self) -> io::Result<Vec<u8>> {
        let message = read_frame(&mut self.stream)?;
        self.session.open(&message).map_err(to_io)
    }
}

/// Writes one length-prefixed frame.
///
/// # Errors
///
/// Fails on I/O errors or when the message does not fit in a frame.
pub fn write_frame<W: Write>(writer: &mut W, message: &[u8]) -> io::Result<()> {
    let len = u16::try_from(message.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame is too large"))?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(message)?;
    writer.flush()
}

/// Reads one length-prefixed frame.
///
/// # Errors
///
/// Fails on I/O errors, including a stream that ends mid-frame.
pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    reader.read_exact(&mut len)?;
    let mut message = vec![0u8; usize::from(u16::from_be_bytes(len))];
    reader.read_exact(&mut message)?;
    Ok(message)
}

fn to_io(error: ChannelError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn noise_params() -> Result<snow::params::NoiseParams, ChannelError> {
    NOISE_PARAMS.parse().map_err(|_| ChannelError::Crypto)
}

fn build_state(private_key: &[u8], initiator: bool) -> Result<snow::HandshakeState, ChannelError> {
    let builder = snow::Builder::new(noise_params()?)
        .local_private_key(private_key)
        .map_err(|_| ChannelError::Crypto)?
        .prologue(PROLOGUE)
        .map_err(|_| ChannelError::Crypto)?;
    let state = if initiator {
        builder.build_initiator()
    } else {
        builder.build_responder()
    };
    state.map_err(|_| ChannelError::Crypto)
}

/// Checks that the identity in `payload` signed `remote_static`.
fn verify_proof(payload: &[u8], remote_static: &[u8]) -> Result<PublicKey, ChannelError> {
    let (identity_key, signature) = decode_proof(payload).ok_or(ChannelError::Protocol)?;
    let mut signed = Vec::with_capacity(STATIC_KEY_CONTEXT.len() + remote_static.len());
    signed.extend_from_slice(STATIC_KEY_CONTEXT);
    signed.extend_from_slice(remote_static);
    if !identity::verify(&identity_key, &signed, &signature) {
        return Err(ChannelError::Authentication);
    }
    Ok(identity_key)
}

fn encode_proof(identity_key: &PublicKey, signature: &[u8; SIGNATURE_LEN]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + PUBLIC_KEY_LEN + SIGNATURE_LEN);
    put_head(&mut out, MAJOR_ARRAY, 4);
    put_head(&mut out, MAJOR_UINT, PROTOCOL_VERSION);
    put_head(&mut out, MAJOR_UINT, MESSAGE_TYPE_PROOF);
    put_head(&mut out, MAJOR_BYTES, PUBLIC_KEY_LEN as u64);
    out.extend_from_slice(identity_key);
    put_head(&mut out, MAJOR_BYTES, SIGNATURE_LEN as u64);
    out.extend_from_slice(signature);
    out
}

fn decode_proof(input: &[u8]) -> Option<(PublicKey, [u8; SIGNATURE_LEN])> {
    let mut reader = Reader { input, pos: 0 };
    if reader.head(MAJOR_ARRAY).ok()? != 4 {
        return None;
    }
    if reader.head(MAJOR_UINT).ok()? != PROTOCOL_VERSION {
        return None;
    }
    if reader.head(MAJOR_UINT).ok()? != MESSAGE_TYPE_PROOF {
        return None;
    }
    if reader.head(MAJOR_BYTES).ok()? != PUBLIC_KEY_LEN as u64 {
        return None;
    }
    let mut identity_key = [0u8; PUBLIC_KEY_LEN];
    identity_key.copy_from_slice(reader.take(PUBLIC_KEY_LEN).ok()?);
    if reader.head(MAJOR_BYTES).ok()? != SIGNATURE_LEN as u64 {
        return None;
    }
    let mut signature = [0u8; SIGNATURE_LEN];
    signature.copy_from_slice(reader.take(SIGNATURE_LEN).ok()?);
    if reader.pos != input.len() {
        return None;
    }
    Some((identity_key, signature))
}

fn encode_data(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    put_head(&mut out, MAJOR_ARRAY, 3);
    put_head(&mut out, MAJOR_UINT, PROTOCOL_VERSION);
    put_head(&mut out, MAJOR_UINT, MESSAGE_TYPE_DATA);
    put_head(&mut out, MAJOR_BYTES, payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

fn decode_data(input: &[u8]) -> Option<Vec<u8>> {
    let mut reader = Reader { input, pos: 0 };
    if reader.head(MAJOR_ARRAY).ok()? != 3 {
        return None;
    }
    if reader.head(MAJOR_UINT).ok()? != PROTOCOL_VERSION {
        return None;
    }
    if reader.head(MAJOR_UINT).ok()? != MESSAGE_TYPE_DATA {
        return None;
    }
    let len = usize::try_from(reader.head(MAJOR_BYTES).ok()?).ok()?;
    if len > MAX_DATA_LEN {
        return None;
    }
    let payload = reader.take(len).ok()?.to_vec();
    if reader.pos != input.len() {
        return None;
    }
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pair {
        alice: Identity,
        bob: Identity,
        initiator: Session,
        responder: Session,
    }

    fn complete(
        mut initiator: Handshake,
        mut responder: Handshake,
    ) -> Result<(Session, Session), ChannelError> {
        let first = initiator.write_message()?;
        responder.read_message(&first)?;
        let second = responder.write_message()?;
        initiator.read_message(&second)?;
        let third = initiator.write_message()?;
        responder.read_message(&third)?;
        Ok((initiator.finish()?, responder.finish()?))
    }

    fn pair() -> Pair {
        let alice = Identity::generate().unwrap();
        let bob = Identity::generate().unwrap();
        let (initiator, responder) = complete(
            Handshake::initiator(&alice).unwrap(),
            Handshake::responder(&bob).unwrap(),
        )
        .unwrap();
        Pair {
            alice,
            bob,
            initiator,
            responder,
        }
    }

    #[test]
    fn each_side_learns_the_identity_of_the_other() {
        let pair = pair();
        assert_eq!(pair.initiator.peer(), pair.bob.public_key());
        assert_eq!(pair.responder.peer(), pair.alice.public_key());
    }

    #[test]
    fn messages_travel_in_both_directions() {
        let mut pair = pair();
        let to_bob = pair.initiator.seal(b"hello bob").unwrap();
        assert_eq!(pair.responder.open(&to_bob), Ok(b"hello bob".to_vec()));
        let to_alice = pair.responder.seal(b"hello alice").unwrap();
        assert_eq!(pair.initiator.open(&to_alice), Ok(b"hello alice".to_vec()));
    }

    #[test]
    fn sealed_message_does_not_contain_the_plaintext() {
        let mut pair = pair();
        let secret = b"a very secret message";
        let sealed = pair.initiator.seal(secret).unwrap();
        assert!(
            !sealed
                .windows(secret.len())
                .any(|window| window == &secret[..])
        );
    }

    #[test]
    fn rejects_a_message_tampered_at_any_position() {
        let mut probe = pair();
        let length = probe.initiator.seal(b"hello").unwrap().len();
        for position in 0..length {
            let mut fresh = pair();
            let mut sealed = fresh.initiator.seal(b"hello").unwrap();
            sealed[position] ^= 0x01;
            assert_eq!(
                fresh.responder.open(&sealed),
                Err(ChannelError::Crypto),
                "position {position}"
            );
        }
    }

    #[test]
    fn rejects_a_replayed_message() {
        let mut pair = pair();
        let sealed = pair.initiator.seal(b"once").unwrap();
        assert_eq!(pair.responder.open(&sealed), Ok(b"once".to_vec()));
        assert_eq!(pair.responder.open(&sealed), Err(ChannelError::Crypto));
    }

    #[test]
    fn rejects_messages_out_of_order() {
        let mut pair = pair();
        let _first = pair.initiator.seal(b"first").unwrap();
        let second = pair.initiator.seal(b"second").unwrap();
        assert_eq!(pair.responder.open(&second), Err(ChannelError::Crypto));
    }

    #[test]
    fn rejects_a_message_from_another_session() {
        let mut one = pair();
        let mut other = pair();
        let sealed = one.initiator.seal(b"hello").unwrap();
        assert_eq!(other.responder.open(&sealed), Err(ChannelError::Crypto));
    }

    #[test]
    fn rejects_a_payload_that_is_too_large() {
        let mut pair = pair();
        let payload = vec![0u8; MAX_DATA_LEN + 1];
        assert_eq!(pair.initiator.seal(&payload), Err(ChannelError::TooLarge));
    }

    #[test]
    fn accepts_a_payload_of_maximum_size() {
        let mut pair = pair();
        let payload = vec![0x5a; MAX_DATA_LEN];
        let sealed = pair.initiator.seal(&payload).unwrap();
        assert_eq!(pair.responder.open(&sealed), Ok(payload));
    }

    #[test]
    fn rejects_an_identity_that_did_not_sign_the_static_key() {
        let alice = Identity::generate().unwrap();
        let bob = Identity::generate().unwrap();
        let mallory = Identity::generate().unwrap();

        // Mallory runs the handshake with her own keys but claims to be Alice.
        let mut initiator = Handshake::initiator(&mallory).unwrap();
        let (_, signature) = decode_proof(&initiator.proof).unwrap();
        initiator.proof = encode_proof(&alice.public_key(), &signature);

        let result = complete(initiator, Handshake::responder(&bob).unwrap());
        assert_eq!(result.err(), Some(ChannelError::Authentication));
    }

    #[test]
    fn rejects_a_proof_made_for_another_static_key() {
        let alice = Identity::generate().unwrap();
        let bob = Identity::generate().unwrap();

        // A valid proof from Alice, captured from a different handshake.
        let captured = Handshake::initiator(&alice).unwrap().proof;
        let mut initiator = Handshake::initiator(&alice).unwrap();
        initiator.proof = captured;

        let result = complete(initiator, Handshake::responder(&bob).unwrap());
        assert_eq!(result.err(), Some(ChannelError::Authentication));
    }

    #[test]
    fn rejects_a_tampered_handshake_message() {
        let alice = Identity::generate().unwrap();
        let bob = Identity::generate().unwrap();
        let mut initiator = Handshake::initiator(&alice).unwrap();
        let mut responder = Handshake::responder(&bob).unwrap();

        let first = initiator.write_message().unwrap();
        responder.read_message(&first).unwrap();
        let mut second = responder.write_message().unwrap();
        let last = second.len() - 1;
        second[last] ^= 0x01;
        assert_eq!(initiator.read_message(&second), Err(ChannelError::Crypto));
    }

    #[test]
    fn rejects_handshake_steps_out_of_turn() {
        let alice = Identity::generate().unwrap();

        let mut initiator = Handshake::initiator(&alice).unwrap();
        assert_eq!(initiator.read_message(&[]), Err(ChannelError::Protocol));

        let mut responder = Handshake::responder(&alice).unwrap();
        assert_eq!(responder.write_message(), Err(ChannelError::Protocol));

        let unfinished = Handshake::initiator(&alice).unwrap();
        assert_eq!(unfinished.finish().err(), Some(ChannelError::Protocol));
    }

    #[test]
    fn data_encoding_matches_the_specification() {
        assert_eq!(encode_data(b"hi"), [0x83, 0x00, 0x03, 0x42, 0x68, 0x69]);
        assert_eq!(decode_data(&encode_data(b"hi")), Some(b"hi".to_vec()));
        assert_eq!(
            decode_data(&[0x83, 0x00, 0x03, 0x42, 0x68, 0x69, 0x00]),
            None
        );
        assert_eq!(decode_data(&[0x83, 0x00, 0x02, 0x42, 0x68, 0x69]), None);
    }

    #[test]
    fn proof_encoding_round_trips_and_is_strict() {
        let key = [0x11; PUBLIC_KEY_LEN];
        let signature = [0x22; SIGNATURE_LEN];
        let encoded = encode_proof(&key, &signature);
        assert_eq!(&encoded[..5], &[0x84, 0x00, 0x02, 0x58, 0x20]);
        assert_eq!(decode_proof(&encoded), Some((key, signature)));

        let mut trailing = encoded.clone();
        trailing.push(0x00);
        assert_eq!(decode_proof(&trailing), None);
        assert_eq!(decode_proof(&encoded[..encoded.len() - 1]), None);
    }

    #[test]
    fn frames_round_trip() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"abc").unwrap();
        write_frame(&mut wire, b"").unwrap();
        assert_eq!(wire, [0x00, 0x03, b'a', b'b', b'c', 0x00, 0x00]);

        let mut reader = io::Cursor::new(wire);
        assert_eq!(read_frame(&mut reader).unwrap(), b"abc");
        assert_eq!(read_frame(&mut reader).unwrap(), b"");
        assert!(read_frame(&mut reader).is_err());
    }

    #[test]
    fn channel_runs_over_a_byte_stream() {
        let alice = Identity::generate().unwrap();
        let bob = Identity::generate().unwrap();
        let alice_key = alice.public_key();
        let bob_key = bob.public_key();
        let (near, far) = std::os::unix::net::UnixStream::pair().unwrap();

        let responder = std::thread::spawn(move || {
            let mut channel = Channel::respond(&bob, far).unwrap();
            let message = channel.receive().unwrap();
            channel.send(&message).unwrap();
            (channel.peer(), message)
        });

        let mut channel = Channel::initiate(&alice, near).unwrap();
        channel.send(b"ping").unwrap();
        assert_eq!(channel.receive().unwrap(), b"ping");
        assert_eq!(channel.peer(), bob_key);

        let (peer_seen_by_bob, message) = responder.join().unwrap();
        assert_eq!(peer_seen_by_bob, alice_key);
        assert_eq!(message, b"ping");
    }
}
