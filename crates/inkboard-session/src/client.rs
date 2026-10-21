//! Optimistic guest: applies its own modifications at once, keeps them in a queue until the host confirms them, and on every
//! `Applied` of the host undoes the optimistic state, applies the host's and replays what is still pending. The host's state always
//! wins, so a guest never diverges for long.
//!
//! The client is a pure state machine: it never touches the network or a clock. The caller feeds it messages and the current time
//! (`now`, in milliseconds, any monotonic origin), and sends what [`Client::take_outbox`] returns.

use crate::error::{Error, Result};
use crate::proto::{
    Applied, ByeReason, Hello, Message, PROTO_VERSION, ParticipantId, RefuseReason, RejectReason, Resume, Role, Seq, Snapshot, Submit, Welcome, check_proto,
};
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
    future: BTreeMap<Seq, Applied>,
    gap_since: Option<u64>,
    last_sync: u64,
    hello_sent_at: u64,
    outbox: Vec<Message>,
    events: Vec<ClientEvent>,
}

impl Client {
    const MAX_FUTURE: usize = 10_000;

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
            gap_since: None,
            last_sync: 0,
            hello_sent_at: 0,
            outbox: Vec::new(),
            events: Vec::new(),
        }
    }

    /// The view: the host's state plus our unconfirmed modifications.
    pub fn doc(&self) -> &Document {
        &self.doc
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn is_live(&self) -> bool {
        self.phase == Phase::Live
    }
    pub fn participant(&self) -> Option<ParticipantId> {
        self.participant
    }
    pub fn role(&self) -> Role {
        self.role
    }
    pub fn color(&self) -> (u8, u8, u8) {
        self.color
    }
    /// `seq` of the last host modification applied.
    pub fn last_seq(&self) -> Seq {
        self.last_seq
    }
    /// Modifications waiting for the host.
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
        if matches!(self.phase, Phase::Joining | Phase::Live) {
            self.phase = Phase::Disconnected;
        }
    }

    /// Makes a modification: applied to the view at once, sent to the host. Returns its `client_seq`, or `None` when it is not
    /// possible (read-only, not joined yet, closed) or changes nothing.
    pub fn edit(&mut self, ops: Vec<WireOp>, now: u64) -> Option<u64> {
        if self.role == Role::View || !self.have_snapshot || !matches!(self.phase, Phase::Live | Phase::Disconnected) {
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
    pub fn handle(&mut self, msg: Message, now: u64) -> Result<()> {
        if self.phase == Phase::Closed {
            return Ok(());
        }
        match msg {
            Message::Welcome(w) => self.on_welcome(w, now),
            Message::Refused { reason } => {
                self.phase = Phase::Closed;
                self.events.push(ClientEvent::Refused(reason));
                Ok(())
            }
            Message::Snapshot(s) => {
                self.on_snapshot(s, now);
                Ok(())
            }
            Message::Applied(a) => {
                if a.seq > self.last_seq {
                    if self.future.len() >= Self::MAX_FUTURE {
                        self.future.clear();
                    }
                    self.future.insert(a.seq, a);
                    self.rebase(now, |_| {});
                }
                Ok(())
            }
            Message::Rejected { client_seq, reason } => {
                match &reason {
                    RejectReason::Duplicate { host_seq } => {
                        let until = *host_seq;
                        self.rebase(now, |s| s.pending.iter_mut().filter(|p| p.client_seq == client_seq).for_each(|p| p.parked_until = Some(until)));
                    }
                    _ => {
                        self.rebase(now, |s| s.pending.retain(|p| p.client_seq != client_seq));
                        self.events.push(ClientEvent::Rejected { client_seq, reason });
                    }
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
            | Message::Follow { .. }
            | Message::Kick { .. }
            | Message::RoleChange { .. }
            | Message::AssetChunk(_)) => {
                self.events.push(ClientEvent::Relay(Box::new(m)));
                Ok(())
            }
            Message::Hello(_) | Message::Submit(_) | Message::AssetRequest { .. } | Message::Resync { .. } => {
                Err(Error::Unexpected("a message that only a guest sends"))
            }
        }
    }

    fn on_welcome(&mut self, w: Welcome, now: u64) -> Result<()> {
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
            while let Some(a) = self.future.remove(&(self.last_seq + 1)) {
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
