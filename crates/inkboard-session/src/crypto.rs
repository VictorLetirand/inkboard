//! Link encryption (end to end, on top of the transport's own): invitation link, key derivation, AEAD frames.
//!
//! - The link is `<base>#<ticket>`; the ticket (host identity, optional relay hints, random session key, role) is in the fragment.
//! - There are two session keys per session: one for the edit link, one for the read-only link. The host finds out which one a
//!   guest holds by the key that opens its first frame, so the granted role never depends on what the guest claims.
//! - Frame: `version(1) | nonce(12) | ChaCha20-Poly1305(ciphertext + tag)`. Each direction has its own sub-key derived with
//!   HKDF-SHA256, so a frame cannot be reflected back. The nonce is random (the key is shared by all the guests of a role, so a
//!   counter would collide). Replays are handled by the protocol (`client_seq`).

use crate::codec::Codec;
use crate::error::{Error, Result};
use crate::limits::Violation;
use crate::proto::{Message, Role};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use std::fmt;

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const FRAME_VERSION: u8 = 1;
const SALT: &[u8] = b"inkboard-session/v1";

/* ------------------------------------------------------------- Entropy */

/// Source of random bytes (injectable so that tests are deterministic).
pub trait Entropy: Send {
    fn fill(&mut self, buf: &mut [u8]) -> Result<()>;
}

/// Operating system randomness (`crypto.getRandomValues` in a browser).
#[derive(Clone, Copy, Debug, Default)]
pub struct OsEntropy;

impl Entropy for OsEntropy {
    fn fill(&mut self, buf: &mut [u8]) -> Result<()> {
        getrandom::fill(buf).map_err(|_| Error::Entropy)
    }
}

/// Deterministic generator (`SplitMix64`) for tests and simulations. NOT secure: never use it for a real session.
#[derive(Clone, Debug)]
pub struct SeededEntropy(u64);

impl SeededEntropy {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Entropy for SeededEntropy {
    fn fill(&mut self, buf: &mut [u8]) -> Result<()> {
        for chunk in buf.chunks_mut(8) {
            let r = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&r[..chunk.len()]);
        }
        Ok(())
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/* ---------------------------------------------------------------- Keys */

/// Random secret shared through the link. Never printed.
#[derive(Clone)]
pub struct SessionKey([u8; KEY_LEN]);

impl SessionKey {
    #[must_use]
    pub fn from_bytes(b: [u8; KEY_LEN]) -> Self {
        Self(b)
    }
    pub fn generate(e: &mut dyn Entropy) -> Result<Self> {
        let mut b = [0u8; KEY_LEN];
        e.fill(&mut b)?;
        Ok(Self(b))
    }
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
    fn derive(&self, info: &[u8], out: &mut [u8]) {
        // expand() only fails when the output is longer than 255 hash blocks
        Hkdf::<Sha256>::new(Some(SALT), &self.0).expand(info, out).expect("output length is small");
    }
}

impl fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionKey(..)")
    }
}

impl PartialEq for SessionKey {
    fn eq(&self, other: &Self) -> bool {
        ct_eq(&self.0, &other.0)
    }
}
impl Eq for SessionKey {}

/// Proof, sent in `Hello`, that the guest knows the key of the link it uses.
#[must_use]
pub fn join_proof(key: &SessionKey, name: &str, role: Role) -> Vec<u8> {
    let mut info = b"join/v1".to_vec();
    info.push(role_byte(role));
    info.extend_from_slice(name.as_bytes());
    let mut out = [0u8; 32];
    key.derive(&info, &mut out);
    out.to_vec()
}

#[must_use]
pub fn verify_join_proof(key: &SessionKey, name: &str, role: Role, proof: &[u8]) -> bool {
    ct_eq(&join_proof(key, name, role), proof)
}

fn role_byte(r: Role) -> u8 {
    match r {
        Role::Edit => 0,
        Role::View => 1,
    }
}

/* -------------------------------------------------------------- Frames */

/// Which end of the connection a cipher is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Guest,
    Host,
}

pub struct FrameCipher {
    tx: ChaCha20Poly1305,
    rx: ChaCha20Poly1305,
}

impl fmt::Debug for FrameCipher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FrameCipher(..)")
    }
}

impl FrameCipher {
    #[must_use]
    pub fn new(key: &SessionKey, side: Side) -> Self {
        let sub = |info: &[u8]| {
            let mut k = [0u8; KEY_LEN];
            key.derive(info, &mut k);
            ChaCha20Poly1305::new(&Key::from(k))
        };
        let (to_host, to_guest) = (sub(b"frame/to-host"), sub(b"frame/to-guest"));
        match side {
            Side::Guest => Self { tx: to_host, rx: to_guest },
            Side::Host => Self { tx: to_guest, rx: to_host },
        }
    }

