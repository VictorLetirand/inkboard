//! Optimistic guest: applies its own modifications at once, keeps them in a queue until the host confirms them, and on every
//! `Applied` of the host undoes the optimistic state, applies the host's and replays what is still pending. The host's state always
//! wins, so a guest never diverges for long.
//!
//! The client is a pure state machine: it never touches the network or a clock. The caller feeds it messages and the current time
//! (`now`, in milliseconds, any monotonic origin), and sends what [`Client::take_outbox`] returns.

use crate::error::{Error, Result};
use crate::limits::{Limits, Violation, check_id, check_manifest, check_message, check_name, check_ops, check_pages, check_seq, check_snapshot_shape};
use crate::proto::{
    Applied, ByeReason, Hello, Message, PROTO_VERSION, ParticipantId, RefuseReason, RejectReason, Resume, Role, Seq, Snapshot, SnapshotPart, Submit, Welcome,
    check_proto,
};
use crate::snapshot::{MAX_PARTS, SnapshotAssembler};
use crate::wire::{WireOp, apply_wire_all, rollback};
use inkboard_core::history::Op;
use inkboard_core::model::{Document, Id};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub name: String,
    pub role_requested: Role,
    /// See `crypto::join_proof`.
    pub join_proof: Vec<u8>,
    /// A `Hello` or a `Submit` without answer after this delay is sent again.
    pub resend_after_ms: u64,
    /// Period of the "what did I miss" message that catches up a lost tail (0: never).
    pub sync_interval_ms: u64,
}

impl ClientConfig {
    #[must_use]
    pub fn new(name: &str, role_requested: Role, join_proof: Vec<u8>) -> Self {
        Self { name: name.to_owned(), role_requested, join_proof, resend_after_ms: 1_000, sync_interval_ms: 5_000 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Joining,
    Live,
    /// The connection dropped: edits keep being queued and are sent after `connect`.
    Disconnected,
    /// Refused, kicked, or the host left: see the events. A copy of the document can still be saved.
    Closed,
}

/// What the interface may want to know.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    Joined {
        participant: ParticipantId,
        role: Role,
    },
    Refused(RefuseReason),
    /// The host refused one of our modifications (it has been rolled back).
    Rejected {
        client_seq: u64,
        reason: RejectReason,
    },
    /// Objects we changed had been changed by somebody else meanwhile (ours won).
    Superseded(Vec<Id>),
    RoleChanged(Role),
    Closed(ByeReason),
    SnapshotLoaded,
    /// The host forgot us while we were away: modifications that were not confirmed were dropped.
    PendingDiscarded(usize),
    /// Presence, ephemeral states, agent events... of other participants, to display.
    Relay(Box<Message>),
}

/// Bounds of one part of a snapshot (the document as a whole is checked when the last part arrives).
fn check_part(l: &Limits, p: &SnapshotPart) -> std::result::Result<(), Violation> {
    check_seq(p.seq)?;
    if p.part as usize >= MAX_PARTS {
        return Err(Violation::TooMany { what: "snapshot parts", count: p.part as usize, max: MAX_PARTS });
    }
    check_pages(l, &p.pages)?;
    match &p.head {
        Some(h) => check_name(l, &h.name).and_then(|()| check_manifest(l, &h.assets)),
        None => Ok(()),
    }
}

#[derive(Clone, Debug)]
struct Pending {
    client_seq: u64,
    base_seq: Seq,
    ops: Vec<WireOp>,
    /// Core operations that applying `ops` to the current view performed (for the rollback).
    applied: Vec<Op>,
    sent_at: Option<u64>,
    /// The host said it already has this one: it is dropped once we have applied everything up to this `seq`.
    parked_until: Option<Seq>,
}

