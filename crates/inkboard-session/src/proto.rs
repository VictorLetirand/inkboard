//! Messages of the session protocol (version 3). All messages travel encrypted (see `crypto`) and encoded by a `Codec`.

use crate::error::{Error, Result};
use crate::wire::WireOp;
use inkboard_core::geom::{Pt, Rect};
use inkboard_core::model::{Background, Document, Id, Object, Page};
use serde::{Deserialize, Serialize};

/// Version of the protocol, carried by the first message (`Hello`) and by `Welcome`.
/// v2: a snapshot too large for one frame travels as numbered `SnapshotPart`s (see `snapshot`).
/// v3: the host may send a snapshot (part) compressed, as `Packed` (see `pack`).
pub const PROTO_VERSION: u32 = 3;

/// Fails with a clear message when the peer speaks another version of the protocol.
pub fn check_proto(theirs: u32) -> Result<()> {
    if theirs == PROTO_VERSION { Ok(()) } else { Err(Error::IncompatibleProtocol { ours: PROTO_VERSION, theirs }) }
}

/// Sequence number of an accepted modification, assigned by the host (the first one is 1; 0 is the initial state).
pub type Seq = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParticipantId(pub u32);

impl ParticipantId {
    /// The host itself (author of its own modifications).
    pub const HOST: ParticipantId = ParticipantId(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Edit,
    View,
}

/// Bytes serialized as base64 text (JSON has no byte type).
pub mod b64 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s.as_bytes()).map_err(serde::de::Error::custom)
    }
}

/// Reconnection: the guest gives back its identity and the last `seq` it applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub participant: ParticipantId,
    pub last_seq: Seq,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u32,
    pub name: String,
    /// Proof that the guest knows the session key (see `crypto::join_proof`).
    #[serde(with = "b64")]
    pub join_proof: Vec<u8>,
    pub role_requested: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Resume>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocMeta {
    pub name: String,
    pub seq: Seq,
    pub pages: usize,
}

/// A participant as the host lists it (names and colors shown on the cursors and in the participant list).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantInfo {
    pub id: ParticipantId,
    pub name: String,
    pub color: (u8, u8, u8),
    pub role: Role,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub participant: ParticipantId,
    pub color: (u8, u8, u8),
    pub role: Role,
    pub doc_meta: DocMeta,
    pub host_proto: u32,
    /// Display name of the host ("Session of X"); empty when the host did not say.
    #[serde(default)]
    pub host_name: String,
    /// Everybody in the session at that moment, the host included.
    #[serde(default)]
    pub participants: Vec<ParticipantInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum RefuseReason {
    Version {
        host_proto: u32,
    },
    BadProof,
    Full,
    Declined,
    Banned,
    /// This device already holds as many places as the host allows.
    TooManyFromPeer,
}

impl RefuseReason {
    /// Text to show to the user.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            RefuseReason::Version { host_proto } => {
                format!("the host speaks protocol v{host_proto}, this build speaks v{PROTO_VERSION}: update Inkboard (or reload the page)")
            }
            RefuseReason::BadProof => "the invitation link is not valid (any more)".into(),
            RefuseReason::Full => "the session is full".into(),
            RefuseReason::Declined => "the host declined the request".into(),
            RefuseReason::Banned => "you were removed from this session".into(),
            RefuseReason::TooManyFromPeer => "this device already holds as many places in the session as the host allows".into(),
        }
    }
}

/// Image listed in a snapshot: transferred on demand, identified by the SHA-256 of its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetInfo {
    pub id: Id,
    pub hash: String,
    pub size: u64,
    pub mime: String,
}

/// The document without the bytes of its images.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub seq: Seq,
    pub name: String,
    pub background: Background,
    pub pages: Vec<Page>,
    pub assets: Vec<AssetInfo>,
}

impl Snapshot {
    /// Snapshot of `doc` at `seq`; `assets` is the manifest to give (see `assets::manifest`).
    #[must_use]
    pub fn new(doc: &Document, seq: Seq, assets: Vec<AssetInfo>) -> Self {
        Self { seq, name: doc.name.clone(), background: doc.background, pages: doc.pages.clone(), assets }
    }
    /// Document with no asset bytes (they are requested by hash).
    #[must_use]
    pub fn into_document(self) -> Document {
        let mut doc = Document { name: self.name, background: self.background, pages: self.pages, assets: std::collections::BTreeMap::default() };
        if doc.pages.is_empty() {
            doc.pages.push(Page::new("Page 1"));
        }
        doc
    }
}

/// What the first part of a split snapshot carries besides pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotHead {
    pub name: String,
    pub background: Background,
    pub assets: Vec<AssetInfo>,
}