    pub fn seal(&self, plaintext: &[u8], entropy: &mut dyn Entropy) -> Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE_LEN];
        entropy.fill(&mut nonce)?;
        let ct = self.tx.encrypt(&Nonce::from(nonce), Payload { msg: plaintext, aad: &[FRAME_VERSION] }).map_err(|_| Error::BadFrame("encryption failed"))?;
        let mut frame = Vec::with_capacity(1 + NONCE_LEN + ct.len());
        frame.push(FRAME_VERSION);
        frame.extend_from_slice(&nonce);
        frame.extend_from_slice(&ct);
        Ok(frame)
    }

    /// Never panics, whatever the bytes. A wrong key or an altered frame gives [`Error::Decrypt`].
    pub fn open(&self, frame: &[u8]) -> Result<Vec<u8>> {
        let Some((&version, rest)) = frame.split_first() else { return Err(Error::BadFrame("empty frame")) };
        if version != FRAME_VERSION {
            return Err(Error::BadFrame("unknown frame version"));
        }
        if rest.len() < NONCE_LEN + TAG_LEN {
            return Err(Error::BadFrame("frame too short"));
        }
        let (nonce, ct) = rest.split_at(NONCE_LEN);
        let mut n = [0u8; NONCE_LEN];
        n.copy_from_slice(nonce);
        self.rx.decrypt(&Nonce::from(n), Payload { msg: ct, aad: &[FRAME_VERSION] }).map_err(|_| Error::Decrypt)
    }
}

/// The two keys of a hosted session (edit link, read-only link). Regenerating them revokes the previous links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostKeys {
    pub edit: SessionKey,
    pub view: SessionKey,
}

impl HostKeys {
    pub fn generate(e: &mut dyn Entropy) -> Result<Self> {
        Ok(Self { edit: SessionKey::generate(e)?, view: SessionKey::generate(e)? })
    }

    #[must_use]
    pub fn key(&self, role: Role) -> &SessionKey {
        match role {
            Role::Edit => &self.edit,
            Role::View => &self.view,
        }
    }

    /// Ticket for the link of `role`.
    pub fn ticket(&self, role: Role, host: &str, relays: &[String]) -> Result<Ticket> {
        Ticket::new(host, relays, self.key(role).clone(), role)
    }

    /// Opens the first frame of a connection: the key that works tells the role the guest is entitled to.
    /// Returns that role, the cipher to use for the rest of the connection and the plaintext of the frame.
    pub fn open_first(&self, frame: &[u8]) -> Result<(Role, FrameCipher, Vec<u8>)> {
        for role in [Role::Edit, Role::View] {
            let cipher = FrameCipher::new(self.key(role), Side::Host);
            match cipher.open(frame) {
                Ok(plain) => return Ok((role, cipher, plain)),
                Err(Error::Decrypt) => {}
                Err(e) => return Err(e),
            }
        }
        Err(Error::Decrypt)
    }
}

/// Messages <-> frames: codec + encryption + size limit.
pub struct SecureChannel {
    cipher: FrameCipher,
    codec: Box<dyn Codec>,
    entropy: Box<dyn Entropy>,
    max_frame: usize,
}

impl fmt::Debug for SecureChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureChannel").field("max_frame", &self.max_frame).finish_non_exhaustive()
    }
}

impl SecureChannel {
    #[must_use]
    pub fn new(cipher: FrameCipher, codec: Box<dyn Codec>, entropy: Box<dyn Entropy>, max_frame: usize) -> Self {
        Self { cipher, codec, entropy, max_frame }
    }

    pub fn seal(&mut self, msg: &Message) -> Result<Vec<u8>> {
        self.cipher.seal(&self.codec.encode(msg), self.entropy.as_mut())
    }

    /// Encrypts bytes the channel's codec already wrote (same as `seal`, without encoding again): a broadcast is encoded once and
    /// sealed for each recipient, which has its own key and nonces.
    pub fn seal_encoded(&mut self, encoded: &[u8]) -> Result<Vec<u8>> {
        self.cipher.seal(encoded, self.entropy.as_mut())
    }

    /// Frame -> message. The size is checked first; nothing here panics on hostile bytes.
    pub fn open(&self, frame: &[u8]) -> Result<Message> {
        if frame.len() > self.max_frame {
            return Err(Violation::MessageTooLarge { size: frame.len(), max: self.max_frame }.into());
        }
        self.codec.decode(&self.cipher.open(frame)?)
    }
}

/* -------------------------------------------------------------- Ticket */