#[derive(Debug)]
pub struct Client {
    cfg: ClientConfig,
    phase: Phase,
    participant: Option<ParticipantId>,
    role: Role,
    color: (u8, u8, u8),
    doc: Document,
    have_snapshot: bool,
    last_seq: Seq,
    next_client_seq: u64,
    pending: Vec<Pending>,
    /// `Applied` that arrived ahead of their turn, with the bytes they weigh (0 for one that is next in line: it never waits).
    future: BTreeMap<Seq, (Applied, usize)>,
    /// Bounds for everything the host says (see `Limits::host_trust`).
    limits: Limits,
    /// Parts of a snapshot being received.
    assembling: Option<SnapshotAssembler>,
    gap_since: Option<u64>,
    last_sync: u64,
    hello_sent_at: u64,
    outbox: Vec<Message>,
    events: Vec<ClientEvent>,
}

impl Client {
    /// Modifications kept while waiting for an earlier one (the host keeps 1 000 to catch guests up: beyond that a snapshot is needed).
    const MAX_FUTURE: usize = 1_000;
    const MAX_FUTURE_BYTES: usize = 32 << 20;
    /// Objects of a snapshot (the host's own ceiling is lower by default).
    const MAX_SNAPSHOT_OBJECTS: usize = 200_000;

    #[must_use]
    pub fn new(cfg: ClientConfig) -> Self {
        let role = cfg.role_requested;
        Self {
            cfg,
            phase: Phase::Idle,
            participant: None,
            role,
            color: (128, 128, 128),
            doc: Document::default(),
            have_snapshot: false,
            last_seq: 0,
            next_client_seq: 1,
            pending: Vec::new(),
            future: BTreeMap::new(),
            limits: Limits::host_trust(),
            assembling: None,
            gap_since: None,
            last_sync: 0,
            hello_sent_at: 0,
            outbox: Vec::new(),
            events: Vec::new(),
        }
    }