/// One piece of a snapshot that does not fit in a single frame. Parts are numbered from 0, part 0 carries the `head`, the part with
/// `last` ends the series. A page that spans several parts appears in each of them with the same identifier (its objects follow each
/// other); a new part 0 (another `seq`) starts over.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPart {
    pub seq: Seq,
    pub part: u32,
    pub last: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<SnapshotHead>,
    pub pages: Vec<Page>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetChunk {
    pub hash: String,
    pub offset: u64,
    #[serde(with = "b64")]
    pub data: Vec<u8>,
    pub last: bool,
}

/// How the host stands with an image a guest is uploading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UploadStatus {
    /// `have` bytes are held, in order; the guest goes on from there.
    Receiving,
    /// The host holds the whole image (or already had it): a modification may use it now.
    Stored,
    /// The upload is over and will not be accepted (too large, over the quota, not an image...).
    Refused { reason: RejectReason },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Submit {
    /// Numbered by the guest, from 1; also used by the host to ignore duplicates.
    pub client_seq: u64,
    /// Last `seq` the guest had applied when it made the modification.
    pub base_seq: Seq,
    pub ops: Vec<WireOp>,
}

/// A modification accepted by the host, broadcast to everybody (including its author) in `seq` order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Applied {
    pub seq: Seq,
    pub author: ParticipantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_seq: Option<u64>,
    pub ops: Vec<WireOp>,
    /// Objects of the author that had been changed by somebody else since its `base_seq` (last writer wins; informative).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub superseded: Vec<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum RejectReason {
    ReadOnly,
    Invalid {
        detail: String,
    },
    /// Already applied by the host, which was at `host_seq` when it answered: everything up to there is on its way.
    Duplicate {
        host_seq: Seq,
    },
    RateLimited,
    Quota,
}

impl RejectReason {
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            RejectReason::ReadOnly => "you have read-only access".into(),
            RejectReason::Invalid { detail } => format!("the host refused the modification: {detail}"),
            RejectReason::Duplicate { .. } => "already applied".into(),
            RejectReason::RateLimited => "you are sending too fast".into(),
            RejectReason::Quota => "upload quota exceeded".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    /// Filled in by the host when relaying (never trusted from a guest).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant: Option<ParticipantId>,
    pub page: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Pt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selection: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Rect>,
}

/// What the host's MCP agent does (shown to everybody as a cursor).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub name: String,
    pub action: String,
    #[serde(default)]
    pub targets: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Pt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum ByeReason {
    Left,
    Kicked,
    HostLeft,
    ProtocolError { detail: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Message {
    // handshake
    Hello(Hello),
    Welcome(Welcome),
    Refused {
        reason: RefuseReason,
    },
    Snapshot(Snapshot),
    /// A snapshot too large for one frame, in pieces (see [`SnapshotPart`]).
    SnapshotPart(SnapshotPart),
    /// Host -> guest: a `Snapshot` or `SnapshotPart` compressed (see `pack`). A host never accepts one.
    Packed {
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    // images
    AssetRequest {
        hash: String,
    },
    AssetChunk(AssetChunk),
    /// Host -> guest: answer to every chunk the guest uploads (`have` = bytes held in order), so that it can pace, resume and stop.
    AssetAck {
        hash: String,
        have: u64,
        status: UploadStatus,
    },
    /// Host -> guests: images the board has gained since the snapshot (the host's own: their identifier is not a hash), so that the
    /// guests already in can ask for them by hash. Additive: a peer that does not know it drops the frame.
    Assets {
        list: Vec<AssetInfo>,
    },
    // modifications
    Submit(Submit),
    Applied(Applied),
    Rejected {
        client_seq: u64,
        reason: RejectReason,
    },
    /// "Tell me what I missed": sent on a gap in `seq` and periodically; the host answers with `Applied`s or a `Snapshot`.
    Resync {
        last_seq: Seq,
    },
    // ephemeral (not part of the history; `author` is filled in by the host)
    Transient {
        object: Id,
        state: Box<Object>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Option<ParticipantId>,
    },
    StrokePoints {
        stroke: Id,
        points: Vec<Pt>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Option<ParticipantId>,
    },
    Presence(Presence),
    Agent(AgentEvent),
    // control
    /// Host -> guests: the complete list of participants, sent whenever someone arrives, leaves, is renamed or changes role.
    Participants {
        list: Vec<ParticipantInfo>,
    },
    /// Guest -> host: undo (redo) my last modification. The host answers with ordinary `Applied`s; nothing is ever undone locally.
    Undo,
    Redo,
    Follow {
        participant: Option<ParticipantId>,
    },
    Kick {
        participant: ParticipantId,
    },
    RoleChange {
        participant: ParticipantId,
        role: Role,
    },
    Bye {
        reason: ByeReason,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
}