const TICKET_MAGIC: &[u8; 3] = b"IBT";
const TICKET_VERSION: u8 = 1;
const CHECK_LEN: usize = 6;
const MAX_TEXT: usize = 256;
const MAX_RELAYS: usize = 8;

/// What the link contains: how to reach the host and the secret to talk to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    /// Identity of the host for the transport (opaque here, e.g. a node id).
    pub host: String,
    /// Optional relay hints.
    pub relays: Vec<String>,
    pub key: SessionKey,
    pub role: Role,
}

fn put_str(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(&u16::try_from(s.len()).unwrap_or(u16::MAX).to_be_bytes());
    b.extend_from_slice(s.as_bytes());
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(bad_ticket("truncated"));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn text(&mut self) -> Result<String> {
        let n = usize::from(u16::from_be_bytes([self.byte()?, self.byte()?]));
        if n > MAX_TEXT {
            return Err(bad_ticket("field too long"));
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| bad_ticket("text is not valid UTF-8"))
    }
}

fn bad_ticket(why: &str) -> Error {
    Error::BadTicket(why.to_owned())
}

fn checksum(payload: &[u8]) -> [u8; CHECK_LEN] {
    let mut c = [0u8; CHECK_LEN];
    c.copy_from_slice(&Sha256::digest(payload)[..CHECK_LEN]);
    c
}

impl Ticket {
    pub fn new(host: &str, relays: &[String], key: SessionKey, role: Role) -> Result<Self> {
        if host.is_empty() || host.len() > MAX_TEXT || relays.len() > MAX_RELAYS || relays.iter().any(|r| r.len() > MAX_TEXT) {
            return Err(bad_ticket("host or relay hints missing or too long"));
        }
        Ok(Self { host: host.to_owned(), relays: relays.to_vec(), key, role })
    }

    /// Compact text (base64url) of the ticket, with a checksum that detects accidental damage. The checksum is not a security
    /// feature: authenticity comes from the AEAD key.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut b = TICKET_MAGIC.to_vec();
        b.push(TICKET_VERSION);
        b.push(role_byte(self.role));
        b.extend_from_slice(self.key.as_bytes());
        put_str(&mut b, &self.host);
        b.push(u8::try_from(self.relays.len()).unwrap_or(u8::MAX));
        for r in &self.relays {
            put_str(&mut b, r);
        }
        let sum = checksum(&b);
        b.extend_from_slice(&sum);
        URL_SAFE_NO_PAD.encode(b)
    }

    /// Never panics, whatever the text.
    pub fn decode(text: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(text.trim().as_bytes()).map_err(|_| bad_ticket("not a valid link (bad encoding)"))?;
        if bytes.len() < TICKET_MAGIC.len() + 2 + KEY_LEN + CHECK_LEN || &bytes[..3] != TICKET_MAGIC {
            return Err(bad_ticket("this is not an Inkboard link"));
        }
        let (payload, sum) = bytes.split_at(bytes.len() - CHECK_LEN);
        if payload[3] != TICKET_VERSION {
            return Err(bad_ticket(&format!("link format v{} is not supported by this build; update Inkboard (or reload the page)", payload[3])));
        }
        if !ct_eq(&checksum(payload), sum) {
            return Err(bad_ticket("the link is damaged or has been altered (checksum mismatch)"));
        }
        let mut r = Reader(&payload[4..]);
        let role = match r.byte()? {
            0 => Role::Edit,
            1 => Role::View,
            _ => return Err(bad_ticket("unknown role")),
        };
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(r.take(KEY_LEN)?);
        let host = r.text()?;
        let n = usize::from(r.byte()?);
        if n > MAX_RELAYS {
            return Err(bad_ticket("too many relay hints"));
        }
        let relays = (0..n).map(|_| r.text()).collect::<Result<Vec<_>>>()?;
        if !r.0.is_empty() || host.is_empty() {
            return Err(bad_ticket("malformed link"));
        }
        Ok(Self { host, relays, key: SessionKey(key), role })
    }
}

/// `<base>#<ticket>`: `base` is the address of the web page (or `inkboard://join`); the ticket stays in the fragment, which
/// browsers never send to a server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub base: String,
    pub ticket: Ticket,
}

impl Link {
    #[must_use]
    pub fn to_url(&self) -> String {
        format!("{}#{}", self.base, self.ticket.encode())
    }

    pub fn parse(s: &str) -> Result<Self> {
        let (base, frag) = s.trim().split_once('#').ok_or_else(|| bad_ticket("the link has no ticket (nothing after '#')"))?;
        Ok(Self { base: base.to_owned(), ticket: Ticket::decode(frag)? })
    }
}
