//! Persistent node identity: an Ed25519 key pair generated on the device.
//!
//! The private key is never logged or printed. It leaves this module only
//! to be written to the identity file.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use zeroize::Zeroize;

/// Length of an identity public key, in bytes.
pub const PUBLIC_KEY_LEN: usize = 32;
/// Length of an identity signature, in bytes.
pub const SIGNATURE_LEN: usize = 64;

const SEED_LEN: usize = 32;
const FILE_HEADER: &str = "perdura-identity-v0";
const FILE_MODE: u32 = 0o600;
/// Header, newline, seed in hexadecimal, newline.
const FILE_LEN: usize = FILE_HEADER.len() + 1 + SEED_LEN * 2 + 1;
const BASE32_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Identity public key.
pub type PublicKey = [u8; PUBLIC_KEY_LEN];

/// The long-term identity of this node.
pub struct Identity {
    signing_key: SigningKey,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Generates a new random identity.
    ///
    /// # Errors
    ///
    /// Fails when the system random source is unavailable.
    pub fn generate() -> io::Result<Self> {
        let mut seed = [0u8; SEED_LEN];
        getrandom::fill(&mut seed)
            .map_err(|_| io::Error::other("system random source unavailable"))?;
        let identity = Self::from_seed(&seed);
        seed.zeroize();
        Ok(identity)
    }

    fn from_seed(seed: &[u8; SEED_LEN]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(seed),
        }
    }

    /// Returns the public key, which is the identity itself.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Returns the human-readable fingerprint of the public key.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public_key())
    }

    /// Loads the identity stored at `path`, creating it first when the file
    /// does not exist. Returns the identity and whether it was created.
    ///
    /// An existing file that is invalid is never overwritten.
    ///
    /// # Errors
    ///
    /// Fails when the file is malformed, readable by other users, or cannot
    /// be read or created.
    pub fn load_or_create(path: &Path) -> io::Result<(Self, bool)> {
        match Self::load(path) {
            Ok(identity) => Ok((identity, false)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let identity = Self::generate()?;
                identity.save_new(path)?;
                Ok((identity, true))
            }
            Err(error) => Err(error),
        }
    }

    /// Loads the identity stored at `path`.
    ///
    /// # Errors
    ///
    /// Fails when the file does not exist, is malformed, or is readable by
    /// other users.
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut file = fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(invalid("identity path is not a regular file"));
        }
        if (metadata.permissions().mode() & 0o077) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "identity file is accessible by other users; expected mode 0600",
            ));
        }
        if metadata.len() != FILE_LEN as u64 {
            return Err(invalid("identity file has an unexpected size"));
        }

        let mut text = String::with_capacity(FILE_LEN);
        let read = file.read_to_string(&mut text);
        let parsed = if read.is_ok() {
            parse_file(&text)
        } else {
            None
        };
        text.zeroize();

        let mut seed = parsed.ok_or_else(|| invalid("identity file is malformed"))?;
        let identity = Self::from_seed(&seed);
        seed.zeroize();
        Ok(identity)
    }

    /// Writes the identity to a new file. Fails when the file already exists.
    fn save_new(&self, path: &Path) -> io::Result<()> {
        let mut seed = self.signing_key.to_bytes();
        let mut hex = to_hex(&seed);
        seed.zeroize();
        let mut text = format!("{FILE_HEADER}\n{hex}\n");
        hex.zeroize();

        let result = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(FILE_MODE)
            .open(path)
            .and_then(|mut file| {
                file.write_all(text.as_bytes())?;
                file.sync_all()
            });
        text.zeroize();
        result
    }
}

/// Formats a public key as a fingerprint: base32 (RFC 4648, lowercase, no
/// padding) in groups of four characters separated by hyphens.
#[must_use]
pub fn fingerprint(public_key: &PublicKey) -> String {
    let encoded = base32(public_key);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / 4);
    for (index, character) in encoded.chars().enumerate() {
        if index != 0 && index.is_multiple_of(4) {
            out.push('-');
        }
        out.push(character);
    }
    out
}

fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(
                BASE32_ALPHABET[((buffer >> bits) & 0x1f) as usize],
            ));
        }
    }
    if bits > 0 {
        out.push(char::from(
            BASE32_ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize],
        ));
    }
    out
}