    /// The view: the host's state plus our unconfirmed modifications.
    #[must_use]
    pub fn doc(&self) -> &Document {
        &self.doc
    }
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.phase == Phase::Live
    }
    #[must_use]
    pub fn participant(&self) -> Option<ParticipantId> {
        self.participant
    }
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }
    #[must_use]
    pub fn color(&self) -> (u8, u8, u8) {
        self.color
    }
    /// `seq` of the last host modification applied.
    #[must_use]
    pub fn last_seq(&self) -> Seq {
        self.last_seq
    }
    /// Modifications waiting for the host.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
    pub fn take_outbox(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }
    pub fn take_events(&mut self) -> Vec<ClientEvent> {
        std::mem::take(&mut self.events)
    }

    /// Starts (or, after `connection_lost`, restarts) the handshake. After a first join, the `Hello` asks to resume the same place.
    pub fn connect(&mut self, now: u64) {
        if self.phase == Phase::Closed {
            return;
        }
        self.phase = Phase::Joining;
        self.assembling = None;
        self.send_hello(now);
    }

    fn send_hello(&mut self, now: u64) {
        self.hello_sent_at = now;
        // without a document there is nothing to resume from: ask for a snapshot
        let resume = self.participant.filter(|_| self.have_snapshot).map(|participant| Resume { participant, last_seq: self.last_seq });
        self.outbox.push(Message::Hello(Hello {
            proto: PROTO_VERSION,
            name: self.cfg.name.clone(),
            join_proof: self.cfg.join_proof.clone(),
            role_requested: self.cfg.role_requested,
            resume,
        }));
    }

    /// The transport dropped. Modifications are still accepted and queued.
    pub fn connection_lost(&mut self) {
        self.assembling = None;
        if matches!(self.phase, Phase::Joining | Phase::Live) {
            self.phase = Phase::Disconnected;
        }
    }

    /// Makes a modification: applied to the view at once, sent to the host. Returns its `client_seq`, or `None` when it is not
    /// possible (read-only, not joined yet, closed) or changes nothing.
    pub fn edit(&mut self, ops: Vec<WireOp>, now: u64) -> Option<u64> {
        // `Joining` with a snapshot is a reconnection in progress: the edit waits, and `on_welcome` sends it
        if self.role == Role::View || !self.have_snapshot || !matches!(self.phase, Phase::Live | Phase::Disconnected | Phase::Joining) {
            return None;
        }
        let applied = apply_wire_all(&mut self.doc, &ops);
        if applied.is_empty() {
            return None;
        }
        let client_seq = self.next_client_seq;
        self.next_client_seq += 1;
        let mut p = Pending { client_seq, base_seq: self.last_seq, ops, applied, sent_at: None, parked_until: None };
        if self.phase == Phase::Live {
            p.sent_at = Some(now);
            self.outbox.push(Message::Submit(Submit { client_seq, base_seq: p.base_seq, ops: p.ops.clone() }));
        }
        self.pending.push(p);
        Some(client_seq)
    }

    /// Asks the host to undo (`redo`: redo) this guest's last modification. Returns false when it cannot be asked (read-only,
    /// not live): the request is not queued, and the local view is never touched, the host answers with ordinary `Applied`s.
    pub fn request_undo(&mut self, redo: bool) -> bool {
        if self.role == Role::View || self.phase != Phase::Live || !self.have_snapshot {
            return false;
        }
        self.outbox.push(if redo { Message::Redo } else { Message::Undo });
        true
    }

    /// Sends an ephemeral message (presence, transient state, stroke points); dropped unless live.
    pub fn send_ephemeral(&mut self, msg: Message) {
        if self.phase == Phase::Live {
            self.outbox.push(msg);
        }
    }

    /// Timers: resends what has no answer and asks for what may have been lost.
    pub fn tick(&mut self, now: u64) {
        let after = self.cfg.resend_after_ms;
        match self.phase {
            Phase::Joining if now.saturating_sub(self.hello_sent_at) >= after => self.send_hello(now),
            // welcomed but the snapshot got lost: the host repeats both when it gets the same `Hello` again
            Phase::Live if !self.have_snapshot => {
                if now.saturating_sub(self.hello_sent_at) >= after {
                    self.send_hello(now);
                }
            }
            Phase::Live => {
                for p in self.pending.iter_mut().filter(|p| p.parked_until.is_none()) {
                    if p.sent_at.is_none_or(|t| now.saturating_sub(t) >= after) {
                        p.sent_at = Some(now);
                        self.outbox.push(Message::Submit(Submit { client_seq: p.client_seq, base_seq: p.base_seq, ops: p.ops.clone() }));
                    }
                }
                let gap = self.gap_since.is_some_and(|t| now.saturating_sub(t) >= after);
                let periodic = self.cfg.sync_interval_ms > 0 && now.saturating_sub(self.last_sync) >= self.cfg.sync_interval_ms;
                if self.have_snapshot && (gap || periodic) {
                    self.last_sync = now;
                    if gap {
                        self.gap_since = Some(now);
                    }
                    self.outbox.push(Message::Resync { last_seq: self.last_seq });
                }
            }
            _ => {}
        }
    }

    /// Handles a message from the host. An `Err` means the host broke the protocol (or speaks another version); the client is closed
    /// in the latter case.
    #[allow(clippy::too_many_lines, reason = "the message table of the client")]
    pub fn handle(&mut self, msg: Message, now: u64) -> Result<()> {
        if self.phase == Phase::Closed {
            return Ok(());
        }
        match msg {
            Message::Welcome(w) => self.on_welcome(&w, now),
            Message::Refused { reason } => {
                self.phase = Phase::Closed;
                self.events.push(ClientEvent::Refused(reason));
                Ok(())
            }
            Message::Snapshot(s) => {
                let l = &self.limits;
                let checked = check_seq(s.seq)
                    .and_then(|()| check_name(l, &s.name))
                    .and_then(|()| check_pages(l, &s.pages))
                    .and_then(|_| check_snapshot_shape(l, &s.pages, Self::MAX_SNAPSHOT_OBJECTS))
                    .and_then(|()| check_manifest(l, &s.assets));
                self.accept_from_host(checked)?;
                self.on_snapshot(s, now);
                Ok(())
            }
            Message::SnapshotPart(p) => self.on_snapshot_part(p, now),
            Message::Packed { data } => {
                // the board, compressed: expanded under a ceiling (a hostile host cannot make a small frame expand without limit)
                let inner = crate::pack::unpack(&data, self.limits.max_message_bytes);
                match inner {
                    Ok(m) => self.handle(m, now),
                    Err(e) => {
                        self.phase = Phase::Closed;
                        self.events.push(ClientEvent::Closed(ByeReason::ProtocolError { detail: e.to_string() }));
                        Err(e)
                    }
                }
            }
            Message::Applied(a) => {
                if a.seq > self.last_seq {
                    let l = &self.limits;
                    let checked = check_seq(a.seq).and_then(|()| check_ops(l, &a.ops)).and_then(|()| {
                        (a.superseded.len() <= l.max_ids_per_message).then_some(()).ok_or(Violation::TooMany {
                            what: "identifiers",
                            count: a.superseded.len(),
                            max: l.max_ids_per_message,
                        })?;
                        a.superseded.iter().try_for_each(|id| check_id(l, id))
                    });
                    self.accept_from_host(checked)?;
                    // one that is next in line never waits; the others are bounded in number and in bytes
                    let held = a.seq > self.last_seq + 1;
                    let bytes = if held { serde_json::to_vec(&a).map_or(0, |b| b.len()) } else { 0 };
                    let waiting: usize = self.future.values().map(|e| e.1).sum();
                    if held && (self.future.len() >= Self::MAX_FUTURE || waiting + bytes > Self::MAX_FUTURE_BYTES) {
                        self.future.clear(); // a gap this wide is closed by a snapshot, which `Resync` brings
                    }
                    self.future.insert(a.seq, (a, bytes));
                    self.rebase(now, |_| {});
                }
                Ok(())
            }
            Message::Rejected { client_seq, reason } => {
                if let RejectReason::Duplicate { host_seq } = &reason {
                    let until = *host_seq;
                    self.rebase(now, |s| s.pending.iter_mut().filter(|p| p.client_seq == client_seq).for_each(|p| p.parked_until = Some(until)));
                } else {
                    self.rebase(now, |s| s.pending.retain(|p| p.client_seq != client_seq));
                    self.events.push(ClientEvent::Rejected { client_seq, reason });
                }
                Ok(())
            }
            Message::Bye { reason } => {
                self.phase = Phase::Closed;
                self.events.push(ClientEvent::Closed(reason));
                Ok(())
            }
            Message::Kick { participant } if Some(participant) == self.participant => {
                self.phase = Phase::Closed;
                self.events.push(ClientEvent::Closed(ByeReason::Kicked));
                Ok(())
            }
            Message::RoleChange { participant, role } if Some(participant) == self.participant => {
                self.role = role;
                self.events.push(ClientEvent::RoleChanged(role));
                Ok(())
            }
            Message::Ping { nonce } => {
                self.outbox.push(Message::Pong { nonce });
                Ok(())
            }
            Message::Pong { .. } => Ok(()),
            m @ (Message::Presence(_)
            | Message::Transient { .. }
            | Message::StrokePoints { .. }
            | Message::Agent(_)
            | Message::Participants { .. }
            | Message::Follow { .. }
            | Message::Kick { .. }
            | Message::RoleChange { .. }
            | Message::AssetChunk(_)
            | Message::AssetAck { .. }
            | Message::Assets { .. }) => {
                // a display-only message that does not pass the bounds is dropped (it never touches the board)
                if check_message(&self.limits, &m).is_ok() {
                    self.events.push(ClientEvent::Relay(Box::new(m)));
                }
                Ok(())
            }
            Message::Hello(_) | Message::Submit(_) | Message::AssetRequest { .. } | Message::Resync { .. } | Message::Undo | Message::Redo => {
                Err(Error::Unexpected("a message that only a guest sends"))
            }
        }
    }

    fn on_welcome(&mut self, w: &Welcome, now: u64) -> Result<()> {
        if let Err(e) = check_proto(w.host_proto) {
            self.phase = Phase::Closed;
            return Err(e);
        }
        if let Some(prev) = self.participant
            && prev != w.participant
        {
            // The host does not know us any more: we cannot tell which of our unconfirmed modifications it applied.
            let n = self.pending.len();
            self.rebase(now, |s| {
                s.pending.clear();
                s.have_snapshot = false;
                s.last_seq = 0;
                s.future.clear();
            });
            if n > 0 {
                self.events.push(ClientEvent::PendingDiscarded(n));
            }
        }
        self.participant = Some(w.participant);
        self.role = w.role;
        self.color = w.color;
        if self.phase != Phase::Live {
            self.phase = Phase::Live;
            self.last_sync = now;
            self.events.push(ClientEvent::Joined { participant: w.participant, role: w.role });
            // reconnection: send again what the host may not have received (it ignores what it already applied)
            for p in self.pending.iter_mut().filter(|p| p.parked_until.is_none()) {
                p.sent_at = Some(now);
                self.outbox.push(Message::Submit(Submit { client_seq: p.client_seq, base_seq: p.base_seq, ops: p.ops.clone() }));
            }
        }
        Ok(())
    }

    /// What the host sent went over the bounds: it is not trusted any more, the client closes and the document stays as it was.
    fn accept_from_host(&mut self, checked: std::result::Result<(), Violation>) -> Result<()> {
        checked.map_err(|v| {
            self.phase = Phase::Closed;
            self.events.push(ClientEvent::Closed(ByeReason::ProtocolError { detail: v.to_string() }));
            Error::Invalid(v)
        })
    }

    fn on_snapshot_part(&mut self, p: SnapshotPart, now: u64) -> Result<()> {
        let checked = check_part(&self.limits, &p);
        self.accept_from_host(checked)?;
        if self.have_snapshot && p.seq <= self.last_seq {
            return Ok(()); // stale or repeated
        }
        let mut asm = match self.assembling.take() {
            Some(a) if a.seq() == p.seq => a,
            Some(a) if a.seq() > p.seq => {
                self.assembling = Some(a); // a straggler of an older snapshot
                return Ok(());
            }
            _ => SnapshotAssembler::new(p.seq),
        };
        match asm.add(p, &self.limits, Self::MAX_SNAPSHOT_OBJECTS) {
            Ok(Some(s)) => self.on_snapshot(s, now),
            Ok(None) => self.assembling = Some(asm),
            Err(Error::Invalid(v)) => return self.accept_from_host(Err(v)),
            Err(e) => return Err(e),
        }
        Ok(())
    }

    fn on_snapshot(&mut self, s: Snapshot, now: u64) {
        if self.have_snapshot && s.seq <= self.last_seq {
            return; // stale or repeated
        }
        let seq = s.seq;
        self.rebase(now, |c| {
            c.doc = s.into_document();
            c.have_snapshot = true;
            c.last_seq = seq;
        });
        self.events.push(ClientEvent::SnapshotLoaded);
    }

    /// Undoes the optimistic state, runs `change`, applies the host's pending run of `Applied`, drops what the host has confirmed and
    /// replays the remaining modifications.
    fn rebase(&mut self, now: u64, change: impl FnOnce(&mut Self)) {
        for p in self.pending.iter_mut().rev() {
            rollback(&mut self.doc, &p.applied);
            p.applied.clear();
        }
        change(self);
        if self.have_snapshot {
            self.future = self.future.split_off(&(self.last_seq + 1));
            while let Some((a, _)) = self.future.remove(&(self.last_seq + 1)) {
                for op in &a.ops {
                    crate::wire::apply_wire(&mut self.doc, op);
                }
                self.last_seq = a.seq;
                if Some(a.author) == self.participant {
                    if let Some(c) = a.client_seq {
                        self.pending.retain(|p| p.client_seq != c);
                    }
                    if !a.superseded.is_empty() {
                        self.events.push(ClientEvent::Superseded(a.superseded));
                    }
                }
            }
            let last = self.last_seq;
            self.pending.retain(|p| p.parked_until.is_none_or(|u| u > last));
            self.gap_since = if self.future.is_empty() { None } else { self.gap_since.or(Some(now)) };
        }
        for p in &mut self.pending {
            p.applied = apply_wire_all(&mut self.doc, &p.ops);
        }
    }
}