fn parse_file(text: &str) -> Option<[u8; SEED_LEN]> {
    let rest = text.strip_prefix(FILE_HEADER)?.strip_prefix('\n')?;
    from_hex(rest.strip_suffix('\n')?)
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Decodes exactly 64 lowercase hexadecimal characters.
fn from_hex(text: &str) -> Option<[u8; SEED_LEN]> {
    let raw = text.as_bytes();
    if raw.len() != SEED_LEN * 2 {
        return None;
    }
    let mut out = [0u8; SEED_LEN];
    for (index, pair) in raw.as_chunks::<2>().0.iter().enumerate() {
        out[index] = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
    }
    Some(out)
}

fn hex_value(character: u8) -> Option<u8> {
    match character {
        b'0'..=b'9' => Some(character - b'0'),
        b'a'..=b'f' => Some(character - b'a' + 10),
        _ => None,
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl Identity {
    /// Signs `message` with the identity key.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.signing_key.sign(message).to_bytes()
    }
}

/// Verifies an identity signature. Returns `false` for any invalid input.
#[must_use]
pub fn verify(public_key: &PublicKey, message: &[u8], signature: &[u8; SIGNATURE_LEN]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let Ok(signature) = Signature::try_from(&signature[..]) else {
        return false;
    };
    key.verify_strict(message, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// RFC 8032, section 7.1, test 1.
    const RFC8032_SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const RFC8032_PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("perdura-identity-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self) -> PathBuf {
            self.0.join("identity")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_private(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn derives_the_rfc8032_public_key() {
        let seed = from_hex(RFC8032_SEED).unwrap();
        let identity = Identity::from_seed(&seed);
        assert_eq!(to_hex(&identity.public_key()), RFC8032_PUBLIC);
    }

    #[test]
    fn base32_matches_rfc4648() {
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn fingerprint_of_known_keys() {
        let zeros = ["aaaa"; 13].join("-");
        assert_eq!(fingerprint(&[0u8; PUBLIC_KEY_LEN]), zeros);

        let mut ones = ["7777"; 12].join("-");
        ones.push_str("-777q");
        assert_eq!(fingerprint(&[0xff; PUBLIC_KEY_LEN]), ones);
    }

    #[test]
    fn debug_output_does_not_contain_the_seed() {
        let seed = from_hex(RFC8032_SEED).unwrap();
        let identity = Identity::from_seed(&seed);
        let debug = format!("{identity:?}");
        assert!(!debug.contains("9d61b19d"));
        assert!(debug.contains(&identity.fingerprint()));
    }

    #[test]
    fn creates_then_loads_the_same_identity() {
        let dir = TempDir::new("roundtrip");
        let (first, created_first) = Identity::load_or_create(&dir.file()).unwrap();
        let (second, created_second) = Identity::load_or_create(&dir.file()).unwrap();
        assert!(created_first);
        assert!(!created_second);
        assert_eq!(first.public_key(), second.public_key());
    }

    #[test]
    fn two_generated_identities_differ() {
        let first = Identity::generate().unwrap();
        let second = Identity::generate().unwrap();
        assert_ne!(first.public_key(), second.public_key());
    }

    #[test]
    fn new_file_is_private() {
        let dir = TempDir::new("mode");
        Identity::load_or_create(&dir.file()).unwrap();
        let mode = fs::metadata(dir.file()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn refuses_file_accessible_by_others() {
        let dir = TempDir::new("open");
        Identity::load_or_create(&dir.file()).unwrap();
        fs::set_permissions(dir.file(), fs::Permissions::from_mode(0o644)).unwrap();
        let error = Identity::load_or_create(&dir.file()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn refuses_malformed_file_without_overwriting_it() {
        let dir = TempDir::new("malformed");
        let content = format!("{FILE_HEADER}\n{}\n", "z".repeat(SEED_LEN * 2));
        write_private(&dir.file(), &content);
        let error = Identity::load_or_create(&dir.file()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(dir.file()).unwrap(), content);
    }

    #[test]
    fn refuses_file_of_wrong_size_without_overwriting_it() {
        let dir = TempDir::new("size");
        write_private(&dir.file(), "garbage");
        let error = Identity::load_or_create(&dir.file()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(dir.file()).unwrap(), "garbage");
    }

    #[test]
    fn rejects_uppercase_hex() {
        assert!(from_hex(&RFC8032_SEED.to_uppercase()).is_none());
    }

    /// RFC 8032, section 7.1, test 1: signature of the empty message.
    const RFC8032_SIGNATURE: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";

    #[test]
    fn signs_like_rfc8032() {
        let seed = from_hex(RFC8032_SEED).unwrap();
        let identity = Identity::from_seed(&seed);
        assert_eq!(to_hex(&identity.sign(b"")), RFC8032_SIGNATURE);
    }

    #[test]
    fn verifies_only_the_signed_message_under_the_signing_key() {
        let identity = Identity::generate().unwrap();
        let other = Identity::generate().unwrap();
        let signature = identity.sign(b"message");

        assert!(verify(&identity.public_key(), b"message", &signature));
        assert!(!verify(&identity.public_key(), b"massage", &signature));
        assert!(!verify(&other.public_key(), b"message", &signature));

        let mut tampered = signature;
        tampered[0] ^= 0x01;
        assert!(!verify(&identity.public_key(), b"message", &tampered));
    }
}
